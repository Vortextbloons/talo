use crate::{automation, credentials, research, run_blocking, scheduler, telegram, Host, Shared};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use reqwest::blocking::Client;
use reqwest::Method;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{channel, Receiver, Sender, TryRecvError},
        Arc,
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const REVOKE_ENDPOINT: &str = "https://oauth2.googleapis.com/revoke";
const GMAIL: &str = "https://gmail.googleapis.com/gmail/v1/users/me";
const CALENDAR: &str = "https://www.googleapis.com/calendar/v3";
const DRIVE: &str = "https://www.googleapis.com/drive/v3";
const DRIVE_UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3";

const SCOPE_GMAIL_READ: &str = "https://www.googleapis.com/auth/gmail.readonly";
const SCOPE_GMAIL_COMPOSE: &str = "https://www.googleapis.com/auth/gmail.compose";
const SCOPE_CALENDAR: &str = "https://www.googleapis.com/auth/calendar.events";
const SCOPE_DRIVE_READ: &str = "https://www.googleapis.com/auth/drive.readonly";
const SCOPE_DRIVE_FILE: &str = "https://www.googleapis.com/auth/drive.file";

pub const SCOPES: [(&str, &str); 5] = [
    (SCOPE_GMAIL_READ, "Read email"),
    (SCOPE_GMAIL_COMPOSE, "Write drafts and send email"),
    (SCOPE_CALENDAR, "See and change calendar events"),
    (SCOPE_DRIVE_READ, "Find and read Drive files"),
    (SCOPE_DRIVE_FILE, "Create and update files Talo makes"),
];

const GMAIL_READ: [&str; 1] = [SCOPE_GMAIL_READ];
const GMAIL_COMPOSE: [&str; 1] = [SCOPE_GMAIL_COMPOSE];
const CALENDAR_EVENTS: [&str; 1] = [SCOPE_CALENDAR];
const DRIVE_READ: [&str; 1] = [SCOPE_DRIVE_READ];
const DRIVE_FILE: [&str; 1] = [SCOPE_DRIVE_FILE];

const MAX_RESULT_BYTES: usize = 14_000;
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(85);
const AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(300);
const READ_BUDGET: Duration = Duration::from_secs(45);
const WRITE_BUDGET: Duration = Duration::from_secs(25);

struct Operation {
    write: bool,
    scopes: &'static [&'static str],
}

fn operation(tool: &str) -> Option<Operation> {
    let (write, scopes) = match tool {
        "google_gmail_list" | "google_gmail_read" => (false, &GMAIL_READ[..]),
        "google_gmail_draft" | "google_gmail_send" => (true, &GMAIL_COMPOSE[..]),
        "google_calendar_list" | "google_calendar_read" => (false, &CALENDAR_EVENTS[..]),
        "google_calendar_create" | "google_calendar_update" | "google_calendar_delete" => {
            (true, &CALENDAR_EVENTS[..])
        }
        "google_drive_list" | "google_drive_read" => (false, &DRIVE_READ[..]),
        "google_drive_create" | "google_drive_update" => (true, &DRIVE_FILE[..]),
        _ => return None,
    };
    Some(Operation { write, scopes })
}

fn scope_label(scope: &str) -> &'static str {
    SCOPES
        .iter()
        .find(|(id, _)| *id == scope)
        .map(|(_, label)| *label)
        .unwrap_or("this Google permission")
}

fn now() -> i64 {
    Utc::now().timestamp()
}

pub fn init(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS google_connection(
           id TEXT PRIMARY KEY,
           account TEXT,
           scopes TEXT NOT NULL,
           sealed BLOB NOT NULL,
           needs_reconnect INTEGER NOT NULL DEFAULT 0,
           updated_at INTEGER NOT NULL
         );",
    )
    .map_err(|_| "Cannot prepare Google sign-in storage".to_string())
}

struct ClientConfig {
    client_id: String,
    client_secret: Option<String>,
}

struct Authorization {
    created: Instant,
    cancel: Arc<AtomicBool>,
}

struct Approval {
    request: String,
    tool: String,
    arguments: Value,
    summary: String,
    conversation: String,
    generation: String,
    created: Instant,
    epoch: u64,
}

struct Reply {
    id: String,
    result: Result<Value, String>,
}

struct Inflight {
    cancel: Arc<AtomicBool>,
    conversation: String,
    generation: String,
}

enum TokenUpdate {
    Fresh { epoch: u64, tokens: Value },
    Expired { epoch: u64 },
}

pub struct Google {
    authorization: Option<Authorization>,
    flow: Option<Receiver<Result<Value, String>>>,
    replies: Sender<Reply>,
    results: Receiver<Reply>,
    tokens: Sender<TokenUpdate>,
    refreshed: Receiver<TokenUpdate>,
    inflight: HashMap<String, Inflight>,
    approvals: HashMap<String, Approval>,
    epoch: Arc<AtomicU64>,
    owner: u64,
    pub error: Option<String>,
}

impl Default for Google {
    fn default() -> Self {
        let (replies, results) = channel();
        let (tokens, refreshed) = channel();
        Self {
            authorization: None,
            flow: None,
            replies,
            results,
            tokens,
            refreshed,
            inflight: HashMap::new(),
            approvals: HashMap::new(),
            epoch: Arc::new(AtomicU64::new(0)),
            owner: 0,
            error: None,
        }
    }
}

fn fail_pending(h: &mut Host, message: &str) {
    let approvals: Vec<Approval> = h.google.approvals.drain().map(|(_, approval)| approval).collect();
    for approval in approvals {
        reply_error(h, &approval.request, message);
    }
    h.google.interrupt();
}

impl Google {
    fn interrupt(&mut self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        for (_, inflight) in self.inflight.drain() {
            inflight.cancel.store(true, Ordering::SeqCst);
        }
    }

    fn reset(&mut self) {
        if let Some(authorization) = &self.authorization {
            authorization.cancel.store(true, Ordering::SeqCst);
        }
        self.authorization = None;
        self.flow = None;
        self.interrupt();
        self.approvals.clear();
        self.error = None;
    }
}

