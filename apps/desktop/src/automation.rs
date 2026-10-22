//! Native background executor. The chat pipeline owns sessions, progress, permissions and credentials.
use crate::{chat, library, run_blocking, scheduler, storage, Host, Shared};
use rusqlite::params;
use serde_json::{json, Value};
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

pub fn paused(h: &Host) -> bool {
    library::setting(&h.db, "automation_paused", "false") == "true"
}
pub fn set_paused(h: &Host, value: bool) -> Result<(), String> {
    h.db.execute("INSERT INTO settings(key,value) VALUES('automation_paused',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[value.to_string()]).map_err(|e|e.to_string())?;
    Ok(())
}
pub fn current(h: &Host) -> Option<String> {
    h.db.query_row(
        "SELECT id FROM automation_runs WHERE status IN ('running','awaiting_approval') LIMIT 1",
        [],
        |r| r.get(0),
    )
    .ok()
}
pub fn notify(app: &tauri::AppHandle, run: &str, title: &str) {
    // Only IDs and fixed event text go to diagnostics/toasts; prompts and credentials stay local.
    let _ = app
        .notification()
        .builder()
        .title("Talo")
        .body(title)
        .extra("runId", json!(run))
        .show();
    let _ = app.emit("automation-changed", ());
}
pub fn event(h: &Host, run: Option<&str>, event: &str) {
    let _ = h.db.execute(
        "INSERT INTO automation_events(run_id,event) VALUES(?1,?2)",
        params![run, event],
    );
}
pub fn reserve(h: &Host, action: &str, scope: &str) -> bool {
    if !["mcp.call", "files.write", "files.edit", "browser.action"].contains(&action) {
        return true;
    }
    let Some(run) = current(h) else { return true };
    scheduler::reserve_action(&h.db, &run, action, scope).unwrap_or(false)
}
pub fn tick(h: &mut Host, app: &tauri::AppHandle) -> Result<(), String> {
    if crate::power::is_suspended() {
        return Ok(());
    }
    let now = chrono::Utc::now().timestamp();
    let active:Option<(String,String,Option<String>,i64,String)>=h.db.query_row(
        "SELECT r.id,r.generation_id,g.state,r.started_at,r.definition FROM automation_runs r LEFT JOIN chat_generations g ON g.id=r.generation_id WHERE r.status IN ('running','awaiting_approval') LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).ok();
    if let Some((run, generation, state, started, definition)) = active {
        let task: scheduler::Task = serde_json::from_str(&definition).map_err(|e| e.to_string())?;
        if matches!(state.as_deref(), Some("completed" | "failed" | "cancelled")) {
            let status = state.unwrap();
            if status != "completed" {
                h.db.execute("UPDATE automation_tasks SET paused=1 WHERE id=(SELECT task_id FROM automation_runs WHERE id=?1)",[&run]).map_err(|e|e.to_string())?;
                stop_run(h, app, generation.clone())?;
            }
            h.db.execute("UPDATE automation_runs SET status=?2,finished_at=?3,error=(SELECT error FROM chat_generations WHERE id=?4) WHERE id=?1",params![run,status,now,generation]).map_err(|e|e.to_string())?;
            event(h, Some(&run), &status);
            notify(
                app,
                &run,
                if status == "completed" {
                    "Scheduled task completed"
                } else {
                    "Scheduled task needs attention"
                },
            );
        } else if now - started >= task.timeout_seconds {
            stop_run(h, app, generation)?;
            h.db.execute("UPDATE automation_tasks SET paused=1 WHERE id=(SELECT task_id FROM automation_runs WHERE id=?1)",[&run]).map_err(|e|e.to_string())?;
            h.db.execute("UPDATE automation_runs SET status='uncertain',finished_at=?2,error='Execution timed out. Review progress before repeating actions.' WHERE id=?1",params![run,now]).map_err(|e|e.to_string())?;
            event(h, Some(&run), "timeout");
            notify(app, &run, "Scheduled task timed out; review its progress");
        } else {
            let waiting = !h.integrations.pending.is_empty() || !h.browser_approvals.is_empty();
            h.db.execute(
                "UPDATE automation_runs SET status=?2 WHERE id=?1 AND status<>?2",
                params![
                    run,
                    if waiting {
                        "awaiting_approval"
                    } else {
                        "running"
                    }
                ],
            )
            .map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if paused(h) || h.chat.any_active() {
        return Ok(());
    }
    let Some(claimed) = scheduler::claim(&mut h.db, now)? else {
        return Ok(());
    };
    let task = claimed.task;
    let run = claimed.run_id;
    let conversation = uuid::Uuid::new_v4().to_string();
    let generation = uuid::Uuid::new_v4().to_string();
    // Persist association BEFORE contacting Pi. A crash at any later point is uncertain, never replayed.
    h.db.execute(
        "UPDATE automation_runs SET conversation_id=?2,generation_id=?3 WHERE id=?1",
        params![run, conversation, generation],
    )
    .map_err(|e| e.to_string())?;
    event(h, Some(&run), "started");
    let launch = (|| {
        let selection = storage::ModelSelection {
            connection_id: task.connection_id,
            model_id: task.model_id,
        };
        storage::create_conversation(&h.db, &conversation, &task.title, Some(&selection))
            .map_err(|e| e.to_string())?;
        h.db.execute(
            "UPDATE chat_conversations SET workspace_id=?2 WHERE id=?1",
            params![conversation, task.workspace_id],
        )
        .map_err(|e| e.to_string())?;
        // Retry startup once, before a prompt or external tool can have been dispatched.
        if h.supervisor.ensure_worker().is_err() {
            event(h, Some(&run), "worker_start_retry");
            h.supervisor.ensure_worker()?;
        }
        // A failed startup attempt may have queued an exit for the previous epoch.
        // Reconcile it before creating this run's live generation.
        for lifecycle in h.supervisor.drain_lifecycle() {
            h.chat.on_lifecycle(&mut h.db, app, lifecycle);
        }
        chat::chat_send_inner(app, h, conversation, generation, task.prompt, vec![])
    })();
    if let Err(error) = launch {
        h.db.execute("UPDATE automation_tasks SET paused=1 WHERE id=(SELECT task_id FROM automation_runs WHERE id=?1)",[&run]).map_err(|e|e.to_string())?;
        h.db.execute(
            "UPDATE automation_runs SET status='failed',finished_at=?2,error=?3 WHERE id=?1",
            params![run, now, chat::sanitize_error(&error)],
        )
        .map_err(|e| e.to_string())?;
        notify(app, &run, "Scheduled task could not start");
    }
    Ok(())
}
pub fn stop_run(h: &mut Host, app: &tauri::AppHandle, generation: String) -> Result<(), String> {
    chat::chat_stop_inner(app, h, generation)?;
    // A timed-out abort acknowledgement is not proof of stopped tool execution. Reap Pi's job tree.
    h.supervisor.stop_pi()?;
    h.chat.on_worker_intentionally_stopped(app);
    h.integrations.pending.clear();
    h.browser_approvals.clear();
    Ok(())
}
#[tauri::command]
pub async fn automation_action(
    app: tauri::AppHandle,
    operation: String,
    payload: Option<Value>,
) -> Result<Value, String> {
    run_blocking(app,move|app|{
        let shared=app.state::<Shared>();let mut h=shared.lock().map_err(|_|"Automation unavailable")?;
        let p=payload.unwrap_or(json!({}));
        match operation.as_str(){
            "notification"=>{
                let run=library::setting(&h.db,"notification_run","");
                h.db.execute("DELETE FROM settings WHERE key='notification_run'",[]).map_err(|e|e.to_string())?;
                return Ok(json!(run));
            }
            "state"=>{
                let tasks=scheduler::list(&h.db).map_err(|e|e.to_string())?;
                let mut stmt=h.db.prepare("SELECT r.id,r.task_id,r.status,r.scheduled_at,r.started_at,r.finished_at,r.conversation_id,r.error,r.definition,r.usage FROM automation_runs r ORDER BY r.started_at DESC LIMIT 200").map_err(|e|e.to_string())?;
                let runs=stmt.query_map([],|r|{
                    let raw:String=r.get(8)?;let task:Value=serde_json::from_str(&raw).unwrap_or(Value::Null);
                    let usage=r.get::<_,Option<String>>(9)?.and_then(|v|serde_json::from_str::<Value>(&v).ok());
                    Ok(json!({"id":r.get::<_,String>(0)?,"taskId":r.get::<_,String>(1)?,"status":r.get::<_,String>(2)?,"scheduledAt":r.get::<_,i64>(3)?,"startedAt":r.get::<_,Option<i64>>(4)?,"finishedAt":r.get::<_,Option<i64>>(5)?,"conversationId":r.get::<_,Option<String>>(6)?,"error":r.get::<_,Option<String>>(7)?,"title":task["title"],"usage":usage}))
                }).map_err(|e|e.to_string())?.collect::<rusqlite::Result<Vec<_>>>().map_err(|e|e.to_string())?;
                let resources=h.db.query_row::<String,_,_>("SELECT event FROM automation_events WHERE event LIKE '{%' ORDER BY id DESC LIMIT 1",[],|r|r.get(0)).ok().and_then(|v|serde_json::from_str::<Value>(&v).ok());
                return Ok(json!({"tasks":tasks,"runs":runs,"resources":resources,"paused":paused(&h),"runtime":if h.chat.any_active(){"working"}else if h.supervisor.worker_ready(){"ready"}else{"sleeping"}}));
            }
            "save"=>{
                let mut task:scheduler::Task=serde_json::from_value(p).map_err(|e|e.to_string())?;
                if task.workspace_id.is_empty(){task.workspace_id=library::setting(&h.db,"active_workspace","general");}
                storage::get_connection(&h.db,&task.connection_id).map_err(|_|"Choose a connected provider")?;
                scheduler::save(&h.db,task,chrono::Utc::now().timestamp())?;
            }
            "pause_automation"=>set_paused(&h,p["paused"].as_bool().ok_or("Paused flag required")?)?,
            "pause"|"delete"=>{
                let id=p["id"].as_str().ok_or("Task ID required")?;
                if operation=="delete" {
                    h.db.execute("UPDATE automation_tasks SET deleted=1,paused=1 WHERE id=?1",[id]).map_err(|e|e.to_string())?;
                } else {
                    h.db.execute("UPDATE automation_tasks SET paused=?2 WHERE id=?1",params![id,p["paused"].as_bool().ok_or("Paused flag required")?]).map_err(|e|e.to_string())?;
                }
            }
            "cancel"=>{
                let id=p["id"].as_str().ok_or("Run ID required")?;
                let generation:String=h.db.query_row("SELECT generation_id FROM automation_runs WHERE id=?1 AND status IN ('running','awaiting_approval')",[id],|r|r.get(0)).map_err(|_|"Run is no longer active")?;
                stop_run(&mut h,app,generation)?;
                h.db.execute("UPDATE automation_tasks SET paused=1 WHERE id=(SELECT task_id FROM automation_runs WHERE id=?1)",[id]).map_err(|e|e.to_string())?;
                h.db.execute("UPDATE automation_runs SET status='cancelled',finished_at=unixepoch() WHERE id=?1",[id]).map_err(|e|e.to_string())?;
                event(&h,Some(id),"cancelled");
            }
            _=>return Err("Unknown automation operation".into())
        }
        let _=app.emit("automation-changed",());Ok(json!(true))
    }).await
}
