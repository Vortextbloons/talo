





use crate::{chat, run_blocking, storage, Host, Shared};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

pub const MAX_ACTIVE_PER_PARENT: i64 = 2;
pub const MAX_ACTIVE_TOTAL: i64 = 4;
pub const MAX_TASKS_PER_PARENT: i64 = 8;
pub const MAX_TITLE: usize = 120;
pub const MAX_BRIEF: usize = 4000;
const MAX_RESULT: usize = 6000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationTask {
    pub id: String,
    pub parent_conversation_id: String,
    pub child_conversation_id: String,
    pub title: String,
    pub brief: String,
    pub state: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub delivered: bool,
    pub thinking_level: Option<String>,
    pub current_activity: Option<String>,
    pub agent_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_delegations(
           id TEXT PRIMARY KEY,
           parent_conversation_id TEXT NOT NULL,
           child_conversation_id TEXT NOT NULL,
           title TEXT NOT NULL,
           brief TEXT NOT NULL,
           state TEXT NOT NULL DEFAULT 'queued'
             CHECK(state IN ('queued','running','waiting','completed','failed','cancelled','interrupted')),
           result TEXT,
           error TEXT,
           generation_id TEXT,
           delivered INTEGER NOT NULL DEFAULT 0,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           thinking_level TEXT,
           current_activity TEXT
         );
         CREATE INDEX IF NOT EXISTS agent_delegations_parent
           ON agent_delegations(parent_conversation_id, state);
         CREATE INDEX IF NOT EXISTS agent_delegations_child
           ON agent_delegations(child_conversation_id);
         CREATE INDEX IF NOT EXISTS agent_delegations_generation
           ON agent_delegations(generation_id);
         CREATE TABLE IF NOT EXISTS agent_delegation_activity(
           task_id TEXT NOT NULL REFERENCES agent_delegations(id) ON DELETE CASCADE,
           id TEXT NOT NULL,
           sequence INTEGER NOT NULL,
           tool TEXT NOT NULL,
           label TEXT NOT NULL,
           detail TEXT,
           state TEXT NOT NULL,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           PRIMARY KEY(task_id,id)
         );
         CREATE INDEX IF NOT EXISTS agent_delegation_activity_task
           ON agent_delegation_activity(task_id,sequence);",
    )?;

    let has_level: i64 = db
        .query_row(
            "SELECT count(*) FROM pragma_table_info('agent_delegations') WHERE name='thinking_level'",
            [],
            |row| row.get(0),
        )?;
    if has_level == 0 {
        db.execute_batch("ALTER TABLE agent_delegations ADD COLUMN thinking_level TEXT")?;
    }
    let has_activity: i64 = db
        .query_row(
            "SELECT count(*) FROM pragma_table_info('agent_delegations') WHERE name='current_activity'",
            [],
            |row| row.get(0),
        )?;
    if has_activity == 0 {
        db.execute_batch("ALTER TABLE agent_delegations ADD COLUMN current_activity TEXT")?;
    }
    let has_name: i64 = db
        .query_row(
            "SELECT count(*) FROM pragma_table_info('agent_delegations') WHERE name='agent_name'",
            [],
            |row| row.get(0),
        )?;
    if has_name == 0 {
        db.execute_batch("ALTER TABLE agent_delegations ADD COLUMN agent_name TEXT")?;
    }
    Ok(())
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DelegationTask> {
    Ok(DelegationTask {
        id: row.get(0)?,
        parent_conversation_id: row.get(1)?,
        child_conversation_id: row.get(2)?,
        title: row.get(3)?,
        brief: row.get(4)?,
        state: row.get(5)?,
        result: row.get(6)?,
        error: row.get(7)?,
        delivered: row.get::<_, i64>(8)? != 0,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        thinking_level: row.get(11)?,
        current_activity: row.get(12)?,
        agent_name: row.get(13)?,
    })
}

const TASK_COLUMNS: &str = "id,parent_conversation_id,child_conversation_id,title,brief,state,result,error,delivered,created_at,updated_at,thinking_level,current_activity,agent_name";

pub fn list(db: &Connection, parent: &str) -> Result<Vec<Value>, String> {
    let mut statement = db
        .prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM agent_delegations WHERE parent_conversation_id=?1 ORDER BY created_at,rowid"
        ))
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([parent], task_from_row)
        .map_err(|error| error.to_string())?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|task| serde_json::to_value(task).unwrap_or(Value::Null))
        .collect())
}