fn valid_client_id(value: &str) -> bool {
    value.len() >= 20
        && value.len() <= 256
        && value.ends_with(".apps.googleusercontent.com")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

fn valid_client_secret(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

const BUILT_IN_CLIENT: &str = include_str!(concat!(env!("OUT_DIR"), "/google-oauth.json"));

fn client_config() -> Result<ClientConfig, String> {
    installed_client(BUILT_IN_CLIENT)
        .ok_or_else(|| "This build of Talo has no Google app configuration.".to_string())
}

fn installed_client(text: &str) -> Option<ClientConfig> {
    if text.len() > 8192 {
        return None;
    }
    let value: Value = serde_json::from_str(text).ok()?;
    let app = value.get("installed").or_else(|| value.get("web"))?;
    let client_id = app["client_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| valid_client_id(value))?;
    let client_secret = app["client_secret"]
        .as_str()
        .map(str::trim)
        .filter(|value| valid_client_secret(value))
        .map(str::to_owned);
    Some(ClientConfig {
        client_id: client_id.to_string(),
        client_secret,
    })
}

struct ConnectionState {
    account: Option<String>,
    scopes: Vec<String>,
    tokens: Value,
    needs_reconnect: bool,
}

fn read_connection(db: &Connection) -> Result<Option<ConnectionState>, String> {
    let row: Option<(Option<String>, String, Vec<u8>, i64)> = db
        .query_row(
            "SELECT account,scopes,sealed,needs_reconnect FROM google_connection LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|_| "Cannot read the Google sign-in".to_string())?;
    let Some((account, scopes, sealed, needs_reconnect)) = row else {
        return Ok(None);
    };
    let secret = credentials::unprotect(&sealed)?;
    let tokens: Value = serde_json::from_str(secret.as_str()?)
        .map_err(|_| "The stored Google sign-in is damaged".to_string())?;
    Ok(Some(ConnectionState {
        account,
        scopes: scopes.split_whitespace().map(str::to_owned).collect(),
        tokens,
        needs_reconnect: needs_reconnect != 0,
    }))
}

fn write_connection(db: &Connection, tokens: &Value, account: Option<&str>) -> Result<(), String> {
    let scopes: String = tokens["scope"]
        .as_str()
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let secret = credentials::Secret::new(tokens.to_string());
    let sealed = credentials::protect(secret.as_str()?.as_bytes())?;
    let existing: Option<String> = db
        .query_row("SELECT id FROM google_connection LIMIT 1", [], |r| r.get(0))
        .optional()
        .map_err(|_| "Cannot read the Google sign-in".to_string())?;
    match existing {
        Some(id) => db.execute(
            "UPDATE google_connection SET account=COALESCE(?2,account),scopes=?3,sealed=?4,needs_reconnect=0,updated_at=?5 WHERE id=?1",
            params![id, account, scopes, sealed, now()],
        ),
        None => db.execute(
            "INSERT INTO google_connection(id,account,scopes,sealed,needs_reconnect,updated_at) VALUES(?1,?2,?3,?4,0,?5)",
            params![uuid::Uuid::new_v4().to_string(), account, scopes, sealed, now()],
        ),
    }
    .map_err(|_| "Cannot save the Google sign-in".to_string())?;
    Ok(())
}

fn force_refresh(db: &Connection) {
    let Ok(Some(mut connection)) = read_connection(db) else {
        return;
    };
    if let Some(object) = connection.tokens.as_object_mut() {
        object.insert("expires_at".into(), json!(0));
        let _ = write_connection(db, &connection.tokens, None);
    }
}

fn mark_reconnect(db: &Connection) {
    let _ = db.execute(
        "UPDATE google_connection SET needs_reconnect=1,updated_at=?1",
        params![now()],
    );
}

pub fn state(h: &Host) -> Value {
    let config = client_config();
    let connection = read_connection(&h.db).ok().flatten();
    let granted = connection
        .as_ref()
        .map(|c| c.scopes.clone())
        .unwrap_or_default();
    let scopes: Vec<Value> = SCOPES
        .iter()
        .map(|(id, label)| {
            json!({"id":id,"label":label,"granted":granted.iter().any(|value| value == id)})
        })
        .collect();
    let missing: Vec<&str> = SCOPES
        .iter()
        .filter(|(id, _)| !granted.iter().any(|value| value == id))
        .map(|(_, label)| *label)
        .collect();
    json!({
        "configured": config.is_ok(),
        "connected": connection.is_some(),
        "account": connection.as_ref().and_then(|c| c.account.clone()),
        "scopes": scopes,
        "missingPermissions": missing,
        "needsReconnect": connection.as_ref().is_some_and(|c| c.needs_reconnect),
        "authorizing": h.google.authorization.is_some(),
        "error": h.google.error.clone().or_else(|| {
            connection.as_ref().filter(|c| c.needs_reconnect)
                .map(|_| "Google sign-in expired. Connect Google again in Settings.".to_string())
        }),
    })
}

pub(crate) fn approvals(h: &Host) -> Value {
    json!(h
        .google
        .approvals
        .iter()
        .map(|(id, approval)| json!({
            "id": id,
            "tool": approval.tool,
            "summary": approval.summary,
            "arguments": approval.arguments,
            "conversationId": approval.conversation,
            "generationId": approval.generation,
        }))
        .collect::<Vec<_>>())
}

fn reply_error(h: &mut Host, id: &str, message: &str) {
    h.supervisor
        .relay_tools(json!({"id":id,"error":{"message":message}}));
}

fn reply_result(h: &mut Host, id: &str, value: Value) {
    h.supervisor.relay_tools(json!({"id":id,"result":value}));
}

pub fn message(h: &mut Host, app: &tauri::AppHandle, message: &Value) {
    let method = message["method"].as_str().unwrap_or("");
    if method == "notifications/cancelled" {
        let request = message["params"]["requestId"].as_str().unwrap_or("");
        if request.is_empty() {
            return;
        }
        if cancel_request(h, request) {
            let _ = app.emit("google-approvals", approvals(h));
        }
        return;
    }
    if method != "google/call" {
        return;
    }
    let request = message["id"].as_str().unwrap_or("").to_string();
    if request.is_empty() {
        return;
    }
    let p = &message["params"];
    let conversation = p["conversationId"].as_str().unwrap_or("").to_string();
    let generation = p["generationId"].as_str().unwrap_or("").to_string();
    let known = h
        .db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chat_conversations WHERE id=?1)",
            [&conversation],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !known {
        reply_error(h, &request, "Google actions require an open Talo chat.");
        return;
    }
    if !crate::integrations::live(h, message) {
        reply_error(h, &request, "This chat turn is no longer active.");
        return;
    }
    let Some(tool) = p["tool"].as_str() else {
        reply_error(h, &request, "Choose a Google operation.");
        return;
    };
    let Some(operation) = operation(tool) else {
        reply_error(h, &request, "Unknown Google operation.");
        return;
    };
    let arguments = match normalize(tool, &p["arguments"]) {
        Ok(value) => value,
        Err(error) => {
            reply_error(h, &request, &error);
            return;
        }
    };
    let summary = summary(tool, &arguments);
    if !operation.write {
        let _ = h.db.execute(
            "INSERT INTO permission_audit(id,action,resource,allowed) VALUES(?1,'google.read',?2,1)",
            params![uuid::Uuid::new_v4().to_string(), summary],
        );
        run_now(h, app, request, tool.to_string(), arguments, &conversation, &generation);
        return;
    }
    let attended = app
        .get_webview_window("main")
        .is_some_and(|window| window.is_visible().unwrap_or(false));
    let background = automation::run_for(h, &conversation, &generation);
    let remote = telegram::owns(h, &conversation, &generation);
    if !(attended || background.is_some() || remote) {
        reply_error(
            h,
            &request,
            "Open Talo to approve this Google change. Nothing was sent.",
        );
        return;
    }
    if h.google.approvals.len() >= 8 {
        reply_error(
            h,
            &request,
            "Too many Google approvals are waiting. Decide one in Talo first.",
        );
        return;
    }
    let id = uuid::Uuid::new_v4().to_string();
    h.google.approvals.insert(
        id,
        Approval {
            request,
            tool: tool.to_string(),
            arguments,
            summary,
            conversation,
            generation,
            created: Instant::now(),
            epoch: h.supervisor.worker_epoch(),
        },
    );
    if let Some(run) = background {
        automation::notify(
            app,
            &run,
            "A scheduled Google change needs your approval. Open Talo within 85 seconds.",
        );
    }
    let _ = app.emit("google-approvals", approvals(h));
}

fn audit(h: &Host, action: &str, summary: &str, allowed: bool) {
    let _ = h.db.execute(
        "INSERT INTO permission_audit(id,action,resource,allowed) VALUES(?1,?2,?3,?4)",
        params![
            uuid::Uuid::new_v4().to_string(),
            action,
            summary,
            i64::from(allowed)
        ],
    );
}

#[derive(Clone)]
struct Guard {
    cancel: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    issued: u64,
    deadline: Instant,
}

impl Guard {
    fn live(&self) -> bool {
        self.epoch.load(Ordering::SeqCst) == self.issued && !self.cancel.load(Ordering::SeqCst)
    }

    fn check(&self) -> Result<(), ApiFailure> {
        if !self.live() {
            return Err(ApiFailure::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(ApiFailure::Slow);
        }
        Ok(())
    }
}

struct Call {
    request: String,
    tool: String,
    arguments: Value,
    config: Option<ClientConfig>,
    tokens: Value,
    cancel: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    issued: u64,
}

type RefreshCall = Box<dyn FnOnce(&ClientConfig, &Value) -> Result<Value, Refresh> + Send>;
type RunCall = Box<dyn FnOnce(&Client, &str, &str, &Value, &Guard) -> Result<Value, ApiFailure> + Send>;

impl Call {
    fn guard(&self) -> Guard {
        Guard {
            cancel: Arc::clone(&self.cancel),
            epoch: Arc::clone(&self.epoch),
            issued: self.issued,
            deadline: Instant::now()
                + if operation(&self.tool).is_some_and(|operation| operation.write) {
                    WRITE_BUDGET
                } else {
                    READ_BUDGET
                },
        }
    }

    fn execute(
        self,
        refresh: RefreshCall,
        perform: RunCall,
        updates: Sender<TokenUpdate>,
    ) -> Result<Value, String> {
        let write = operation(&self.tool).is_some_and(|operation| operation.write);
        if !self.guard().live() {
            return Err(describe(ApiFailure::Cancelled, write));
        }
        let token = match self.config.as_ref() {
            Some(config) => match refresh(config, &self.tokens) {
                Ok(fresh) => {
                    let _ = updates.send(TokenUpdate::Fresh {
                        epoch: self.issued,
                        tokens: fresh.clone(),
                    });
                    fresh["access_token"]
                        .as_str()
                        .filter(|value| !value.is_empty())
                        .ok_or("The stored Google sign-in is damaged.")?
                        .to_string()
                }
                Err(Refresh::Expired) => {
                    let _ = updates.send(TokenUpdate::Expired { epoch: self.issued });
                    return Err("Google sign-in expired. Reconnect Google in Settings.".into());
                }
                Err(Refresh::Offline) => {
                    return Err(
                        "Google is unreachable right now. Check your connection and try again."
                            .into(),
                    )
                }
            },
            None => self.tokens["access_token"]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or("Google sign-in expired. Reconnect Google in Settings.")?
                .to_string(),
        };
        let token = credentials::Secret::new(token);
        let client = client()?;
        let guard = self.guard();
        guard.check().map_err(|failure| describe(failure, write))?;
        perform(
            &client,
            token.as_str()?,
            &self.tool,
            &self.arguments,
            &guard,
        )
        .map_err(|failure| describe(failure, write))
    }
}

fn prepare(
    h: &mut Host,
    request: String,
    tool: String,
    arguments: Value,
    conversation: &str,
    generation: &str,
) -> Result<(Call, Inflight), String> {
    let operation = operation(&tool).ok_or_else(|| "Unknown Google operation.".to_string())?;
    let connection = read_connection(&h.db)?
        .ok_or("Connect Google in Settings before using Gmail, Calendar or Drive.")?;
    if connection.needs_reconnect {
        return Err("Google sign-in expired. Reconnect Google in Settings.".into());
    }
    for scope in operation.scopes {
        if !connection.scopes.iter().any(|granted| granted == scope) {
            return Err(format!(
                "Google did not grant the permission to {}. Reconnect Google and allow it.",
                scope_label(scope)
            ));
        }
    }
    let stale = connection
        .tokens
        .get("expires_at")
        .and_then(Value::as_i64)
        .is_none_or(|expires| expires <= now() + 90);
    let config = if stale {
        Some(client_config()?)
    } else {
        connection
            .tokens
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or("Google sign-in expired. Reconnect Google in Settings.")?;
        None
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let call = Call {
        request,
        tool,
        arguments,
        config,
        tokens: connection.tokens,
        cancel: Arc::clone(&cancel),
        epoch: Arc::clone(&h.google.epoch),
        issued: h.google.epoch.load(Ordering::SeqCst),
    };
    let inflight = Inflight {
        cancel,
        conversation: conversation.to_string(),
        generation: generation.to_string(),
    };
    Ok((call, inflight))
}

fn dispatch(
    h: &mut Host,
    request: String,
    tool: String,
    arguments: Value,
    conversation: &str,
    generation: &str,
) -> Result<(), String> {
    dispatch_with(
        h,
        request,
        tool,
        arguments,
        conversation,
        generation,
        Box::new(refresh),
        Box::new(run),
    )
}

fn dispatch_with(
    h: &mut Host,
    request: String,
    tool: String,
    arguments: Value,
    conversation: &str,
    generation: &str,
    refresh: RefreshCall,
    perform: RunCall,
) -> Result<(), String> {
    let (call, inflight) = prepare(h, request, tool, arguments, conversation, generation)?;
    Ok(spawn_call(h, call, inflight, refresh, perform))
}

fn spawn_call(
    h: &mut Host,
    call: Call,
    inflight: Inflight,
    refresh: RefreshCall,
    perform: RunCall,
) {
    let request = call.request.clone();
    let name = call.tool.clone();
    let replies = h.google.replies.clone();
    let updates = h.google.tokens.clone();
    h.google.inflight.insert(request.clone(), inflight);
    std::thread::spawn(move || {
        let result = call
            .execute(refresh, perform, updates)
            .map(|value| {
                json!({
                    "content": [{"type": "text", "text": bound(value).to_string()}],
                    "details": {"tool": name},
                })
            });
        let _ = replies.send(Reply { id: request, result });
    });
}

fn run_now(
    h: &mut Host,
    app: &tauri::AppHandle,
    request: String,
    tool: String,
    arguments: Value,
    conversation: &str,
    generation: &str,
) {
    if let Err(error) = dispatch(h, request.clone(), tool, arguments, conversation, generation) {
        h.google.error = Some(error.clone());
        let _ = app.emit("google-changed", ());
        reply_error(h, &request, &error);
    }
}

fn cancel_request(h: &mut Host, request: &str) -> bool {
    if let Some(inflight) = h.google.inflight.remove(request) {
        inflight.cancel.store(true, Ordering::SeqCst);
        reply_error(h, request, "Google operation cancelled.");
    }
    let pending: Vec<String> = h
        .google
        .approvals
        .iter()
        .filter(|(_, approval)| approval.request == request)
        .map(|(id, _)| id.clone())
        .collect();
    for id in &pending {
        if let Some(approval) = h.google.approvals.remove(id) {
            reply_error(h, &approval.request, "Google operation cancelled.");
        }
    }
    !pending.is_empty()
}

fn clear_connection(h: &mut Host) -> Result<(bool, Option<String>), String> {
    let connection = read_connection(&h.db)?;
    let token = connection.as_ref().and_then(|connection| {
        connection.tokens["refresh_token"]
            .as_str()
            .filter(|value| !value.is_empty())
            .or_else(|| connection.tokens["access_token"].as_str())
            .map(str::to_owned)
    });
    h.db.execute("DELETE FROM google_connection", [])
        .map_err(|_| "Cannot remove the Google sign-in")?;
    fail_pending(h, "Google was disconnected. Nothing was sent.");
    h.google.reset();
    Ok((connection.is_some(), token))
}

fn apply_refresh(h: &mut Host, update: TokenUpdate) {
    let (epoch, tokens, expired) = match update {
        TokenUpdate::Fresh { epoch, tokens } => (epoch, Some(tokens), false),
        TokenUpdate::Expired { epoch } => (epoch, None, true),
    };
    if epoch != h.google.epoch.load(Ordering::SeqCst) {
        return;
    }
    let Some(mut connection) = read_connection(&h.db).ok().flatten() else {
        return;
    };
    if expired {
        mark_reconnect(&h.db);
        return;
    }
    let Some(mut tokens) = tokens else { return };
    if let (Some(stored), Some(fresh)) = (
        connection.tokens.as_object_mut(),
        tokens.as_object_mut(),
    ) {
        stored.extend(fresh.clone());
    }
    if write_connection(&h.db, &connection.tokens, None).is_ok() {
        h.google.error = None;
    }
}

pub fn awaiting(h: &Host, run: &str) -> bool {
    let owner: Option<(Option<String>, Option<String>)> = h
        .db
        .query_row(
            "SELECT conversation_id,generation_id FROM automation_runs WHERE id=?1",
            [run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .ok()
        .flatten();
    let Some((Some(conversation), Some(generation))) = owner else {
        return false;
    };
    h.google
        .approvals
        .values()
        .any(|approval| approval.conversation == conversation && approval.generation == generation)
}

pub fn tick(h: &mut Host, app: &tauri::AppHandle) {
    if let Some(flow) = h.google.flow.take() {
        match flow.try_recv() {
            Ok(result) => finish_authorization(h, app, result),
            Err(TryRecvError::Empty) => h.google.flow = Some(flow),
            Err(TryRecvError::Disconnected) => {
                h.google.authorization = None;
                let _ = app.emit("google-changed", ());
            }
        }
    }
    while let Ok(update) = h.google.refreshed.try_recv() {
        apply_refresh(h, update);
    }
    let mut finished = Vec::new();
    while let Ok(reply) = h.google.results.try_recv() {
        finished.push(reply);
    }
    if !finished.is_empty() {
        for reply in finished {
            if h.google.inflight.remove(&reply.id).is_none() {
                continue;
            }
            match reply.result {
                Ok(value) => reply_result(h, &reply.id, value),
                Err(message) => {
                    if message.starts_with("Google asked for a fresh sign-in") {
                        force_refresh(&h.db);
                    }
                    reply_error(h, &reply.id, &message);
                }
            }
        }
    }
    let epoch = h.supervisor.worker_epoch();
    if h.google.owner != epoch {
        h.google.owner = epoch;
        h.google.interrupt();
    }
    let stale: Vec<String> = h
        .google
        .inflight
        .iter()
        .filter(|(_, inflight)| {
            !crate::integrations::live(
                h,
                &json!({"params":{"conversationId":inflight.conversation,"generationId":inflight.generation}}),
            )
        })
        .map(|(id, _)| id.clone())
        .collect();
    for id in stale {
        if let Some(inflight) = h.google.inflight.remove(&id) {
            inflight.cancel.store(true, Ordering::SeqCst);
        }
    }
    let expired: Vec<String> = h
        .google
        .approvals
        .iter()
        .filter(|(_, approval)| {
            approval.epoch != epoch
                || approval.created.elapsed() > APPROVAL_TIMEOUT
                || !crate::integrations::live(
                    h,
                    &json!({"params":{"conversationId":approval.conversation,"generationId":approval.generation}}),
                )
        })
        .map(|(id, _)| id.clone())
        .collect();
    if !expired.is_empty() {
        for id in expired {
            if let Some(approval) = h.google.approvals.remove(&id) {
                reply_error(
                    h,
                    &approval.request,
                    "Google approval expired or the chat turn ended. Nothing was sent.",
                );
            }
        }
        let _ = app.emit("google-approvals", approvals(h));
    }
    if h
        .google
        .authorization
        .as_ref()
        .is_some_and(|authorization| authorization.created.elapsed() > AUTHORIZATION_TIMEOUT)
    {
        if let Some(authorization) = h.google.authorization.take() {
            authorization.cancel.store(true, Ordering::SeqCst);
        }
        h.google.flow = None;
        h.google.error = Some("Google sign-in timed out. Try again.".to_string());
        let _ = app.emit("google-changed", ());
    }
}

fn finish_authorization(h: &mut Host, app: &tauri::AppHandle, result: Result<Value, String>) {
    h.google.authorization = None;
    match result {
        Err(message) => h.google.error = Some(message),
        Ok(payload) => {
            let tokens = payload["tokens"].clone();
            let account = payload["account"].as_str();
            match write_connection(&h.db, &tokens, account) {
                Ok(()) => {
                    h.google.error = None;
                    let missing: Vec<&str> = SCOPES
                        .iter()
                        .filter(|(id, _)| {
                            !tokens["scope"]
                                .as_str()
                                .unwrap_or_default()
                                .split_whitespace()
                                .any(|granted| granted == *id)
                        })
                        .map(|(_, label)| *label)
                        .collect();
                    if !missing.is_empty() {
                        h.google.error = Some(format!(
                            "Google connected without {}. Reconnect and allow those to use every operation.",
                            missing.join(", ")
                        ));
                    }
                    fail_pending(h, "Google sign-in changed. Request and approve the change again.");
                    let _ = app.emit("google-approvals", approvals(h));
                }
                Err(error) => h.google.error = Some(error),
            }
        }
    }
    let _ = app.emit("google-changed", ());
}

enum Refresh {
    Expired,
    Offline,
}

fn apply_tokens(mut current: Value, response: &Value) -> Result<Value, String> {
    let access = response["access_token"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("Google did not return an access token")?
        .to_string();
    let expires = response["expires_in"]
        .as_i64()
        .unwrap_or(3600)
        .clamp(60, 86_400);
    let refresh = response["refresh_token"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| current["refresh_token"].as_str().map(str::to_owned));
    let object = current
        .as_object_mut()
        .ok_or("The stored Google sign-in is damaged")?;
    object.insert("access_token".into(), json!(access));
    object.insert("expires_at".into(), json!(now() + expires));
    if let Some(refresh) = refresh {
        object.insert("refresh_token".into(), json!(refresh));
    }
    for key in ["scope", "token_type"] {
        if let Some(value) = response[key].as_str().filter(|value| !value.is_empty()) {
            object.insert(key.into(), json!(value));
        }
    }
    Ok(current)
}

fn refresh(config: &ClientConfig, tokens: &Value) -> Result<Value, Refresh> {
    let refresh_token = tokens["refresh_token"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or(Refresh::Expired)?;
    let client = client().map_err(|_| Refresh::Offline)?;
    let mut form = vec![
        ("client_id", config.client_id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ];
    if let Some(secret) = config.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let (status, bytes) =
        post_form(&client, TOKEN_ENDPOINT, &form).map_err(|_| Refresh::Offline)?;
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if status != 200 {
        let error = value["error"].as_str().unwrap_or("");
        if ["invalid_grant", "invalid_client", "unauthorized_client"].contains(&error) {
            return Err(Refresh::Expired);
        }
        return Err(Refresh::Offline);
    }
    apply_tokens(tokens.clone(), &value).map_err(|_| Refresh::Expired)
}

fn random_token() -> String {
    let bytes: Vec<u8> = [
        uuid::Uuid::new_v4().into_bytes().as_slice(),
        uuid::Uuid::new_v4().into_bytes().as_slice(),
    ]
    .concat();
    base64(&bytes, true)
}

fn challenge(verifier: &str) -> String {
    base64(&Sha256::digest(verifier.as_bytes()), true)
}

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(6))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Cannot start the Google connection".to_string())
}

fn authorize_url(config: &ClientConfig, redirect: &str, state: &str, verifier: &str) -> String {
    let scopes = SCOPES
        .iter()
        .map(|(id, _)| *id)
        .collect::<Vec<_>>()
        .join(" ");
    let challenge = challenge(verifier);
    let params = [
        ("client_id", config.client_id.as_str()),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("scope", scopes.as_str()),
        ("state", state),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("include_granted_scopes", "true"),
    ];
    let query = params
        .iter()
        .map(|(key, value)| format!("{}={}", encode_component(key), encode_component(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{AUTH_ENDPOINT}?{query}")
}

fn begin(h: &mut Host, app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(authorization) = &h.google.authorization {
        if authorization.created.elapsed() < AUTHORIZATION_TIMEOUT {
            return Err("Google sign-in is already open in your browser.".into());
        }
    }
    let config = client_config()?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "Talo could not open a local port for Google sign-in".to_string())?;
    let port = listener
        .local_addr()
        .map_err(|_| "Talo could not read the local sign-in port".to_string())?
        .port();
    let redirect = format!("http://127.0.0.1:{port}");
    let state = random_token();
    let verifier = random_token();
    let url = authorize_url(&config, &redirect, &state, &verifier);
    let cancel = Arc::new(AtomicBool::new(false));
    let (sender, receiver) = channel();
    let thread_state = state.clone();
    let thread_cancel = Arc::clone(&cancel);
    let deadline = Instant::now() + AUTHORIZATION_TIMEOUT;
    std::thread::spawn(move || {
        let result = (|| -> Result<Value, String> {
            let code = wait_for_callback(&listener, &thread_cancel, &thread_state, deadline)?;
            let tokens = exchange(&config, &code, &verifier, &redirect)?;
            let account = tokens["access_token"]
                .as_str()
                .and_then(profile_email);
            Ok(json!({"tokens": tokens, "account": account}))
        })();
        let _ = sender.send(result);
    });
    h.google.authorization = Some(Authorization {
        created: Instant::now(),
        cancel,
    });
    h.google.flow = Some(receiver);
    h.google.error = None;
    if let Err(error) = research::open_external(&url) {
        if let Some(authorization) = h.google.authorization.take() {
            authorization.cancel.store(true, Ordering::SeqCst);
        }
        h.google.flow = None;
        return Err(error);
    }
    let _ = app.emit("google-changed", ());
    Ok(())
}

fn exchange(
    config: &ClientConfig,
    code: &str,
    verifier: &str,
    redirect: &str,
) -> Result<Value, String> {
    let client = client()?;
    let mut form = vec![
        ("client_id", config.client_id.as_str()),
        ("code", code),
        ("code_verifier", verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect),
    ];
    if let Some(secret) = config.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let (status, bytes) = post_form(&client, TOKEN_ENDPOINT, &form)
        .map_err(|_| "Google is unreachable. Check your connection and try again.".to_string())?;
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if status != 200 {
        let error = value["error"].as_str().unwrap_or("unknown_error");
        let detail = match error {
            "access_denied" => "the Google account did not grant access",
            "invalid_client" | "unauthorized_client" => {
                "the OAuth client ID or secret is not valid for this app"
            }
            "redirect_uri_mismatch" => "the OAuth client is not a Desktop app client",
            "invalid_grant" => "the sign-in code was already used or expired",
            _ => "Google rejected the sign-in",
        };
        return Err(format!(
            "Talo could not finish the Google sign-in: {detail}. Check the OAuth client in Settings."
        ));
    }
    apply_tokens(json!({}), &value)
        .map_err(|_| "Google did not return usable sign-in tokens".to_string())
}

fn profile_email(token: &str) -> Option<String> {
    let client = client().ok()?;
    let (status, bytes) = request(
        &client,
        Some(token),
        Method::GET,
        &format!("{GMAIL}/profile"),
        None,
        None,
    )
    .ok()?;
    let value = parse_status(status, &bytes).ok()?;
    value["emailAddress"]
        .as_str()
        .filter(|value| value.len() <= 320)
        .map(str::to_owned)
}

fn wait_for_callback(
    listener: &TcpListener,
    cancel: &AtomicBool,
    expected: &str,
    deadline: Instant,
) -> Result<String, String> {
    listener
        .set_nonblocking(true)
        .map_err(|_| "Talo could not listen for Google's response".to_string())?;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("Google sign-in was cancelled.".into());
        }
        if Instant::now() > deadline {
            return Err("Google sign-in timed out. Try again.".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(4)));
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 1024];
                while buffer.len() < 8192 {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(read) => {
                            buffer.extend_from_slice(&chunk[..read]);
                            if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let request = String::from_utf8_lossy(&buffer);
                let target = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("");
                let callback = parse_callback(target);
                if callback.state.as_deref() != Some(expected) {
                    respond(&mut stream, 400, "This Google response does not match Talo's sign-in request. Return to Talo and try again.");
                    continue;
                }
                if let Some(error) = callback.error {
                    respond(&mut stream, 200, "Sign-in was not completed. Close this tab and return to Talo.");
                    return Err(if error == "access_denied" {
                        "Google sign-in was cancelled.".into()
                    } else {
                        format!("Google could not complete the sign-in ({error}).")
                    });
                }
                match callback.code {
                    Some(code) if !code.is_empty() && code.len() <= 4096 => {
                        respond(&mut stream, 200, "Talo is connected to Google. Close this tab and return to Talo.");
                        return Ok(code);
                    }
                    _ => {
                        respond(&mut stream, 400, "This Google response had no sign-in code. Return to Talo and try again.");
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(120));
            }
            Err(_) => return Err("Talo could not listen for Google's response.".into()),
        }
    }
}

struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

fn parse_callback(target: &str) -> Callback {
    let query = target.split_once('?').map(|(_, query)| query).unwrap_or("");
    let mut callback = Callback {
        code: None,
        state: None,
        error: None,
    };
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let value = decode_component(value);
        match key {
            "code" => callback.code = Some(value),
            "state" => callback.state = Some(value),
            "error" => callback.error = Some(value),
            _ => {}
        }
    }
    callback
}

fn respond(stream: &mut std::net::TcpStream, status: u16, message: &str) {
    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Talo</title><body style=\"font:16px system-ui;padding:48px;max-width:34rem;margin:auto;color:#1d1d1b\"><h1 style=\"font-size:1.2rem\">Talo</h1><p>{message}</p></body>"
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn base64(bytes: &[u8], url_safe: bool) -> String {
    const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let table = if url_safe { URL } else { STANDARD };
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let second = *chunk.get(1).unwrap_or(&0) as u32;
        let third = *chunk.get(2).unwrap_or(&0) as u32;
        let value = ((chunk[0] as u32) << 16) | (second << 8) | third;
        out.push(table[(value >> 18) as usize & 63] as char);
        out.push(table[(value >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(table[(value >> 6) as usize & 63] as char);
        } else if !url_safe {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(table[value as usize & 63] as char);
        } else if !url_safe {
            out.push('=');
        }
    }
    out
}

fn base64_decode(value: &str) -> Option<Vec<u8>> {
    fn index(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some((byte - b'A') as u32),
            b'a'..=b'z' => Some((byte - b'a') as u32 + 26),
            b'0'..=b'9' => Some((byte - b'0') as u32 + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in value.bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() {
            continue;
        }
        let digit = index(byte)?;
        buffer = (buffer << 6) | digit;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

#[derive(Debug, PartialEq)]
enum ApiFailure {
    Auth,
    Missing,
    Rejected(String),
    Retry,
    Slow,
    Cancelled,
}

fn describe(failure: ApiFailure, write: bool) -> String {
    match failure {
        ApiFailure::Auth if write => {
            "Google asked for a fresh sign-in, so Talo did not apply this change. It refreshes on the next request."
                .into()
        }
        ApiFailure::Auth => {
            "Google asked for a fresh sign-in. Talo refreshes it on the next request.".into()
        }
        ApiFailure::Missing => {
            "Google denied this request. The connected account may not have the permission it needs."
                .into()
        }
        ApiFailure::Rejected(message) => message,
        ApiFailure::Retry if write => {
            "Talo could not confirm whether Google applied this change. Check Gmail, Calendar or Drive before trying again; Talo will not repeat it."
                .into()
        }
        ApiFailure::Retry => {
            "Google did not answer in time or dropped the connection. Try the request again.".into()
        }
        ApiFailure::Slow if write => {
            "Google did not answer quickly enough, so nothing was sent. Try again.".into()
        }
        ApiFailure::Slow => {
            "Google took too long to answer. Narrow the request and try again.".into()
        }
        ApiFailure::Cancelled if write => {
            "Talo stopped before sending this Google change. Nothing was changed.".into()
        }
        ApiFailure::Cancelled => "Google operation cancelled.".into(),
    }
}

fn reason(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    value["error"]["errors"][0]["reason"]
        .as_str()
        .or_else(|| value["error"]["status"].as_str())
        .map(str::to_owned)
}

fn request(
    client: &Client,
    token: Option<&str>,
    method: Method,
    url: &str,
    body: Option<(&str, Vec<u8>)>,
    guard: Option<&Guard>,
) -> Result<(u16, Vec<u8>), ApiFailure> {
    if let Some(guard) = guard {
        guard.check()?;
    }
    let mut builder = client.request(method, url);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if let Some((kind, bytes)) = body {
        builder = builder.header("content-type", kind).body(bytes);
    }
    let response = builder.send().map_err(|_| ApiFailure::Retry)?;
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    response
        .take(4_000_001)
        .read_to_end(&mut bytes)
        .map_err(|_| ApiFailure::Retry)?;
    if bytes.len() > 4_000_000 {
        return Err(ApiFailure::Rejected(
            "Google's response was larger than Talo can read. Narrow the request.".into(),
        ));
    }
    Ok((status, bytes))
}

fn parse_status(status: u16, bytes: &[u8]) -> Result<Value, ApiFailure> {
    if status == 401 {
        return Err(ApiFailure::Auth);
    }
    if status == 403 {
        return Err(match reason(bytes).as_deref() {
            Some("insufficientFilePermissions") => ApiFailure::Rejected(
                "Talo can only change files it created in Drive. Change other files in a browser."
                    .into(),
            ),
            Some("rateLimitExceeded") | Some("userRateLimitExceeded") | Some("quotaExceeded") => {
                ApiFailure::Retry
            }
            _ => ApiFailure::Missing,
        });
    }
    if status == 404 {
        return Err(ApiFailure::Rejected(
            "Google couldn't find that item. Check the details or search again.".into(),
        ));
    }
    if status == 429 || status >= 500 {
        return Err(ApiFailure::Retry);
    }
    if !(200..300).contains(&status) {
        return Err(ApiFailure::Rejected(match reason(bytes).as_deref() {
            Some("invalidArgument")
            | Some("badRequest")
            | Some("failedPrecondition")
            | Some("INVALID_ARGUMENT")
            | Some("FAILED_PRECONDITION") => {
                "Google rejected the request details. Check the dates, addresses and IDs.".into()
            }
            _ => format!("Google rejected the request (status {status})."),
        }));
    }
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(bytes)
        .map_err(|_| ApiFailure::Rejected("Google's response could not be read.".into()))
}

fn api_json(
    client: &Client,
    token: &str,
    method: Method,
    url: &str,
    guard: &Guard,
) -> Result<Value, ApiFailure> {
    let (status, bytes) = request(client, Some(token), method, url, None, Some(guard))?;
    parse_status(status, &bytes)
}

fn media(token: &str, url: &str, guard: &Guard) -> Result<(u16, Vec<u8>), ApiFailure> {
    let download = Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(6))
        .redirect(reqwest::redirect::Policy::limited(2))
        .build()
        .map_err(|_| ApiFailure::Retry)?;
    request(&download, Some(token), Method::GET, url, None, Some(guard))
}

fn post_form(
    client: &Client,
    url: &str,
    form: &[(&str, &str)],
) -> Result<(u16, Vec<u8>), ApiFailure> {
    let body = form
        .iter()
        .map(|(key, value)| format!("{}={}", encode_component(key), encode_component(value)))
        .collect::<Vec<_>>()
        .join("&");
    request(
        client,
        None,
        Method::POST,
        url,
        Some((
            "application/x-www-form-urlencoded",
            body.into_bytes(),
        )),
        None,
    )
}

fn text_field<'a>(args: &'a Value, key: &str, max: usize) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            if value.chars().count() > max {
                return Err(format!("{key} is longer than {max} characters."));
            }
            Ok(Some(value.as_str()))
        }
        Some(_) => Err(format!("{key} must be text.")),
    }
}

fn line(args: &Value, key: &str, max: usize) -> Result<Option<String>, String> {
    let Some(value) = text_field(args, key, max)? else {
        return Ok(None);
    };
    if value
        .chars()
        .any(|character| character.is_control() && character != '\t')
    {
        return Err(format!("{key} cannot contain line breaks or control characters."));
    }
    Ok(Some(value.trim().to_string()))
}

fn body(args: &Value, key: &str, max: usize) -> Result<Option<String>, String> {
    let Some(value) = text_field(args, key, max)? else {
        return Ok(None);
    };
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(format!("{key} contains characters Talo cannot send."));
    }
    Ok(Some(value.to_string()))
}

fn required(value: Option<String>, key: &str) -> Result<String, String> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{key} is required."))
}

fn identifier(args: &Value, key: &str) -> ResultOption {
    let value = line(args, key, 256)?.unwrap_or_default();
    if value.is_empty() {
        return Ok(None);
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'@'))
    {
        return Err(format!("{key} is not a valid Google identifier."));
    }
    Ok(Some(value))
}

type ResultOption = Result<Option<String>, String>;

fn count(args: &Value, key: &str, default: u64, min: u64, max: u64) -> Result<u64, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Number(number)) => number
            .as_u64()
            .filter(|value| (min..=max).contains(value))
            .ok_or_else(|| format!("{key} must be between {min} and {max}.")),
        Some(_) => Err(format!("{key} must be a number.")),
    }
}

fn addresses(args: &Value, key: &str) -> Result<Vec<String>, String> {
    let raw: Vec<String> = match args.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(value)) => value
            .split([',', ';'])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect(),
        Some(Value::Array(items)) => {
            let mut list = Vec::new();
            for item in items {
                let value = item
                    .as_str()
                    .ok_or_else(|| format!("{key} must be a list of addresses."))?;
                list.extend(
                    value
                        .split([',', ';'])
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned),
                );
            }
            list
        }
        Some(_) => return Err(format!("{key} must be an address or a list of addresses.")),
    };
    if raw.len() > 10 {
        return Err(format!("Use at most 10 {key} addresses."));
    }
    for address in &raw {
        if !valid_address(address) {
            return Err(format!("{key} contains an address Talo cannot use: {address}"));
        }
    }
    Ok(raw)
}

