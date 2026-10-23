





use crate::storage;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::Path;

const MAX_TITLE: usize = 60;
const MAX_DESCRIPTION: usize = 1024;
const MAX_INSTRUCTIONS: usize = 12_000;

pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_skills(
           id TEXT PRIMARY KEY,
           name TEXT NOT NULL DEFAULT '',
           title TEXT NOT NULL,
           description TEXT NOT NULL,
           instructions TEXT NOT NULL,
           workspace_id TEXT,
           status TEXT NOT NULL DEFAULT 'proposed' CHECK(status IN ('proposed','approved','disabled')),
           revision INTEGER NOT NULL DEFAULT 1,
           replaces TEXT,
           source_conversation_id TEXT,
           created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
           updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
         );
         CREATE UNIQUE INDEX IF NOT EXISTS agent_skills_name ON agent_skills(name) WHERE status<>'proposed';
         CREATE INDEX IF NOT EXISTS agent_skills_scope ON agent_skills(status,workspace_id);",
    )
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

fn text(p: &Value, key: &str, max: usize, multiline: bool) -> Result<String, String> {
    p.get(key)
        .and_then(Value::as_str)
        .and_then(|value| clean(value, max, multiline))
        .ok_or_else(|| format!("{key} must be 1–{max} characters without control characters"))
}


fn slug(title: &str) -> String {
    let mut name = String::new();
    for c in title.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c);
        } else if !name.ends_with('-') {
            name.push('-');
        }
    }
    name.trim_matches('-').chars().take(48).collect::<String>().trim_end_matches('-').to_string()
}

fn scope(db: &Connection, p: &Value, conversation: Option<&str>) -> Result<Option<String>, String> {
    let requested = match p.get("workspaceId") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.trim().to_string()),
        Some(_) => return Err("workspaceId must be text".into()),
    };
    let fallback = || {
        conversation
            .and_then(|id| {
                storage::get_conversation(db, id)
                    .ok()
                    .map(|conversation| conversation.workspace_id)
            })
            .filter(|workspace| !workspace.is_empty())
    };
    let workspace = match requested.as_deref() {
        Some("") | Some("global") | Some("all") => return Ok(None),
        Some(id) => Some(id.to_string()),
        None => fallback(),
    };
    if let Some(id) = workspace.as_deref() {
        let exists: i64 = db
            .query_row(
                "SELECT count(*) FROM library_workspaces WHERE id=?1 AND archived=0 AND draft=0",
                [id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            return Err("Choose a workspace that exists, or use global scope".into());
        }
    }
    Ok(workspace)
}

pub fn path(data: &Path, id: &str) -> String {
    data.join("skills")
        .join(id)
        .join("SKILL.md")
        .to_string_lossy()
        .into_owned()
}

fn render(name: &str, description: &str, instructions: &str) -> String {
    let escaped = description.replace('\\', "\\\\").replace('"', "\\\"");
    format!("---\nname: {name}\ndescription: \"{escaped}\"\n---\n\n{instructions}\n")
}

fn write_file(data: &Path, id: &str, name: &str, description: &str, instructions: &str) -> Result<(), String> {
    let directory = data.join("skills").join(id);
    std::fs::create_dir_all(&directory).map_err(|_| "The skill file could not be saved")?;
    std::fs::write(directory.join("SKILL.md"), render(name, description, instructions))
        .map_err(|_| "The skill file could not be saved".to_string())
}

fn remove_file(data: &Path, id: &str) {
    let _ = std::fs::remove_dir_all(data.join("skills").join(id));
}

const COLUMNS: &str = "id,name,title,description,instructions,workspace_id,status,revision,replaces,source_conversation_id,created_at,updated_at";

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, String>(0)?,
        "name": r.get::<_, String>(1)?,
        "title": r.get::<_, String>(2)?,
        "description": r.get::<_, String>(3)?,
        "instructions": r.get::<_, String>(4)?,
        "workspaceId": r.get::<_, Option<String>>(5)?,
        "status": r.get::<_, String>(6)?,
        "revision": r.get::<_, i64>(7)?,
        "replaces": r.get::<_, Option<String>>(8)?,
        "sourceConversationId": r.get::<_, Option<String>>(9)?,
        "createdAt": r.get::<_, String>(10)?,
        "updatedAt": r.get::<_, String>(11)?,
    }))
}

fn list(db: &Connection, status: &str) -> Result<Vec<Value>, String> {
    let mut statement = db
        .prepare(&format!("SELECT {COLUMNS} FROM agent_skills WHERE status=?1 ORDER BY updated_at DESC,rowid DESC"))
        .map_err(|_| "Skills are unavailable")?;
    let rows = statement
        .query_map([status], row)
        .map_err(|_| "Skills are unavailable")?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|_| "Skills are unavailable".into())
}

