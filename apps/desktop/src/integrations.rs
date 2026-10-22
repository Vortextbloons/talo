//! Host-owned MCP configuration and exact-operation permission broker.
use crate::{credentials, run_blocking, Shared};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

#[derive(Default)]
pub struct Integrations {
    pub status: Value,
    pub pending: HashMap<String, (Value, Instant, u64)>,
}
pub fn load(data: &Path) -> Result<Value, String> {
    let path = data.join("tools.sealed");
    if !path.exists() {
        return Ok(json!({"servers":[],"grants":[],"disabled":[]}));
    }
    let secret = credentials::unprotect(
        &std::fs::read(path).map_err(|_| "Cannot read protected tool configuration")?,
    )?;
    serde_json::from_str(secret.as_str()?)
        .map_err(|_| "Protected tool configuration is damaged".into())
}
fn save(data: &Path, value: &Value) -> Result<(), String> {
    let plaintext = credentials::Secret::new(value.to_string());
    let sealed = credentials::protect(plaintext.as_str()?.as_bytes())?;
    let temp = data.join("tools.sealed.tmp");
    std::fs::write(&temp, sealed).map_err(|_| "Cannot save tool configuration")?;
    std::fs::rename(&temp, data.join("tools.sealed"))
        .map_err(|_| "Cannot replace tool configuration".into())
}
fn public_config(config: &Value) -> Value {
    let mut result = config.clone();
    if let Some(servers) = result["servers"].as_array_mut() {
        for server in servers {
            let has = server["credential"].as_str().is_some_and(|s| !s.is_empty());
            if let Some(s) = server.as_object_mut() {
                s.remove("credential");
                s.insert("hasCredential".into(), json!(has));
            }
        }
    }
    result
}
pub fn reply(h: &mut crate::Host, id: Value, allowed: bool) {
    h.supervisor
        .relay_tools(json!({"id":id,"result":{"allowed":allowed}}));
}
pub fn message(h: &mut crate::Host, app: &tauri::AppHandle, message: Value) {
    if message["method"] == "automation.usage" {
        let p=&message["params"];
        let generation=p["generationId"].as_str().unwrap_or("");
        let _=h.db.execute("UPDATE automation_runs SET usage=?2 WHERE generation_id=?1",rusqlite::params![generation,p.to_string()]);
        return;
    }
    if message["method"] == "status" {
        h.integrations.status = message["params"].clone();
        let _ = app.emit("tools-changed", ());
        return;
    }
    if message["method"] == "notifications/cancelled" {
        let request = &message["params"]["requestId"];
        h.integrations
            .pending
            .retain(|_, (m, _, _)| &m["id"] != request);
        let _ = app.emit("tool-approvals-changed", ());
        return;
    }
    if message["method"] != "permission" {
        return;
    }
    let config = match load(&h.supervisor.data) {
        Ok(c) => c,
        Err(_) => {
            reply(h, message["id"].clone(), false);
            return;
        }
    };
    let p = &message["params"];
    let action = p["action"].as_str().unwrap_or("");
    let scope = p["scope"].as_str().unwrap_or("");
    if action == "tool.use" {
        let known = [
            "web_search",
            "web_fetch",
            "web_crawl",
            "web_screenshot",
            "browser_open",
            "browser_tabs",
            "browser_snapshot",
            "browser_interact",
            "browser_ai",
            "browser_screenshot",
            "browser_download",
            "browser_handoff",
            "mcp",
            "tools",
            "agent",
            "read",
            "write",
            "edit",
        ];
        let disabled = config["disabled"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(scope)));
        reply(
            h,
            message["id"].clone(),
            known.contains(&scope) && !disabled,
        );
        return;
    }
    if !["mcp.call", "files.read", "files.write", "files.edit"].contains(&action)
        || scope.len() > 32768
    {
        reply(h, message["id"].clone(), false);
        return;
    }
    // Scheduled work waits at the same exact-operation broker; it never acquires grants itself.
    let attended = app
        .get_webview_window("main")
        .is_some_and(|w| w.is_visible().unwrap_or(false));
    let background = crate::automation::current(h);
    let remote = crate::telegram::owns(h, p["conversationId"].as_str().unwrap_or(""), p["generationId"].as_str().unwrap_or(""));
    match crate::permissions::policy(action, scope, &config["grants"], attended || background.is_some() || remote) {
        crate::permissions::Decision::Allow => {
            let allowed=crate::automation::reserve(h,action,scope);
            reply(h, message["id"].clone(), allowed);
            return;
        }
        crate::permissions::Decision::Deny => {
            reply(h, message["id"].clone(), false);
            return;
        }
        crate::permissions::Decision::Prompt => {}
    }
    if h.integrations.pending.len() >= 8 {
        reply(h, message["id"].clone(), false);
        return;
    }
    h.integrations.pending.insert(
        uuid::Uuid::new_v4().to_string(),
        (message, Instant::now(), h.supervisor.worker_epoch()),
    );
    if let Some(run) = background {
        crate::automation::notify(app, &run, "A scheduled task needs your approval. Open Talo within 85 seconds.");
    }
    let _ = app.emit("tool-approvals-changed", ());
}
pub fn expire(h: &mut crate::Host) -> bool {
    let epoch = h.supervisor.worker_epoch();
    let ids: Vec<_> = h
        .integrations
        .pending
        .iter()
        .filter(|(_, (m, at, e))| *e != epoch || at.elapsed() > Duration::from_secs(85) || !live(h, m))
        .map(|(id, _)| id.clone())
        .collect();
    let changed = !ids.is_empty();
    for id in ids {
        if let Some((m, _, owner)) = h.integrations.pending.remove(&id) {
            if owner == epoch {
                reply(h, m["id"].clone(), false);
            }
        }
    }
    changed
}