fn valid_address(value: &str) -> bool {
    if value.is_empty() || value.len() > 320 || value.contains('\r') || value.contains('\n') {
        return false;
    }
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    if local.is_empty()
        || local.len() > 64
        || !local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'%' | b'+' | b'-'))
        || domain.is_empty()
        || !domain.contains('.')
        || domain.starts_with('.')
        || domain.ends_with('.')
        || !domain
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
    {
        return false;
    }
    true
}

fn time_zone(args: &Value) -> Result<Option<String>, String> {
    let Some(zone) = line(args, "timeZone", 64)? else {
        return Ok(None);
    };
    if !zone
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'+' | b'-'))
    {
        return Err("timeZone must be an IANA time zone name.".into());
    }
    Ok(Some(zone))
}

fn event_time(args: &Value, key: &str, zone: Option<&str>) -> Result<Value, String> {
    let value = required(line(args, key, 40)?, key)?;
    if value.len() == 10 {
        NaiveDate::parse_from_str(&value, "%Y-%m-%d")
            .map_err(|_| format!("{key} must be a date like 2026-03-14 or a full timestamp."))?;
        return Ok(json!({"date": value}));
    }
    let parsed = DateTime::parse_from_rfc3339(&value)
        .map_err(|_| format!("{key} must be a date like 2026-03-14 or an RFC 3339 timestamp."))?;
    let mut result = Map::new();
    result.insert(
        "dateTime".into(),
        json!(parsed.to_rfc3339_opts(SecondsFormat::Secs, false)),
    );
    if let Some(zone) = zone {
        result.insert("timeZone".into(), json!(zone));
    }
    Ok(Value::Object(result))
}

