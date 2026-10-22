use rusqlite::{params, Connection};
use std::collections::HashMap;
use serde_json::Value;

#[derive(Debug,PartialEq)]
pub enum Decision { Allow, Prompt, Deny }
/// Shared native/browser/MCP policy. Server annotations and model flags never
/// authorize an operation. Persistent grants are exact and host-protected.
pub fn policy(action:&str,scope:&str,grants:&Value,attended:bool)->Decision{
    if action=="web.public" {return Decision::Allow;}
    if !["mcp.call","files.read","files.write","files.edit","browser.action"].contains(&action){return Decision::Deny;}
    if grants.as_array().is_some_and(|items|items.iter().any(|g|g["action"]==action&&g["scope"]==scope)){return Decision::Allow;}
    if attended{Decision::Prompt}else{Decision::Deny}
}

#[derive(Default)]
pub struct Permissions {
    pending: HashMap<String, (String, String)>,
    grants: HashMap<String, (String, String)>,
}
impl Permissions {
    pub fn request(&mut self, action: String, resource: String) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.pending.insert(id.clone(), (action, resource));
        id
    }
    pub fn resolve(&mut self, db: &Connection, id: &str, allowed: bool) -> Result<(), String> {
        let (action, resource) = self
            .pending
            .remove(id)
            .ok_or("Unknown permission request")?;
        db.execute(
            "INSERT INTO permission_audit(id,action,resource,allowed) VALUES(?1,?2,?3,?4)",
            params![id, action, resource, allowed],
        )
        .map_err(|e| e.to_string())?;
        if allowed {
            self.grants.insert(id.into(), (action, resource));
        }
        Ok(())
    }
    pub fn consume(&mut self, id: &str, action: &str, resource: &str) -> Result<(), String> {
        match self.grants.remove(id) {
            Some((a, r)) if a == action && r == resource => Ok(()),
            _ => Err("Permission required".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_policy_is_exact_revocable_and_fails_closed_unattended(){
        let grants=serde_json::json!([{"action":"mcp.call","scope":"server/tool/exact-arguments"}]);
        assert_eq!(policy("web.public","url",&Value::Null,false),Decision::Allow);
        assert_eq!(policy("mcp.call","server/tool/exact-arguments",&grants,false),Decision::Allow);
        assert_eq!(policy("mcp.call","server/tool/other-arguments",&grants,false),Decision::Deny);
        assert_eq!(policy("mcp.call","server/tool/exact-arguments",&Value::Null,false),Decision::Deny);
        assert_eq!(policy("files.read","approved-file",&Value::Null,true),Decision::Prompt);
        assert_eq!(policy("browser.action","page-revision",&Value::Null,false),Decision::Deny);
        assert_eq!(policy("shell.execute","command",&grants,true),Decision::Deny);
    }
    #[test]
    fn approval_is_bound_one_use_and_audited() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../services/memory/schema.sql"))
            .unwrap();
        let mut p = Permissions::default();
        let denied = p.request("browser.start".into(), "profile".into());
        p.resolve(&db, &denied, false).unwrap();
        assert!(p.consume(&denied, "browser.start", "profile").is_err());
        let id = p.request("browser.start".into(), "profile".into());
        p.resolve(&db, &id, true).unwrap();
        assert!(p.consume(&id, "browser.start", "other").is_err());
        assert!(p.consume(&id, "browser.start", "profile").is_err());
        let id = p.request("browser.start".into(), "profile".into());
        p.resolve(&db, &id, true).unwrap();
        p.consume(&id, "browser.start", "profile").unwrap();
        assert!(p.consume(&id, "browser.start", "profile").is_err());
        let count: i64 = db
            .query_row("SELECT count(*) FROM permission_audit", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }
}
