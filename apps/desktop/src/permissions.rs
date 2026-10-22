use rusqlite::{params, Connection};
use std::collections::HashMap;

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
