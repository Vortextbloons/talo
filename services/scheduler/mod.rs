//! Native scheduler: mark due tasks for approval; never execute automatically.
pub fn tick(db: &rusqlite::Connection) -> rusqlite::Result<usize> {
    db.execute("UPDATE tasks SET state='awaiting_permission' WHERE state='pending' AND due_at <= unixepoch()", [])
}

#[cfg(test)]
mod tests {
    #[test]
    fn due_tasks_wait_for_permission_and_memory_index_tracks_mutations() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../memory/schema.sql"))
            .unwrap();
        db.execute("INSERT INTO tasks VALUES('due','Due',0,'pending','{}')", [])
            .unwrap();
        assert_eq!(super::tick(&db).unwrap(), 1);
        assert_eq!(super::tick(&db).unwrap(), 0);
        let state: String = db
            .query_row("SELECT state FROM tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(state, "awaiting_permission");
        assert!(db
            .execute(
                "INSERT INTO memories(scope,content) VALUES('workspace','invalid')",
                []
            )
            .is_err());
        db.execute(
            "INSERT INTO memories(scope,content) VALUES('global','sunflower')",
            [],
        )
        .unwrap();
        let count = || {
            db.query_row::<i64, _, _>(
                "SELECT count(*) FROM memory_fts WHERE memory_fts MATCH 'sunflower'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(count(), 1);
        db.execute("UPDATE memories SET content='daisy'", [])
            .unwrap();
        assert_eq!(count(), 0);
        db.execute("DELETE FROM memories", []).unwrap();
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM memory_fts WHERE memory_fts MATCH 'daisy'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