pub fn get(db: &Connection, id: &str) -> Result<Option<DelegationTask>, String> {
    db.query_row(
        &format!("SELECT {TASK_COLUMNS} FROM agent_delegations WHERE id=?1"),
        [id],
        task_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

pub fn task_for_child_conversation(
    db: &Connection,
    conversation_id: &str,
) -> Result<Option<DelegationTask>, String> {
    db.query_row(
        &format!("SELECT {TASK_COLUMNS} FROM agent_delegations WHERE child_conversation_id=?1"),
        [conversation_id],
        task_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

pub fn is_child(db: &Connection, conversation_id: &str) -> bool {
    storage::conversation_is_delegated(db, conversation_id).unwrap_or(true)
}

pub fn child_conversation_for_generation(
    db: &Connection,
    generation_id: &str,
) -> Result<Option<DelegationTask>, String> {
    db.query_row(
        &format!("SELECT {TASK_COLUMNS} FROM agent_delegations WHERE generation_id=?1"),
        [generation_id],
        task_from_row,
    )
    .optional()
    .map_err(|error| error.to_string())
}

pub fn set_state(
    db: &Connection,
    id: &str,
    state: &str,
    result: Option<&str>,
    error: Option<&str>,
) -> Result<(), String> {
    db.execute(
        "UPDATE agent_delegations SET state=?2,result=COALESCE(?3,result),error=COALESCE(?4,error),updated_at=?5 WHERE id=?1",
        params![id, state, result, error, now()],
    )
    .map_err(|_| "The delegated task could not be updated".to_string())?;
    Ok(())
}

pub fn set_generation(db: &Connection, id: &str, generation_id: &str, state: &str) -> Result<(), String> {
    db.execute(
        "UPDATE agent_delegations SET generation_id=?2,state=?3,updated_at=?4 WHERE id=?1",
        params![id, generation_id, state, now()],
    )
    .map_err(|_| "The delegated task could not be updated".to_string())?;
    Ok(())
}



pub fn record_activity(
    db: &Connection,
    conversation_id: &str,
    generation_id: &str,
    activity: &Value,
) -> Result<Option<(String, String, Value)>, String> {
    let task = match child_conversation_for_generation(db, generation_id)? {
        Some(task) => Some(task),
        None => task_for_child_conversation(db, conversation_id)?,
    };
    let Some(task) = task else {
        return Ok(None);
    };
    let id = activity["id"].as_str().filter(|value| !value.is_empty() && value.len() <= 200)
        .ok_or("Activity id is required")?;
    let tool = activity["tool"].as_str().filter(|value| !value.is_empty() && value.len() <= 100)
        .ok_or("Activity tool is required")?;
    let label = activity["label"].as_str().filter(|value| !value.is_empty() && value.chars().count() <= 120)
        .ok_or("Activity label is required")?;
    let detail = activity["detail"].as_str()
        .map(|value| value.chars().take(400).collect::<String>())
        .filter(|value| !value.is_empty());
    let state = match activity["state"].as_str().unwrap_or("running") {
        "completed" => "completed",
        "failed" => "failed",
        "cancelled" => "cancelled",
        _ => "running",
    };
    let stamp = now();
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_delegation_activity WHERE task_id=?1 AND id=?2)",
            params![task.id, id],
            |row| row.get(0),
        )
        .unwrap_or(false);
    let created_at: String = if exists {
        db.execute(
            "UPDATE agent_delegation_activity SET tool=?3,label=?4,detail=?5,state=?6,updated_at=?7 WHERE task_id=?1 AND id=?2",
            params![task.id, id, tool, label, detail, state, stamp],
        )
        .map_err(|error| error.to_string())?;
        db.query_row(
            "SELECT created_at FROM agent_delegation_activity WHERE task_id=?1 AND id=?2",
            params![task.id, id],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| stamp.clone())
    } else {
        let sequence: i64 = db
            .query_row(
                "SELECT COALESCE(MAX(sequence),0)+1 FROM agent_delegation_activity WHERE task_id=?1",
                [&task.id],
                |row| row.get(0),
            )
            .unwrap_or(1);
        db.execute(
            "INSERT INTO agent_delegation_activity(task_id,id,sequence,tool,label,detail,state,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?8)",
            params![task.id, id, sequence, tool, label, detail, state, stamp],
        )
        .map_err(|error| error.to_string())?;
        db.execute(
            "DELETE FROM agent_delegation_activity WHERE task_id=?1 AND id NOT IN (
               SELECT id FROM agent_delegation_activity WHERE task_id=?1 ORDER BY sequence DESC LIMIT 100
             )",
            [&task.id],
        )
        .map_err(|error| error.to_string())?;
        stamp.clone()
    };
    db.execute(
        "UPDATE agent_delegations SET current_activity=?2,updated_at=?3 WHERE id=?1",
        params![task.id, label, stamp],
    )
    .map_err(|error| error.to_string())?;
    Ok(Some((
        task.id,
        task.parent_conversation_id,
        json!({
            "id": id,
            "tool": tool,
            "label": label,
            "detail": detail,
            "state": state,
            "createdAt": created_at,
        }),
    )))
}

pub fn activity_for_task(db: &Connection, task_id: &str) -> Vec<Value> {
    let Ok(mut statement) = db.prepare(
        "SELECT id,tool,label,detail,state,created_at FROM agent_delegation_activity
         WHERE task_id=?1 ORDER BY sequence ASC LIMIT 100",
    ) else {
        return Vec::new();
    };
    let rows = statement.query_map([task_id], |row| {
        Ok(json!({
            "id": row.get::<_, String>(0)?,
            "tool": row.get::<_, String>(1)?,
            "label": row.get::<_, String>(2)?,
            "detail": row.get::<_, Option<String>>(3)?,
            "state": row.get::<_, String>(4)?,
            "createdAt": row.get::<_, String>(5)?,
        }))
    });
    rows.ok()
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
}

pub fn activity_for_conversation(db: &Connection, conversation_id: &str) -> Vec<Value> {
    match task_for_child_conversation(db, conversation_id) {
        Ok(Some(task)) => activity_for_task(db, &task.id),
        _ => Vec::new(),
    }
}

pub fn finish(
    db: &Connection,
    id: &str,
    state: &str,
    result: Option<&str>,
    error: Option<&str>,
) -> Result<(), String> {
    let result = result.map(|value| value.chars().take(MAX_RESULT).collect::<String>());
    set_state(db, id, state, result.as_deref(), error)
}

fn clean(value: &str, max: usize, multiline: bool) -> Option<String> {
    let text = value.trim();
    if text.is_empty() || text.chars().count() > max {
        return None;
    }
    if text
        .chars()
        .any(|character| character.is_control() && !(multiline && (character == '\n' || character == '\t')))
    {
        return None;
    }
    Some(text.to_string())
}

fn active_counts(db: &Connection, parent: &str) -> Result<(i64, i64), String> {
    let per_parent: i64 = db
        .query_row(
            "SELECT count(*) FROM agent_delegations WHERE parent_conversation_id=?1 AND state IN ('queued','running','waiting')",
            [parent],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let total: i64 = db
        .query_row(
            "SELECT count(*) FROM agent_delegations WHERE state IN ('queued','running','waiting')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    Ok((per_parent, total))
}

struct RolePick {
    model: &'static str,
    effort: &'static str,
}





fn worker_routing(provider_id: &str) -> Option<(RolePick, RolePick)> {
    let (flash, smart) = match provider_id {
        "chatgpt" => (
            RolePick { model: "gpt-6-luna", effort: "low" },
            RolePick { model: "gpt-6-luna", effort: "high" },
        ),
        "opencode" => (
            RolePick { model: "deepseek-v4.1-flash", effort: "low" },
            RolePick { model: "deepseek-v4.1-flash", effort: "high" },
        ),
        "opencode-go" => (
            RolePick { model: "muse-spark-1.3-contributor", effort: "high" },
            RolePick { model: "deepseek-v4.1-flash", effort: "high" },
        ),
        "openrouter" => (
            RolePick { model: "openrouter/free", effort: "low" },
            RolePick { model: "deepseek/deepseek-v4.1-flash", effort: "max" },
        ),
        "nvidia" => (
            RolePick { model: "nvidia/nemotron-3.5-lightning-30b-a3b", effort: "low" },
            RolePick { model: "nvidia/nemotron-3.5-lightning-30b-a3b", effort: "high" },
        ),
        _ => return None,
    };
    Some((flash, smart))
}


fn routing_settings(db: &Connection) -> (bool, bool) {
    let value: Option<String> = db
        .query_row(
            "SELECT value FROM settings WHERE key='onboarding'",
            [],
            |row| row.get(0),
        )
        .ok();
    let settings = value
        .and_then(|value| serde_json::from_str::<Value>(&value).ok())
        .unwrap_or(json!({}));
    let auto = settings["routing"].as_str().unwrap_or("auto") != "manual";
    let economy = settings["performance"].as_str().unwrap_or("recommended") == "economy";
    (auto, economy)
}

pub fn start(
    db: &Connection,
    parent_id: &str,
    title: &str,
    brief: &str,
    depth: Option<&str>,
    name: Option<&str>,
) -> Result<DelegationTask, String> {
    let parent = storage::get_conversation(db, parent_id).map_err(|_| "That conversation no longer exists".to_string())?;
    if parent.selection.is_none() {
        return Err("Choose a model for this chat before delegating work.".into());
    }
    if storage::conversation_is_delegated(db, parent_id).unwrap_or(true) {
        return Err("A delegated task cannot create more delegated work.".into());
    }
    let scheduled: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM automation_runs WHERE conversation_id=?1)",
            [parent_id],
            |row| row.get(0),
        )
        .unwrap_or(true);
    if scheduled {
        return Err("Scheduled runs cannot delegate work.".into());
    }
    let title = clean(title, MAX_TITLE, false).ok_or("title must be 1–120 characters")?;
    let brief = clean(brief, MAX_BRIEF, true).ok_or("brief must be 1–4000 characters")?;
    let agent_name = name.and_then(|value| clean(value, 40, false));
    let (per_parent, total) = active_counts(db, parent_id)?;
    if per_parent >= MAX_ACTIVE_PER_PARENT {
        return Err(format!(
            "This chat already has {MAX_ACTIVE_PER_PARENT} children working. Wait for one to settle."
        ));
    }
    if total >= MAX_ACTIVE_TOTAL {
        return Err("Talo is already running as many child tasks as it can. Wait for one to settle.".into());
    }
    let existing: i64 = db
        .query_row(
            "SELECT count(*) FROM agent_delegations WHERE parent_conversation_id=?1",
            [parent_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if existing >= MAX_TASKS_PER_PARENT {
        return Err(format!(
            "This chat has reached its limit of {MAX_TASKS_PER_PARENT} delegated tasks."
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let child_id = uuid::Uuid::new_v4().to_string();
    let selection = parent.selection.as_ref();
    let (auto, economy) = routing_settings(db);
    let mut routed_level: Option<String> = None;
    let mut child_selection = selection.map(|selection| storage::ModelSelection {
        connection_id: selection.connection_id.clone(),
        model_id: selection.model_id.clone(),
    });
    if auto {
        let routed = selection.and_then(|selection| {
            let connection = storage::get_connection(db, &selection.connection_id).ok()?;
            let (flash, smart) = worker_routing(&connection.provider_id)?;
            let pick = match depth {
                Some("flash") => &flash,
                Some("smart") => &smart,
                _ => if economy { &flash } else { &smart },
            };
            Some((selection.connection_id.clone(), pick.model.to_string(), pick.effort.to_string()))
        });
        if let Some((connection_id, model, effort)) = routed {
            routed_level = Some(effort);
            child_selection = Some(storage::ModelSelection { connection_id, model_id: model });
        }
    }
    storage::create_conversation(db, &child_id, &title, child_selection.as_ref())
        .map_err(|_| "The child conversation could not be created".to_string())?;
    storage::mark_delegated_conversation(db, &child_id, parent_id, &id, &parent.workspace_id)
        .map_err(|_| "The child conversation could not be attached".to_string())?;
    let stamp = now();
    db.execute(
        "INSERT INTO agent_delegations(id,parent_conversation_id,child_conversation_id,title,brief,state,delivered,created_at,updated_at,thinking_level,agent_name)
         VALUES(?1,?2,?3,?4,?5,'queued',0,?6,?6,?7,?8)",
        params![id, parent_id, child_id, title, brief, stamp, routed_level, agent_name],
    )
    .map_err(|_| "The delegated task could not be saved".to_string())?;
    crate::telegram::track_task(db, &id, parent_id).map_err(|_| "The task's reply channel could not be saved".to_string())?;
    Ok(DelegationTask {
        id,
        parent_conversation_id: parent_id.to_string(),
        child_conversation_id: child_id,
        title,
        brief,
        state: "queued".into(),
        result: None,
        error: None,
        delivered: false,
        thinking_level: routed_level,
        current_activity: None,
        agent_name,
        created_at: stamp.clone(),
        updated_at: stamp,
    })
}

pub fn emit_task_update(db: &Connection, app: &tauri::AppHandle, id: &str) {
    if let Ok(Some(task)) = get(db, id) {
        emit_task(app, &task);
    }
}

fn emit_task(app: &tauri::AppHandle, task: &DelegationTask) {
    let _ = app.emit(
        chat::EVENT_NAME,
        chat::ChatEvent::Delegation {
            task: serde_json::to_value(task).unwrap_or(Value::Null),
        },
    );
}

pub fn on_generation_finished(
    db: &Connection,
    app: &tauri::AppHandle,
    generation_id: &str,
    state: chat::GenerationState,
    text: &str,
    error: Option<&str>,
) {
    let Ok(Some(task)) = child_conversation_for_generation(db, generation_id) else {
        return;
    };
    let (next, result) = match state {
        chat::GenerationState::Completed => ("completed", Some(text)),
        chat::GenerationState::Cancelled => ("cancelled", Some(text)),
        chat::GenerationState::Failed => ("failed", None),
        _ => return,
    };
    let error = if next == "failed" {
        Some(chat::sanitize_error(
            error.unwrap_or("The delegated task did not complete"),
        ))
    } else {
        None
    };
    let _ = db.execute(
        "UPDATE agent_delegation_activity SET state=?2,updated_at=?3 WHERE task_id=?1 AND state='running'",
        params![task.id, next, now()],
    );
    if finish(db, &task.id, next, result, error.as_deref()).is_err() {
        return;
    }
    if let Ok(Some(updated)) = get(db, &task.id) {
        emit_task(app, &updated);
    }
}

fn delivery_text(tasks: &[DelegationTask]) -> String {
    let mut text = String::from(
        "Delegated work has settled. Use these results to produce the coordinated answer for the user. Do not claim that failed, cancelled or interrupted work succeeded.\n",
    );
    for task in tasks {
        text.push_str(&format!("\nTask: {}\nState: {}", task.title, task.state));
        if let Some(result) = task.result.as_deref().filter(|value| !value.trim().is_empty()) {
            text.push_str("\nResult:\n");
            text.push_str(result);
        }
        if let Some(error) = task.error.as_deref().filter(|value| !value.trim().is_empty()) {
            text.push_str("\nError: ");
            text.push_str(error);
        }
        text.push('\n');
    }
    text.truncate(text.floor_char_boundary(16_000));
    text
}

fn enqueue_deliveries(db: &mut Connection) -> Result<Vec<String>, String> {
    let parents: Vec<String> = {
        let mut statement = db
            .prepare(
                "SELECT DISTINCT parent_conversation_id FROM agent_delegations d
                 WHERE d.delivered=0 AND d.state IN ('completed','failed','cancelled','interrupted')
                   AND EXISTS(SELECT 1 FROM chat_conversations c WHERE c.id=d.parent_conversation_id)
                   AND NOT EXISTS(SELECT 1 FROM agent_delegations pending
                                  WHERE pending.parent_conversation_id=d.parent_conversation_id
                                    AND pending.state IN ('queued','running','waiting'))",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?
    };
    let mut queued_parents = Vec::new();
    for parent in parents {
        let tasks = {
            let mut statement = db
                .prepare(&format!(
                    "SELECT {TASK_COLUMNS} FROM agent_delegations
                     WHERE parent_conversation_id=?1 AND delivered=0
                       AND state IN ('completed','failed','cancelled','interrupted')
                     ORDER BY created_at,rowid"
                ))
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([&parent], task_from_row)
                .map_err(|error| error.to_string())?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| error.to_string())?
        };
        if tasks.is_empty() {
            continue;
        }
        let generation_id = uuid::Uuid::new_v4().to_string();
        let text = delivery_text(&tasks);
        let stamp = now();
        let tx = db.transaction().map_err(|error| error.to_string())?;
        let task_ids = json!(tasks.iter().map(|task| &task.id).collect::<Vec<_>>()).to_string();
        tx.execute("INSERT OR IGNORE INTO telegram_origins(id,connection_id,kind) SELECT ?1,min(connection_id),'generation' FROM telegram_origins WHERE kind='task' AND id IN (SELECT value FROM json_each(?2)) HAVING count(*)=?3 AND count(DISTINCT connection_id)=1",params![generation_id,task_ids,tasks.len() as i64]).map_err(|error|error.to_string())?;
        for task in &tasks {
            tx.execute(
                "UPDATE agent_delegations SET delivered=1,updated_at=?2 WHERE id=?1",
                params![task.id, stamp],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.execute(
            "INSERT INTO chat_turn_queue(generation_id,conversation_id,kind,text,created_at) VALUES(?1,?2,'delegation',?3,?4)",
            params![generation_id, parent, text, stamp],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        queued_parents.push(parent);
    }
    Ok(queued_parents)
}

fn launch_queued(host: &mut Host, app: &tauri::AppHandle) {
    loop {
        let total: i64 = host
            .db
            .query_row(
                "SELECT count(*) FROM agent_delegations d WHERE d.state IN ('running','waiting') AND EXISTS(SELECT 1 FROM chat_conversations c WHERE c.id=d.parent_conversation_id)",
                [],
                |row| row.get(0),
            )
            .unwrap_or(i64::MAX);
        if total >= MAX_ACTIVE_TOTAL {
            break;
        }
        let task = host
            .db
            .query_row(
                &format!(
                    "SELECT {TASK_COLUMNS} FROM agent_delegations d
                     WHERE d.state='queued'
                       AND EXISTS(SELECT 1 FROM chat_conversations c WHERE c.id=d.parent_conversation_id)
                       AND (SELECT count(*) FROM agent_delegations running
                            WHERE running.parent_conversation_id=d.parent_conversation_id
                              AND running.state IN ('running','waiting')) < ?1
                     ORDER BY d.created_at,rowid LIMIT 1"
                ),
                [MAX_ACTIVE_PER_PARENT],
                task_from_row,
            )
            .optional()
            .ok()
            .flatten();
        let Some(task) = task else {
            break;
        };
        if set_state(&host.db, &task.id, "running", None, None).is_err() {
            break;
        }
        emit_task_update(&host.db, app, &task.id);
        if let Err(error) = chat::chat_child_send_inner(app, host, &task)
            .and_then(|deferred| chat::drive(app, host, deferred))
        {
            let _ = finish(&host.db, &task.id, "failed", None, Some(&chat::sanitize_error(&error)));
            emit_task_update(&host.db, app, &task.id);
        }
    }
}


pub fn tick(host: &mut Host, app: &tauri::AppHandle) {
    launch_queued(host, app);
    match enqueue_deliveries(&mut host.db) {
        Ok(parents) => {
            for parent in parents {
                chat::emit_queue(app, &host.db, &parent);
            }
        }
        Err(_) => return,
    }
    chat::drain_queue_inner(app, host);
}

pub fn has_pending(db: &Connection) -> bool {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_delegations d WHERE (d.state IN ('queued','running','waiting') OR d.delivered=0) AND EXISTS(SELECT 1 FROM chat_conversations c WHERE c.id=d.parent_conversation_id))",
        [],
        |row| row.get::<_, bool>(0),
    )
    .unwrap_or(false)
}


pub fn recover(db: &Connection) -> Result<usize, String> {
    let stamp = now();
    let _ = db.execute(
        "UPDATE agent_delegation_activity SET state='failed',updated_at=?1
         WHERE state='running' AND task_id IN (
           SELECT id FROM agent_delegations WHERE state IN ('queued','running','waiting')
         )",
        [&stamp],
    );
    let changed = db
        .execute(
            "UPDATE agent_delegations SET state='interrupted',error='Interrupted before the delegated task completed',updated_at=?1 WHERE state IN ('queued','running','waiting')",
            [&stamp],
        )
        .map_err(|error| error.to_string())?;
    Ok(changed)
}

fn reply(host: &mut Host, id: Value, result: Result<Value, String>) {
    let message = match result {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(error) => json!({"jsonrpc":"2.0","id":id,"error":{"message":error}}),
    };
    host.supervisor.relay_delegation(message);
}

pub fn message(host: &mut Host, app: &tauri::AppHandle, message: Value) {
    if message["method"] != "delegation.call" {
        return;
    }
    let p = message["params"].clone();
    let conversation = p["conversationId"].as_str().unwrap_or("").to_string();
    let operation = p["operation"].as_str().unwrap_or("").to_string();
    let result = (|| -> Result<Value, String> {
        if is_child(&host.db, &conversation) {
            return Err("Delegated child tasks cannot create more delegated work.".into());
        }
        match operation.as_str() {
            "start" => {
                let title = p["title"].as_str().unwrap_or("");
                let brief = p["brief"].as_str().unwrap_or("");
                let depth = p["depth"].as_str().filter(|value| matches!(*value, "flash" | "smart"));
                let name = p["name"].as_str();
                let task = start(&host.db, &conversation, title, brief, depth, name)?;
                emit_task(app, &task);
                Ok(json!({"id":task.id,"title":task.title,"state":task.state}))
            }
            "list" => Ok(json!({"tasks": list(&host.db, &conversation)?})),
            "cancel" => {
                let id = p["taskId"].as_str().ok_or("taskId is required")?;
                let task = get(&host.db, id)?.ok_or("That delegated task no longer exists")?;
                if task.parent_conversation_id != conversation {
                    return Err("That delegated task belongs to another chat.".into());
                }
                cancel(host, app, id)?;
                Ok(json!({"cancelled":true}))
            }
            "cancel_all" => {
                let count = cancel_all(host, app, &conversation)?;
                Ok(json!({"cancelled":count}))
            }
            _ => Err("Unknown delegation operation".into()),
        }
    })();
    reply(host, message["id"].clone(), result);
}

pub fn cancel(host: &mut Host, app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let Some(task) = get(&host.db, id)? else {
        return Err("That delegated task no longer exists".into());
    };
    if matches!(task.state.as_str(), "completed" | "failed" | "cancelled" | "interrupted") {
        return Ok(());
    }
    let generation = host
        .db
        .query_row(
            "SELECT generation_id FROM agent_delegations WHERE id=?1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten();
    if let Some(generation) = generation {
        if host.chat.has_active_for(&task.child_conversation_id) {
            chat::chat_stop_inner(app, host, generation)?;
        }
    }
    finish(&host.db, id, "cancelled", None, Some("Cancelled by the user"))?;
    if let Ok(Some(updated)) = get(&host.db, id) {
        emit_task(app, &updated);
    }
    Ok(())
}

pub fn cancel_all(host: &mut Host, app: &tauri::AppHandle, parent: &str) -> Result<usize, String> {
    let ids: Vec<String> = {
        let mut statement = host
            .db
            .prepare(
                "SELECT id FROM agent_delegations WHERE parent_conversation_id=?1 AND state IN ('queued','running','waiting')",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([parent], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?
    };
    for id in &ids {
        cancel(host, app, id)?;
    }
    Ok(ids.len())
}

pub fn open_task(host: &mut Host, id: &str) -> Result<storage::ConversationSnapshot, String> {
    let task = get(&host.db, id)?.ok_or("That delegated task no longer exists")?;
    let conversation =
        storage::get_conversation(&host.db, &task.child_conversation_id).map_err(|error| error.to_string())?;
    chat::conversation_snapshot_inner(host, conversation)
}

#[tauri::command]
pub async fn delegation_action(
    app: tauri::AppHandle,
    operation: String,
    payload: Option<Value>,
) -> Result<Value, String> {
    run_blocking(app, move |app| {
        let shared = app.state::<Shared>();
        let mut host = shared.lock().map_err(|_| "Delegation is unavailable")?;
        let payload = payload.unwrap_or(json!({}));
        match operation.as_str() {
            "list" => {
                let conversation = payload["conversationId"]
                    .as_str()
                    .ok_or("conversationId is required")?;
                Ok(json!({"tasks":list(&host.db, conversation)?}))
            }
            "open" => {
                let id = payload["taskId"].as_str().ok_or("taskId is required")?;
                Ok(serde_json::to_value(open_task(&mut host, id)?).unwrap_or(Value::Null))
            }
            "cancel" => {
                let id = payload["taskId"].as_str().ok_or("taskId is required")?;
                cancel(&mut host, app, id)?;
                Ok(json!(true))
            }
            "cancel_all" => {
                let conversation = payload["conversationId"]
                    .as_str()
                    .ok_or("conversationId is required")?;
                Ok(json!({"cancelled":cancel_all(&mut host, app, conversation)?}))
            }
            _ => Err("Unknown delegation operation".into()),
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../services/memory/schema.sql"))
            .unwrap();
        storage::init(&db).unwrap();
        crate::scheduler::init(&db).unwrap();
        init(&db).unwrap();
        db
    }

    #[test]
    fn children_are_hidden_and_limits_are_per_parent() {
        let db = database();
        let selection = storage::ModelSelection { connection_id: "conn".into(), model_id: "model".into() };
        storage::create_conversation(&db, "parent", "Parent", Some(&selection)).unwrap();
        let first = start(&db, "parent", "One", "Read source A", None, Some("Scout")).unwrap();
        let second = start(&db, "parent", "Two", "Read source B", None, None).unwrap();
        assert_eq!(first.state, "queued");
        assert_eq!(first.agent_name.as_deref(), Some("Scout"));
        assert!(second.agent_name.is_none());
        assert!(storage::get_conversation(&db, &first.child_conversation_id).is_ok());
        assert!(storage::list_conversations(&db).unwrap().iter().all(|c| c.id != first.child_conversation_id));
        assert!(start(&db, "parent", "Three", "Too many", None, None).is_err());

        finish(&db, &first.id, "completed", Some("A done"), None).unwrap();
        set_state(&db, &second.id, "running", None, None).unwrap();
        let third = start(&db, "parent", "Three", "Now there is room", None, None).unwrap();
        assert_eq!(third.state, "queued");
        assert!(storage::conversation_is_delegated(&db, &third.child_conversation_id).unwrap());
    }

    #[test]
    fn settled_children_are_delivered_once_as_one_parent_turn() {
        let mut db = database();
        let selection = storage::ModelSelection { connection_id: "conn".into(), model_id: "model".into() };
        storage::create_conversation(&db, "parent", "Parent", Some(&selection)).unwrap();
        let first = start(&db, "parent", "One", "Read source A", None, None).unwrap();
        let second = start(&db, "parent", "Two", "Read source B", None, None).unwrap();
        finish(&db, &first.id, "completed", Some("A done"), None).unwrap();
        finish(&db, &second.id, "failed", None, Some("B failed")).unwrap();

        assert_eq!(enqueue_deliveries(&mut db).unwrap(), vec!["parent".to_string()]);
        assert!(enqueue_deliveries(&mut db).unwrap().is_empty());
        let queue = storage::list_queue(&db, "parent").unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].kind, "delegation");
        assert!(queue[0].text.contains("A done"));
        assert!(queue[0].text.contains("B failed"));
        assert!(get(&db, &first.id).unwrap().unwrap().delivered);
        assert!(get(&db, &second.id).unwrap().unwrap().delivered);
    }

    #[test]
    fn child_actions_are_persisted_as_bounded_plain_steps() {
        let db = database();
        let selection = storage::ModelSelection { connection_id: "conn".into(), model_id: "model".into() };
        storage::create_conversation(&db, "parent", "Parent", Some(&selection)).unwrap();
        let task = start(&db, "parent", "One", "Read source A", None, None).unwrap();
        set_generation(&db, &task.id, "gen-child", "running").unwrap();
        record_activity(
            &db,
            &task.child_conversation_id,
            "gen-child",
            &json!({"id":"call-1","tool":"web_search","label":"Searching the web","detail":"Rust docs","state":"running"}),
        )
        .unwrap();
        record_activity(
            &db,
            &task.child_conversation_id,
            "gen-child",
            &json!({"id":"call-1","tool":"web_search","label":"Searching the web","detail":"Rust docs","state":"completed"}),
        )
        .unwrap();
        let stored = activity_for_task(&db, &task.id);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0]["state"], "completed");
        assert_eq!(get(&db, &task.id).unwrap().unwrap().current_activity.as_deref(), Some("Searching the web"));
        assert_eq!(activity_for_conversation(&db, &task.child_conversation_id).len(), 1);
    }

    #[test]
    fn recovery_marks_interrupted_without_replaying() {
        let db = database();
        let selection = storage::ModelSelection { connection_id: "conn".into(), model_id: "model".into() };
        storage::create_conversation(&db, "parent", "Parent", Some(&selection)).unwrap();
        let task = start(&db, "parent", "One", "Read source A", None, None).unwrap();
        set_state(&db, &task.id, "running", None, None).unwrap();
        set_generation(&db, &task.id, "gen-child", "running").unwrap();
        assert_eq!(recover(&db).unwrap(), 1);
        let recovered = get(&db, &task.id).unwrap().unwrap();
        assert_eq!(recovered.state, "interrupted");
        assert!(recovered.error.unwrap().contains("Interrupted"));
        assert_eq!(recover(&db).unwrap(), 0);
    }

    #[test]
    fn routing_swaps_the_child_model_and_level_for_curated_providers() {
        let db = database();
        db.execute(
            "INSERT INTO provider_connections(id,provider_id,name,connected,created_at,updated_at)
             VALUES('conn','openrouter','OpenRouter',1,'2026-01-01','2026-01-01')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO settings(key,value) VALUES('onboarding','{\"routing\":\"auto\",\"performance\":\"economy\"}')",
            [],
        )
        .unwrap();
        let selection = storage::ModelSelection { connection_id: "conn".into(), model_id: "pilot-model".into() };
        storage::create_conversation(&db, "parent", "Parent", Some(&selection)).unwrap();

        let economy = start(&db, "parent", "One", "Quick lookup", None, None).unwrap();
        assert_eq!(economy.thinking_level.as_deref(), Some("low"));
        let child = storage::get_conversation(&db, &economy.child_conversation_id).unwrap();
        let child_selection = child.selection.expect("routed child keeps a selection");
        assert_eq!(child_selection.connection_id, "conn");
        assert_eq!(child_selection.model_id, "openrouter/free");

        let smart = start(&db, "parent", "Two", "Hard analysis", Some("smart"), None).unwrap();
        assert_eq!(smart.thinking_level.as_deref(), Some("max"));
        let child = storage::get_conversation(&db, &smart.child_conversation_id).unwrap();
        assert_eq!(child.selection.unwrap().model_id, "deepseek/deepseek-v4.1-flash");
    }

    #[test]
    fn manual_routing_and_unmapped_providers_inherit_the_pilot_model() {
        let db = database();
        db.execute(
            "INSERT INTO provider_connections(id,provider_id,name,connected,created_at,updated_at)
             VALUES('conn','openrouter','OpenRouter',1,'2026-01-01','2026-01-01')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO settings(key,value) VALUES('onboarding','{\"routing\":\"manual\"}')",
            [],
        )
        .unwrap();
        let selection = storage::ModelSelection { connection_id: "conn".into(), model_id: "pilot-model".into() };
        storage::create_conversation(&db, "parent", "Parent", Some(&selection)).unwrap();
        let manual = start(&db, "parent", "One", "Read source A", Some("smart"), None).unwrap();
        assert_eq!(manual.thinking_level, None);
        let child = storage::get_conversation(&db, &manual.child_conversation_id).unwrap();
        assert_eq!(child.selection.unwrap().model_id, "pilot-model");

        let unmapped = database();
        let selection = storage::ModelSelection { connection_id: "other".into(), model_id: "pilot-model".into() };
        storage::create_conversation(&unmapped, "parent", "Parent", Some(&selection)).unwrap();
        let inherited = start(&unmapped, "parent", "Two", "Read source B", None, None).unwrap();
        assert_eq!(inherited.thinking_level, None);
        let child = storage::get_conversation(&unmapped, &inherited.child_conversation_id).unwrap();
        assert_eq!(child.selection.unwrap().model_id, "pilot-model");
    }
}
