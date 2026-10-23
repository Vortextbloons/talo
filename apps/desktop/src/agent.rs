




use crate::{library, run_blocking, Host, Shared};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};


pub const DAILY_OBJECT_LIMIT: i64 = 5;

pub const ASK_TIMEOUT: Duration = Duration::from_secs(300);
pub const MAX_PENDING_ASKS: usize = 4;
pub const MAX_ASK_QUESTIONS: usize = 3;
pub const CAPABILITIES: [&str; 4] = ["web", "browser", "files", "google"];
pub const MEMORY_SCOPES: [&str; 3] = ["inherit", "workspace", "off"];



pub struct PendingAsk {
    pub call: Value,
    pub ask: Value,
    pub at: Instant,
    pub epoch: u64,
}


pub fn init(db: &Connection) -> rusqlite::Result<()> {
    for statement in [
        "ALTER TABLE library_workspaces ADD COLUMN instructions TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE library_workspaces ADD COLUMN pinned TEXT NOT NULL DEFAULT '[]'",
        "ALTER TABLE library_workspaces ADD COLUMN memory_scope TEXT NOT NULL DEFAULT 'inherit'",
        "ALTER TABLE library_workspaces ADD COLUMN capabilities TEXT NOT NULL DEFAULT '[]'",
        "ALTER TABLE library_workspaces ADD COLUMN agent_owned INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE library_workspaces ADD COLUMN draft INTEGER NOT NULL DEFAULT 0",
    ] {

        let _ = db.execute(statement, []);
    }
    Ok(())
}

fn clean(value: &str, max: usize, multiline: bool) -> Option<String> {
    let text = value.trim();
    if text.is_empty() || text.chars().count() > max {
        return None;
    }
    if text
        .chars()
        .any(|c| c.is_control() && !(multiline && (c == '\n' || c == '\t')))
    {
        return None;
    }
    Some(text.to_string())
}

fn label(p: &Value, key: &str, max: usize) -> Result<String, String> {
    p.get(key)
        .and_then(Value::as_str)
        .and_then(|value| clean(value, max, false))
        .ok_or_else(|| format!("{key} must be 1–{max} characters without control characters"))
}

fn optional_text(p: &Value, key: &str, max: usize, multiline: bool) -> Result<Option<String>, String> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.trim().is_empty() => Ok(Some(String::new())),
        Some(Value::String(value)) => clean(value, max, multiline)
            .map(Some)
            .ok_or_else(|| format!("{key} must be at most {max} characters without control characters")),
        Some(_) => Err(format!("{key} must be text")),
    }
}


#[derive(Default)]
pub struct BundlePatch {
    pub instructions: Option<String>,
    pub pinned: Option<String>,
    pub scope: Option<String>,
    pub capabilities: Option<String>,
}

pub fn bundle_patch(p: &Value) -> Result<BundlePatch, String> {
    let mut patch = BundlePatch {
        instructions: optional_text(p, "instructions", 4000, true)?,
        ..Default::default()
    };
    if let Some(Value::Array(items)) = p.get("pinned") {
        if items.len() > 8 {
            return Err("Pin at most 8 notes to a workspace".into());
        }
        let mut notes: Vec<String> = Vec::new();
        for item in items {
            let note = item
                .as_str()
                .and_then(|value| clean(value, 400, false))
                .ok_or("Pinned notes must be 1–400 characters each")?;
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
        patch.pinned = Some(json!(notes).to_string());
    } else if p.get("pinned").is_some_and(|value| !value.is_null()) {
        return Err("Pinned notes must be a list of short notes".into());
    }
    if let Some(value) = p.get("memoryScope") {
        let scope = value.as_str().ok_or("memoryScope must be text")?;
        if !MEMORY_SCOPES.contains(&scope) {
            return Err("memoryScope must be inherit, workspace or off".into());
        }
        patch.scope = Some(scope.to_string());
    }
    if let Some(Value::Array(items)) = p.get("capabilities") {
        if items.len() > CAPABILITIES.len() {
            return Err("Unknown workspace capability".into());
        }
        let mut allowed: Vec<String> = Vec::new();
        for item in items {
            let capability = item.as_str().ok_or("capabilities must be text")?;
            if !CAPABILITIES.contains(&capability) {
                return Err("capabilities may only contain web, browser, files and google".into());
            }
            if !allowed.iter().any(|existing| existing == capability) {
                allowed.push(capability.to_string());
            }
        }
        patch.capabilities = Some(json!(allowed).to_string());
    } else if p.get("capabilities").is_some_and(|value| !value.is_null()) {
        return Err("capabilities must be a list".into());
    }
    Ok(patch)
}


pub fn apply_bundle(db: &Connection, id: &str, patch: &BundlePatch) -> Result<(), String> {
    if let Some(instructions) = &patch.instructions {
        db.execute(
            "UPDATE library_workspaces SET instructions=?2 WHERE id=?1",
            params![id, instructions],
        )
        .map_err(|_| "The workspace bundle could not be saved")?;
    }
    if let Some(pinned) = &patch.pinned {
        db.execute(
            "UPDATE library_workspaces SET pinned=?2 WHERE id=?1",
            params![id, pinned],
        )
        .map_err(|_| "The workspace bundle could not be saved")?;
    }
    if let Some(scope) = &patch.scope {
        db.execute(
            "UPDATE library_workspaces SET memory_scope=?2 WHERE id=?1",
            params![id, scope],
        )
        .map_err(|_| "The workspace bundle could not be saved")?;
    }
    if let Some(capabilities) = &patch.capabilities {
        db.execute(
            "UPDATE library_workspaces SET capabilities=?2 WHERE id=?1",
            params![id, capabilities],
        )
        .map_err(|_| "The workspace bundle could not be saved")?;
    }
    Ok(())
}

fn parse_list(text: &str) -> Value {
    serde_json::from_str::<Value>(text)
        .ok()
        .filter(Value::is_array)
        .unwrap_or_else(|| json!([]))
}

pub fn space_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let owner: String = r.get(8)?;
    Ok(json!({
        "id": r.get::<_, String>(0)?,
        "name": r.get::<_, String>(1)?,
        "folder": r.get::<_, Option<String>>(2)?,
        "archived": r.get::<_, bool>(3)?,
        "instructions": r.get::<_, String>(4)?,
        "pinned": parse_list(&r.get::<_, String>(5)?),
        "memoryScope": r.get::<_, String>(6)?,
        "capabilities": parse_list(&r.get::<_, String>(7)?),
        "agentOwned": owner == "agent",
        "draft": r.get::<_, bool>(9)?,
        "createdAt": r.get::<_, String>(10)?,
    }))
}

