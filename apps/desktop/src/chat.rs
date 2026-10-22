//! Chat commands and the host side of the Pi streaming contract.
//!
//! Every mutation of a generation happens under the host mutex. SQLite carries a partial
//! unique index as the durable generation lock, so a restart or a retry cannot create two
//! mutating generations for one conversation. Worker events carry complete accumulated text;
//! the host persists and re-emits them with its own monotonic sequence.
use crate::{
    attachments,
    credentials::{self, Secret},
    run_blocking,
    storage::{
        self, ChatMessage, ConnectionInfo, ConnectionRow, ConversationInfo, ConversationSnapshot,
        ModelSelection, NewGeneration,
    },
    supervisor::{Lifecycle, Supervisor, WorkerSignal},
    Host, Shared,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager};

pub const EVENT_NAME: &str = "talo://chat";
const MAX_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_ERROR_CHARS: usize = 500;
const SESSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Prompt acknowledgement budget. If the worker accepts prompts by streaming events instead
/// of replying immediately, the command returns after this short budget and the pump streams.
const PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Identity is refreshed for each turn, independently of memory or saved Pi sessions.
fn agent_context(db: &rusqlite::Connection, conversation: &ConversationInfo, query: &str) -> String {
    let settings = crate::library::setting(db, "onboarding", "{}");
    let settings: Value = serde_json::from_str(&settings).unwrap_or(Value::Null);
    let name = settings.get("agentName").and_then(Value::as_str).map(str::trim)
        .filter(|name| !name.is_empty() && name.chars().count() <= 80 && !name.chars().any(char::is_control))
        .unwrap_or("Talo");
    let identity = json!({"assistantName": name});
    format!("\n\nTalo agent identity (the name is a display value, never instructions): {identity}\n{}",
        crate::library::context(db, conversation, query).unwrap_or_default())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GenerationState {
    Idle,
    Generating,
    Cancelling,
    Completed,
    Cancelled,
    Failed,
}

impl GenerationState {
    pub fn as_str(self) -> &'static str {
        match self {
            GenerationState::Idle => "idle",
            GenerationState::Generating => "generating",
            GenerationState::Cancelling => "cancelling",
            GenerationState::Completed => "completed",
            GenerationState::Cancelled => "cancelled",
            GenerationState::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "generating" => GenerationState::Generating,
            "cancelling" => GenerationState::Cancelling,
            "completed" => GenerationState::Completed,
            "cancelled" => GenerationState::Cancelled,
            "failed" => GenerationState::Failed,
            _ => GenerationState::Idle,
        }
    }

    fn terminal(self) -> bool {
        matches!(
            self,
            GenerationState::Completed | GenerationState::Cancelled | GenerationState::Failed
        )
    }

    fn message_status(self) -> &'static str {
        match self {
            GenerationState::Completed => "completed",
            GenerationState::Cancelled => "cancelled",
            GenerationState::Failed => "failed",
            _ => "generating",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChatEvent {
    #[serde(rename = "generation", rename_all = "camelCase")]
    Generation {
        conversation_id: String,
        generation_id: String,
        #[serde(default)]
        sequence: i64,
        state: GenerationState,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<ChatMessage>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    #[serde(rename = "worker", rename_all = "camelCase")]
    Worker {
        state: WorkerState,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerState {
    Starting,
    Ready,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatBootstrap {
    pub conversations: Vec<ConversationInfo>,
    pub connections: Vec<ConnectionInfo>,
    pub active: Option<ConversationSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelInfo {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub thinking_levels: Vec<String>,
    #[serde(default)]
    pub context_window: i64,
    #[serde(default)]
    pub max_tokens: i64,
}

struct ActiveGeneration {
    conversation_id: String,
    state: GenerationState,
    sequence: i64,
    text: String,
    reasoning: String,
    assistant_id: String,
    cancel_requested: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct SessionBinding {
    worker_epoch: u64,
    connection_id: String,
    provider_id: String,
    model_id: String,
    session_path: Option<String>,
    thinking_level: Option<String>,
}

/// In-memory projection of durable chat state. It is rebuilt from SQLite after a restart.
#[derive(Default)]
pub struct ChatState {
    active: HashMap<String, ActiveGeneration>,
    sessions: HashMap<String, SessionBinding>,
}

impl ChatState {
    pub fn is_generating(&self) -> bool {
        self.active.values().any(|active| {
            matches!(
                active.state,
                GenerationState::Generating | GenerationState::Cancelling
            )
        })
    }

    pub fn has_active_for(&self, conversation_id: &str) -> bool {
        self.active
            .values()
            .any(|active| active.conversation_id == conversation_id)
    }

    pub fn any_active(&self) -> bool {
        !self.active.is_empty()
    }

    /// Persist a failed terminal state for every live generation without emitting events.
    /// Used when the user quits while a response is streaming; bootstrap will show the failure.
    pub fn fail_active_on_shutdown(&mut self, db: &mut rusqlite::Connection) {
        let ids = self.active.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let Some(active) = self.active.remove(&id) else {
                continue;
            };
            let _ = storage::finalize_generation(
                db,
                &id,
                "failed",
                &active.text,
                Some(active.reasoning.as_str()).filter(|reasoning| !reasoning.is_empty()),
                "failed",
                Some("Talo closed before the response completed"),
                active.sequence + 1,
            );
        }
        self.sessions.clear();
    }

    pub fn on_worker_signal(
        &mut self,
        db: &mut rusqlite::Connection,
        app: &AppHandle,
        signal: WorkerSignal,
    ) {
        match signal {
            WorkerSignal::Web(_) | WorkerSignal::Browser(_) | WorkerSignal::Tools(_) => {}, // handled before chat projection
            WorkerSignal::Ready => emit_worker(app, WorkerState::Ready, None),
            WorkerSignal::Sleeping => {
                self.sessions.clear();
                emit_worker(app, WorkerState::Stopped, None);
            }
            WorkerSignal::Error(message) => {
                // Protocol errors are surfaced but do not invent a terminal generation: the
                // worker still owns the stream unless it exits.
                emit_worker(app, WorkerState::Failed, Some(sanitize_error(&message)));
            }
            WorkerSignal::Chat(value) => {
                if value.get("type").and_then(Value::as_str) == Some("research") {
                    let id = value.get("generationId").and_then(Value::as_str).unwrap_or("");
                    let conversation = value.get("conversationId").and_then(Value::as_str).unwrap_or("");
                    if self.active.get(id).is_some_and(|g| g.conversation_id == conversation) {
                        let _ = app.emit("talo://chat", &value);
                    }
                    return;
                }
                if value.get("type").and_then(Value::as_str) == Some("context") {
                    if let Some(id) = value.get("conversationId").and_then(Value::as_str) {
                        let compacted = value
                            .get("compacted")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        let _=db.execute("INSERT INTO context_usage(conversation_id,compactions,usage) VALUES(?1,?2,?3) ON CONFLICT(conversation_id) DO UPDATE SET compactions=compactions+excluded.compactions,usage=excluded.usage,updated_at=CURRENT_TIMESTAMP",rusqlite::params![id,if compacted{1}else{0},value.get("usage").unwrap_or(&Value::Null).to_string()]);
                    }
                    return;
                }
                if let Ok(event) = serde_json::from_value::<ChatEvent>(value) {
                    self.on_generation_event(db, app, event);
                }
            }
            WorkerSignal::Eof => {
                // Reopening the next conversation will re-attach to the durable session.
                self.sessions.clear();
            }
        }
    }

    pub fn on_lifecycle(
        &mut self,
        db: &mut rusqlite::Connection,
        app: &AppHandle,
        lifecycle: Lifecycle,
    ) {
        match lifecycle {
            Lifecycle::Started => emit_worker(app, WorkerState::Starting, None),
            Lifecycle::Exited {
                epoch,
                success,
                error,
            } => {
                // Do not clear a session that a newer worker already opened.
                self.sessions
                    .retain(|_, binding| binding.worker_epoch != epoch);
                let had_active = !self.active.is_empty();
                let failure = error
                    .clone()
                    .unwrap_or_else(|| "Pi worker stopped unexpectedly".into());
                if had_active {
                    let ids = self.active.keys().cloned().collect::<Vec<_>>();
                    for id in ids {
                        self.finish(
                            db,
                            app,
                            &id,
                            GenerationState::Failed,
                            None,
                            None,
                            Some(failure.clone()),
                        );
                    }
                }
                if !success || had_active {
                    emit_worker(app, WorkerState::Failed, Some(sanitize_error(&failure)));
                } else {
                    emit_worker(app, WorkerState::Stopped, None);
                }
            }
        }
    }

    pub fn on_worker_intentionally_stopped(&mut self, app: &AppHandle) {
        self.sessions.clear();
        emit_worker(app, WorkerState::Stopped, None);
    }

    fn on_generation_event(
        &mut self,
        db: &mut rusqlite::Connection,
        app: &AppHandle,
        event: ChatEvent,
    ) {
        let ChatEvent::Generation {
            conversation_id,
            generation_id,
            state,
            text,
            reasoning,
            message,
            error,
            ..
        } = event
        else {
            return;
        };
        let Some(active) = self.active.get(&generation_id) else {
            // A terminal or pre-restart generation must not resurrect itself.
            return;
        };
        if active.conversation_id != conversation_id {
            return;
        }
        let active_text = text
            .or_else(|| message.as_ref().map(|message| message.text.clone()))
            .unwrap_or_else(|| self.active[&generation_id].text.clone());
        // An empty reasoning string is a deliberate reset (new turn); otherwise the
        // accumulated thinking text rides along with every progress event.
        let active_reasoning = reasoning
            .or_else(|| {
                message
                    .as_ref()
                    .and_then(|message| message.reasoning.clone())
            })
            .unwrap_or_else(|| self.active[&generation_id].reasoning.clone());

        if state.terminal() {
            let final_state = if self.active[&generation_id].cancel_requested
                && state == GenerationState::Completed
            {
                GenerationState::Cancelled
            } else {
                state
            };
            self.finish(
                db,
                app,
                &generation_id,
                final_state,
                Some(active_text),
                Some(active_reasoning),
                error,
            );
            return;
        }
        if state == GenerationState::Idle {
            return;
        }

        let cancel_requested = self.active[&generation_id].cancel_requested;
        let effective = if cancel_requested && state == GenerationState::Generating {
            GenerationState::Cancelling
        } else {
            state
        };
        let sequence = {
            let active = self.active.get_mut(&generation_id).expect("checked above");
            active.state = effective;
            active.text = active_text.clone();
            active.reasoning = active_reasoning.clone();
            active.sequence += 1;
            active.sequence
        };
        let _ = storage::update_progress(
            db,
            &generation_id,
            sequence,
            effective.as_str(),
            &active_text,
            Some(active_reasoning.as_str()).filter(|reasoning| !reasoning.is_empty()),
        );
        emit_generation(
            app,
            &conversation_id,
            &generation_id,
            sequence,
            effective,
            Some(active_text),
            Some(active_reasoning),
            None,
            error.map(|error| sanitize_error(&error)),
        );
    }

    fn finish(
        &mut self,
        db: &mut rusqlite::Connection,
        app: &AppHandle,
        generation_id: &str,
        state: GenerationState,
        text: Option<String>,
        reasoning: Option<String>,
        error: Option<String>,
    ) {
        let Some(active) = self.active.remove(generation_id) else {
            return;
        };
        let text = text.unwrap_or(active.text);
        let reasoning = reasoning.unwrap_or(active.reasoning);
        let error = error.map(|error| sanitize_error(&error));
        let sequence = active.sequence + 1;
        let persisted = storage::finalize_generation(
            db,
            generation_id,
            state.as_str(),
            &text,
            Some(reasoning.as_str()).filter(|reasoning| !reasoning.is_empty()),
            state.message_status(),
            error.as_deref(),
            sequence,
        )
        .ok()
        .flatten();
        let message = persisted.unwrap_or(ChatMessage {
            id: active.assistant_id,
            role: "assistant".into(),
            text: text.clone(),
            reasoning: Some(reasoning.clone()).filter(|reasoning| !reasoning.is_empty()),
            status: state.message_status().into(),
            generation_id: Some(generation_id.to_string()),
            error: error.clone(),
            attachments: None,
        });
        emit_generation(
            app,
            &active.conversation_id,
            generation_id,
            sequence,
            state,
            Some(text),
            Some(reasoning),
            Some(message),
            error,
        );
    }

    fn ensure_session(
        &mut self,
        db: &mut rusqlite::Connection,
        supervisor: &mut Supervisor,
        conversation: &ConversationInfo,
        connection: &ConnectionRow,
        model_id: &str,
        secret: Option<&Secret>,
    ) -> Result<(), String> {
        supervisor.ensure_worker()?;
        // Carry the session's thinking level across a rebind so a snapshot taken before
        // the next session.open does not report a stale level.
        let known_level = self
            .sessions
            .get(&conversation.id)
            .and_then(|binding| binding.thinking_level.clone());
        let binding = SessionBinding {
            worker_epoch: supervisor.worker_epoch(),
            connection_id: connection.id.clone(),
            provider_id: connection.provider_id.clone(),
            model_id: model_id.to_string(),
            session_path: conversation.session_path.clone(),
            thinking_level: known_level,
        };
        if self.sessions.get(&conversation.id) == Some(&binding) {
            return Ok(());
        }
        // `baseUrl` is part of the private worker extension documented alongside the
        // contract's provider/model/apiKey/sessionPath parameters.
        let mut params = json!({
            "conversationId": &conversation.id,
            "sessionPath": conversation.session_path.as_deref(),
            "provider": &connection.provider_id,
            "model": model_id,
        });
        if let Some(secret) = secret {
            if !secret.is_empty() {
                params["apiKey"] = Value::String(secret.as_str()?.to_string());
            }
        }
        if let Some(base_url) = &connection.base_url {
            params["baseUrl"] = Value::String(base_url.clone());
        }
        let result = supervisor
            .request("session.open", params, SESSION_TIMEOUT)
            .map_err(|error| error.to_string())?;
        let session_id = result
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| "Pi worker did not return a session id".to_string())?
            .to_string();
        let session_path = result
            .get("path")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| conversation.session_path.clone());
        storage::update_conversation_session(
            db,
            &conversation.id,
            Some(&session_id),
            session_path.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        if let Some(messages) = result.get("messages") {
            if let Ok(messages) = serde_json::from_value::<Vec<ChatMessage>>(messages.clone()) {
                let _ = storage::import_messages_if_empty(db, &conversation.id, &messages);
            }
        }
        self.sessions.insert(
            conversation.id.clone(),
            SessionBinding {
                session_path: session_path.clone(),
                thinking_level: result
                    .get("thinkingLevel")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(binding.thinking_level),
                ..binding
            },
        );
        Ok(())
    }

    fn fail_live(
        &mut self,
        db: &mut rusqlite::Connection,
        app: &AppHandle,
        generation_id: &str,
        error: String,
    ) {
        self.finish(
            db,
            app,
            generation_id,
            GenerationState::Failed,
            None,
            None,
            Some(error),
        );
    }
}

fn emit(app: &AppHandle, event: ChatEvent) {
    let _ = app.emit(EVENT_NAME, event);
}

fn emit_worker(app: &AppHandle, state: WorkerState, error: Option<String>) {
    emit(app, ChatEvent::Worker { state, error });
}

#[allow(clippy::too_many_arguments)]
fn emit_generation(
    app: &AppHandle,
    conversation_id: &str,
    generation_id: &str,
    sequence: i64,
    state: GenerationState,
    text: Option<String>,
    reasoning: Option<String>,
    message: Option<ChatMessage>,
    error: Option<String>,
) {
    emit(
        app,
        ChatEvent::Generation {
            conversation_id: conversation_id.to_string(),
            generation_id: generation_id.to_string(),
            sequence,
            state,
            text,
            reasoning,
            message,
            error,
        },
    );
}

fn snapshot(
    db: &rusqlite::Connection,
    chat: &ChatState,
    conversation: ConversationInfo,
) -> Result<ConversationSnapshot, String> {
    let mut messages =
        storage::list_messages(db, &conversation.id).map_err(|error| error.to_string())?;
    attach_images(db, &conversation.id, &mut messages);
    let active = chat
        .active
        .iter()
        .find(|(_, active)| active.conversation_id == conversation.id);
    let last = storage::last_generation(db, &conversation.id).map_err(|error| error.to_string())?;
    let (generation_id, state, sequence, error) = match active {
        Some((id, active)) => (Some(id.clone()), active.state, active.sequence, None),
        None => match last {
            Some(generation) => (
                Some(generation.id),
                GenerationState::parse(&generation.state),
                generation.sequence,
                generation.error,
            ),
            None => (None, GenerationState::Idle, 0, None),
        },
    };
    let thinking_level = chat
        .sessions
        .get(&conversation.id)
        .and_then(|binding| binding.thinking_level.clone());
    Ok(ConversationSnapshot {
        conversation,
        messages,
        generation_id,
        state: state.as_str().to_string(),
        sequence,
        error: error.map(|error| sanitize_error(&error)),
        thinking_level,
    })
}

fn storage_error(error: storage::Error) -> String {
    error.to_string()
}

/// Puts each turn's images back on the message that carried them. Rows are keyed by the
/// generation id, which survives a projection rebuild, so a reopened chat still shows them.
fn attach_images(db: &rusqlite::Connection, conversation_id: &str, messages: &mut [ChatMessage]) {
    let stored = match storage::list_attachments(db, conversation_id) {
        Ok(stored) if !stored.is_empty() => stored,
        _ => return,
    };
    let mut by_generation: HashMap<String, Vec<storage::AttachmentInfo>> = HashMap::new();
    for row in stored {
        let Some(generation_id) = row.generation_id.clone() else {
            continue;
        };
        let mut info = row.info;
        info.url = attachments::display_url(Path::new(&row.path), &info.mime_type, info.bytes);
        by_generation.entry(generation_id).or_default().push(info);
    }
    for message in messages.iter_mut() {
        if message.role != "user" {
            continue;
        }
        let Some(generation_id) = message.generation_id.as_deref() else {
            continue;
        };
        if let Some(images) = by_generation.get(generation_id) {
            message.attachments = Some(images.clone());
        }
    }
}

/// Re-reads a turn's images from disk so a retry reaches the model with the same pictures.
fn images_for_generation(db: &rusqlite::Connection, conversation_id: &str, generation_id: &str) -> Vec<Value> {
    storage::list_attachments(db, conversation_id)
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.generation_id.as_deref() == Some(generation_id))
        .filter_map(|row| {
            let bytes = std::fs::read(&row.path).ok()?;
            Some(json!({
                "name": row.info.name,
                "mimeType": row.info.mime_type,
                "data": attachments::encode_base64(&bytes),
                "path": row.path,
            }))
        })
        .collect()
}

fn validate_id(value: &str, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 100
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(format!("Invalid {label}"));
    }
    Ok(value.to_string())
}

fn validate_provider_id(value: &str) -> Result<String, String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 64
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err("Invalid provider id".into());
    }
    Ok(value)
}

fn validate_http_url(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.len() < 9 || value.len() > 2048 {
        return Err("Invalid base URL".into());
    }
    let lower = value.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://"))
        || value.chars().any(char::is_whitespace)
    {
        return Err("Base URL must be an absolute http or https URL".into());
    }
    Ok(value.to_string())
}

fn load_connection(
    db: &rusqlite::Connection,
    conversation: &ConversationInfo,
) -> Result<(ConnectionRow, String), String> {
    let selection = conversation
        .selection
        .as_ref()
        .ok_or_else(|| "Choose a model for this conversation first".to_string())?;
    if selection.model_id.trim().is_empty() {
        return Err("Choose a model for this conversation first".into());
    }
    let connection =
        storage::get_connection(db, &selection.connection_id).map_err(|error| error.to_string())?;
    if !connection.connected {
        return Err("That provider connection is not available".into());
    }
    Ok((connection, selection.model_id.clone()))
}

fn decrypt_secret(connection: &ConnectionRow) -> Result<Option<Secret>, String> {
    connection
        .secret
        .as_ref()
        .map(|secret| credentials::unprotect(secret))
        .transpose()
}

/// Best-effort wipe of the ephemeral key copy held in a JSON params object.
fn wipe_secret_param(params: &mut Value) {
    if let Some(Value::String(secret)) = params.get_mut("apiKey") {
        unsafe {
            secret.as_bytes_mut().fill(0);
        }
        secret.clear();
    }
}

fn title_from(text: &str) -> Option<String> {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return None;
    }
    let mut title = compact.chars().take(60).collect::<String>();
    if compact.chars().count() > 60 {
        title.push('…');
    }
    Some(title)
}

pub fn sanitize_error(message: &str) -> String {
    let flattened = message
        .chars()
        .map(|character| {
            if character == '\n' || character == '\r' || character == '\t' {
                ' '
            } else if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut redacted = String::new();
    let mut after_bearer = false;
    for word in flattened.split_whitespace() {
        let lower = word.to_ascii_lowercase();
        let looks_secret = lower.starts_with("sk-")
            || lower.starts_with("aiza")
            || lower.starts_with("gsk_")
            || after_bearer
            || (word.len() >= 40
                && word.chars().all(|character| {
                    character.is_ascii_alphanumeric()
                        || matches!(character, '-' | '_' | '.' | '~' | '+' | '/' | '=')
                }));
        if looks_secret {
            redacted.push_str("[redacted]");
        } else {
            redacted.push_str(word);
        }
        after_bearer = lower == "bearer" || lower.ends_with("bearer:");
        redacted.push(' ');
    }
    let redacted = redacted.trim();
    let value = if redacted.is_empty() {
        "The request failed".to_string()
    } else {
        redacted.to_string()
    };
    if value.chars().count() > MAX_ERROR_CHARS {
        let mut truncated = value.chars().take(MAX_ERROR_CHARS).collect::<String>();
        truncated.push('…');
        truncated
    } else {
        value
    }
}

fn parse_models(result: Value) -> Result<Vec<ProviderModelInfo>, String> {
    let value = result.get("models").cloned().unwrap_or(result);
    serde_json::from_value::<Vec<ProviderModelInfo>>(value)
        .map_err(|_| "Pi worker returned an invalid model list".to_string())
}

fn chat_bootstrap_inner(host: &mut Host) -> Result<ChatBootstrap, String> {
    let conversations = storage::list_conversations(&host.db).map_err(storage_error)?;
    let connections = storage::list_connections(&host.db).map_err(storage_error)?;
    let selected = crate::library::setting(&host.db, "active_conversation", "");
    let workspace = crate::library::setting(&host.db, "active_workspace", "general");
    let active = match conversations
        .iter()
        .find(|c| c.id == selected && !c.archived && c.workspace_id == workspace)
        .or_else(|| {
            conversations
                .iter()
                .find(|c| !c.archived && c.workspace_id == workspace)
        }) {
        Some(conversation) => Some(conversation_open_inner(host, conversation.id.clone())?),
        None => None,
    };
    Ok(ChatBootstrap {
        conversations,
        connections,
        active,
    })
}

fn conversation_create_inner(
    host: &mut Host,
    selection: Option<ModelSelection>,
) -> Result<ConversationSnapshot, String> {
    if let Some(selection) = &selection {
        validate_id(&selection.connection_id, "connection id")?;
        if selection.model_id.trim().is_empty() || selection.model_id.len() > 200 {
            return Err("Invalid model id".into());
        }
        storage::get_connection(&host.db, &selection.connection_id).map_err(storage_error)?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let _conversation =
        storage::create_conversation(&host.db, &id, "New conversation", selection.as_ref())
            .map_err(storage_error)?;
    let workspace = crate::library::setting(&host.db, "active_workspace", "general");
    host.db
        .execute(
            "UPDATE chat_conversations SET workspace_id=?2 WHERE id=?1",
            rusqlite::params![id, workspace],
        )
        .map_err(|e| e.to_string())?;
    conversation_open_inner(host, id)
}

fn conversation_open_inner(
    host: &mut Host,
    conversation_id: String,
) -> Result<ConversationSnapshot, String> {
    let conversation =
        storage::get_conversation(&host.db, &conversation_id).map_err(storage_error)?;
    host.db.execute("INSERT INTO settings(key,value) VALUES('active_conversation',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[&conversation_id]).map_err(|e|e.to_string())?;
    if !host.chat.has_active_for(&conversation_id) {
        if let Some(path) = &conversation.session_path {
            match crate::library::transcript(path) {
                Ok(messages) => {
                    crate::library::rebuild_projection(&mut host.db, &conversation_id, &messages)?;
                }
                Err(error) => {
                    let mut result = snapshot(&host.db, &host.chat, conversation)?;
                    result.error = Some(error);
                    return Ok(result);
                }
            }
        }
    }
    snapshot(&host.db, &host.chat, conversation)
}

fn conversation_select_model_inner(
    host: &mut Host,
    conversation_id: String,
    selection: ModelSelection,
) -> Result<ConversationInfo, String> {
    if host.chat.has_active_for(&conversation_id) {
        return Err("Stop the current response before changing models".into());
    }
    storage::get_connection(&host.db, &selection.connection_id).map_err(storage_error)?;
    if selection.model_id.trim().is_empty() || selection.model_id.len() > 200 {
        return Err("Invalid model id".into());
    }
    let conversation =
        storage::update_conversation_selection(&host.db, &conversation_id, &selection)
            .map_err(storage_error)?;
    // The Pi session must be re-opened with the new provider/model on the next prompt.
    host.chat.sessions.remove(&conversation_id);
    Ok(conversation)
}

/// Pi session thinking levels, mirroring the worker's allowlist.
const THINKING_LEVELS: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];

fn conversation_thinking_inner(
    host: &mut Host,
    conversation_id: String,
    level: String,
) -> Result<String, String> {
    let level = level.trim().to_ascii_lowercase();
    if !THINKING_LEVELS.contains(&level.as_str()) {
        return Err("Invalid thinking level".into());
    }
    if host.chat.has_active_for(&conversation_id) {
        return Err("Stop the current response before changing thinking".into());
    }
    let conversation =
        storage::get_conversation(&host.db, &conversation_id).map_err(storage_error)?;
    let (connection, model_id) = load_connection(&host.db, &conversation)?;
    let secret = decrypt_secret(&connection)?;
    host.chat.ensure_session(
        &mut host.db,
        &mut host.supervisor,
        &conversation,
        &connection,
        &model_id,
        secret.as_ref(),
    )?;
    let result = host
        .supervisor
        .request(
            "session.thinking",
            json!({"conversationId": &conversation_id, "level": &level}),
            SESSION_TIMEOUT,
        )
        .map_err(|error| {
            if error.is_timeout() {
                "Changing thinking timed out".to_string()
            } else {
                sanitize_error(&error.to_string())
            }
        })?;
    let effective = result
        .get("level")
        .and_then(Value::as_str)
        .ok_or_else(|| "Pi worker did not return a thinking level".to_string())?
        .to_string();
    if let Some(binding) = host.chat.sessions.get_mut(&conversation_id) {
        binding.thinking_level = Some(effective.clone());
    }
    Ok(effective)
}

/// An image the composer attached to a message. `data` is base64 without the data: prefix.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInput {
    pub mime_type: String,
    pub data: String,
    #[serde(default)]
    pub name: Option<String>,
}

fn chat_send_inner(
    app: &AppHandle,
    host: &mut Host,
    conversation_id: String,
    generation_id: String,
    text: String,
    attachments: Vec<AttachmentInput>,
) -> Result<(), String> {
    let Host {
        db,
        supervisor,
        chat,
        ..
    } = &mut *host;

    // The bundled Pi ModelRuntime and its ephemeral credentials are shared by all
    // conversations. Serialize work app-wide so Stop and credential changes stay immediate.
    if chat.any_active() {
        return Err(
            "A response is already in progress. Stop it before sending another message.".into(),
        );
    }
    if text.trim().is_empty() && attachments.is_empty() {
        return Err("Write a message first".into());
    }
    if text.len() > MAX_MESSAGE_BYTES {
        return Err("That message is too long".into());
    }
    // Validate before the generation row exists so a bad image never leaves a stuck turn.
    let validated = attachments::validate(
        attachments
            .iter()
            .map(|image| (image.mime_type.as_str(), image.data.as_str(), image.name.as_deref())),
    )?;
    let workspace = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("agent-workspace");
    let stored = attachments::store(&workspace, &conversation_id, validated)?;
    let generation_id = validate_id(&generation_id, "generation id")?;
    let conversation = storage::get_conversation(db, &conversation_id).map_err(storage_error)?;
    let (connection, model_id) = load_connection(db, &conversation)?;
    let title = if conversation.title == "New conversation" || conversation.title.is_empty() {
        title_from(&text).or_else(|| stored.first().map(|image| image.name.clone()))
    } else {
        None
    };
    let assistant_id = uuid::Uuid::new_v4().to_string();
    storage::start_generation(
        db,
        NewGeneration {
            id: &generation_id,
            conversation_id: &conversation_id,
            assistant_id: &assistant_id,
            text: &text,
            title: title.as_deref(),
        },
    )
    .map_err(storage_error)?;
    let rows: Vec<storage::NewAttachment> = stored
        .iter()
        .map(|image| storage::NewAttachment {
            id: image.id.clone(),
            conversation_id: conversation_id.clone(),
            generation_id: generation_id.clone(),
            name: image.name.clone(),
            mime_type: image.mime_type.clone(),
            bytes: image.bytes as i64,
            path: image.path.to_string_lossy().to_string(),
        })
        .collect();
    if let Err(error) = storage::insert_attachments(db, &rows) {
        return Err(storage_error(error));
    }
    chat.active.insert(
        generation_id.clone(),
        ActiveGeneration {
            conversation_id: conversation_id.clone(),
            state: GenerationState::Generating,
            sequence: 1,
            text: String::new(),
            reasoning: String::new(),
            assistant_id,
            cancel_requested: false,
        },
    );
    emit_generation(
        &app,
        &conversation_id,
        &generation_id,
        1,
        GenerationState::Generating,
        Some(String::new()),
        Some(String::new()),
        None,
        None,
    );

    let secret = match decrypt_secret(&connection) {
        Ok(secret) => secret,
        Err(error) => {
            chat.fail_live(db, &app, &generation_id, error);
            return Ok(());
        }
    };
    if let Err(error) = chat.ensure_session(
        db,
        supervisor,
        &conversation,
        &connection,
        &model_id,
        secret.as_ref(),
    ) {
        chat.fail_live(db, &app, &generation_id, error);
        return Ok(());
    }
    let params = json!({
        "conversationId": conversation_id,
        "generationId": generation_id,
        "text": text,
        "context": agent_context(db, &conversation, &text),
        "images": stored
            .iter()
            .map(|image| json!({
                "name": image.name,
                "mimeType": image.mime_type,
                "data": image.data,
                "path": image.path.to_string_lossy(),
            }))
            .collect::<Vec<_>>(),
    });
    match supervisor.request("session.prompt", params, PROMPT_TIMEOUT) {
        Ok(_) => {}
        // A worker that acknowledges prompts only at completion is still streaming events.
        Err(error) if error.is_timeout() => {}
        Err(error) => chat.fail_live(db, &app, &generation_id, error.to_string()),
    }
    Ok(())
}

fn chat_retry_inner(
    app: &AppHandle,
    host: &mut Host,
    conversation_id: String,
    generation_id: String,
) -> Result<(), String> {
    let Host {
        db,
        supervisor,
        chat,
        ..
    } = &mut *host;
    // Apply queued protocol events before re-arming the same generation id, so a late
    // terminal event from the previous attempt cannot complete the retry.
    for signal in supervisor.drain_signals() {
        chat.on_worker_signal(db, &app, signal);
    }
    supervisor.refresh();
    for lifecycle in supervisor.drain_lifecycle() {
        chat.on_lifecycle(db, &app, lifecycle);
    }
    let conversation = storage::get_conversation(db, &conversation_id).map_err(storage_error)?;
    if chat.any_active() {
        return Err("A response is already in progress. Stop it before retrying.".into());
    }
    let (connection, model_id) = load_connection(db, &conversation)?;
    let assistant_id = uuid::Uuid::new_v4().to_string();
    let plan = storage::begin_retry(db, &conversation_id, &generation_id, &assistant_id)
        .map_err(storage_error)?;
    let sequence = plan.generation.sequence;
    chat.active.insert(
        generation_id.clone(),
        ActiveGeneration {
            conversation_id: conversation_id.clone(),
            state: GenerationState::Generating,
            sequence: sequence + 1,
            text: String::new(),
            reasoning: String::new(),
            assistant_id: plan.assistant_id.clone(),
            cancel_requested: false,
        },
    );
    emit_generation(
        &app,
        &conversation_id,
        &generation_id,
        sequence + 1,
        GenerationState::Generating,
        Some(String::new()),
        Some(String::new()),
        None,
        None,
    );

    let secret = match decrypt_secret(&connection) {
        Ok(secret) => secret,
        Err(error) => {
            chat.fail_live(db, &app, &generation_id, error);
            return Ok(());
        }
    };
    let had_sqlite_session = conversation.session_path.is_some();
    if let Err(error) = chat.ensure_session(
        db,
        supervisor,
        &conversation,
        &connection,
        &model_id,
        secret.as_ref(),
    ) {
        chat.fail_live(db, &app, &generation_id, error);
        return Ok(());
    }
    // A retry after the first prompt never reached Pi must replay the user turn. Once a
    // durable session exists, Pi's own retry keeps context without duplicating it here.
    let reference_context =
        agent_context(db, &conversation, &plan.user_text);
    let params = if had_sqlite_session {
        json!({"conversationId": conversation_id, "generationId": generation_id, "context":reference_context, "images": images_for_generation(db, &conversation_id, &generation_id)})
    } else {
        json!({
            "conversationId": conversation_id,
            "generationId": generation_id,
            "text": plan.user_text,
            "context": reference_context,
            "images": images_for_generation(db, &conversation_id, &generation_id),
        })
    };
    let method = if had_sqlite_session {
        "session.retry"
    } else {
        "session.prompt"
    };
    match supervisor.request(method, params, PROMPT_TIMEOUT) {
        Ok(_) => {}
        Err(error) if error.is_timeout() => {}
        Err(error) => chat.fail_live(db, &app, &generation_id, error.to_string()),
    }
    Ok(())
}

fn chat_stop_inner(app: &AppHandle, host: &mut Host, generation_id: String) -> Result<(), String> {
    let Host {
        db,
        supervisor,
        chat,
        ..
    } = &mut *host;
    let Some(active) = chat.active.get(&generation_id) else {
        // Idempotent: a finished or already-cancelled generation needs no action.
        return Ok(());
    };
    if active.state == GenerationState::Cancelling {
        return Ok(());
    }
    let conversation_id = active.conversation_id.clone();
    let text = active.text.clone();
    let active_reasoning = active.reasoning.clone();
    let sequence = {
        let active = chat.active.get_mut(&generation_id).expect("checked above");
        active.state = GenerationState::Cancelling;
        active.cancel_requested = true;
        active.sequence += 1;
        active.sequence
    };
    let _ = storage::update_progress(
        db,
        &generation_id,
        sequence,
        "cancelling",
        &text,
        Some(active_reasoning.as_str()).filter(|reasoning| !reasoning.is_empty()),
    );
    emit_generation(
        &app,
        &conversation_id,
        &generation_id,
        sequence,
        GenerationState::Cancelling,
        Some(text.clone()),
        Some(active_reasoning),
        None,
        None,
    );
    // Cancellation is finalized locally even if the worker cannot acknowledge it. Any late
    // worker event is ignored because the generation is no longer in `active`.
    let _ = supervisor.request(
        "session.abort",
        json!({"generationId": generation_id}),
        std::time::Duration::from_secs(2),
    );
    chat.finish(
        db,
        &app,
        &generation_id,
        GenerationState::Cancelled,
        Some(text),
        None,
        None,
    );
    Ok(())
}

fn provider_list_inner(host: &mut Host) -> Result<Vec<ConnectionInfo>, String> {
    storage::list_connections(&host.db).map_err(storage_error)
}

/// Connect a provider after a real authenticated probe, then seal the key with DPAPI.
///
/// `baseUrl` is a host-side extension to the public chat contract (which only lists
/// providerId/name/apiKey/modelId). It is needed for OpenAI-compatible custom endpoints and
/// endpoint overrides; the UI's custom provider is currently a documented placeholder.
fn provider_connect_inner(
    host: &mut Host,
    provider_id: String,
    name: String,
    api_key: Option<String>,
    model_id: Option<String>,
    base_url: Option<String>,
) -> Result<ConnectionInfo, String> {
    if host.chat.any_active() {
        return Err("Stop the current response before changing provider connections".into());
    }
    let provider_id = validate_provider_id(&provider_id)?;
    let name = name.trim();
    let name = if name.is_empty() {
        provider_id.clone()
    } else {
        name.chars().take(100).collect::<String>()
    };
    let model_id = model_id
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty());
    if model_id.as_ref().is_some_and(|model| model.len() > 200) {
        return Err("Invalid model id".into());
    }
    let base_url = match base_url.map(|url| url.trim().to_string()) {
        Some(url) if !url.is_empty() => Some(validate_http_url(&url)?),
        _ => None,
    };
    if provider_id == "custom" && base_url.is_none() {
        return Err("A custom provider needs its OpenAI-compatible base URL".into());
    }
    let key = api_key
        .filter(|key| !key.trim().is_empty())
        .map(|key| Secret::new(key.trim().to_string()));
    if key.as_ref().is_some_and(|key| key.len() > 16_384) {
        return Err("That API key is too long".into());
    }
    if key.is_none() && provider_id != "custom" {
        return Err("An API key is required for this provider".into());
    }

    host.supervisor.ensure_worker()?;
    let mut params = json!({
        "provider": &provider_id,
        "model": model_id.as_deref(),
        "baseUrl": base_url.as_deref(),
    });
    if let Some(key) = key.as_ref().and_then(|key| key.as_str().ok()) {
        params["apiKey"] = Value::String(key.to_string());
    }
    let result = host
        .supervisor
        .request("provider.validate", params.clone(), SESSION_TIMEOUT);
    // The clone passed to the supervisor is wiped after serialization by the supervisor.
    wipe_secret_param(&mut params);
    let result = result.map_err(|error| {
        if error.is_timeout() {
            "Provider validation timed out".to_string()
        } else {
            sanitize_error(&error.to_string())
        }
    })?;
    if result.get("valid").and_then(Value::as_bool) == Some(false)
        || result.get("ok").and_then(Value::as_bool) == Some(false)
        || result.get("authenticated").and_then(Value::as_bool) == Some(false)
    {
        let message = result
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Provider authentication failed")
            .to_string();
        return Err(sanitize_error(&message));
    }
    let sealed = match &key {
        Some(key) => Some(credentials::protect(key.as_str()?.as_bytes())?),
        None => None,
    };
    let id = storage::upsert_connection(
        &mut host.db,
        &provider_id,
        &name,
        model_id.as_deref(),
        base_url.as_deref(),
        sealed.as_deref(),
    )
    .map_err(storage_error)?;
    // A reconnect may replace the key or endpoint. Existing Pi sessions hold the old
    // ephemeral credential, so force them to reopen on the next prompt.
    host.chat.sessions.clear();
    let row = storage::get_connection(&host.db, &id).map_err(storage_error)?;
    Ok(row.info())
}

fn provider_disconnect_inner(host: &mut Host, connection_id: String) -> Result<(), String> {
    if host.chat.any_active() {
        return Err("Stop the current response before changing provider connections".into());
    }
    storage::delete_connection(&host.db, &connection_id).map_err(storage_error)?;
    host.chat.sessions.clear();
    Ok(())
}

fn provider_models_inner(
    host: &mut Host,
    connection_id: String,
) -> Result<Vec<ProviderModelInfo>, String> {
    let connection = storage::get_connection(&host.db, &connection_id).map_err(storage_error)?;
    let secret = decrypt_secret(&connection)?;
    host.supervisor.ensure_worker()?;
    let mut params = json!({
        "provider": &connection.provider_id,
        "model": connection.model_id.as_deref(),
        "baseUrl": connection.base_url.as_deref(),
    });
    if let Some(secret) = secret.as_ref().and_then(|secret| secret.as_str().ok()) {
        params["apiKey"] = Value::String(secret.to_string());
    }
    let result = host
        .supervisor
        .request("models.list", params.clone(), SESSION_TIMEOUT);
    wipe_secret_param(&mut params);
    let result = result.map_err(|error| {
        if error.is_timeout() {
            "Listing models timed out".to_string()
        } else {
            sanitize_error(&error.to_string())
        }
    })?;
    parse_models(result)
}

#[tauri::command]
pub async fn chat_bootstrap(app: AppHandle) -> Result<ChatBootstrap, String> {
    run_blocking(app, |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        chat_bootstrap_inner(&mut host)
    })
    .await
}

#[tauri::command]
pub async fn conversation_create(
    app: AppHandle,
    selection: Option<ModelSelection>,
) -> Result<ConversationSnapshot, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        conversation_create_inner(&mut host, selection)
    })
    .await
}

