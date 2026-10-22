use crate::storage;
use rusqlite::Connection;

pub const ID: &str = "talo-primary";

pub fn ensure(db: &Connection) -> storage::Result<()> {
    if !db.query_row(
        "SELECT EXISTS(SELECT 1 FROM chat_conversations WHERE id=?1)",
        [ID],
        |r| r.get::<_, bool>(0),
    )? {
        let selection = db.query_row(
            "SELECT connection_id,model_id FROM chat_conversations WHERE connection_id IS NOT NULL AND model_id IS NOT NULL AND delegation_parent_id IS NULL ORDER BY updated_at DESC LIMIT 1",
            [],
            |r| Ok(storage::ModelSelection { connection_id: r.get(0)?, model_id: r.get(1)? }),
        ).ok();
        storage::create_conversation(db, ID, "Talo", selection.as_ref())?;
    }
    Ok(())
}