const SPACE_SELECT: &str = "SELECT id,name,folder,archived,instructions,pinned,memory_scope,capabilities,\
     CASE agent_owned WHEN 1 THEN 'agent' ELSE 'user' END, draft, created_at FROM library_workspaces";

pub fn spaces(db: &Connection, drafts: bool) -> Result<Vec<Value>, String> {
    let mut statement = db
        .prepare(&format!(
            "{SPACE_SELECT} WHERE draft=?1 ORDER BY created_at,rowid"
        ))
        .map_err(|_| "Workspaces are unavailable")?;
    let rows = statement
        .query_map([if drafts { 1 } else { 0 }], space_row)
        .map_err(|_| "Workspaces are unavailable")?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| "Workspaces are unavailable".into())
}

pub fn space(db: &Connection, id: &str) -> Option<Value> {
    db.query_row(&format!("{SPACE_SELECT} WHERE id=?1"), [id], space_row)
        .ok()
}

pub fn capabilities_for(db: &Connection, workspace: &str) -> Vec<String> {
    let stored: Option<String> = db
        .query_row(
            "SELECT capabilities FROM library_workspaces WHERE id=?1",
            [workspace],
            |r| r.get(0),
        )
        .ok();
    let list = stored.map(|text| parse_list(&text)).unwrap_or_else(|| json!([]));
    let list = list.as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        return CAPABILITIES.iter().map(|value| value.to_string()).collect();
    }
    list.iter()
        .filter_map(Value::as_str)
        .filter(|value| CAPABILITIES.contains(value))
        .map(str::to_owned)
        .collect()
}

pub fn used_today(db: &Connection) -> i64 {
    db.query_row(
        "SELECT (SELECT count(*) FROM library_workspaces WHERE agent_owned=1 AND created_at>=date('now')) \
              + (SELECT count(*) FROM agent_skills WHERE created_at>=date('now'))",
        [],
        |r| r.get(0),
    )
    .unwrap_or(DAILY_OBJECT_LIMIT)
}

fn budget(db: &Connection) -> Result<(), String> {
    if used_today(db) >= DAILY_OBJECT_LIMIT {
        return Err(format!(
            "The agent may create {DAILY_OBJECT_LIMIT} things per day and has used today's budget. Ask the user before creating more."
        ));
    }
    Ok(())
}