fn normalized_arguments(tool: &str, args: &Value) -> Result<Value, String> {
    if !args.is_object() && !args.is_null() {
        return Err("Google tool arguments must be an object.".into());
    }
    let empty = Map::new();
    let args = args.as_object().unwrap_or(&empty);
    let args = Value::Object(args.clone());
    match tool {
        "google_gmail_list" => Ok(json!({
            "query": line(&args, "query", 300)?.unwrap_or_default(),
            "maxResults": count(&args, "maxResults", 8, 1, 10)?,
        })),
        "google_gmail_read" => Ok(json!({
            "messageId": identifier(&args, "messageId")?.ok_or("messageId is required.")?,
        })),
        "google_gmail_draft" | "google_gmail_send" => {
            let to = addresses(&args, "to")?;
            if to.is_empty() {
                return Err("Add at least one recipient.".into());
            }
            Ok(json!({
                "to": to,
                "cc": addresses(&args, "cc")?,
                "bcc": addresses(&args, "bcc")?,
                "subject": required(line(&args, "subject", 200)?, "subject")?,
                "body": required(body(&args, "body", 20_000)?, "body")?,
            }))
        }
        "google_calendar_list" => {
            let from = line(&args, "timeMin", 40)?;
            let to = line(&args, "timeMax", 40)?;
            let start = match from {
                Some(value) => DateTime::parse_from_rfc3339(&value)
                    .map_err(|_| "timeMin must be an RFC 3339 timestamp.".to_string())?,
                None => Utc::now().fixed_offset(),
            };
            let end = match to {
                Some(value) => DateTime::parse_from_rfc3339(&value)
                    .map_err(|_| "timeMax must be an RFC 3339 timestamp.".to_string())?,
                None => start + chrono::Duration::days(7),
            };
            if end <= start {
                return Err("timeMax must be after timeMin.".into());
            }
            Ok(json!({
                "calendarId": identifier(&args, "calendarId")?.unwrap_or_else(|| "primary".into()),
                "timeMin": start.to_rfc3339_opts(SecondsFormat::Secs, true),
                "timeMax": end.to_rfc3339_opts(SecondsFormat::Secs, true),
                "maxResults": count(&args, "maxResults", 10, 1, 20)?,
            }))
        }
        "google_calendar_read" | "google_calendar_delete" => Ok(json!({
            "calendarId": identifier(&args, "calendarId")?.unwrap_or_else(|| "primary".into()),
            "eventId": identifier(&args, "eventId")?.ok_or("eventId is required.")?,
        })),
        "google_calendar_create" => {
            let zone = time_zone(&args)?;
            let start = event_time(&args, "start", zone.as_deref())?;
            let end = event_time(&args, "end", zone.as_deref())?;
            if start.get("date").is_some() != end.get("date").is_some() {
                return Err("start and end must both be dates or both be timestamps.".into());
            }
            if let (Some(start), Some(end)) =
                (start["dateTime"].as_str(), end["dateTime"].as_str())
            {
                if end <= start {
                    return Err("end must be after start.".into());
                }
            }
            if let (Some(start), Some(end)) = (start["date"].as_str(), end["date"].as_str()) {
                if end < start {
                    return Err("end must be on or after start.".into());
                }
            }
            Ok(json!({
                "calendarId": identifier(&args, "calendarId")?.unwrap_or_else(|| "primary".into()),
                "summary": required(line(&args, "summary", 200)?, "summary")?,
                "description": body(&args, "description", 4000)?,
                "location": line(&args, "location", 300)?,
                "start": start,
                "end": end,
            }))
        }
        "google_calendar_update" => {
            let zone = time_zone(&args)?;
            let summary = line(&args, "summary", 200)?;
            let description = body(&args, "description", 4000)?;
            let location = line(&args, "location", 300)?;
            let has_start = args.get("start").is_some_and(|value| !value.is_null());
            let has_end = args.get("end").is_some_and(|value| !value.is_null());
            if has_start != has_end {
                return Err("Change both start and end together.".into());
            }
            let start = if has_start {
                Some(event_time(&args, "start", zone.as_deref())?)
            } else {
                None
            };
            let end = if has_end {
                Some(event_time(&args, "end", zone.as_deref())?)
            } else {
                None
            };
            if let Some((start, end)) = start.as_ref().zip(end.as_ref()) {
                if let (Some(start), Some(end)) =
                    (start["dateTime"].as_str(), end["dateTime"].as_str())
                {
                    if end <= start {
                        return Err("end must be after start.".into());
                    }
                }
                if let (Some(start), Some(end)) = (start["date"].as_str(), end["date"].as_str()) {
                    if end < start {
                        return Err("end must be on or after start.".into());
                    }
                }
            }
            if summary.is_none() && description.is_none() && location.is_none() && start.is_none() {
                return Err("Change at least one event detail.".into());
            }
            let mut value = json!({
                "calendarId": identifier(&args, "calendarId")?.unwrap_or_else(|| "primary".into()),
                "eventId": identifier(&args, "eventId")?.ok_or("eventId is required.")?,
            });
            let object = value.as_object_mut().unwrap();
            if let Some(summary) = summary {
                object.insert("summary".into(), json!(summary));
            }
            if let Some(description) = description {
                object.insert("description".into(), json!(description));
            }
            if let Some(location) = location {
                object.insert("location".into(), json!(location));
            }
            if let Some(start) = start {
                object.insert("start".into(), start);
            }
            if let Some(end) = end {
                object.insert("end".into(), end);
            }
            Ok(value)
        }
        "google_drive_list" => Ok(json!({
            "query": line(&args, "query", 500)?.unwrap_or_default(),
            "maxResults": count(&args, "maxResults", 10, 1, 20)?,
        })),
        "google_drive_read" => Ok(json!({
            "fileId": identifier(&args, "fileId")?.ok_or("fileId is required.")?,
        })),
        "google_drive_create" => Ok(json!({
            "name": required(line(&args, "name", 200)?, "name")?,
            "content": required(body(&args, "content", 20_000)?, "content")?,
            "mimeType": drive_mime(&line(&args, "mimeType", 100)?)?,
        })),
        "google_drive_update" => {
            let content = body(&args, "content", 20_000)?;
            let name = line(&args, "name", 200)?;
            if content.is_none() && name.is_none() {
                return Err("Change the file name or its contents.".into());
            }
            let mut value = json!({
                "fileId": identifier(&args, "fileId")?.ok_or("fileId is required.")?,
            });
            let object = value.as_object_mut().unwrap();
            if let Some(content) = content {
                object.insert("content".into(), json!(content));
            }
            if let Some(name) = name.filter(|value| !value.trim().is_empty()) {
                object.insert("name".into(), json!(name));
            }
            Ok(value)
        }
        _ => Err("Unknown Google operation.".into()),
    }
}

fn normalize(tool: &str, args: &Value) -> Result<Value, String> {
    let arguments = normalized_arguments(tool, args)?;
    if arguments.to_string().len() > 32_768 {
        return Err("These Google arguments are too large.".into());
    }
    Ok(arguments)
}

fn drive_mime(value: &Option<String>) -> Result<String, String> {
    let value = value.clone().unwrap_or_else(|| "text/plain".into());
    if ["text/plain", "text/markdown", "text/csv", "application/json"].contains(&value.as_str()) {
        Ok(value)
    } else {
        Err("Talo creates text, Markdown, CSV or JSON files in Drive.".into())
    }
}

fn summary(tool: &str, args: &Value) -> String {
    let subject = args["subject"].as_str().unwrap_or("");
    let recipients = |key: &str| {
        args[key]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "yourself".into())
    };
    match tool {
        "google_gmail_send" => format!("Send an email to {}: \"{}\"", recipients("to"), short(subject)),
        "google_gmail_draft" => format!("Save an email draft to {}: \"{}\"", recipients("to"), short(subject)),
        "google_calendar_create" => format!(
            "Create a calendar event: \"{}\"",
            short(args["summary"].as_str().unwrap_or(""))
        ),
        "google_calendar_update" => format!(
            "Change calendar event {}: \"{}\"",
            short(args["eventId"].as_str().unwrap_or("")),
            short(args["summary"].as_str().unwrap_or(""))
        ),
        "google_calendar_delete" => format!(
            "Delete calendar event {}",
            short(args["eventId"].as_str().unwrap_or(""))
        ),
        "google_drive_create" => format!(
            "Create a Drive file named \"{}\"",
            short(args["name"].as_str().unwrap_or(""))
        ),
        "google_drive_update" => format!(
            "Change the Drive file {}",
            short(args["fileId"].as_str().unwrap_or(""))
        ),
        "google_gmail_list" => format!(
            "Read Gmail messages{}",
            args["query"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(|value| format!(" matching \"{}\"", short(value)))
                .unwrap_or_default()
        ),
        "google_gmail_read" => format!(
            "Read Gmail message {}",
            short(args["messageId"].as_str().unwrap_or(""))
        ),
        "google_calendar_list" => "Read calendar events".to_string(),
        "google_calendar_read" => format!(
            "Read calendar event {}",
            short(args["eventId"].as_str().unwrap_or(""))
        ),
        "google_drive_list" => "Find Drive files".to_string(),
        "google_drive_read" => format!(
            "Read Drive file {}",
            short(args["fileId"].as_str().unwrap_or(""))
        ),
        _ => tool.replace('_', " "),
    }
}