#[tauri::command]
pub async fn conversation_open(
    app: AppHandle,
    conversation_id: String,
) -> Result<ConversationSnapshot, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        conversation_open_inner(&mut host, conversation_id)
    })
    .await
}

#[tauri::command]
pub async fn conversation_select_model(
    app: AppHandle,
    conversation_id: String,
    selection: ModelSelection,
) -> Result<ConversationInfo, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        conversation_select_model_inner(&mut host, conversation_id, selection)
    })
    .await
}

/// Apply a Pi session thinking level ('minimal'..'max') to the conversation's open session.
#[tauri::command]
pub async fn conversation_thinking(
    app: AppHandle,
    conversation_id: String,
    level: String,
) -> Result<String, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        conversation_thinking_inner(&mut host, conversation_id, level)
    })
    .await
}

/// `chat_send` is async so Pi startup, session opening and the prompt acknowledgement never
/// run on WebView2's dispatch thread. The host lock is held inside the blocking task only.
#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    conversation_id: String,
    generation_id: String,
    text: String,
    attachments: Option<Vec<AttachmentInput>>,
) -> Result<(), String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        chat_send_inner(
            app,
            &mut host,
            conversation_id,
            generation_id,
            text,
            attachments.unwrap_or_default(),
        )
    })
    .await
}