fn insert_space(db: &Connection, p: &Value) -> Result<Value, String> {
    budget(db)?;
    let name = label(p, "name", 60)?;
    if db
        .query_row(
            "SELECT count(*) FROM library_workspaces WHERE lower(name)=lower(?1) AND archived=0",
            [&name],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0
    {
        return Err("A workspace with that name already exists. Pick another name.".into());
    }
    let patch = bundle_patch(p)?;
    let id = uuid::Uuid::new_v4().to_string();
    db.execute(
        "INSERT INTO library_workspaces(id,name,instructions,pinned,memory_scope,capabilities,agent_owned,draft) \
         VALUES(?1,?2,?3,?4,?5,?6,1,1)",
        params![
            id,
            name,
            patch.instructions.unwrap_or_default(),
            patch.pinned.unwrap_or_else(|| "[]".into()),
            patch.scope.unwrap_or_else(|| "inherit".into()),
            patch.capabilities.unwrap_or_else(|| "[]".into()),
        ],
    )
    .map_err(|_| "The workspace draft could not be saved")?;
    Ok(json!({"id":id,"name":name,"draft":true}))
}

fn update_space(db: &Connection, p: &Value) -> Result<Value, String> {
    let id = label(p, "spaceId", 100)?;
    let owner: i64 = db
        .query_row(
            "SELECT agent_owned FROM library_workspaces WHERE id=?1 AND archived=0",
            [&id],
            |r| r.get(0),
        )
        .map_err(|_| "That workspace does not exist")?;
    if owner != 1 {
        return Err("This workspace belongs to the user. Propose the change to them instead of editing it.".into());
    }
    let patch = bundle_patch(p)?;
    apply_bundle(db, &id, &patch)?;
    if let Some(Value::String(name)) = p.get("name") {
        let name = clean(name, 60, false).ok_or("name must be 1–60 characters")?;
        db.execute(
            "UPDATE library_workspaces SET name=?2 WHERE id=?1",
            params![id, name],
        )
        .map_err(|_| "The workspace could not be renamed")?;
    }
    Ok(json!({"id":id}))
}

fn scheduled_task(db: &Connection, p: &Value, conversation: &str) -> Result<Value, String> {
    let source = crate::storage::get_conversation(db, conversation).map_err(|e| e.to_string())?;
    let operation = p["operation"].as_str().unwrap_or("");
    let tasks = crate::scheduler::list(db).map_err(|e| e.to_string())?;
    if operation == "schedule_list" {
        return Ok(json!({"tasks": tasks.into_iter().filter(|task| task.workspace_id == source.workspace_id).collect::<Vec<_>>(), "now": chrono::Utc::now().to_rfc3339()}));
    }
    if crate::storage::conversation_is_delegated(db, conversation).map_err(|e| e.to_string())? {
        return Err("Report scheduling needs to the parent chat instead of changing scheduled tasks.".into());
    }
    let id = p["taskId"].as_str().unwrap_or("");
    let existing = if id.is_empty() {
        None
    } else {
        Some(tasks.into_iter().find(|task| task.id == id && task.workspace_id == source.workspace_id)
            .ok_or("That scheduled task does not exist in this chat's workspace")?)
    };
    if operation != "schedule_save" {
        let task = existing.ok_or("Task ID required")?;
        return crate::automation::task_action(db, match operation {
            "schedule_pause" => "pause",
            "schedule_delete" => "delete",
            _ => return Err("Unknown scheduled task operation".into()),
        }, json!({"id": task.id, "paused": p["paused"]}), &source.workspace_id);
    }
    let mut payload = if let Some(task) = existing {
        json!(task)
    } else {
        json!({
            "id": "", "workspaceId": source.workspace_id,
            "paused": false, "nextAt": null, "timeoutSeconds": 600,
            "connectionId": source.selection.as_ref().map(|s| s.connection_id.as_str()).unwrap_or(""),
            "modelId": source.selection.as_ref().map(|s| s.model_id.as_str()).unwrap_or(""),
        })
    };
    for key in ["title", "prompt", "localTime", "timezone", "recurrence", "paused", "timeoutSeconds", "connectionId", "modelId"] {
        if let Some(value) = p.get(key) {
            payload[key] = value.clone();
        }
    }
    crate::automation::task_action(db, "save", payload, &source.workspace_id)
}

fn normalize_questions(p: &Value) -> Result<Vec<Value>, String> {
    let raw = p
        .get("questions")
        .and_then(Value::as_array)
        .ok_or("questions must be a list")?;
    if raw.is_empty() || raw.len() > MAX_ASK_QUESTIONS {
        return Err(format!("Ask between 1 and {MAX_ASK_QUESTIONS} questions at once"));
    }
    let mut questions = Vec::new();
    for (index, item) in raw.iter().enumerate() {
        let text = label(item, "question", 300)?;
        let raw_options = item
            .get("options")
            .and_then(Value::as_array)
            .ok_or("Each question needs 2–5 options")?;
        if raw_options.len() < 2 || raw_options.len() > 5 {
            return Err("Each question needs 2–5 options".into());
        }
        let mut options = Vec::new();
        for (position, option) in raw_options.iter().enumerate() {
            let id = option
                .get("id")
                .and_then(Value::as_str)
                .and_then(|value| clean(value, 40, false))
                .unwrap_or_else(|| format!("o{}", position + 1));
            let label = label(option, "label", 120)?;
            let detail = optional_text(option, "detail", 200, false)?.unwrap_or_default();
            if options.iter().any(|existing: &Value| existing["id"] == json!(id)) {
                return Err("Option IDs must be unique within a question".into());
            }
            options.push(json!({"id":id,"label":label,"detail":detail}));
        }
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .and_then(|value| clean(value, 40, false))
            .unwrap_or_else(|| format!("q{}", index + 1));
        if questions.iter().any(|existing: &Value| existing["id"] == json!(id)) {
            return Err("Question IDs must be unique".into());
        }
        let recommended = item
            .get("recommended")
            .and_then(Value::as_str)
            .filter(|value| options.iter().any(|option| option["id"] == json!(value)))
            .map(str::to_owned)
            .unwrap_or_else(|| options[0]["id"].as_str().unwrap_or("o1").to_string());
        let other = item.get("other").and_then(Value::as_bool).unwrap_or(true);
        questions.push(json!({"id":id,"question":text,"options":options,"recommended":recommended,"other":other}));
    }
    Ok(questions)
}

fn reply(h: &mut Host, id: Value, result: Result<Value, String>) {
    let message = match result {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(error) => json!({"jsonrpc":"2.0","id":id,"error":{"message":error}}),
    };
    h.supervisor.relay_agent(message);
}


pub fn message(h: &mut Host, app: &tauri::AppHandle, message: Value) {
    let method = message["method"].as_str().unwrap_or("");
    if method == "notifications/cancelled" {
        let request = message["params"]["requestId"].clone();
        let ids: Vec<String> = h
            .agent_asks
            .iter()
            .filter(|(_, ask)| ask.call["id"] == request)
            .map(|(id, _)| id.clone())
            .collect();
        let changed = !ids.is_empty();
        for id in ids {
            h.agent_asks.remove(&id);
        }
        if changed {
            let _ = app.emit("agent-changed", ());
        }
        return;
    }
    if method != "agent.call" {
        return;
    }
    let p = message["params"].clone();
    let conversation = p["conversationId"].as_str().unwrap_or("").to_string();
    let operation = p["operation"].as_str().unwrap_or("").to_string();
    let child_task = crate::delegation::task_for_child_conversation(&h.db, &conversation).ok().flatten();
    if child_task.is_some()
        && matches!(operation.as_str(), "space_create" | "space_update" | "schedule_save" | "schedule_pause" | "schedule_delete")
    {
        reply(
            h,
            message["id"].clone(),
            Err("Delegated child tasks cannot change scheduled tasks or workspaces. Report the need in your result.".into()),
        );
        return;
    }
    if operation == "ask" {
        if h.agent_asks.len() >= MAX_PENDING_ASKS {
            reply(h, message["id"].clone(), Err("Too many questions are already waiting. Ask again after some are answered.".into()));
            return;
        }
        let questions = match normalize_questions(&p) {
            Ok(questions) => questions,
            Err(error) => {
                reply(h, message["id"].clone(), Err(error));
                return;
            }
        };
        let header = optional_text(&p, "header", 120, false)
            .ok()
            .flatten()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "A quick decision".into());
        let id = uuid::Uuid::new_v4().to_string();
        let created = chrono::Utc::now().timestamp_millis();
        let ask = json!({
            "id": id,
            "conversationId": conversation,
            "header": header,
            "questions": questions,
            "createdAt": created,
            "expiresAt": created + ASK_TIMEOUT.as_millis() as i64,
            "parentConversationId": child_task.as_ref().map(|task| task.parent_conversation_id.clone()),
            "taskId": child_task.as_ref().map(|task| task.id.clone()),
            "taskTitle": child_task.as_ref().map(|task| task.title.clone()),
        });
        if let Some(task) = &child_task {
            if !matches!(task.state.as_str(), "queued" | "running" | "waiting") {
                reply(h, message["id"].clone(), Err("This delegated task is no longer active.".into()));
                return;
            }
            let _ = crate::delegation::set_state(&h.db, &task.id, "waiting", None, None);
            crate::delegation::emit_task_update(&h.db, app, &task.id);
        }
        h.agent_asks.insert(
            id,
            PendingAsk {
                call: message["id"].clone(),
                ask,
                at: Instant::now(),
                epoch: h.supervisor.worker_epoch(),
            },
        );
        let _ = app.emit("agent-changed", ());
        return;
    }
    let result = (|| -> Result<Value, String> {
        match operation.as_str() {
            "state" => Ok(state(&h.db, h)),
            "space_list" => Ok(json!({"spaces": spaces(&h.db, false)?})),
            "space_create" => insert_space(&h.db, &p),
            "space_update" => update_space(&h.db, &p),
            "schedule_list" | "schedule_save" | "schedule_pause" | "schedule_delete" => {
                let result = scheduled_task(&h.db, &p, &conversation);
                if result.is_ok() && operation != "schedule_list" {
                    let _ = app.emit("automation-changed", ());
                }
                result
            }
            "skill_list" => {
                let workspace = crate::storage::get_conversation(&h.db, &conversation)
                    .map(|conversation| conversation.workspace_id)
                    .unwrap_or_else(|_| library::setting(&h.db, "active_workspace", "general"));
                Ok(crate::skills::agent_view(&h.db, &h.supervisor.data, &workspace))
            }
            "skill_create" | "skill_update" => {
                if crate::storage::conversation_is_delegated(&h.db, &conversation).unwrap_or(false) {
                    return Err("Report the suggested skill change in your result. The parent conversation proposes it so the user reviews one card, not several.".into());
                }
                crate::skills::propose(&h.db, &p, &conversation)
            }
            _ => Err("Unknown agent operation".into()),
        }
    })();
    let write = !matches!(
        operation.as_str(),
        "state" | "space_list" | "schedule_list" | "skill_list"
    );
    reply(h, message["id"].clone(), result);
    if write {
        let _ = app.emit("agent-changed", ());
    }
}

