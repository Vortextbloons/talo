
use chrono::{DateTime, Days, LocalResult, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub local_time: String,
    pub timezone: String,
    pub recurrence: String,
    pub paused: bool,
    pub next_at: Option<i64>,
    #[serde(default)]
    pub workspace_id: String,
    pub timeout_seconds: i64,
    pub connection_id: String,
    pub model_id: String,
}
pub fn init(db: &Connection) -> rusqlite::Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS automation_tasks(
      id TEXT PRIMARY KEY, definition TEXT NOT NULL, next_at INTEGER, paused INTEGER NOT NULL DEFAULT 0,
      deleted INTEGER NOT NULL DEFAULT 0);
      CREATE TABLE IF NOT EXISTS automation_runs(
      id TEXT PRIMARY KEY, task_id TEXT NOT NULL REFERENCES automation_tasks(id), scheduled_at INTEGER NOT NULL,
      status TEXT NOT NULL, started_at INTEGER, finished_at INTEGER, conversation_id TEXT, generation_id TEXT,
      error TEXT, usage TEXT, definition TEXT NOT NULL, UNIQUE(task_id,scheduled_at));
      CREATE INDEX IF NOT EXISTS automation_due ON automation_tasks(paused,deleted,next_at);
      CREATE UNIQUE INDEX IF NOT EXISTS automation_one_active ON automation_runs((1)) WHERE status IN ('running','awaiting_approval');
      CREATE TABLE IF NOT EXISTS automation_events(id INTEGER PRIMARY KEY, at INTEGER NOT NULL DEFAULT(unixepoch()),
      run_id TEXT, event TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS automation_actions(run_id TEXT NOT NULL REFERENCES automation_runs(id),
      fingerprint TEXT NOT NULL, dispatched_at INTEGER NOT NULL DEFAULT(unixepoch()),PRIMARY KEY(run_id,fingerprint));")?;
    let has_definition: i64 = db.query_row(
        "SELECT count(*) FROM pragma_table_info('automation_runs') WHERE name='definition'",
        [],
        |row| row.get(0),
    )?;
    if has_definition == 0 {
        db.execute_batch(
            "ALTER TABLE automation_runs ADD COLUMN definition TEXT NOT NULL DEFAULT '{}';
             UPDATE automation_runs SET definition=COALESCE((SELECT definition FROM automation_tasks WHERE id=automation_runs.task_id),'{}');",
        )?;
    }
    Ok(())
}
pub fn reserve_action(
    db: &Connection,
    run: &str,
    action: &str,
    scope: &str,
) -> rusqlite::Result<bool> {
    use sha2::{Digest, Sha256};

    fn canonical(v: &serde_json::Value) -> String {
        match v {
            serde_json::Value::Object(map) => {
                let sorted: std::collections::BTreeMap<_, _> =
                    map.iter().map(|(k, v)| (k, canonical(v))).collect();
                serde_json::to_string(&sorted).unwrap_or_default()
            }
            serde_json::Value::Array(items) => format!(
                "[{}]",
                items.iter().map(canonical).collect::<Vec<_>>().join(",")
            ),
            _ => v.to_string(),
        }
    }
    let scope = serde_json::from_str::<serde_json::Value>(scope)
        .map(|v| canonical(&v))
        .unwrap_or_else(|_| scope.into());
    let fingerprint = format!("{:x}", Sha256::digest(format!("{action}\0{scope}")));
    Ok(db.execute(
        "INSERT OR IGNORE INTO automation_actions(run_id,fingerprint) VALUES(?1,?2)",
        params![run, fingerprint],
    )? == 1)
}
pub fn recover(db: &Connection) -> rusqlite::Result<usize> {
    db.execute("UPDATE automation_tasks SET paused=1 WHERE id IN (SELECT task_id FROM automation_runs WHERE status IN ('running','awaiting_approval'))",[])?;
    db.execute("UPDATE automation_runs SET status='uncertain',finished_at=unixepoch(),error='Host stopped during execution. Review the conversation before running again.' WHERE status IN ('running','awaiting_approval')", [])
}
fn instant(wall: NaiveDateTime, tz: Tz) -> Option<i64> {
    match tz.from_local_datetime(&wall) {
        LocalResult::Single(t) => Some(t.timestamp()),
        LocalResult::Ambiguous(a, b) => Some(a.timestamp().min(b.timestamp())),
        LocalResult::None => None,
    }
}
pub fn next(task: &Task, after: Option<i64>) -> Result<Option<i64>, String> {
    let mut wall = NaiveDateTime::parse_from_str(&task.local_time, "%Y-%m-%dT%H:%M")
        .map_err(|_| "Choose a valid local date and time")?;
    let tz = task
        .timezone
        .parse::<Tz>()
        .map_err(|_| "Choose an IANA timezone, such as America/Denver")?;
    let days = match task.recurrence.as_str() {
        "once" => 0,
        "daily" => 1,
        "weekly" => 7,
        _ => return Err("Choose once, daily or weekly".into()),
    };
    if days == 0 {
        let at = instant(wall, tz)
            .ok_or("This local time does not exist because of daylight saving time")?;
        return Ok(if after.is_none_or(|after| at > after) {
            Some(at)
        } else {
            None
        });
    }
    if let Some(after) = after {
        let date = DateTime::<Utc>::from_timestamp(after, 0)
            .ok_or("Invalid timestamp")?
            .with_timezone(&tz)
            .date_naive();
        let elapsed = date.signed_duration_since(wall.date()).num_days().max(0) as u64;
        wall = wall
            .checked_add_days(Days::new((elapsed / days) * days))
            .ok_or("Date out of range")?;
    }

    for _ in 0..370 {
        if let Some(at) = instant(wall, tz) {
            if after.is_none_or(|after| at > after) {
                return Ok(Some(at));
            }
        }
        wall = wall
            .checked_add_days(Days::new(days))
            .ok_or("Date out of range")?;
    }
    Err("No valid upcoming date".into())
}
pub fn save(db: &Connection, mut task: Task, now: i64) -> Result<(), String> {
    let active:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM automation_runs WHERE task_id=?1 AND status IN ('running','awaiting_approval'))",[&task.id],|r|r.get(0)).map_err(|e|e.to_string())?;
    if active {
        return Err("Wait for this task to finish or cancel its run before editing".into());
    }
    if task.title.trim().is_empty()
        || task.title.len() > 200
        || task.prompt.trim().is_empty()
        || task.prompt.len() > 32000
        || task.connection_id.is_empty()
        || task.model_id.is_empty()
        || !(30..=3600).contains(&task.timeout_seconds)
    {
        return Err(
            "A title, prompt, connected model and timeout of 30–3600 seconds are required".into(),
        );
    }
    if task.id.is_empty() {
        task.id = uuid::Uuid::new_v4().to_string();
    }
    uuid::Uuid::parse_str(&task.id).map_err(|_| "Invalid task ID")?;
    task.next_at = next(&task, Some(now))?;
    if task.next_at.is_none() {
        return Err("Choose a future time".into());
    }
    let changed = db.execute("INSERT INTO automation_tasks(id,definition,next_at,paused) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET definition=excluded.definition,next_at=excluded.next_at,paused=excluded.paused WHERE deleted=0",params![task.id,serde_json::to_string(&task).map_err(|e|e.to_string())?,task.next_at,task.paused]).map_err(|e|e.to_string())?;
    if changed == 0 {
        return Err("That scheduled task no longer exists".into());
    }
    Ok(())
}
pub fn list(db: &Connection) -> rusqlite::Result<Vec<Task>> {
    let mut stmt=db.prepare("SELECT definition,next_at,paused FROM automation_tasks WHERE deleted=0 ORDER BY next_at IS NULL,next_at")?;
    let rows = stmt.query_map([], |r| {
        let raw: String = r.get(0)?;
        let mut task: Task = serde_json::from_str(&raw)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        task.next_at = r.get(1)?;
        task.paused = r.get(2)?;
        Ok(task)
    })?;
    rows.collect()
}
pub struct Claimed {
    pub task: Task,
    pub run_id: String,
}
pub fn claim(db: &mut Connection, now: i64) -> Result<Option<Claimed>, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let busy:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM automation_runs WHERE status IN ('running','awaiting_approval'))",[],|r|r.get(0)).map_err(|e|e.to_string())?;
    if busy {
        return Ok(None);
    }
    let task = list(&tx)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|t| !t.paused && t.next_at.is_some_and(|at| at <= now));
    let Some(task) = task else { return Ok(None) };
    let run_id = uuid::Uuid::new_v4().to_string();
    let upcoming = next(&task, Some(now))?;
    tx.execute("INSERT INTO automation_runs(id,task_id,scheduled_at,status,started_at,definition) VALUES(?1,?2,?3,'running',?4,?5)",params![run_id,task.id,task.next_at,now,serde_json::to_string(&task).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    tx.execute(
        "UPDATE automation_tasks SET next_at=?2 WHERE id=?1",
        params![task.id, upcoming],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(Some(Claimed { task, run_id }))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn task(time: &str, recurrence: &str) -> Task {
        Task {
            id: String::new(),
            title: "Report".into(),
            prompt: "Report".into(),
            local_time: time.into(),
            timezone: "America/Denver".into(),
            recurrence: recurrence.into(),
            paused: false,
            next_at: None,
            timeout_seconds: 600,
            connection_id: "c".into(),
            model_id: "m".into(),
            workspace_id: "general".into(),
        }
    }
    #[test]
    fn recurrence_handles_dst_gap_and_overlap() {
        let spring = task("2026-03-07T02:30", "daily");
        let a = next(&spring, None).unwrap().unwrap();
        let b = next(&spring, Some(a)).unwrap().unwrap();
        assert_eq!(b - a, 47 * 3600);
        let fall = task("2026-10-31T01:30", "daily");
        let a = next(&fall, None).unwrap().unwrap();
        let b = next(&fall, Some(a)).unwrap().unwrap();
        let c = next(&fall, Some(b)).unwrap().unwrap();
        assert_eq!(b - a, 24 * 3600);
        assert_eq!(c - b, 25 * 3600);
        assert!(next(&task("2026-03-08T02:30", "once"), None).is_err());
    }
    #[test]
    fn claims_are_coalesced_and_interrupted_runs_never_replay() {
        let mut db = Connection::open_in_memory().unwrap();
        init(&db).unwrap();
        save(&db, task("2026-01-01T09:00", "daily"), 0).unwrap();
        let now = Utc::now().timestamp();
        let run = claim(&mut db, now).unwrap().unwrap();
        assert!(claim(&mut db, now).unwrap().is_none());
        assert_eq!(recover(&db).unwrap(), 1);
        assert_eq!(recover(&db).unwrap(), 0);
        let state: String = db
            .query_row(
                "SELECT status FROM automation_runs WHERE id=?1",
                [run.run_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "uncertain");
    }
    #[test]
    fn runs_created_before_definitions_gain_the_column() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE automation_runs(id TEXT PRIMARY KEY,task_id TEXT NOT NULL,scheduled_at INTEGER NOT NULL,
             status TEXT NOT NULL,started_at INTEGER,finished_at INTEGER,conversation_id TEXT,generation_id TEXT,
             error TEXT,usage TEXT,UNIQUE(task_id,scheduled_at));",
        )
        .unwrap();
        let definition = serde_json::to_string(&task("2026-01-01T09:00", "once")).unwrap();
        db.execute(
            "INSERT INTO automation_tasks(id,definition,next_at,paused) VALUES('t',?1,NULL,0)",
            [&definition],
        )
        .unwrap();
        db.execute(
            "INSERT INTO automation_runs(id,task_id,scheduled_at,status) VALUES('r','t',1,'uncertain')",
            [],
        )
        .unwrap();
        init(&db).unwrap();
        let stored: String = db
            .query_row(
                "SELECT definition FROM automation_runs WHERE id='r'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, definition);
    }
    #[test]
    fn schedules_survive_reopen_and_pause_delete_are_respected() {
        let path = std::env::temp_dir().join(format!("talo-schedule-{}.db", uuid::Uuid::new_v4()));
        {
            let db = Connection::open(&path).unwrap();
            init(&db).unwrap();
            save(&db, task("2026-01-01T09:00", "once"), 0).unwrap();
            let id = list(&db).unwrap()[0].id.clone();
            db.execute("UPDATE automation_tasks SET paused=1 WHERE id=?1", [&id])
                .unwrap();
        }
        {
            let mut db = Connection::open(&path).unwrap();
            init(&db).unwrap();
            let saved = list(&db).unwrap();
            assert_eq!(saved.len(), 1);
            assert!(saved[0].paused);
            let now = Utc::now().timestamp();
            assert!(claim(&mut db, now).unwrap().is_none());
            db.execute("UPDATE automation_tasks SET paused=0", [])
                .unwrap();
            let run = claim(&mut db, now).unwrap().unwrap();
            assert_eq!(run.task.title, "Report");
            assert!(list(&db).unwrap()[0].next_at.is_none());
            recover(&db).unwrap();
            assert!(list(&db).unwrap()[0].paused);
            db.execute("UPDATE automation_tasks SET deleted=1", [])
                .unwrap();
            assert!(list(&db).unwrap().is_empty());
            assert_eq!(
                db.query_row::<i64, _, _>("SELECT count(*) FROM automation_runs", [], |r| r.get(0))
                    .unwrap(),
                1
            );
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn consequential_actions_are_reserved_once_before_dispatch_without_storing_secrets() {
        let mut db = Connection::open_in_memory().unwrap();
        init(&db).unwrap();
        save(&db, task("2026-01-01T09:00", "once"), 0).unwrap();
        let run = claim(&mut db, Utc::now().timestamp()).unwrap().unwrap();
        assert!(reserve_action(
            &db,
            &run.run_id,
            "mcp.call",
            r#"{"tool":"send","arguments":{"token":"private","to":"a"}}"#
        )
        .unwrap());
        assert!(!reserve_action(
            &db,
            &run.run_id,
            "mcp.call",
            r#"{"arguments":{"to":"a","token":"private"},"tool":"send"}"#
        )
        .unwrap());
        let fingerprint: String = db
            .query_row("SELECT fingerprint FROM automation_actions", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(fingerprint.len(), 64);
        assert!(!fingerprint.contains("private"));
        recover(&db).unwrap();
        assert!(!reserve_action(
            &db,
            &run.run_id,
            "mcp.call",
            r#"{"tool":"send","arguments":{"token":"private","to":"a"}}"#
        )
        .unwrap());
    }
}