fn short(value: &str) -> String {
    let value: String = value.chars().take(80).collect();
    value.replace(['\n', '\r'], " ")
}

fn bound(value: Value) -> Value {
    let text = value.to_string();
    if text.len() <= MAX_RESULT_BYTES {
        return value;
    }
    let preview: String = text.chars().take(12_000).collect();
    json!({"truncated":true,"note":"The result was too large. Narrow the request.","preview":preview})
}

fn gmail_url(message_id: &str, suffix: &str) -> String {
    format!(
        "{GMAIL}/messages/{}?{suffix}",
        encode_component(message_id)
    )
}

fn decode_header(value: &str) -> String {
    fn q_decode(text: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let bytes = text.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'_' => {
                    out.push(b' ');
                    index += 1;
                }
                b'=' if index + 2 < bytes.len() => {
                    let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                    match u8::from_str_radix(hex, 16) {
                        Ok(byte) => {
                            out.push(byte);
                            index += 3;
                        }
                        Err(_) => {
                            out.push(bytes[index]);
                            index += 1;
                        }
                    }
                }
                byte => {
                    out.push(byte);
                    index += 1;
                }
            }
        }
        out
    }
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("=?") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("?=") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let mut parts = after[..end].splitn(3, '?');
        let charset = parts.next().unwrap_or("").split('*').next().unwrap_or("").to_ascii_lowercase();
        let encoding = parts.next().unwrap_or("").to_ascii_uppercase();
        let text = parts.next().unwrap_or("");
        let decoded = match encoding.as_str() {
            "B" => base64_decode(text),
            "Q" => Some(q_decode(text)),
            _ => None,
        }
        .filter(|_| {
            matches!(
                charset.as_str(),
                "" | "utf-8" | "utf8" | "us-ascii" | "ascii" | "iso-8859-1" | "windows-1252"
            )
        });
        match decoded {
            Some(bytes) => out.push_str(&String::from_utf8_lossy(&bytes)),
            None => {
                out.push_str("=?");
                out.push_str(&after[..end + 2]);
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

fn header<'a>(message: &'a Value, name: &str) -> &'a str {
    message["payload"]["headers"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|header| header["name"].as_str().is_some_and(|value| value.eq_ignore_ascii_case(name)))
        .and_then(|header| header["value"].as_str())
        .unwrap_or("")
}

fn collect_text(part: &Value, plain: &mut String, html: &mut String, attachments: &mut Vec<Value>) {
    let mime = part["mimeType"].as_str().unwrap_or("");
    if let Some(filename) = part["filename"].as_str().filter(|name| !name.is_empty()) {
        if attachments.len() < 10 {
            attachments.push(json!({
                "filename": filename.chars().take(160).collect::<String>(),
                "mimeType": mime.chars().take(80).collect::<String>(),
                "size": part["body"]["size"],
            }));
        }
    }
    if let Some(data) = part["body"]["data"].as_str() {
        if let Some(bytes) = base64_decode(data) {
            let text = String::from_utf8_lossy(&bytes).to_string();
            if mime == "text/html" {
                html.push_str(&text);
            } else if mime == "text/plain" || mime.is_empty() {
                plain.push_str(&text);
            }
        }
    }
    for child in part["parts"].as_array().into_iter().flatten() {
        collect_text(child, plain, html, attachments);
    }
}