pub fn state(db: &Connection, h: &Host) -> Value {
    let asks: Vec<Value> = h.agent_asks.values().map(|ask| ask.ask.clone()).collect();
    let skills = crate::skills::state(db);
    json!({
        "asks": asks,
        "drafts": spaces(db, true).unwrap_or_default(),
        "skills": skills["skills"],
        "skillProposals": skills["skillProposals"],
        "budget": {"used": used_today(db), "limit": DAILY_OBJECT_LIMIT},
    })
}


pub fn resolve_answers(
    questions: &[Value],
    provided: &serde_json::Map<String, Value>,
) -> Result<Vec<Value>, String> {
    let mut answers = Vec::new();
    for question in questions {
        let question_id = question["id"].as_str().unwrap_or("");
        let options = question["options"].as_array().cloned().unwrap_or_default();
        let recommended = question["recommended"].as_str().unwrap_or("");
        let choice = provided
            .get(question_id)
            .cloned()
            .unwrap_or_else(|| json!("decided"));
        let selected = match &choice {
            Value::String(value) if value == "decided" => {
                let option = options
                    .iter()
                    .find(|option| option["id"] == json!(recommended))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                json!({
                    "questionId": question_id,
                    "optionId": recommended,
                    "label": option["label"].clone(),
                    "decided": true,
                })
            }
            Value::String(value) => {
                let option = options
                    .iter()
                    .find(|option| option["id"] == json!(value))
                    .ok_or("That answer does not belong to the question")?;
                json!({
                    "questionId": question_id,
                    "optionId": value,
                    "label": option["label"].clone(),
                    "decided": false,
                })
            }
            Value::Object(map) if map.get("other").is_some() => {
                let text = map
                    .get("other")
                    .and_then(Value::as_str)
                    .and_then(|value| clean(value, 400, false))
                    .ok_or("Custom answers must be 1-400 characters")?;
                json!({"questionId": question_id, "other": text, "decided": false})
            }
            _ => return Err("Answer with an option ID, {\"other\": text}, or decided".into()),
        };
        answers.push(selected);
    }
    Ok(answers)
}