fn live(h: &crate::Host, message: &Value) -> bool {
    let p = &message["params"];
    let Some(generation) = p["generationId"].as_str() else { return true; };
    h.db.query_row("SELECT EXISTS(SELECT 1 FROM chat_generations WHERE id=?1 AND conversation_id=?2 AND state='generating')", rusqlite::params![generation,p["conversationId"].as_str().unwrap_or("")], |r| r.get(0)).unwrap_or(false)
}

pub fn resolve_pending(h: &mut crate::Host, app: &tauri::AppHandle, id: &str, decision: &str) -> Result<(), String> {
    let mut config = load(&h.supervisor.data)?;
    let valid = h.integrations.pending.get(id).is_some_and(|(m,at,epoch)| at.elapsed()<Duration::from_secs(85) && *epoch==h.supervisor.worker_epoch() && live(h,m));
    let (message,_,_) = h.integrations.pending.remove(id).ok_or("Approval expired or cancelled")?;
    let allowed = valid && ["once","scope"].contains(&decision) && crate::automation::reserve(h,message["params"]["action"].as_str().unwrap_or(""),message["params"]["scope"].as_str().unwrap_or(""));
    if allowed && decision=="scope" {
        config["grants"].as_array_mut().ok_or("Invalid grants")?.push(json!({"id":uuid::Uuid::new_v4().to_string(),"action":message["params"]["action"],"scope":message["params"]["scope"],"summary":message["params"]["summary"]}));
        if let Err(error)=save(&h.supervisor.data,&config) { reply(h,message["id"].clone(),false); return Err(error); }
    }
    let audit = h.db.execute("INSERT INTO permission_audit(id,action,resource,allowed) VALUES(?1,?2,?3,?4)",rusqlite::params![id,message["params"]["action"].as_str().unwrap_or(""),"protected tool operation",allowed]);
    reply(h,message["id"].clone(),allowed && audit.is_ok());
    let _ = app.emit("tool-approvals-changed",());
    audit.map_err(|_|"Cannot record approval".to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn tools_action(
    app: tauri::AppHandle,
    operation: String,
    payload: Option<Value>,
) -> Result<Value, String> {
    run_blocking(app,move|app|{
        let shared=app.state::<Shared>();let mut h=shared.lock().map_err(|_|"Tool state unavailable")?;
        let p=payload.unwrap_or(json!({}));let mut config=load(&h.supervisor.data)?;
        match operation.as_str(){
            "state"=>{
                let mut state=public_config(&config);
                if let Some(servers)=state["servers"].as_array_mut(){for s in servers{
                    if let Some(health)=h.integrations.status["servers"].as_array().filter(|_|h.supervisor.worker_ready()).and_then(|a|a.iter().find(|r|r["id"]==s["id"])){
                        s["state"]=health["state"].clone();s["tools"]=health["tools"].clone();s["error"]=health["error"].clone();
                    }else{s["state"]=json!("disconnected");}
                }}
                state["tools"]=catalog(&config);
                state["errors"]=h.integrations.status["errors"].as_array().map(|a|json!(a)).unwrap_or(json!([]));
                state["pending"]=json!(h.integrations.pending.iter().map(|(id,(m,_,_))|json!({"id":id,"action":m["params"]["action"],"scope":m["params"]["scope"],"summary":m["params"]["summary"]})).collect::<Vec<_>>());
                return Ok(state);
            }
            "save"=>{
                let mut server=p["server"].clone();
                let id=server["id"].as_str().ok_or("Server ID required")?.to_string();
                let servers=config["servers"].as_array_mut().ok_or("Invalid stored servers")?;
                if let Some(old)=servers.iter().find(|s|s["id"]==id){if server["credential"].is_null(){server["credential"]=old["credential"].clone();}}
                servers.retain(|s|s["id"]!=id);servers.push(server);
                // Definition changes revoke every old approval before the new process starts.
                config["grants"]=json!([]);
            }
            "remove"=>{config["servers"].as_array_mut().ok_or("Invalid stored servers")?.retain(|s|s["id"]!=p["id"]);config["grants"]=json!([]);}
            "revoke"=>{config["grants"].as_array_mut().ok_or("Invalid grants")?.retain(|g|g["id"]!=p["id"]);save(&h.supervisor.data,&config)?;return Ok(json!(true));}
            "toggle"=>{
                let id=p["id"].as_str().ok_or("Tool ID required")?;
                let disabled=config["disabled"].as_array_mut().ok_or("Invalid tool configuration")?;
                disabled.retain(|v|v!=id);if p["enabled"]!=true{disabled.push(json!(id));}
                if h.supervisor.worker_ready(){h.supervisor.request("tools.configure",json!({"servers":config["servers"],"disabled":config["disabled"]}),Duration::from_secs(15)).map_err(|e|e.to_string())?;}
                save(&h.supervisor.data,&config)?;return Ok(json!(true));
            }
            "resolve"=>{
                let id=p["id"].as_str().ok_or("Approval ID required")?;
                resolve_pending(&mut h,app,id,p["decision"].as_str().unwrap_or("deny"))?;
                return Ok(json!(true));
            }
            "test"|"cancel"=>{
                h.supervisor.ensure_worker()?;
                return h.supervisor.request(if operation=="test"{"tools.test"}else{"tools.cancel"},json!({"server":p["id"]}),Duration::from_secs(10)).map_err(|e|e.to_string());
            }
            _=>return Err("Unknown tools operation".into())
        }
        // Validate through the worker without executing a process. No project files are read.
        h.supervisor.ensure_worker()?;
        h.supervisor.request("tools.configure",json!({"servers":config["servers"],"disabled":config["disabled"]}),Duration::from_secs(15)).map_err(|e|e.to_string())?;
        save(&h.supervisor.data,&config)?;
        h.integrations.pending.clear();
        let _=app.emit("tools-changed",());Ok(json!(true))
    }).await
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn configuration_and_revocable_grants_survive_restart_without_plaintext() {
        let data = std::env::temp_dir().join(format!("talo-tool-config-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&data).unwrap();
        let mut config = json!({"servers":[{"id":"private","transport":"http","url":"https://example.com/mcp","credential":"fixture-private-token","trusted":true,"enabled":true}],"grants":[{"id":"grant","action":"mcp.call","scope":"exact operation"}],"disabled":["write"]});
        save(&data, &config).unwrap();
        let bytes = std::fs::read(data.join("tools.sealed")).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("fixture-private-token"));
        assert_eq!(load(&data).unwrap(), config);
        let public = public_config(&load(&data).unwrap());
        assert!(!public.to_string().contains("fixture-private-token"));
        assert_eq!(public["servers"][0]["hasCredential"], true);
        config["grants"] = json!([]);
        save(&data, &config).unwrap();
        assert_eq!(load(&data).unwrap()["grants"], json!([]));
        std::fs::remove_dir_all(&data).unwrap();
    }
}
fn catalog(config: &Value) -> Value {
    let groups = [
        (
            "Web Research",
            vec!["web_search", "web_fetch", "web_crawl", "web_screenshot"],
            "web.public",
            "Research MCP / DonSeTch",
        ),
        (
            "Browser",
            vec![
                "browser_open",
                "browser_tabs",
                "browser_snapshot",
                "browser_interact",
                "browser_ai",
                "browser_screenshot",
                "browser_download",
                "browser_handoff",
            ],
            "browser.action",
            "Browser MCP / Chromium / Playwright / Stagehand",
        ),
        (
            "Files",
            vec!["read", "write", "edit"],
            "files.approve",
            "Pi file operations",
        ),
        (
            "Memory",
            vec![
                "memory_search",
                "memory_list",
                "memory_save",
                "memory_update",
                "memory_delete",
            ],
            "memory.consent",
            "Existing local memory",
        ),
        (
            "Integrations",
            vec!["tools", "agent"],
            "tool.use",
            "Managed Pi extension / discovery and routing",
        ),
        (
            "Integrations",
            vec!["mcp"],
            "mcp.call",
            "External MCP integrations / through tools",
        ),
    ];
    json!(groups.into_iter().flat_map(|(category,names,permission,resource)|names.into_iter().map(move|id|(category,id,permission,resource))).map(|(category,id,permission,resource)|{
        let disabled=config["disabled"].as_array().is_some_and(|a|a.iter().any(|x|x==id));
        json!({"id":id,"category":category,"description":resource,"permissions":[permission],"availability":if disabled{"disabled"}else{"available"},"timeoutMs":if category=="Browser"{145000}else{65000},"maxResultBytes":if category=="Browser"{98304}else{16384},"resources":[resource]})
    }).collect::<Vec<_>>())
}