fn strip_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut inside = false;
    for character in value.chars() {
        match character {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(character),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn trim_text(value: &str, max: usize) -> String {
    let mut text = value.replace('\r', "");
    if text.chars().count() > max {
        text = text.chars().take(max).collect::<String>() + "…";
    }
    text
}

fn event_row(event: &Value) -> Value {
    let link = event["htmlLink"]
        .as_str()
        .filter(|value| value.starts_with("https://"))
        .unwrap_or("");
    json!({
        "id": event["id"],
        "summary": event["summary"].as_str().map(|v| trim_text(v, 200)),
        "status": event["status"],
        "start": event["start"],
        "end": event["end"],
        "location": event["location"].as_str().map(|v| trim_text(v, 200)),
        "htmlLink": link,
    })
}

fn run(
    client: &Client,
    token: &str,
    tool: &str,
    args: &Value,
    guard: &Guard,
) -> Result<Value, ApiFailure> {
    match tool {
        "google_gmail_list" => gmail_list(client, token, args, guard),
        "google_gmail_read" => gmail_read(client, token, args, guard),
        "google_gmail_draft" | "google_gmail_send" => gmail_write(client, token, tool, args, guard),
        "google_calendar_list" => calendar_list(client, token, args, guard),
        "google_calendar_read" => calendar_read(client, token, args, guard),
        "google_calendar_create" | "google_calendar_update" | "google_calendar_delete" => {
            calendar_write(client, token, tool, args, guard)
        }
        "google_drive_list" => drive_list(client, token, args, guard),
        "google_drive_read" => drive_read(client, token, args, guard),
        "google_drive_create" | "google_drive_update" => {
            drive_write(client, token, tool, args, guard)
        }
        _ => Err(ApiFailure::Rejected("Unknown Google operation.".into())),
    }
}

fn gmail_list(
    client: &Client,
    token: &str,
    args: &Value,
    guard: &Guard,
) -> Result<Value, ApiFailure> {
    let max = args["maxResults"].as_u64().unwrap_or(8) as usize;
    let query = args["query"].as_str().unwrap_or("");
    let mut url = format!("{GMAIL}/messages?maxResults={max}");
    if !query.is_empty() {
        url.push_str(&format!("&q={}", encode_component(query)));
    }
    let list = api_json(client, token, Method::GET, &url, guard)?;
    let mut messages = Vec::new();
    for entry in list["messages"].as_array().into_iter().flatten().take(max) {
        let id = entry["id"].as_str().unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let meta = api_json(
            client,
            token,
            Method::GET,
            &gmail_url(
                id,
                "format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date",
            ),
            guard,
        )?;
        messages.push(json!({
            "id": meta["id"],
            "threadId": meta["threadId"],
            "subject": trim_text(&decode_header(header(&meta, "Subject")), 200),
            "from": trim_text(&decode_header(header(&meta, "From")), 200),
            "date": trim_text(header(&meta, "Date"), 100),
            "snippet": trim_text(meta["snippet"].as_str().unwrap_or(""), 300),
        }));
    }
    Ok(json!({
        "messages": messages,
        "resultSizeEstimate": list["resultSizeEstimate"],
        "moreResults": list["nextPageToken"].is_string(),
    }))
}

fn gmail_read(
    client: &Client,
    token: &str,
    args: &Value,
    guard: &Guard,
) -> Result<Value, ApiFailure> {
    let id = args["messageId"].as_str().unwrap_or("");
    let message = api_json(
        client,
        token,
        Method::GET,
        &gmail_url(id, "format=full"),
        guard,
    )?;
    let mut plain = String::new();
    let mut html = String::new();
    let mut attachments = Vec::new();
    collect_text(&message["payload"], &mut plain, &mut html, &mut attachments);
    let text = if plain.trim().is_empty() {
        trim_text(&strip_html(&html), 10_000)
    } else {
        trim_text(&plain, 10_000)
    };
    Ok(json!({
        "id": message["id"],
        "threadId": message["threadId"],
        "subject": trim_text(&decode_header(header(&message, "Subject")), 200),
        "from": trim_text(&decode_header(header(&message, "From")), 200),
        "to": trim_text(&decode_header(header(&message, "To")), 300),
        "date": trim_text(header(&message, "Date"), 100),
        "snippet": trim_text(message["snippet"].as_str().unwrap_or(""), 300),
        "text": text,
        "attachments": attachments,
    }))
}

fn encode_header(value: &str) -> String {
    if value.is_ascii() {
        return value.to_string();
    }
    format!("=?UTF-8?B?{}?=", base64(value.as_bytes(), false))
}

fn rfc822(args: &Value) -> Vec<u8> {
    let list = |key: &str| {
        args[key]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    let mut message = String::new();
    message.push_str(&format!("To: {}\r\n", list("to")));
    for key in ["cc", "bcc"] {
        let value = list(key);
        if !value.is_empty() {
            message.push_str(&format!("{}: {value}\r\n", if key == "cc" { "Cc" } else { "Bcc" }));
        }
    }
    message.push_str(&format!(
        "Subject: {}\r\n",
        encode_header(args["subject"].as_str().unwrap_or(""))
    ));
    message.push_str("MIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n");
    let encoded = base64(
        args["body"].as_str().unwrap_or("").as_bytes(),
        false,
    );
    for chunk in encoded.as_bytes().chunks(76) {
        message.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        message.push_str("\r\n");
    }
    message.into_bytes()
}

fn gmail_write(
    client: &Client,
    token: &str,
    tool: &str,
    args: &Value,
    guard: &Guard,
) -> Result<Value, ApiFailure> {
    let raw = base64(&rfc822(args), true);
    let (url, body) = if tool == "google_gmail_draft" {
        (
            format!("{GMAIL}/drafts"),
            json!({"message":{"raw": raw}}),
        )
    } else {
        (
            format!("{GMAIL}/messages/send"),
            json!({"raw": raw}),
        )
    };
    let (status, bytes) = request(
        client,
        Some(token),
        Method::POST,
        &url,
        Some((
            "application/json",
            body.to_string().into_bytes(),
        )),
        Some(guard),
    )?;
    let value = parse_status(status, &bytes)?;
    if tool == "google_gmail_draft" {
        Ok(json!({
            "draftId": value["id"],
            "messageId": value["message"]["id"],
            "threadId": value["message"]["threadId"],
        }))
    } else {
        Ok(json!({
            "messageId": value["id"],
            "threadId": value["threadId"],
        }))
    }
}

fn calendar_list(
    client: &Client,
    token: &str,
    args: &Value,
    guard: &Guard,
) -> Result<Value, ApiFailure> {
    let calendar = args["calendarId"].as_str().unwrap_or("primary");
    let url = format!(
        "{CALENDAR}/calendars/{}/events?timeMin={}&timeMax={}&maxResults={}&singleEvents=true&orderBy=startTime",
        encode_component(calendar),
        encode_component(args["timeMin"].as_str().unwrap_or("")),
        encode_component(args["timeMax"].as_str().unwrap_or("")),
        args["maxResults"].as_u64().unwrap_or(10),
    );
    let value = api_json(client, token, Method::GET, &url, guard)?;
    let events: Vec<Value> = value["items"]
        .as_array()
        .into_iter()
        .flatten()
        .take(20)
        .map(event_row)
        .collect();
    Ok(json!({
        "calendarId": calendar,
        "events": events,
        "moreResults": value["nextPageToken"].is_string(),
    }))
}

fn calendar_read(
    client: &Client,
    token: &str,
    args: &Value,
    guard: &Guard,
) -> Result<Value, ApiFailure> {
    let url = format!(
        "{CALENDAR}/calendars/{}/events/{}",
        encode_component(args["calendarId"].as_str().unwrap_or("primary")),
        encode_component(args["eventId"].as_str().unwrap_or("")),
    );
    let event = api_json(client, token, Method::GET, &url, guard)?;
    let mut row = event_row(&event);
    let object = row.as_object_mut().unwrap();
    object.insert(
        "description".into(),
        json!(trim_text(event["description"].as_str().unwrap_or(""), 2000)),
    );
    object.insert("created".into(), event["created"].clone());
    object.insert("updated".into(), event["updated"].clone());
    Ok(row)
}

fn calendar_write(
    client: &Client,
    token: &str,
    tool: &str,
    args: &Value,
    guard: &Guard,
) -> Result<Value, ApiFailure> {
    let calendar = args["calendarId"].as_str().unwrap_or("primary");
    let base = format!(
        "{CALENDAR}/calendars/{}/events",
        encode_component(calendar)
    );
    let event_id = args["eventId"].as_str().unwrap_or("");
    if tool == "google_calendar_delete" {
        let url = format!("{base}/{}", encode_component(event_id));
        let (status, bytes) = request(client, Some(token), Method::DELETE, &url, None, Some(guard))?;
        parse_status(status, &bytes)?;
        return Ok(json!({"deleted": true, "eventId": event_id}));
    }
    let mut body = Map::new();
    for key in ["summary", "description", "location", "start", "end"] {
        if let Some(value) = args.get(key) {
            body.insert(key.into(), value.clone());
        }
    }
    let (url, method) = if tool == "google_calendar_create" {
        (base, Method::POST)
    } else {
        (
            format!("{base}/{}", encode_component(event_id)),
            Method::PATCH,
        )
    };
    let (status, bytes) = request(
        client,
        Some(token),
        method,
        &url,
        Some((
            "application/json",
            Value::Object(body).to_string().into_bytes(),
        )),
        Some(guard),
    )?;
    let event = parse_status(status, &bytes)?;
    Ok(event_row(&event))
}

fn drive_list(
    client: &Client,
    token: &str,
    args: &Value,
    guard: &Guard,
) -> Result<Value, ApiFailure> {
    let fields = "files(id,name,mimeType,modifiedTime,size,webViewLink),nextPageToken";
    let mut url = format!(
        "{DRIVE}/files?pageSize={}&orderBy=modifiedTime%20desc&fields={}",
        args["maxResults"].as_u64().unwrap_or(10),
        encode_component(fields),
    );
    let query = args["query"].as_str().unwrap_or("");
    if !query.is_empty() {
        url.push_str(&format!("&q={}", encode_component(query)));
    }
    let value = api_json(client, token, Method::GET, &url, guard)?;
    let files: Vec<Value> = value["files"]
        .as_array()
        .into_iter()
        .flatten()
        .take(20)
        .map(|file| {
            json!({
                "id": file["id"],
                "name": trim_text(file["name"].as_str().unwrap_or(""), 200),
                "mimeType": file["mimeType"],
                "modifiedTime": file["modifiedTime"],
                "size": file["size"],
                "webViewLink": file["webViewLink"].as_str().filter(|value| value.starts_with("https://")),
            })
        })
        .collect();
    Ok(json!({
        "files": files,
        "moreResults": value["nextPageToken"].is_string(),
    }))
}

fn export_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "application/vnd.google-apps.document" => Some("text/plain"),
        "application/vnd.google-apps.spreadsheet" => Some("text/csv"),
        "application/vnd.google-apps.presentation" => Some("text/plain"),
        _ => None,
    }
}

fn drive_read(
    client: &Client,
    token: &str,
    args: &Value,
    guard: &Guard,
) -> Result<Value, ApiFailure> {
    let id = args["fileId"].as_str().unwrap_or("");
    let encoded = encode_component(id);
    let meta = api_json(
        client,
        token,
        Method::GET,
        &format!(
            "{DRIVE}/files/{encoded}?fields=id,name,mimeType,size,modifiedTime,webViewLink"
        ),
        guard,
    )?;
    let mime = meta["mimeType"].as_str().unwrap_or("");
    let mut row = json!({
        "id": meta["id"],
        "name": trim_text(meta["name"].as_str().unwrap_or(""), 200),
        "mimeType": mime,
        "size": meta["size"],
        "modifiedTime": meta["modifiedTime"],
        "webViewLink": meta["webViewLink"].as_str().filter(|value| value.starts_with("https://")),
    });
    let text = if let Some(export) = export_mime(mime) {
        let (status, bytes) = media(
            token,
            &format!(
                "{DRIVE}/files/{encoded}/export?mimeType={}",
                encode_component(export)
            ),
            guard,
        )?;
        Some(media_text(status, &bytes)?)
    } else if mime.starts_with("text/") || mime == "application/json" {
        let (status, bytes) = media(token, &format!("{DRIVE}/files/{encoded}?alt=media"), guard)?;
        Some(media_text(status, &bytes)?)
    } else {
        None
    };
    match text {
        Some(text) => {
            row.as_object_mut()
                .unwrap()
                .insert("text".into(), json!(trim_text(&text, 10_000)));
        }
        None => {
            row.as_object_mut().unwrap().insert(
                "note".into(),
                json!("This is a binary file. Talo reads its details but does not download binary content."),
            );
        }
    }
    Ok(row)
}

fn multipart(metadata: &Value, content: Option<(&str, &[u8])>) -> (String, Vec<u8>) {
    let boundary = format!("talo{}", uuid::Uuid::new_v4().simple());
    let mut body = Vec::new();
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n")
            .as_bytes(),
    );
    if let Some((mime, bytes)) = content {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Type: {mime}; charset=UTF-8\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/related; boundary={boundary}"), body)
}

fn media_text(status: u16, bytes: &[u8]) -> Result<String, ApiFailure> {
    if (200..300).contains(&status) {
        return Ok(String::from_utf8_lossy(bytes).to_string());
    }
    Err(parse_status(status, bytes).err().unwrap_or(ApiFailure::Retry))
}

fn drive_write(
    client: &Client,
    token: &str,
    tool: &str,
    args: &Value,
    guard: &Guard,
) -> Result<Value, ApiFailure> {
    let fields = "id,name,mimeType,modifiedTime,webViewLink";
    let content = args["content"].as_str();
    if tool == "google_drive_create" {
        let mime = args["mimeType"].as_str().unwrap_or("text/plain");
        let metadata = json!({"name": args["name"], "mimeType": mime});
        let (kind, body) = multipart(&metadata, content.map(|text| (mime, text.as_bytes())));
        let (status, bytes) = request(
            client,
            Some(token),
            Method::POST,
            &format!("{DRIVE_UPLOAD}/files?uploadType=multipart&fields={}", encode_component(fields)),
            Some((&kind, body)),
            Some(guard),
        )?;
        let file = parse_status(status, &bytes)?;
        return Ok(json!({
            "id": file["id"],
            "name": file["name"],
            "mimeType": file["mimeType"],
            "webViewLink": file["webViewLink"].as_str().filter(|value| value.starts_with("https://")),
        }));
    }
    let id = args["fileId"].as_str().unwrap_or("");
    let encoded = encode_component(id);
    let (url, kind, body, method) = match content {
        Some(text) => {
            let mut metadata = Map::new();
            if let Some(name) = args["name"].as_str() {
                metadata.insert("name".into(), json!(name));
            }
            let (kind, body) = multipart(&Value::Object(metadata), Some(("text/plain", text.as_bytes())));
            (
                format!("{DRIVE_UPLOAD}/files/{encoded}?uploadType=multipart&fields={}", encode_component(fields)),
                kind,
                body,
                Method::PATCH,
            )
        }
        None => (
            format!("{DRIVE}/files/{encoded}?fields={}", encode_component(fields)),
            "application/json".to_string(),
            json!({"name": args["name"]}).to_string().into_bytes(),
            Method::PATCH,
        ),
    };
    let (status, bytes) = request(client, Some(token), method, &url, Some((&kind, body)), Some(guard))?;
    let file = parse_status(status, &bytes)?;
    Ok(json!({
        "id": file["id"],
        "name": file["name"],
        "mimeType": file["mimeType"],
        "modifiedTime": file["modifiedTime"],
        "webViewLink": file["webViewLink"].as_str().filter(|value| value.starts_with("https://")),
    }))
}

fn revoke_token(token: &str) -> bool {
    let Ok(client) = client() else { return false };
    post_form(&client, REVOKE_ENDPOINT, &[("token", token)])
        .map(|(status, _)| status == 200)
        .unwrap_or(false)
}

#[tauri::command]
pub async fn google_action(
    app: tauri::AppHandle,
    operation: String,
    payload: Option<Value>,
) -> Result<Value, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let payload = payload.unwrap_or(json!({}));
        match operation.as_str() {
            "state" => {
                let h = shared.lock().map_err(|_| "Google is unavailable")?;
                return Ok(state(&h));
            }
            "approvals" => {
                let h = shared.lock().map_err(|_| "Google is unavailable")?;
                return Ok(approvals(&h));
            }
            "connect" => {
                let mut h = shared.lock().map_err(|_| "Google is unavailable")?;
                begin(&mut h, app)?;
                return Ok(state(&h));
            }
            "cancel" => {
                let mut h = shared.lock().map_err(|_| "Google is unavailable")?;
                if let Some(authorization) = h.google.authorization.take() {
                    authorization.cancel.store(true, Ordering::SeqCst);
                }
                h.google.flow = None;
                let _ = app.emit("google-changed", ());
                return Ok(state(&h));
            }
            "disconnect" => {
                let (has_connection, token) = {
                    let mut h = shared.lock().map_err(|_| "Google is unavailable")?;
                    clear_connection(&mut h)?
                };
                let revoked = token.as_deref().map(revoke_token);
                let h = shared.lock().map_err(|_| "Google is unavailable")?;
                let _ = app.emit("google-changed", ());
                let _ = app.emit("google-approvals", json!([]));
                let mut result = state(&h);
                if has_connection && revoked != Some(true) {
                    result["error"] = json!(
                        "Disconnected on this PC. Google could not be reached to revoke the token automatically; revoke Talo in your Google Account permissions if you want that too."
                    );
                }
                return Ok(result);
            }
            "resolve" => {
                let mut h = shared.lock().map_err(|_| "Google is unavailable")?;
                let id = payload["id"].as_str().ok_or("Approval ID required")?.to_string();
                let allowed = payload["decision"].as_str() == Some("once");
                let epoch = h.supervisor.worker_epoch();
                let Some(approval) = h.google.approvals.remove(&id) else {
                    return Err("That Google approval already ended.".into());
                };
                let live = approval.created.elapsed() < APPROVAL_TIMEOUT
                    && approval.epoch == epoch
                    && crate::integrations::live(
                        &h,
                        &json!({"params":{"conversationId":approval.conversation,"generationId":approval.generation}}),
                    );
                audit(&h, "google.write", &approval.summary, allowed && live);
                let _ = app.emit("google-approvals", approvals(&h));
                if !allowed {
                    reply_error(&mut h, &approval.request, "Google change denied. Nothing was sent.");
                    return Ok(state(&h));
                }
                if !live {
                    reply_error(
                        &mut h,
                        &approval.request,
                        "Google approval expired or the chat turn ended. Nothing was sent.",
                    );
                    return Ok(state(&h));
                }
                if let Some(run) = automation::run_for(&h, &approval.conversation, &approval.generation) {
                    let scope = json!({"tool": approval.tool, "arguments": approval.arguments}).to_string();
                    if !scheduler::reserve_action(&h.db, &run, "google.write", &scope)
                        .unwrap_or(false)
                    {
                        reply_error(
                            &mut h,
                            &approval.request,
                            "This scheduled Google change already ran. Review the conversation.",
                        );
                        return Ok(state(&h));
                    }
                }
                run_now(
                    &mut h,
                    app,
                    approval.request,
                    approval.tool,
                    approval.arguments,
                    &approval.conversation,
                    &approval.generation,
                );
                return Ok(state(&h));
            }
            _ => Err("Unknown Google operation".into()),
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn arguments(tool: &str, value: Value) -> Result<Value, String> {
        normalize(tool, &value)
    }

    fn host() -> Host {
        let data = std::env::temp_dir().join(format!("talo-google-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&data);
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../services/memory/schema.sql"))
            .unwrap();
        crate::storage::init(&db).unwrap();
        init(&db).unwrap();
        crate::scheduler::init(&db).unwrap();
        Host {
            telegram: crate::telegram::Telegram::default(),
            google: Google::default(),
            integrations: crate::integrations::Integrations::default(),
            db,
            supervisor: crate::supervisor::Supervisor::new(data.clone(), data),
            permissions: crate::permissions::Permissions::default(),
            chat: crate::chat::ChatState::default(),
            last_maintenance: Instant::now(),
            browser_approvals: Default::default(),
            agent_asks: Default::default(),
        }
    }

    fn connected(h: &mut Host, scopes: &[&str], expires_at: i64) {
        write_connection(
            &h.db,
            &json!({
                "access_token": "access-value",
                "refresh_token": "refresh-value",
                "expires_at": expires_at,
                "scope": scopes.join(" "),
                "token_type": "Bearer"
            }),
            Some("alex@example.com"),
        )
        .unwrap();
    }

    #[test]
    fn pkce_and_state_are_url_safe_and_paired() {
        let verifier = random_token();
        assert!((43..=128).contains(&verifier.len()));
        assert!(verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_eq!(
            challenge(&verifier),
            base64(&Sha256::digest(verifier.as_bytes()), true)
        );
        assert_ne!(challenge(&verifier), verifier);
        assert_ne!(random_token(), random_token());

        let config = ClientConfig {
            client_id: "1234567890-abc.apps.googleusercontent.com".into(),
            client_secret: None,
        };
        let url = authorize_url(&config, "http://127.0.0.1:49152", "state-value", &verifier);
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains(
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A49152"
        ));
        assert!(url.contains(&format!(
            "code_challenge={}",
            challenge(&verifier)
        )));
        let scopes = url
            .split("scope=")
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .unwrap_or("");
        for (scope, _) in SCOPES {
            assert!(scopes.contains(&encode_component(scope)));
        }
    }

    #[test]
    fn base64_helpers_round_trip_both_alphabets() {
        for text in ["", "a", "ab", "abc", "hello world", "üñîçødé"] {
            let standard = base64(text.as_bytes(), false);
            assert_eq!(
                base64_decode(&standard).unwrap(),
                text.as_bytes(),
                "standard padding round trip"
            );
            let url = base64(text.as_bytes(), true);
            assert!(!url.contains(['+', '/', '=']));
            assert_eq!(base64_decode(&url).unwrap(), text.as_bytes());
        }
        assert_eq!(decode_component("a%2Fb+c%20d%zz"), "a/b c d%zz");
        assert_eq!(encode_component("a/b c"), "a%2Fb%20c");
    }

    #[test]
    fn callback_parses_code_state_and_errors() {
        let callback = parse_callback("/?code=4%2Fabc-def&state=xyz");
        assert_eq!(callback.code.as_deref(), Some("4/abc-def"));
        assert_eq!(callback.state.as_deref(), Some("xyz"));
        assert_eq!(callback.error, None);
        let denied = parse_callback("/?error=access_denied&state=xyz");
        assert_eq!(denied.error.as_deref(), Some("access_denied"));
        assert_eq!(denied.code, None);
        assert_eq!(parse_callback("/favicon.ico").state, None);
    }

    #[test]
    fn loopback_listener_ignores_a_racing_state_then_accepts_the_real_one() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = Arc::clone(&cancel);
        let worker = std::thread::spawn(move || {
            wait_for_callback(
                &listener,
                &thread_cancel,
                "expected-state",
                Instant::now() + Duration::from_secs(10),
            )
        });
        let mut response = String::new();
        std::net::TcpStream::connect(("127.0.0.1", port))
            .unwrap()
            .write_all(b"GET /?code=attacker&state=forged HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut attacker = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        attacker.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 400"));

        std::net::TcpStream::connect(("127.0.0.1", port))
            .unwrap()
            .write_all(b"GET /?code=real-code&state=expected-state HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), "real-code");
        cancel.store(true, Ordering::SeqCst);
    }

    #[test]
    fn cancellation_and_denied_callbacks_end_the_flow_without_a_code() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = Arc::clone(&cancel);
        let worker = std::thread::spawn(move || {
            wait_for_callback(
                &listener,
                &thread_cancel,
                "state",
                Instant::now() + Duration::from_secs(10),
            )
        });
        std::net::TcpStream::connect(("127.0.0.1", port))
            .unwrap()
            .write_all(b"GET /?error=access_denied&state=state HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        assert_eq!(worker.join().unwrap().unwrap_err(), "Google sign-in was cancelled.");

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let cancel = Arc::new(AtomicBool::new(true));
        assert_eq!(
            wait_for_callback(
                &listener,
                &cancel,
                "state",
                Instant::now() + Duration::from_secs(2)
            )
            .unwrap_err(),
            "Google sign-in was cancelled."
        );
    }

    #[test]
    fn token_merge_keeps_the_refresh_token_and_tracks_expiry() {
        let current = json!({
            "access_token":"old",
            "refresh_token":"refresh-value",
            "expires_at": 1,
            "scope":"https://www.googleapis.com/auth/gmail.readonly"
        });
        let merged = apply_tokens(
            current,
            &json!({
                "access_token":"new",
                "expires_in": 3599,
                "scope":"https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/calendar.events"
            }),
        )
        .unwrap();
        assert_eq!(merged["access_token"], "new");
        assert_eq!(merged["refresh_token"], "refresh-value");
        assert!(merged["expires_at"].as_i64().unwrap() > now() + 3000);
        assert!(merged["scope"].as_str().unwrap().contains("calendar.events"));
        assert!(apply_tokens(json!({}), &json!({"expires_in":10})).is_err());
        assert!(apply_tokens(json!({"access_token":"x"}), &json!({})).is_err());
    }

    #[test]
    fn writes_require_exact_approval_and_each_operation_declares_minimal_scopes() {
        assert!(operation("google_gmail_send").unwrap().write);
        assert!(operation("google_calendar_delete").unwrap().write);
        assert!(!operation("google_gmail_list").unwrap().write);
        assert!(!operation("google_calendar_read").unwrap().write);
        assert_eq!(
            operation("google_drive_create").unwrap().scopes,
            &DRIVE_FILE[..]
        );
        assert_eq!(
            operation("google_drive_list").unwrap().scopes,
            &DRIVE_READ[..]
        );
        assert!(operation("google_shell").is_none());
        let first = arguments(
            "google_gmail_send",
            json!({"to":"alex@example.com","subject":"Hello","body":"Body"}),
        )
        .unwrap();
        let second = arguments(
            "google_gmail_send",
            json!({"body":"Body","subject":"Hello","to":"alex@example.com"}),
        )
        .unwrap();
        assert_eq!(
            json!({"tool":"google_gmail_send","arguments":first}).to_string(),
            json!({"tool":"google_gmail_send","arguments":second}).to_string()
        );
    }

    #[test]
    fn message_arguments_reject_header_injection_and_unsafe_values() {
        let base = json!({"to":"alex@example.com","subject":"Report","body":"Line one\nLine two"});
        assert!(arguments("google_gmail_send", base.clone()).is_ok());
        let mut injected = base.clone();
        injected["subject"] = json!("Report\r\nBcc: attacker@example.com");
        assert!(arguments("google_gmail_send", injected).is_err());
        let mut newline_address = base.clone();
        newline_address["to"] = json!("alex@example.com\nBcc: attacker@example.com");
        assert!(arguments("google_gmail_send", newline_address).is_err());
        let mut too_many = base.clone();
        too_many["cc"] = json!((0..11)
            .map(|index| format!("person{index}@example.com"))
            .collect::<Vec<_>>());
        assert!(arguments("google_gmail_send", too_many).is_err());
        let mut bad_id = json!({"fileId":"../../etc/passwd"});
        assert!(arguments("google_drive_read", bad_id.clone()).is_err());
        bad_id["fileId"] = json!("1AbC_d-ef");
        assert!(arguments("google_drive_read", bad_id).is_ok());
        assert!(arguments(
            "google_drive_update",
            json!({"fileId":"1AbC_d-ef"})
        )
        .is_err());
        assert!(arguments(
            "google_drive_update",
            json!({"fileId":"1AbC_d-ef","content":"new text"})
        )
        .is_ok());
        assert!(arguments(
            "google_drive_create",
            json!({"name":"notes.txt","content":"hello","mimeType":"application/x-msdownload"})
        )
        .is_err());
    }

    #[test]
    fn calendar_dates_and_ranges_are_validated() {
        assert!(arguments(
            "google_calendar_create",
            json!({"summary":"Standup","start":"2026-03-14T09:00:00Z","end":"2026-03-14T09:30:00Z"})
        )
        .is_ok());
        assert!(arguments(
            "google_calendar_create",
            json!({"summary":"Holiday","start":"2026-03-14","end":"2026-03-15"})
        )
        .is_ok());
        assert!(arguments(
            "google_calendar_create",
            json!({"summary":"Backwards","start":"2026-03-14T10:00:00Z","end":"2026-03-14T09:00:00Z"})
        )
        .is_err());
        assert!(arguments(
            "google_calendar_create",
            json!({"summary":"Mixed","start":"2026-03-14","end":"2026-03-14T10:00:00Z"})
        )
        .is_err());
        assert!(arguments(
            "google_calendar_create",
            json!({"summary":"Nonsense","start":"tomorrow","end":"tomorrow"})
        )
        .is_err());
        assert!(arguments(
            "google_calendar_update",
            json!({"eventId":"abc_123","start":"2026-03-14T09:00:00Z"})
        )
        .is_err());
        assert!(arguments("google_calendar_list", json!({})).is_ok());
        assert!(arguments(
            "google_calendar_list",
            json!({"timeMin":"2026-03-14T09:00:00Z","timeMax":"2026-03-14T08:00:00Z"})
        )
        .is_err());
    }

    #[test]
    fn encoded_mail_headers_become_readable_text() {
        assert_eq!(decode_header("Quarterly report"), "Quarterly report");
        assert_eq!(
            decode_header("=?UTF-8?B?0J7RgtGH0ZHRgg==?="),
            "Отчёт"
        );
        assert_eq!(decode_header("=?utf-8?Q?Caf=C3=A9_r=C3=A9sum=C3=A9?="), "Café résumé");
        assert_eq!(
            decode_header("Re: =?UTF-8?B?0J7RgtGH0ZHRgg==?= today"),
            "Re: Отчёт today"
        );
        assert_eq!(decode_header("=?x-unknown?B?SGk=?="), "=?x-unknown?B?SGk=?=");
        assert_eq!(decode_header("unterminated =?UTF-8?B?"), "unterminated =?UTF-8?B?");
    }

    #[test]
    fn google_statuses_map_to_bounded_errors_and_empty_success_bodies() {
        assert_eq!(parse_status(204, b"").ok(), Some(Value::Null));
        assert_eq!(
            parse_status(200, br#"{"ok":true}"#).ok(),
            Some(json!({"ok":true}))
        );
        assert_eq!(parse_status(401, b"{}").err(), Some(ApiFailure::Auth));
        assert_eq!(
            parse_status(
                403,
                br#"{"error":{"errors":[{"reason":"insufficientFilePermissions"}]}}"#
            )
            .err(),
            Some(ApiFailure::Rejected(
                "Talo can only change files it created in Drive. Change other files in a browser."
                    .into()
            ))
        );
        assert_eq!(parse_status(429, b"{}").err(), Some(ApiFailure::Retry));
        assert_eq!(parse_status(500, b"").err(), Some(ApiFailure::Retry));
        assert!(matches!(
            parse_status(400, br#"{"error":{"status":"INVALID_ARGUMENT"}}"#).err(),
            Some(ApiFailure::Rejected(message)) if message.contains("request details")
        ));
    }

    #[test]
    fn result_budget_is_bounded() {
        let small = json!({"ok":true});
        assert_eq!(bound(small.clone()), small);
        let large = json!({"text":"x".repeat(40_000)});
        let bounded = bound(large);
        assert_eq!(bounded["truncated"], true);
        assert!(bounded["preview"].as_str().unwrap().chars().count() <= 12_000);
    }

    #[cfg(windows)]
    #[test]
    fn sealed_tokens_never_appear_in_plaintext() {
        let db = Connection::open_in_memory().unwrap();
        init(&db).unwrap();
        let tokens = json!({
            "access_token":"access-token-value",
            "refresh_token":"refresh-token-value",
            "expires_at": now() + 3600,
            "scope": format!("{SCOPE_GMAIL_READ} {SCOPE_CALENDAR}"),
            "token_type":"Bearer"
        });
        write_connection(&db, &tokens, Some("alex@example.com")).unwrap();
        let sealed: Vec<u8> = db
            .query_row("SELECT sealed FROM google_connection", [], |r| r.get(0))
            .unwrap();
        let text = String::from_utf8_lossy(&sealed);
        assert!(!text.contains("access-token-value"));
        assert!(!text.contains("refresh-token-value"));
        let stored = read_connection(&db).unwrap().unwrap();
        assert_eq!(stored.account.as_deref(), Some("alex@example.com"));
        assert_eq!(stored.tokens["refresh_token"], "refresh-token-value");
        assert!(stored.scopes.contains(&SCOPE_GMAIL_READ.to_string()));
        assert_eq!(stored.needs_reconnect, false);

        mark_reconnect(&db);
        assert!(read_connection(&db).unwrap().unwrap().needs_reconnect);
        db.execute("DELETE FROM google_connection", []).unwrap();
        assert!(read_connection(&db).unwrap().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn a_stale_access_token_is_recoverable_without_a_new_sign_in() {
        let db = Connection::open_in_memory().unwrap();
        init(&db).unwrap();
        write_connection(
            &db,
            &json!({
                "access_token":"stale",
                "refresh_token":"refresh-value",
                "expires_at": now() + 3600,
                "scope": SCOPE_GMAIL_READ
            }),
            None,
        )
        .unwrap();
        assert!(read_connection(&db).unwrap().unwrap().tokens["expires_at"].as_i64().unwrap() > now());
        force_refresh(&db);
        let stored = read_connection(&db).unwrap().unwrap();
        assert_eq!(stored.tokens["expires_at"], 0);
        assert_eq!(stored.tokens["refresh_token"], "refresh-value");
        assert!(!stored.needs_reconnect, "a stale token is not a revoked sign-in");
    }

    #[test]
    fn client_configuration_validation_is_strict() {
        assert!(valid_client_id("123-obc.apps.googleusercontent.com"));
        assert!(!valid_client_id("short"));
        assert!(!valid_client_id("123-obc.apps.googleusercontent.com/?x=1"));
        assert!(!valid_client_id("123-obc.example.com"));
        assert!(valid_client_secret("GOCSPX-abc_DEF-123"));
        assert!(!valid_client_secret("secret with spaces"));
        assert!(!valid_client_secret(""));
    }

    #[test]
    fn approvals_show_the_complete_normalized_arguments() {
        let mut h = host();
        let mail = "Long paragraph about the quarterly numbers. ".repeat(400);
        let files = "x".repeat(20_000);
        let cases = [
            (
                "google_gmail_send",
                json!({"to":"alex@example.com","subject":"Report","body":mail}),
                "body",
            ),
            (
                "google_calendar_create",
                json!({
                    "summary":"Review",
                    "start":"2026-03-14T09:00:00Z",
                    "end":"2026-03-14T10:00:00Z",
                    "description":"d".repeat(4_000)
                }),
                "description",
            ),
            (
                "google_drive_create",
                json!({"name":"notes.txt","content":files}),
                "content",
            ),
        ];
        for (tool, raw, key) in cases {
            let arguments = arguments(tool, raw).unwrap();
            let expected = arguments[key].as_str().unwrap().chars().count();
            h.google.approvals.insert(
                tool.to_string(),
                Approval {
                    request: format!("request-{tool}"),
                    tool: tool.to_string(),
                    summary: summary(tool, &arguments),
                    arguments: arguments.clone(),
                    conversation: "conversation-1".into(),
                    generation: "generation-1".into(),
                    created: Instant::now(),
                    epoch: 0,
                },
            );
            let shown = approvals(&h);
            let entry = shown
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["tool"] == tool)
                .cloned()
                .unwrap();
            assert_eq!(entry["arguments"], arguments);
            let text = entry["arguments"][key].as_str().unwrap();
            assert_eq!(text.chars().count(), expected);
            assert!(!text.contains('…'), "{tool} arguments were shortened");
        }
        assert!(approvals(&h).to_string().len() > 40_000);
    }

    #[test]
    fn google_write_failures_never_invite_a_blind_retry() {
        let uncertain = describe(ApiFailure::Retry, true);
        assert!(uncertain.contains("could not confirm"));
        assert!(!uncertain.contains("Retry or narrow"));
        assert!(describe(ApiFailure::Retry, false).contains("Try the request again"));
        assert!(describe(ApiFailure::Slow, true).contains("nothing was sent"));
        assert!(describe(ApiFailure::Cancelled, true).contains("Nothing was changed"));
        assert!(describe(ApiFailure::Auth, true).contains("did not apply this change"));
        assert!(describe(ApiFailure::Auth, true)
            .starts_with("Google asked for a fresh sign-in"));
    }

    #[test]
    fn the_operation_guard_stops_work_after_a_cancel_an_epoch_change_or_a_deadline() {
        let cancel = Arc::new(AtomicBool::new(false));
        let epoch = Arc::new(AtomicU64::new(4));
        let guard = Guard {
            cancel: Arc::clone(&cancel),
            epoch: Arc::clone(&epoch),
            issued: 4,
            deadline: Instant::now() + Duration::from_secs(30),
        };
        assert_eq!(guard.check(), Ok(()));
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(guard.check(), Err(ApiFailure::Cancelled));

        let guard = Guard {
            cancel: Arc::new(AtomicBool::new(false)),
            epoch: Arc::clone(&epoch),
            issued: 4,
            deadline: Instant::now() + Duration::from_secs(30),
        };
        epoch.fetch_add(1, Ordering::SeqCst);
        assert_eq!(guard.check(), Err(ApiFailure::Cancelled));

        let expired = Guard {
            cancel: Arc::new(AtomicBool::new(false)),
            epoch: Arc::clone(&epoch),
            issued: 5,
            deadline: Instant::now() - Duration::from_millis(1),
        };
        assert_eq!(expired.check(), Err(ApiFailure::Slow));
    }

    #[test]
    fn google_write_routing_belongs_to_its_own_conversation_and_turn() {
        let h = host();
        h.db.execute(
            "INSERT INTO automation_tasks(id,definition,next_at,paused,deleted) VALUES('task','{}',0,0,0)",
            [],
        )
        .unwrap();
        h.db.execute(
            "INSERT INTO automation_runs(id,task_id,scheduled_at,status,started_at,conversation_id,generation_id,definition) \
             VALUES('run','task',1,'running',0,'scheduled-chat','scheduled-turn','{}')",
            [],
        )
        .unwrap();
        assert_eq!(
            automation::run_for(&h, "scheduled-chat", "scheduled-turn").as_deref(),
            Some("run")
        );
        assert_eq!(automation::run_for(&h, "ordinary-chat", "ordinary-turn"), None);
        assert_eq!(
            automation::run_for(&h, "scheduled-chat", "later-turn"),
            None
        );

        let draft = json!({"tool":"google_gmail_draft","arguments":{"body":"same"}}).to_string();
        let send = json!({"tool":"google_gmail_send","arguments":{"body":"same"}}).to_string();
        assert!(scheduler::reserve_action(&h.db, "run", "google.write", &draft).unwrap());
        assert!(!scheduler::reserve_action(&h.db, "run", "google.write", &draft).unwrap());
        assert!(
            scheduler::reserve_action(&h.db, "run", "google.write", &send).unwrap(),
            "a draft and a send of identical text are different actions"
        );
    }

    #[test]
    fn a_scheduled_run_only_waits_for_an_approval_from_its_own_conversation() {
        let mut h = host();
        h.db.execute(
            "INSERT INTO automation_tasks(id,definition,next_at,paused,deleted) VALUES('task','{}',0,0,0)",
            [],
        )
        .unwrap();
        h.db.execute(
            "INSERT INTO automation_runs(id,task_id,scheduled_at,status,started_at,conversation_id,generation_id,definition) \
             VALUES('run','task',1,'running',0,'scheduled-chat','scheduled-turn','{}')",
            [],
        )
        .unwrap();
        assert!(!awaiting(&h, "run"));
        for (id, conversation, generation) in [
            ("scheduled", "scheduled-chat", "scheduled-turn"),
            ("ordinary", "ordinary-chat", "ordinary-turn"),
        ] {
            h.google.approvals.insert(
                id.to_string(),
                Approval {
                    request: format!("request-{id}"),
                    tool: "google_gmail_send".into(),
                    arguments: json!({}),
                    summary: "Send an email".into(),
                    conversation: conversation.into(),
                    generation: generation.into(),
                    created: Instant::now(),
                    epoch: 0,
                },
            );
        }
        assert!(awaiting(&h, "run"));
        h.google.approvals.remove("scheduled");
        assert!(
            !awaiting(&h, "run"),
            "another chat's approval must not keep a scheduled run waiting"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_build_reads_google_installed_app_client_json() {
        let downloaded = r#"{"installed":{"client_id":"1234567890-abc.apps.googleusercontent.com","project_id":"talo-1234","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token","client_secret":"GOCSPX-example-secret","redirect_uris":["http://localhost"]}}"#;
        let config = installed_client(downloaded).unwrap();
        assert_eq!(
            config.client_id,
            "1234567890-abc.apps.googleusercontent.com"
        );
        assert_eq!(config.client_secret.as_deref(), Some("GOCSPX-example-secret"));

        let without_secret = r#"{"installed":{"client_id":"1234567890-abc.apps.googleusercontent.com","client_secret":""}}"#;
        assert!(installed_client(without_secret)
            .unwrap()
            .client_secret
            .is_none());

        assert!(installed_client("null").is_none());
        assert!(installed_client("").is_none());
        assert!(installed_client("{}").is_none());
        assert!(installed_client(r#"{"installed":{"client_id":"not-a-google-client"}}"#).is_none());
        assert!(installed_client(&format!("{downloaded}{downloaded}")).is_none());
        if let Some(built) = installed_client(BUILT_IN_CLIENT) {
            assert!(valid_client_id(&built.client_id));
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_slow_refresh_does_not_hold_the_host_lock_and_cancellation_stops_the_write() {
        let shared = Arc::new(Mutex::new(host()));
        let arguments = arguments(
            "google_gmail_send",
            json!({"to":"alex@example.com","subject":"Report","body":"Hello"}),
        )
        .unwrap();
        let (started, refresh_started) = channel();
        let (release, release_refresh) = channel::<()>();
        let ran = Arc::new(AtomicBool::new(false));
        let ran_copy = Arc::clone(&ran);
        {
            let mut h = shared.lock().unwrap();
            connected(&mut h, &[SCOPE_GMAIL_COMPOSE], 0);
            let cancel = Arc::new(AtomicBool::new(false));
            let call = Call {
                request: "request-slow".into(),
                tool: "google_gmail_send".into(),
                arguments,
                config: Some(ClientConfig {
                    client_id: "1234567890-test.apps.googleusercontent.com".into(),
                    client_secret: Some("client-secret-value".into()),
                }),
                tokens: json!({"refresh_token":"refresh-value"}),
                cancel: Arc::clone(&cancel),
                epoch: Arc::clone(&h.google.epoch),
                issued: h.google.epoch.load(Ordering::SeqCst),
            };
            let inflight = Inflight {
                cancel,
                conversation: "conversation-1".into(),
                generation: "generation-1".into(),
            };
            spawn_call(
                &mut h,
                call,
                inflight,
                Box::new(move |_config, _tokens| {
                    let _ = started.send(());
                    let _ = release_refresh.recv();
                    Ok(json!({
                        "access_token":"fresh-access",
                        "refresh_token":"refresh-value",
                        "expires_at": now() + 3600,
                        "scope": SCOPE_GMAIL_COMPOSE
                    }))
                }),
                Box::new(move |_client, _token, _tool, _args, _guard| {
                    ran_copy.store(true, Ordering::SeqCst);
                    Ok(json!({"messageId":"m1"}))
                }),
            );
        }
        refresh_started
            .recv_timeout(Duration::from_secs(5))
            .expect("the refresh never started");
        let free = shared.try_lock();
        assert!(
            free.is_ok(),
            "the host lock stays held while a refresh waits on the network"
        );
        drop(free);
        {
            let mut h = shared.lock().unwrap();
            cancel_request(&mut h, "request-slow");
            assert!(h.google.inflight.is_empty());
        }
        release.send(()).unwrap();
        let reply = shared
            .lock()
            .unwrap()
            .google
            .results
            .recv_timeout(Duration::from_secs(5))
            .expect("the cancelled call never finished");
        assert_eq!(reply.id, "request-slow");
        assert!(
            !ran.load(Ordering::SeqCst),
            "a cancelled write must never reach Google"
        );
        assert!(reply.result.unwrap_err().contains("Nothing was changed"));
    }

    #[cfg(windows)]
    #[test]
    fn a_connection_change_stops_a_queued_write_before_google_sees_it() {
        let mut h = host();
        connected(&mut h, &[SCOPE_GMAIL_COMPOSE], now() + 3600);
        let arguments = arguments(
            "google_gmail_send",
            json!({"to":"alex@example.com","subject":"Report","body":"Hello"}),
        )
        .unwrap();
        let (call, _inflight) = prepare(
            &mut h,
            "request-queued".into(),
            "google_gmail_send".into(),
            arguments,
            "conversation-1",
            "generation-1",
        )
        .unwrap();
        assert!(call.config.is_none(), "a fresh access token needs no refresh");
        h.google.interrupt();
        let ran = Arc::new(AtomicBool::new(false));
        let ran_copy = Arc::clone(&ran);
        let (updates, _refreshed) = channel();
        let result = call.execute(
            Box::new(|_, _| unreachable!("a fresh token must not refresh")),
            Box::new(move |_client, _token, _tool, _args, _guard| {
                ran_copy.store(true, Ordering::SeqCst);
                Ok(json!({}))
            }),
            updates,
        );
        assert!(
            !ran.load(Ordering::SeqCst),
            "an operation prepared for the old connection must not run"
        );
        assert!(result.unwrap_err().contains("Nothing was changed"));
    }

    #[cfg(windows)]
    #[test]
    fn disconnecting_cancels_queued_work_before_any_revocation() {
        let mut h = host();
        connected(&mut h, &[SCOPE_GMAIL_COMPOSE], now() + 3600);
        let arguments = arguments(
            "google_gmail_send",
            json!({"to":"alex@example.com","subject":"Report","body":"Hello"}),
        )
        .unwrap();
        let (call, inflight) = prepare(
            &mut h,
            "request-queued".into(),
            "google_gmail_send".into(),
            arguments,
            "conversation-1",
            "generation-1",
        )
        .unwrap();
        h.google.inflight.insert("request-queued".into(), inflight);
        h.google.approvals.insert(
            "approval".into(),
            Approval {
                request: "request-queued".into(),
                tool: "google_gmail_send".into(),
                arguments: json!({}),
                summary: "Send an email".into(),
                conversation: "conversation-1".into(),
                generation: "generation-1".into(),
                created: Instant::now(),
                epoch: 0,
            },
        );
        let (has_connection, token) = clear_connection(&mut h).unwrap();
        assert!(has_connection);
        assert_eq!(token.as_deref(), Some("refresh-value"));
        assert!(read_connection(&h.db).unwrap().is_none());
        assert!(h.google.inflight.is_empty());
        assert!(h.google.approvals.is_empty());
        let ran = Arc::new(AtomicBool::new(false));
        let ran_copy = Arc::clone(&ran);
        let (updates, _refreshed) = channel();
        let result = call.execute(
            Box::new(|_, _| unreachable!("a fresh token must not refresh")),
            Box::new(move |_client, _token, _tool, _args, _guard| {
                ran_copy.store(true, Ordering::SeqCst);
                Ok(json!({}))
            }),
            updates,
        );
        assert!(
            !ran.load(Ordering::SeqCst),
            "queued work cannot run while the token revocation is still in flight"
        );
        assert!(result.unwrap_err().contains("Nothing was changed"));
    }

    #[cfg(windows)]
    #[test]
    fn reconnecting_discards_changes_approved_for_the_previous_account() {
        let mut h = host();
        connected(&mut h, &[SCOPE_GMAIL_COMPOSE], now() + 3600);
        let epoch = h.google.epoch.load(Ordering::SeqCst);
        h.google.approvals.insert("approval".into(), Approval {
            request: "request".into(),
            tool: "google_gmail_send".into(),
            arguments: json!({"to":["alex@example.com"],"subject":"Report","body":"Original account"}),
            summary: "Send a message".into(),
            conversation: "conversation".into(),
            generation: "generation".into(),
            created: Instant::now(),
            epoch: h.supervisor.worker_epoch(),
        });
        let app = tauri::Builder::default()
            .any_thread()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        finish_authorization(&mut h, app.handle(), Ok(json!({
            "tokens":{
                "access_token":"new-account-access",
                "refresh_token":"new-account-refresh",
                "expires_at":now() + 3600,
                "scope":SCOPE_GMAIL_COMPOSE
            },
            "account":"new@example.com"
        })));
        assert!(h.google.approvals.is_empty(), "an old account's change must require a new request and approval");
        apply_refresh(&mut h, TokenUpdate::Fresh { epoch, tokens: json!({
            "access_token":"old-account-refresh",
            "scope":SCOPE_GMAIL_COMPOSE
        }) });
        let connection = read_connection(&h.db).unwrap().unwrap();
        assert_eq!(connection.account.as_deref(), Some("new@example.com"));
        assert_eq!(connection.tokens["access_token"], "new-account-access");
    }

    #[cfg(windows)]
    #[test]
    fn an_obsolete_refresh_cannot_overwrite_a_newer_connection() {
        let mut h = host();
        connected(&mut h, &[SCOPE_GMAIL_COMPOSE], now() + 3600);
        let original = h.google.epoch.load(Ordering::SeqCst);
        h.google.interrupt();
        apply_refresh(
            &mut h,
            TokenUpdate::Fresh {
                epoch: original,
                tokens: json!({
                    "access_token":"stale-access",
                    "expires_at": now() + 3600,
                    "scope": SCOPE_GMAIL_COMPOSE
                }),
            },
        );
        assert_eq!(
            read_connection(&h.db).unwrap().unwrap().tokens["access_token"],
            "access-value",
            "a refresh from the replaced connection must not be stored"
        );

        let current = h.google.epoch.load(Ordering::SeqCst);
        apply_refresh(
            &mut h,
            TokenUpdate::Fresh {
                epoch: current,
                tokens: json!({
                    "access_token":"rotated-access",
                    "refresh_token":"rotated-refresh",
                    "expires_at": now() + 3600,
                    "scope": SCOPE_GMAIL_COMPOSE
                }),
            },
        );
        let stored = read_connection(&h.db).unwrap().unwrap();
        assert_eq!(stored.tokens["access_token"], "rotated-access");
        assert_eq!(stored.tokens["refresh_token"], "rotated-refresh");

        h.db.execute("DELETE FROM google_connection", []).unwrap();
        apply_refresh(
            &mut h,
            TokenUpdate::Fresh {
                epoch: current,
                tokens: json!({
                    "access_token":"late-access",
                    "expires_at": now() + 3600,
                    "scope": SCOPE_GMAIL_COMPOSE
                }),
            },
        );
        assert!(
            read_connection(&h.db).unwrap().is_none(),
            "a late refresh must not recreate a disconnected account"
        );
    }
}