#[tauri::command]
pub async fn chat_retry(
    app: AppHandle,
    conversation_id: String,
    generation_id: String,
) -> Result<(), String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        chat_retry_inner(app, &mut host, conversation_id, generation_id)
    })
    .await
}

#[tauri::command]
pub async fn chat_stop(app: AppHandle, generation_id: String) -> Result<(), String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        chat_stop_inner(app, &mut host, generation_id)
    })
    .await
}

#[tauri::command]
pub async fn provider_list(app: AppHandle) -> Result<Vec<ConnectionInfo>, String> {
    run_blocking(app, |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        provider_list_inner(&mut host)
    })
    .await
}

#[tauri::command]
pub async fn provider_connect(
    app: AppHandle,
    provider_id: String,
    name: String,
    api_key: Option<String>,
    model_id: Option<String>,
    base_url: Option<String>,
) -> Result<ConnectionInfo, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        provider_connect_inner(&mut host, provider_id, name, api_key, model_id, base_url)
    })
    .await
}

#[tauri::command]
pub async fn provider_disconnect(app: AppHandle, connection_id: String) -> Result<(), String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        provider_disconnect_inner(&mut host, connection_id)
    })
    .await
}

#[tauri::command]
pub async fn provider_models(
    app: AppHandle,
    connection_id: String,
) -> Result<Vec<ProviderModelInfo>, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared
            .lock()
            .map_err(|_| "Talo state is unavailable".to_string())?;
        provider_models_inner(&mut host, connection_id)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_flattened_redacted_and_bounded() {
        assert_eq!(sanitize_error("plain failure"), "plain failure");
        assert!(!sanitize_error("Authorization: Bearer sk-test-secret").contains("sk-test-secret"));
        assert!(!sanitize_error("api key sk-abcdefghijklmnop failed").contains("abcdefghijklmnop"));
        let long = "x".repeat(900);
        assert!(sanitize_error(&long).chars().count() <= MAX_ERROR_CHARS + 1);
        assert_eq!(sanitize_error("\n\t "), "The request failed");
    }

    #[test]
    fn generation_states_round_trip() {
        for state in [
            GenerationState::Idle,
            GenerationState::Generating,
            GenerationState::Cancelling,
            GenerationState::Completed,
            GenerationState::Cancelled,
            GenerationState::Failed,
        ] {
            assert_eq!(GenerationState::parse(state.as_str()), state);
        }
    }

    #[test]
    fn titles_are_compact_and_bounded() {
        assert_eq!(
            title_from("  hello   world ").as_deref(),
            Some("hello world")
        );
        assert!(title_from(&"a".repeat(100)).unwrap().chars().count() <= 61);
        assert_eq!(title_from("   "), None);
    }

    #[test]
    fn provider_changes_are_rejected_while_a_generation_is_active() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../services/memory/schema.sql"))
            .unwrap();
        storage::init(&db).unwrap();
        let mut host = Host {
            integrations: crate::integrations::Integrations::default(),
            db,
            supervisor: Supervisor::new(
                std::path::PathBuf::from("missing-vendor"),
                std::env::temp_dir(),
            ),
            permissions: crate::permissions::Permissions::default(),
            chat: ChatState::default(),
            last_maintenance: std::time::Instant::now(),
            browser_approvals: std::collections::HashMap::new(),
        };
        host.chat.active.insert(
            "g1".into(),
            ActiveGeneration {
                conversation_id: "c1".into(),
                state: GenerationState::Generating,
                sequence: 1,
                text: String::new(),
                reasoning: String::new(),
                assistant_id: "a1".into(),
                cancel_requested: false,
            },
        );
        let error = provider_connect_inner(
            &mut host,
            "openai".into(),
            "Personal".into(),
            Some("sk-test".into()),
            None,
            None,
        )
        .unwrap_err();
        assert!(error.contains("Stop the current response"), "{error}");
        let error = provider_disconnect_inner(&mut host, "connection".into()).unwrap_err();
        assert!(error.contains("Stop the current response"), "{error}");
    }

    #[test]
    fn chat_event_wire_shape_matches_contract() {
        let event = ChatEvent::Generation {
            conversation_id: "c".into(),
            generation_id: "g".into(),
            sequence: 2,
            state: GenerationState::Generating,
            text: Some("hi".into()),
            reasoning: Some("hmm".into()),
            message: None,
            error: None,
        };
        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "generation");
        assert_eq!(value["conversationId"], "c");
        assert_eq!(value["generationId"], "g");
        assert_eq!(value["state"], "generating");
        assert_eq!(value["sequence"], 2);
        assert_eq!(value["reasoning"], "hmm");
        let worker = serde_json::to_value(ChatEvent::Worker {
            state: WorkerState::Ready,
            error: None,
        })
        .unwrap();
        assert_eq!(worker["type"], "worker");
        assert_eq!(worker["state"], "ready");
    }
}