pub fn answer(h: &mut Host, app: &tauri::AppHandle, p: &Value) -> Result<(), String> {
    let id = p["id"].as_str().ok_or("Ask ID required")?.to_string();
    let provided = p["answers"]
        .as_object()
        .cloned()
        .ok_or("answers must be an object")?;
    let pending = h
        .agent_asks
        .get(&id)
        .ok_or("This question has already been answered or expired")?;
    let questions = pending
        .ask["questions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let answers = resolve_answers(&questions, &provided)?;
    let pending = h.agent_asks.remove(&id).expect("pending ask checked");
    if let Some(task_id) = pending.ask["taskId"].as_str() {
        if crate::delegation::get(&h.db, task_id).ok().flatten().is_some_and(|task| task.state == "waiting") {
            let _ = crate::delegation::set_state(&h.db, task_id, "running", None, None);
            crate::delegation::emit_task_update(&h.db, app, task_id);
        }
    }
    h.supervisor.relay_agent(json!({
        "jsonrpc": "2.0",
        "id": pending.call["id"],
        "result": {"askId": id, "answers": answers, "expired": false},
    }));
    let _ = app.emit("agent-changed", ());
    Ok(())
}


pub fn expire(h: &mut Host, app: &tauri::AppHandle) -> bool {
    let epoch = h.supervisor.worker_epoch();
    let ids: Vec<String> = h
        .agent_asks
        .iter()
        .filter(|(_, ask)| ask.epoch != epoch || ask.at.elapsed() > ASK_TIMEOUT)
        .map(|(id, _)| id.clone())
        .collect();
    let changed = !ids.is_empty();
    for id in ids {
        if let Some(pending) = h.agent_asks.remove(&id) {
            if let Some(task_id) = pending.ask["taskId"].as_str() {
                if crate::delegation::get(&h.db, task_id).ok().flatten().is_some_and(|task| task.state == "waiting") {
                    let _ = crate::delegation::set_state(&h.db, task_id, "running", None, None);
                    crate::delegation::emit_task_update(&h.db, app, task_id);
                }
            }
            if pending.epoch == epoch {
                let answers = pending.ask["questions"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|question| {
                        json!({
                            "questionId": question["id"],
                            "optionId": question["recommended"],
                            "decided": true,
                            "expired": true,
                        })
                    })
                    .collect::<Vec<_>>();
                h.supervisor.relay_agent(json!({
                    "jsonrpc": "2.0",
                    "id": pending.call["id"],
                    "result": {"askId": id, "answers": answers, "expired": true},
                }));
            }
        }
    }
    if changed {
        let _ = app.emit("agent-changed", ());
    }
    changed
}

fn accept_space(db: &Connection, p: &Value) -> Result<Value, String> {
    let id = label(p, "id", 100)?;
    let changed = db
        .execute(
            "UPDATE library_workspaces SET draft=0 WHERE id=?1 AND draft=1",
            [&id],
        )
        .map_err(|_| "The workspace could not be added")?;
    if changed == 0 {
        return Err("That workspace draft no longer exists".into());
    }
    Ok(json!({"id":id}))
}

fn dismiss_space(db: &Connection, p: &Value) -> Result<Value, String> {
    let id = label(p, "id", 100)?;
    db.execute(
        "DELETE FROM library_workspaces WHERE id=?1 AND draft=1 AND agent_owned=1",
        [&id],
    )
    .map_err(|_| "The workspace draft could not be removed")?;
    Ok(json!(true))
}


fn set_permission(db: &Connection, p: &Value) -> Result<Value, String> {
    let id = label(p, "id", 100)?;
    if id == "general" {
        return Err("General stays user-owned".into());
    }
    let owned = p
        .get("agentOwned")
        .and_then(Value::as_bool)
        .ok_or("agentOwned must be true or false")?;
    let changed = db
        .execute(
            "UPDATE library_workspaces SET agent_owned=?2 WHERE id=?1",
            params![id, if owned { 1 } else { 0 }],
        )
        .map_err(|_| "The workspace permission could not be saved")?;
    if changed == 0 {
        return Err("That workspace no longer exists".into());
    }
    Ok(json!({"id":id,"agentOwned":owned}))
}

#[tauri::command]
pub async fn agent_action(
    app: tauri::AppHandle,
    operation: String,
    payload: Option<Value>,
) -> Result<Value, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut h = shared.lock().map_err(|_| "Agent state unavailable")?;
        let p = payload.unwrap_or(json!({}));
        let data = || {
            app.path()
                .app_data_dir()
                .map_err(|error| error.to_string())
        };
        let result = match operation.as_str() {
            "state" | "refresh" => Ok(state(&h.db, &h)),
            "answer" => answer(&mut h, app, &p).map(|_| state(&h.db, &h)),
            "space_accept" => accept_space(&h.db, &p).map(|_| json!(true)),
            "space_dismiss" => dismiss_space(&h.db, &p).map(|_| json!(true)),
            "space_permission" => set_permission(&h.db, &p).map(|_| json!(true)),
            "skill_save" => crate::skills::save(&h.db, &data()?, &p).map(|_| json!(true)),
            "skill_enable" => {
                let id = label(&p, "id", 100)?;
                let enabled = p
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .ok_or("enabled must be true or false")?;
                crate::skills::set_enabled(&h.db, &id, enabled).map(|_| json!(true))
            }
            "skill_delete" | "skill_dismiss" => {
                let id = label(&p, "id", 100)?;
                crate::skills::delete(&h.db, &data()?, &id).map(|_| json!(true))
            }
            _ => Err("Unknown agent operation".into()),
        };
        let result = result?;
        if !matches!(operation.as_str(), "state" | "refresh") {
            let _ = app.emit("agent-changed", ());
        }
        Ok(result)
    })
    .await
}