fn load(db: &Connection, id: &str) -> Option<Value> {
    db.query_row(&format!("SELECT {COLUMNS} FROM agent_skills WHERE id=?1"), [id], row)
        .ok()
}


fn for_workspace(db: &Connection, workspace: &str) -> Vec<Value> {
    let Ok(mut statement) = db.prepare(&format!(
        "SELECT {COLUMNS} FROM agent_skills WHERE status='approved' AND (workspace_id IS NULL OR workspace_id=?1) ORDER BY name"
    )) else {
        return Vec::new();
    };
    statement
        .query_map([workspace], row)
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .unwrap_or_default()
}


pub fn for_conversation(db: &Connection, data: &Path, conversation: &crate::storage::ConversationInfo) -> Value {
    json!(for_workspace(db, &conversation.workspace_id)
        .iter()
        .map(|skill| json!({
            "name": skill["name"],
            "description": skill["description"],
            "path": path(data, skill["id"].as_str().unwrap_or("")),
        }))
        .collect::<Vec<_>>())
}

pub fn state(db: &Connection) -> Value {
    json!({
        "skills": list(db, "approved").unwrap_or_default().into_iter()
            .chain(list(db, "disabled").unwrap_or_default())
            .collect::<Vec<_>>(),
        "skillProposals": list(db, "proposed").unwrap_or_default(),
    })
}