pub fn workspace_context(db: &Connection, workspace: &str) -> String {
    let Some(space) = space(db, workspace) else {
        return String::new();
    };
    let instructions = space["instructions"].as_str().unwrap_or("").trim();
    let pinned = space["pinned"].as_array().cloned().unwrap_or_default();
    if instructions.is_empty() && pinned.is_empty() {
        return String::new();
    }
    let mut text = String::from("\n# Workspace brief (written for this workspace)\n");
    let mut budget = 1200usize;
    if !instructions.is_empty() {
        let trimmed: String = instructions.chars().take(900).collect();
        text.push_str(&trimmed);
        text.push('\n');
        budget = budget.saturating_sub(trimmed.len());
    }
    for note in pinned {
        let note = note.as_str().unwrap_or("");
        let line = format!("- {}\n", note.chars().take(240).collect::<String>());
        if line.len() > budget {
            break;
        }
        budget -= line.len();
        text.push_str(&line);
    }
    text
}


#[cfg(test)]
pub fn allows(db: &Connection, workspace: &str, capability: &str) -> bool {
    capabilities_for(db, workspace).iter().any(|value| value == capability)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../services/memory/schema.sql"))
            .unwrap();
        crate::storage::init(&db).unwrap();
        init(&db).unwrap();
        crate::skills::init(&db).unwrap();
        init(&db).unwrap();
        db
    }

    #[test]
    fn workspace_bundles_validate_and_drafts_stay_out_of_the_library() {
        let db = database();
        let created = insert_space(
            &db,
            &json!({
                "name": "Launch notes",
                "instructions": "Keep updates short and cite the launch checklist.",
                "pinned": ["Ship on the 14th", "Ship on the 14th"],
                "memoryScope": "off",
                "capabilities": ["files"]
            }),
        )
        .unwrap();
        let id = created["id"].as_str().unwrap();
        let draft = space(&db, id).unwrap();
        assert_eq!(draft["draft"], true);
        assert_eq!(draft["pinned"], json!(["Ship on the 14th"]));
        assert_eq!(draft["agentOwned"], true);
        assert!(spaces(&db, false).unwrap().iter().all(|s| s["id"] != json!(id)));
        assert_eq!(spaces(&db, true).unwrap().len(), 1);
        assert!(workspace_context(&db, id).contains("launch checklist"));
        assert_eq!(capabilities_for(&db, id), vec!["files"]);
        assert!(!allows(&db, id, "browser"));
        accept_space(&db, &json!({"id":id})).unwrap();
        assert!(spaces(&db, false).unwrap().iter().any(|s| s["id"] == json!(id)));
        assert!(insert_space(&db, &json!({"name":"Launch notes"})).is_err());
        assert!(insert_space(&db, &json!({"name":"Bad","capabilities":["shell"]})).is_err());
    }

    #[test]
    fn user_owned_workspaces_are_read_only_for_the_agent() {
        let db = database();
        assert!(update_space(&db, &json!({"spaceId":"general","instructions":"hijack"})).is_err());
        let created = insert_space(&db, &json!({"name":"Agent space"})).unwrap();
        let id = created["id"].as_str().unwrap();
        update_space(&db, &json!({"spaceId":id,"instructions":"new brief"})).unwrap();
        assert_eq!(space(&db, id).unwrap()["instructions"], "new brief");
        set_permission(&db, &json!({"id":id,"agentOwned":false})).unwrap();
        assert!(update_space(&db, &json!({"spaceId":id,"instructions":"again"})).is_err());
    }

    #[test]
    fn scheduled_tasks_use_the_chat_model_and_support_the_user_controls() {
        let mut db = database();
        crate::scheduler::init(&db).unwrap();
        let connection = crate::storage::upsert_connection(&mut db, "custom", "Test", Some("model-1"), None, None).unwrap();
        let selection = crate::storage::ModelSelection { connection_id: connection.clone(), model_id: "model-1".into() };
        crate::storage::create_conversation(&db, "chat-1", "Chat", Some(&selection)).unwrap();
        let created = scheduled_task(&db, &json!({
            "operation": "schedule_save", "title": "Daily briefing", "prompt": "Summarize my priorities",
            "localTime": "2099-01-01T09:00", "timezone": "America/Denver", "recurrence": "daily"
        }), "chat-1").unwrap();
        assert_eq!(created["connectionId"], connection);
        assert_eq!(created["modelId"], "model-1");
        assert_eq!(created["workspaceId"], "general");
        assert!(created["nextAt"].as_i64().is_some());
        assert_eq!(used_today(&db), 0);
        let id = created["id"].as_str().unwrap();
        let edited = scheduled_task(&db, &json!({"operation":"schedule_save","taskId":id,"recurrence":"weekly","timeoutSeconds":120}), "chat-1").unwrap();
        assert_eq!(edited["recurrence"], "weekly");
        assert_eq!(edited["prompt"], created["prompt"]);
        assert_eq!(edited["timezone"], created["timezone"]);
        scheduled_task(&db, &json!({"operation":"schedule_pause","taskId":id,"paused":true}), "chat-1").unwrap();
        let list = scheduled_task(&db, &json!({"operation":"schedule_list"}), "chat-1").unwrap();
        assert_eq!(list["tasks"][0]["paused"], true);
        scheduled_task(&db, &json!({"operation":"schedule_pause","taskId":id,"paused":false}), "chat-1").unwrap();
        assert!(!crate::scheduler::list(&db).unwrap()[0].paused);
        scheduled_task(&db, &json!({"operation":"schedule_delete","taskId":id}), "chat-1").unwrap();
        assert!(crate::scheduler::list(&db).unwrap().is_empty());
        assert!(scheduled_task(&db, &json!({"operation":"schedule_save","taskId":id,"title":"Revive"}), "chat-1").is_err());
    }

    #[test]
    fn scheduling_validates_times_and_cannot_escape_the_parent_workspace() {
        let mut db = database();
        crate::scheduler::init(&db).unwrap();
        let connection = crate::storage::upsert_connection(&mut db, "custom", "Test", Some("model-1"), None, None).unwrap();
        let selection = crate::storage::ModelSelection { connection_id: connection, model_id: "model-1".into() };
        crate::storage::create_conversation(&db, "chat-1", "Chat", Some(&selection)).unwrap();
        let mut request = json!({
            "operation":"schedule_save", "title":"Briefing", "prompt":"Summarize priorities",
            "localTime":"2099-01-01T09:00", "timezone":"invalid", "recurrence":"daily"
        });
        assert!(scheduled_task(&db, &request, "chat-1").is_err());
        request["timezone"] = json!("America/Denver");
        let created = scheduled_task(&db, &request, "chat-1").unwrap();
        let id = created["id"].as_str().unwrap();
        db.execute("INSERT INTO library_workspaces(id,name) VALUES('other','Other')", []).unwrap();
        crate::storage::create_conversation(&db, "other-chat", "Other", Some(&selection)).unwrap();
        db.execute("UPDATE chat_conversations SET workspace_id='other' WHERE id='other-chat'", []).unwrap();
        assert!(scheduled_task(&db, &json!({"operation":"schedule_delete","taskId":id}), "other-chat").is_err());
        assert!(scheduled_task(&db, &json!({"operation":"schedule_save","taskId":id,"title":"Changed"}), "other-chat").is_err());
        let list = scheduled_task(&db, &json!({"operation":"schedule_list"}), "other-chat").unwrap();
        assert!(list["tasks"].as_array().unwrap().is_empty());
        db.execute("UPDATE chat_conversations SET delegation_parent_id='chat-1' WHERE id='other-chat'", []).unwrap();
        assert!(scheduled_task(&db, &request, "other-chat").is_err());
        assert_eq!(crate::scheduler::list(&db).unwrap().len(), 1);
    }

    #[test]
    fn questions_normalize_and_answers_must_match_the_options() {
        let normalized = normalize_questions(&json!({"questions":[{"question":"Which folder?","options":[{"id":"a","label":"Docs"},{"id":"b","label":"Desktop"}]}]})).unwrap();
        assert_eq!(normalized[0]["recommended"], "a");
        assert!(normalize_questions(&json!({"questions":[]})).is_err());
        assert!(normalize_questions(&json!({"questions":[{"question":"One option","options":[{"label":"Only"}]}]})).is_err());
    }

    #[test]
    fn answers_are_resolved_against_stored_options() {
        let questions = normalize_questions(&json!({"questions":[{"id":"folder","question":"Which folder?","options":[{"id":"a","label":"Docs"},{"id":"b","label":"Desktop"}]},{"id":"tone","question":"Tone?","options":[{"id":"x","label":"Short"},{"id":"y","label":"Detailed"}]}]})).unwrap();
        let mut provided = serde_json::Map::new();
        provided.insert("folder".into(), json!("b"));
        provided.insert("tone".into(), json!({"other":"Warm, no headings"}));
        let answers = resolve_answers(&questions, &provided).unwrap();
        assert_eq!(answers[0]["optionId"], "b");
        assert_eq!(answers[1]["other"], "Warm, no headings");
        provided.insert("folder".into(), json!("missing"));
        assert!(resolve_answers(&questions, &provided).is_err());
        provided.clear();
        let defaults = resolve_answers(&questions, &provided).unwrap();
        assert_eq!(defaults[0]["optionId"], "a");
        assert_eq!(defaults[0]["decided"], true);
    }

    #[test]
    fn bundle_patch_rejects_unbounded_or_unsafe_content() {
        assert!(bundle_patch(&json!({"instructions":"x".repeat(4001)})).is_err());
        assert!(bundle_patch(&json!({"pinned":["ok",7]})).is_err());
        assert!(bundle_patch(&json!({"memoryScope":"everything"})).is_err());
        assert!(bundle_patch(&json!({"instructions":"hi\u{0}"})).is_err());
        let patch = bundle_patch(&json!({"instructions":"Use short sentences."})).unwrap();
        assert_eq!(patch.instructions.as_deref(), Some("Use short sentences."));
        assert!(patch.pinned.is_none());
    }
}