fn unique_name(db: &Connection, name: &str, except: Option<&str>) -> Result<(), String> {
    let taken: i64 = db
        .query_row(
            "SELECT count(*) FROM agent_skills WHERE name=?1 AND status<>'proposed' AND id IS NOT ?2",
            params![name, except],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if taken > 0 {
        return Err(format!("A skill named {name} already exists. Propose an update to it instead."));
    }
    Ok(())
}

fn budget(db: &Connection) -> Result<(), String> {
    let used = crate::agent::used_today(db);
    if used >= crate::agent::DAILY_OBJECT_LIMIT {
        return Err(format!(
            "The agent may create {} things per day and has used today's budget. Ask the user before proposing more.",
            crate::agent::DAILY_OBJECT_LIMIT
        ));
    }
    Ok(())
}


pub fn propose(db: &Connection, p: &Value, conversation: &str) -> Result<Value, String> {
    budget(db)?;
    let target = p
        .get("skillId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    let existing = match target {
        Some(id) => Some(
            load(db, id)
                .filter(|skill| skill["status"] != json!("proposed"))
                .ok_or("That skill does not exist")?,
        ),
        None => None,
    };
    let title = match (p.get("title"), &existing) {
        (Some(_), _) => text(p, "title", MAX_TITLE, false)?,
        (None, Some(skill)) => skill["title"].as_str().unwrap_or_default().to_string(),
        (None, None) => return Err("title must be 1–60 characters".into()),
    };
    let description = match (p.get("description"), &existing) {
        (Some(_), _) => text(p, "description", MAX_DESCRIPTION, false)?,
        (None, Some(skill)) => skill["description"].as_str().unwrap_or_default().to_string(),
        (None, None) => return Err("description must be 1–1024 characters".into()),
    };
    let instructions = match (p.get("instructions"), &existing) {
        (Some(_), _) => text(p, "instructions", MAX_INSTRUCTIONS, true)?,
        (None, Some(skill)) => skill["instructions"].as_str().unwrap_or_default().to_string(),
        (None, None) => return Err("instructions must be 1–12000 characters".into()),
    };
    let workspace = match (&existing, p.get("workspaceId")) {
        (Some(skill), None) => skill["workspaceId"].as_str().map(str::to_owned),
        _ => scope(db, p, Some(conversation))?,
    };
    if existing.is_some() {
        let pending: i64 = db
            .query_row(
                "SELECT count(*) FROM agent_skills WHERE status='proposed' AND replaces=?1",
                [target],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if pending > 0 {
            return Err("That skill already has a proposal waiting for the user".into());
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    db.execute(
        "INSERT INTO agent_skills(id,title,description,instructions,workspace_id,status,replaces,source_conversation_id) \
         VALUES(?1,?2,?3,?4,?5,'proposed',?6,?7)",
        params![id, title, description, instructions, workspace, target, conversation],
    )
    .map_err(|_| "The skill proposal could not be saved")?;
    Ok(json!({"id":id,"title":title,"status":"proposed","replaces":target}))
}



pub fn save(db: &Connection, data: &Path, p: &Value) -> Result<Value, String> {
    let id = text(p, "id", 100, false)?;
    let stored = load(db, &id).ok_or("That skill no longer exists")?;
    let edited = |key: &str, max: usize, multiline: bool| -> Result<Option<String>, String> {
        match p.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(_) => text(p, key, max, multiline).map(Some),
        }
    };
    let title = edited("title", MAX_TITLE, false)?
        .unwrap_or_else(|| stored["title"].as_str().unwrap_or_default().to_string());
    let description = edited("description", MAX_DESCRIPTION, false)?
        .unwrap_or_else(|| stored["description"].as_str().unwrap_or_default().to_string());
    let instructions = edited("instructions", MAX_INSTRUCTIONS, true)?
        .unwrap_or_else(|| stored["instructions"].as_str().unwrap_or_default().to_string());
    let workspace = match p.get("workspaceId") {
        None => stored["workspaceId"].as_str().map(str::to_owned),
        Some(_) => scope(db, p, None)?,
    };
    let name = slug(&title);
    if name.is_empty() {
        return Err("The title needs at least one letter or number".into());
    }
    if stored["status"] != json!("proposed") {
        unique_name(db, &name, Some(&id))?;
        db.execute(
            "UPDATE agent_skills SET name=?2,title=?3,description=?4,instructions=?5,\n             workspace_id=?6,revision=revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
            params![id, name, title, description, instructions, workspace],
        )
        .map_err(|_| "The skill could not be saved")?;
        write_file(data, &id, &name, &description, &instructions)?;
        return Ok(json!({"id":id,"name":name,"status":stored["status"]}));
    }
    match stored["replaces"].as_str().map(str::to_owned).as_deref() {
        Some(target) => {
            load(db, target).ok_or("The skill this proposal updates no longer exists")?;
            unique_name(db, &name, Some(target))?;
            db.execute(
                "UPDATE agent_skills SET name=?2,title=?3,description=?4,instructions=?5,\n                 workspace_id=?6,revision=revision+1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                params![target, name, title, description, instructions, workspace],
            )
            .map_err(|_| "The skill could not be updated")?;
            db.execute("DELETE FROM agent_skills WHERE id=?1", [&id])
                .map_err(|_| "The skill proposal could not be cleared")?;
            write_file(data, target, &name, &description, &instructions)?;
            Ok(json!({"id":target,"name":name,"status":"approved"}))
        }
        None => {
            unique_name(db, &name, Some(&id))?;
            db.execute(
                "UPDATE agent_skills SET name=?2,title=?3,description=?4,instructions=?5,\n                 workspace_id=?6,status='approved',revision=1,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
                params![id, name, title, description, instructions, workspace],
            )
            .map_err(|_| "The skill could not be saved")?;
            write_file(data, &id, &name, &description, &instructions)?;
            Ok(json!({"id":id,"name":name,"status":"approved"}))
        }
    }
}


pub fn set_enabled(db: &Connection, id: &str, enabled: bool) -> Result<Value, String> {
    let skill = load(db, id).ok_or("That skill no longer exists")?;
    if skill["status"] == json!("proposed") {
        return Err("Approve or dismiss the proposal first".into());
    }
    db.execute(
        "UPDATE agent_skills SET status=?2,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
        params![id, if enabled { "approved" } else { "disabled" }],
    )
    .map_err(|_| "The skill could not be changed")?;
    Ok(json!({"id":id,"status":if enabled { "approved" } else { "disabled" }}))
}


pub fn delete(db: &Connection, data: &Path, id: &str) -> Result<Value, String> {
    let changed = db
        .execute("DELETE FROM agent_skills WHERE id=?1", [id])
        .map_err(|_| "The skill could not be removed")?;
    if changed == 0 {
        return Err("That skill no longer exists".into());
    }
    remove_file(data, id);
    Ok(json!(true))
}


pub fn sync(db: &Connection, data: &Path) {
    let Ok(mut statement) = db.prepare(
        "SELECT id,name,description,instructions FROM agent_skills WHERE status IN ('approved','disabled')",
    ) else {
        return;
    };
    let Ok(rows) = statement.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    }) else {
        return;
    };
    for (id, name, description, instructions) in rows.flatten() {
        if name.is_empty() {
            continue;
        }
        let _ = write_file(data, &id, &name, &description, &instructions);
    }
}


pub fn agent_view(db: &Connection, data: &Path, workspace: &str) -> Value {
    let mut skills: Vec<Value> = for_workspace(db, workspace);
    for skill in &mut skills {
        let id = skill["id"].as_str().unwrap_or("");
        skill["path"] = json!(path(data, id));
        if let Some(object) = skill.as_object_mut() {
            object.remove("instructions");
            object.remove("sourceConversationId");
        }
    }
    json!({"skills": skills})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../services/memory/schema.sql"))
            .unwrap();
        crate::storage::init(&db).unwrap();
        crate::agent::init(&db).unwrap();
        init(&db).unwrap();
        db
    }

    fn directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!("talo-skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn propose_new(db: &Connection, title: &str) -> String {
        let created = propose(
            db,
            &json!({
                "title": title,
                "description": "Organize downloaded statements into folders.",
                "instructions": "## When to use\nMonthly statements arrive in Downloads.",
                "workspaceId": "global"
            }),
            "c1",
        )
        .unwrap();
        created["id"].as_str().unwrap().to_string()
    }

    #[test]
    fn proposals_stay_inactive_until_approved() {
        let db = database();
        let data = directory();
        let id = propose_new(&db, "Organize statements");
        let draft = load(&db, &id).unwrap();
        assert_eq!(draft["status"], "proposed");
        assert!(draft["workspaceId"].is_null());
        assert!(list(&db, "approved").unwrap().is_empty());
        save(&db, &data, &json!({"id":id})).unwrap();
        let accepted = load(&db, &id).unwrap();
        assert_eq!(accepted["status"], "approved");
        assert_eq!(accepted["name"], "organize-statements");
        assert_eq!(accepted["revision"], 1);
        let file = std::fs::read_to_string(path(&data, &id)).unwrap();
        assert!(file.starts_with("---\nname: organize-statements\n"));
        assert!(file.contains("## When to use"));
        assert!(for_workspace(&db, "general").iter().any(|skill| skill["id"] == json!(id)));
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn a_correction_waits_and_keeps_the_approved_revision() {
        let db = database();
        let data = directory();
        let id = propose_new(&db, "Organize statements");
        save(&db, &data, &json!({"id":id})).unwrap();
        let update = propose(
            &db,
            &json!({"skillId":id,"instructions":"## When to use\nStatements arrive weekly."}),
            "c2",
        )
        .unwrap();
        let update_id = update["id"].as_str().unwrap();
        assert_eq!(update["replaces"], json!(id));
        assert_eq!(load(&db, &id).unwrap()["revision"], 1);
        assert!(load(&db, &id).unwrap()["instructions"]
            .as_str()
            .unwrap()
            .contains("Monthly"));
        assert!(propose(&db, &json!({"skillId":id,"instructions":"again"}), "c3").is_err());
        save(&db, &data, &json!({"id":update_id})).unwrap();
        let current = load(&db, &id).unwrap();
        assert_eq!(current["revision"], 2);
        assert!(current["instructions"].as_str().unwrap().contains("weekly"));
        assert!(load(&db, &update_id).is_none());
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn scope_records_validation_and_removal() {
        let db = database();
        let data = directory();
        assert!(propose(&db, &json!({"title":"x"}), "c1").is_err());
        assert!(propose(&db, &json!({"title":"x","description":"d","instructions":"i","workspaceId":"missing"}), "c1").is_err());
        let created = propose(
            &db,
            &json!({"title":"Organize statements","description":"d","instructions":"i","workspaceId":"general"}),
            "c1",
        )
        .unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        assert_eq!(load(&db, &id).unwrap()["workspaceId"], "general");
        save(&db, &data, &json!({"id":id,"title":"Organize statements"})).unwrap();
        assert_eq!(for_workspace(&db, "other"), Vec::<Value>::new());
        assert_eq!(for_workspace(&db, "general").len(), 1);
        set_enabled(&db, &id, false).unwrap();
        assert_eq!(for_workspace(&db, "general"), Vec::<Value>::new());
        assert_eq!(load(&db, &id).unwrap()["status"], "disabled");
        delete(&db, &data, &id).unwrap();
        assert!(load(&db, &id).is_none());
        assert!(!Path::new(&path(&data, &id)).exists());
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn approval_accepts_user_edits_and_rejects_duplicate_names() {
        let db = database();
        let data = directory();
        let first = propose_new(&db, "Organize statements");
        save(&db, &data, &json!({"id":first})).unwrap();
        let second = propose_new(&db, "Organize statements");
        assert!(save(&db, &data, &json!({"id":second})).is_err());
        save(&db, &data, &json!({"id":second,"title":"File receipts"})).unwrap();
        assert_eq!(load(&db, &second).unwrap()["name"], "file-receipts");
        let third = propose_new(&db, "Invoices");
        save(
            &db,
            &data,
            &json!({"id":third,"instructions":"## When to use\nEdited by the user."}),
        )
        .unwrap();
        assert!(load(&db, &third).unwrap()["instructions"]
            .as_str()
            .unwrap()
            .contains("Edited by the user."));
        std::fs::remove_dir_all(data).unwrap();
    }
}
