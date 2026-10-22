//! SQLite metadata owned by the native host.
//!
//! Chat conversations, their durable Pi session paths, generation rows and provider
//! connection metadata live here. API keys are never stored in this module directly:
//! callers pass an already DPAPI-protected blob (`provider_connections.secret`).
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = include_str!("schema.sql");
pub const INTERRUPTED_ERROR: &str = "Interrupted before the response completed";

const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ','now')";

#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    /// A generation is already mutating this conversation.
    Busy,
    /// The requested generation/conversation already exists or does not exist.
    Duplicate,
    Missing,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Sqlite(error) => write!(f, "{error}"),
            Error::Busy => write!(f, "A response is already in progress for this conversation"),
            Error::Duplicate => write!(f, "That conversation or generation already exists"),
            Error::Missing => write!(
                f,
                "That conversation, generation or connection was not found"
            ),
        }
    }
}

impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error {
    fn from(value: rusqlite::Error) -> Self {
        Error::Sqlite(value)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn init(db: &Connection) -> Result<()> {
    db.execute_batch(SCHEMA)?;
    // Databases created before reasoning display lack the column; add it in place.
    let has_reasoning: i64 = db.query_row(
        "SELECT count(*) FROM pragma_table_info('chat_messages') WHERE name='reasoning'",
        [],
        |row| row.get(0),
    )?;
    if has_reasoning == 0 {
        db.execute_batch("ALTER TABLE chat_messages ADD COLUMN reasoning TEXT")?;
    }
    db.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)")?;
    let migrated: i64 = db.query_row(
        "SELECT count(*) FROM schema_migrations WHERE version=3",
        [],
        |r| r.get(0),
    )?;
    if migrated == 0 {
        db.execute_batch("BEGIN IMMEDIATE")?;
        if let Err(error) = db.execute_batch(include_str!("slice3.sql")) {
            let _ = db.execute_batch("ROLLBACK");
            return Err(error.into());
        }
        // Preserve the existing user's opt-out when upgrading from Slice 2.
        let preference: Option<String> = db
            .query_row(
                "SELECT value FROM settings WHERE key='onboarding'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if preference
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|s| {
                s.get("memory")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .as_deref()
            == Some("off")
        {
            db.execute(
                "UPDATE settings SET value='false' WHERE key='memory_enabled'",
                [],
            )?;
        }
        db.execute_batch(
            "INSERT INTO schema_migrations(version) VALUES(3); PRAGMA user_version=3; COMMIT",
        )?;
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelection {
    pub connection_id: String,
    pub model_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationInfo {
    pub id: String,
    pub title: String,
    pub session_id: Option<String>,
    pub session_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub selection: Option<ModelSelection>,
    pub workspace_id: String,
    pub archived: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Images the user attached to this message; filled in by the host from chat_attachments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<AttachmentInfo>>,
}

/// One display-ready attachment. `url` loads a preview from native storage on demand.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub bytes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A stored attachment row: the metadata plus where the bytes live on disk.
#[derive(Clone, Debug)]
pub struct StoredAttachment {
    pub generation_id: Option<String>,
    pub info: AttachmentInfo,
    pub path: String,
}

pub struct NewAttachment {
    pub id: String,
    pub conversation_id: String,
    pub generation_id: String,
    pub name: String,
    pub mime_type: String,
    pub bytes: i64,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSnapshot {
    pub conversation: ConversationInfo,
    pub messages: Vec<ChatMessage>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub generation_id: Option<String>,
    pub state: String,
    pub sequence: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessagePage {
    pub messages: Vec<ChatMessage>,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ConnectionRow {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub model_id: Option<String>,
    pub base_url: Option<String>,
    /// DPAPI-protected bytes. Never exposed in `ConnectionInfo` or events.
    pub secret: Option<Vec<u8>>,
    pub connected: bool,
}

impl ConnectionRow {
    pub fn info(&self) -> ConnectionInfo {
        ConnectionInfo {
            id: self.id.clone(),
            provider_id: self.provider_id.clone(),
            name: self.name.clone(),
            connected: self.connected,
            model_id: self.model_id.clone(),
            base_url: self.base_url.clone(),
        }
    }
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct GenerationRow {
    pub id: String,
    pub conversation_id: String,
    pub state: String,
    pub sequence: i64,
    pub error: Option<String>,
}

/// The assistant row a retry will rewrite, plus the user text needed when Pi has no
/// durable turn yet (for example, a first send that failed before `session.open`).
#[derive(Clone, Debug)]
pub struct RetryPlan {
    pub generation: GenerationRow,
    pub assistant_id: String,
    pub user_text: String,
}

pub struct NewGeneration<'a> {
    pub id: &'a str,
    pub conversation_id: &'a str,
    pub assistant_id: &'a str,
    pub text: &'a str,
    pub title: Option<&'a str>,
}

fn conversation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConversationInfo> {
    let connection_id: Option<String> = row.get(4)?;
    let model_id: Option<String> = row.get(5)?;
    let selection = match (connection_id, model_id) {
        (Some(connection_id), Some(model_id)) => Some(ModelSelection {
            connection_id,
            model_id,
        }),
        _ => None,
    };
    Ok(ConversationInfo {
        id: row.get(0)?,
        title: row.get(1)?,
        session_id: row.get(2)?,
        session_path: row.get(3)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        selection,
        workspace_id: row.get(8)?,
        archived: row.get(9)?,
    })
}

const CONVERSATION_COLUMNS: &str =
    "id,title,session_id,session_path,connection_id,model_id,created_at,updated_at,workspace_id,archived";

pub fn list_conversations(db: &Connection) -> Result<Vec<ConversationInfo>> {
    let sql = format!(
        "SELECT {CONVERSATION_COLUMNS} FROM chat_conversations ORDER BY updated_at DESC, rowid DESC"
    );
    let mut statement = db.prepare(&sql)?;
    let rows = statement.query_map([], conversation_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get_conversation(db: &Connection, id: &str) -> Result<ConversationInfo> {
    let sql = format!("SELECT {CONVERSATION_COLUMNS} FROM chat_conversations WHERE id=?1");
    db.query_row(&sql, params![id], conversation_from_row)
        .optional()?
        .ok_or(Error::Missing)
}

pub fn create_conversation(
    db: &Connection,
    id: &str,
    title: &str,
    selection: Option<&ModelSelection>,
) -> Result<ConversationInfo> {
    let (connection_id, model_id) = selection
        .map(|selection| {
            (
                Some(selection.connection_id.as_str()),
                Some(selection.model_id.as_str()),
            )
        })
        .unwrap_or((None, None));
    db.execute(
        &format!(
            "INSERT INTO chat_conversations(id,title,connection_id,model_id,created_at,updated_at)
             VALUES(?1,?2,?3,?4,{NOW},{NOW})"
        ),
        params![id, title, connection_id, model_id],
    )?;
    get_conversation(db, id)
}

pub fn update_conversation_selection(
    db: &Connection,
    id: &str,
    selection: &ModelSelection,
) -> Result<ConversationInfo> {
    let changed = db.execute(
        &format!(
            "UPDATE chat_conversations SET connection_id=?2,model_id=?3,updated_at={NOW} WHERE id=?1"
        ),
        params![id, selection.connection_id, selection.model_id],
    )?;
    if changed == 0 {
        return Err(Error::Missing);
    }
    get_conversation(db, id)
}

pub fn update_conversation_session(
    db: &Connection,
    id: &str,
    session_id: Option<&str>,
    session_path: Option<&str>,
) -> Result<()> {
    db.execute(
        &format!(
            "UPDATE chat_conversations SET session_id=?2,session_path=?3,updated_at={NOW} WHERE id=?1"
        ),
        params![id, session_id, session_path],
    )?;
    Ok(())
}

pub fn list_messages(db: &Connection, conversation_id: &str) -> Result<Vec<ChatMessage>> {
    let mut statement = db.prepare(
        "SELECT id,role,text,reasoning,status,generation_id,error FROM chat_messages
         WHERE conversation_id=?1 ORDER BY rowid ASC",
    )?;
    let rows = statement.query_map(params![conversation_id], |row| {
        Ok(ChatMessage {
            id: row.get(0)?,
            role: row.get(1)?,
            text: row.get(2)?,
            reasoning: row.get(3)?,
            status: row.get(4)?,
            generation_id: row.get(5)?,
            error: row.get(6)?,
            attachments: None,
        })
    })?;
    let mut messages = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    for message in &mut messages {
        hydrate_stream(db, message)?;
    }
    Ok(messages)
}

pub fn message_page(db: &Connection, conversation_id: &str, before: Option<&str>, after: Option<&str>) -> Result<MessagePage> {
    let cursor: Option<i64> = match before.or(after) {
        Some(id) => Some(db.query_row(
            "SELECT rowid FROM chat_messages WHERE conversation_id=?1 AND id=?2",
            params![conversation_id, id], |row| row.get(0),
        ).optional()?.ok_or(Error::Missing)?),
        None => None,
    };
    let (comparison, order) = if after.is_some() { (">", "ASC") } else { ("<", "DESC") };
    let mut statement = db.prepare(&format!(
        "SELECT rowid,id,role,text,reasoning,status,generation_id,error FROM chat_messages
         WHERE conversation_id=?1 AND (?2 IS NULL OR rowid {comparison} ?2) ORDER BY rowid {order} LIMIT 40"
    ))?;
    let mut rows = statement.query_map(params![conversation_id, cursor], |row| Ok((row.get::<_, i64>(0)?, ChatMessage {
        id: row.get(1)?, role: row.get(2)?, text: row.get(3)?, reasoning: row.get(4)?,
        status: row.get(5)?, generation_id: row.get(6)?, error: row.get(7)?, attachments: None,
    })))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if after.is_none() { rows.reverse(); }
    let before = match rows.first() {
        Some((rowid, message)) if db.query_row("SELECT EXISTS(SELECT 1 FROM chat_messages WHERE conversation_id=?1 AND rowid<?2)", params![conversation_id,rowid], |row| row.get::<_, bool>(0))? => Some(message.id.clone()),
        _ => None,
    };
    let after = match rows.last() {
        Some((rowid, message)) if db.query_row("SELECT EXISTS(SELECT 1 FROM chat_messages WHERE conversation_id=?1 AND rowid>?2)", params![conversation_id,rowid], |row| row.get::<_, bool>(0))? => Some(message.id.clone()),
        _ => None,
    };
    let mut messages = rows.into_iter().map(|(_, message)| message).collect::<Vec<_>>();
    for message in &mut messages { hydrate_stream(db, message)?; }
    Ok(MessagePage { messages, before, after })
}

fn hydrate_stream(db: &Connection, message: &mut ChatMessage) -> Result<()> {
    if message.status != "generating" { return Ok(()); }
    let mut statement = db.prepare("SELECT text,reasoning,reset FROM chat_stream_chunks WHERE generation_id=?1 ORDER BY sequence")?;
    let mut rows = statement.query(params![message.generation_id])?;
    while let Some(row) = rows.next()? {
        if row.get::<_, bool>(2)? { message.text.clear(); message.reasoning = None; }
        message.text.push_str(&row.get::<_, String>(0)?);
        let reasoning: String = row.get(1)?;
        if !reasoning.is_empty() { message.reasoning.get_or_insert_with(String::new).push_str(&reasoning); }
    }
    Ok(())
}

fn materialize_stream(db: &Connection, generation_id: &str) -> Result<()> {
    let assistant_id: Option<String> = db.query_row(
        "SELECT id FROM chat_messages WHERE generation_id=?1 AND role='assistant' AND status='generating' ORDER BY rowid DESC LIMIT 1",
        params![generation_id], |row| row.get(0),
    ).optional()?;
    if let Some(assistant_id) = assistant_id {
        let message = get_message(db, &assistant_id)?;
        db.execute("UPDATE chat_messages SET text=?2,reasoning=?3 WHERE id=?1", params![assistant_id,message.text,message.reasoning])?;
    }
    db.execute("DELETE FROM chat_stream_chunks WHERE generation_id=?1", params![generation_id])?;
    Ok(())
}

pub fn append_progress(db: &mut Connection, generation_id: &str, sequence: i64, state: &str, text: &str, reasoning: &str, reset: bool) -> Result<bool> {
    let tx = db.transaction()?;
    let changed = tx.execute(
        &format!("UPDATE chat_generations SET sequence=?2,state=?3,updated_at={NOW} WHERE id=?1 AND sequence<?2 AND state IN ('generating','cancelling')"),
        params![generation_id,sequence,state],
    )?;
    if changed == 0 { return Ok(false); }
    tx.execute("INSERT INTO chat_stream_chunks(generation_id,sequence,text,reasoning,reset) VALUES(?1,?2,?3,?4,?5)", params![generation_id,sequence,text,reasoning,reset])?;
    tx.commit()?;
    Ok(true)
}

/// Attachments for one conversation, oldest first. The caller decides what to render.
pub fn list_attachments(db: &Connection, conversation_id: &str, generations: &[&str]) -> Result<Vec<StoredAttachment>> {
    if generations.is_empty() { return Ok(Vec::new()); }
    let generations = serde_json::to_string(generations).unwrap();
    let mut statement = db.prepare(
        "SELECT id,generation_id,name,mime_type,bytes,path FROM chat_attachments
         WHERE conversation_id=?1 AND generation_id IN (SELECT value FROM json_each(?2)) ORDER BY rowid ASC",
    )?;
    let rows = statement.query_map(params![conversation_id, generations], |row| {
        Ok(StoredAttachment {
            generation_id: row.get(1)?,
            info: AttachmentInfo {
                id: row.get(0)?,
                name: row.get(2)?,
                mime_type: row.get(3)?,
                bytes: row.get(4)?,
                url: None,
            },
            path: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn insert_attachments(db: &Connection, items: &[NewAttachment]) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    let tx = db.unchecked_transaction()?;
    for item in items {
        tx.execute(
            &format!(
                "INSERT INTO chat_attachments(id,conversation_id,generation_id,name,mime_type,bytes,path,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,{NOW})"
            ),
            params![
                &item.id,
                &item.conversation_id,
                &item.generation_id,
                &item.name,
                &item.mime_type,
                item.bytes,
                &item.path
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

pub fn get_message(db: &Connection, id: &str) -> Result<ChatMessage> {
    let mut message = db.query_row(
        "SELECT id,role,text,reasoning,status,generation_id,error FROM chat_messages WHERE id=?1",
        params![id],
        |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                role: row.get(1)?,
                text: row.get(2)?,
                reasoning: row.get(3)?,
                status: row.get(4)?,
                generation_id: row.get(5)?,
                error: row.get(6)?,
                attachments: None,
            })
        },
    )
    .optional()?
    .ok_or(Error::Missing)?;
    hydrate_stream(db, &mut message)?;
    Ok(message)
}

fn generation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GenerationRow> {
    Ok(GenerationRow {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        state: row.get(2)?,
        sequence: row.get(3)?,
        error: row.get(4)?,
    })
}

const GENERATION_COLUMNS: &str = "id,conversation_id,state,sequence,error";

#[cfg_attr(not(test), allow(dead_code))]
pub fn active_generation(db: &Connection, conversation_id: &str) -> Result<Option<GenerationRow>> {
    let sql = format!(
        "SELECT {GENERATION_COLUMNS} FROM chat_generations
         WHERE conversation_id=?1 AND state IN ('generating','cancelling') ORDER BY rowid DESC LIMIT 1"
    );
    Ok(db
        .query_row(&sql, params![conversation_id], generation_from_row)
        .optional()?)
}

pub fn last_generation(db: &Connection, conversation_id: &str) -> Result<Option<GenerationRow>> {
    let sql = format!(
        "SELECT {GENERATION_COLUMNS} FROM chat_generations WHERE conversation_id=?1
         ORDER BY rowid DESC LIMIT 1"
    );
    Ok(db
        .query_row(&sql, params![conversation_id], generation_from_row)
        .optional()?)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn get_generation(db: &Connection, id: &str) -> Result<GenerationRow> {
    let sql = format!("SELECT {GENERATION_COLUMNS} FROM chat_generations WHERE id=?1");
    db.query_row(&sql, params![id], generation_from_row)
        .optional()?
        .ok_or(Error::Missing)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn count_active_generations(db: &Connection) -> Result<i64> {
    Ok(db.query_row(
        "SELECT count(*) FROM chat_generations WHERE state IN ('generating','cancelling')",
        [],
        |row| row.get(0),
    )?)
}

fn ensure_no_active(
    tx: &Transaction<'_>,
    conversation_id: &str,
    ignore: Option<&str>,
) -> Result<()> {
    let active: Option<String> = tx
        .query_row(
            "SELECT id FROM chat_generations
             WHERE conversation_id=?1 AND state IN ('generating','cancelling') LIMIT 1",
            params![conversation_id],
            |row| row.get(0),
        )
        .optional()?;
    match active {
        Some(id) if Some(id.as_str()) != ignore => Err(Error::Busy),
        _ => Ok(()),
    }
}

/// Atomically reserve a generation and append the user turn plus the assistant placeholder.
pub fn start_generation(db: &mut Connection, new: NewGeneration<'_>) -> Result<()> {
    let tx = db.transaction()?;
    let exists: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM chat_conversations WHERE id=?1",
            params![new.conversation_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(Error::Missing);
    }
    ensure_no_active(&tx, new.conversation_id, None)?;
    let duplicate: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM chat_generations WHERE id=?1",
            params![new.id],
            |row| row.get(0),
        )
        .optional()?;
    if duplicate.is_some() {
        return Err(Error::Duplicate);
    }
    tx.execute(
        &format!(
            "INSERT INTO chat_generations(id,conversation_id,state,sequence,error,created_at,updated_at)
             VALUES(?1,?2,'generating',0,NULL,{NOW},{NOW})"
        ),
        params![new.id, new.conversation_id],
    )?;
    tx.execute(
        &format!(
            "INSERT INTO chat_messages(id,conversation_id,role,text,status,generation_id,error,created_at,updated_at)
             VALUES(?1,?2,'user',?3,'completed',?4,NULL,{NOW},{NOW})"
        ),
        params![
            uuid::Uuid::new_v4().to_string(),
            new.conversation_id,
            new.text,
            new.id
        ],
    )?;
    tx.execute(
        &format!(
            "INSERT INTO chat_messages(id,conversation_id,role,text,status,generation_id,error,created_at,updated_at)
             VALUES(?1,?2,'assistant','','generating',?3,NULL,{NOW},{NOW})"
        ),
        params![new.assistant_id, new.conversation_id, new.id],
    )?;
    if let Some(title) = new.title {
        tx.execute(
            &format!("UPDATE chat_conversations SET title=?2,updated_at={NOW} WHERE id=?1"),
            params![new.conversation_id, title],
        )?;
    } else {
        tx.execute(
            &format!("UPDATE chat_conversations SET updated_at={NOW} WHERE id=?1"),
            params![new.conversation_id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Re-arm an existing terminal generation. The user turn is not duplicated: Pi's session
/// (or the fallback prompt) owns the actual retry.
pub fn begin_retry(
    db: &mut Connection,
    conversation_id: &str,
    generation_id: &str,
    assistant_id: &str,
) -> Result<RetryPlan> {
    let tx = db.transaction()?;
    let generation = tx
        .query_row(
            &format!(
                "SELECT {GENERATION_COLUMNS} FROM chat_generations WHERE id=?1 AND conversation_id=?2"
            ),
            params![generation_id, conversation_id],
            generation_from_row,
        )
        .optional()?
        .ok_or(Error::Missing)?;
    if generation.state == "generating" || generation.state == "cancelling" {
        return Err(Error::Busy);
    }
    ensure_no_active(&tx, conversation_id, Some(generation_id))?;
    tx.execute(
        &format!(
            "UPDATE chat_generations SET state='generating',error=NULL,updated_at={NOW} WHERE id=?1"
        ),
        params![generation_id],
    )?;
    let assistant: Option<String> = tx
        .query_row(
            "SELECT id FROM chat_messages WHERE conversation_id=?1 AND generation_id=?2
             AND role='assistant' ORDER BY rowid DESC LIMIT 1",
            params![conversation_id, generation_id],
            |row| row.get(0),
        )
        .optional()?;
    let user_text: Option<String> = tx
        .query_row(
            "SELECT text FROM chat_messages WHERE conversation_id=?1 AND role='user'
             ORDER BY rowid DESC LIMIT 1",
            params![conversation_id],
            |row| row.get(0),
        )
        .optional()?;
    let assistant_id = match assistant {
        Some(id) => {
            tx.execute(
                &format!(
                    "UPDATE chat_messages SET text='',reasoning=NULL,status='generating',error=NULL,updated_at={NOW}
                     WHERE id=?1"
                ),
                params![id],
            )?;
            id
        }
        None => {
            tx.execute(
                &format!(
                    "INSERT INTO chat_messages(id,conversation_id,role,text,status,generation_id,error,created_at,updated_at)
                     VALUES(?1,?2,'assistant','','generating',?3,NULL,{NOW},{NOW})"
                ),
                params![assistant_id, conversation_id, generation_id],
            )?;
            assistant_id.to_string()
        }
    };
    tx.execute(
        &format!("UPDATE chat_conversations SET updated_at={NOW} WHERE id=?1"),
        params![conversation_id],
    )?;
    tx.execute("DELETE FROM chat_stream_chunks WHERE generation_id=?1", params![generation_id])?;
    tx.commit()?;
    Ok(RetryPlan {
        generation,
        assistant_id,
        user_text: user_text.unwrap_or_default(),
    })
}

/// Persist streamed assistant text, the event sequence and the current state.
pub fn update_progress(
    db: &mut Connection,
    generation_id: &str,
    sequence: i64,
    state: &str,
    text: &str,
    reasoning: Option<&str>,
) -> Result<()> {
    let tx = db.transaction()?;
    let conversation_id: Option<String> = tx
        .query_row(
            "SELECT conversation_id FROM chat_generations WHERE id=?1",
            params![generation_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(conversation_id) = conversation_id else {
        return Err(Error::Missing);
    };
    tx.execute(
        &format!(
            "UPDATE chat_generations SET sequence=?2,state=?3,updated_at={NOW}
             WHERE id=?1 AND state IN ('generating','cancelling')"
        ),
        params![generation_id, sequence, state],
    )?;
    tx.execute(
        &format!(
            "UPDATE chat_messages SET text=?2,reasoning=?4,status='generating',updated_at={NOW}
             WHERE conversation_id=?1 AND generation_id=?3 AND role='assistant'
             AND status='generating'"
        ),
        params![conversation_id, text, generation_id, reasoning],
    )?;
    tx.execute(
        &format!("UPDATE chat_conversations SET updated_at={NOW} WHERE id=?1"),
        params![conversation_id],
    )?;
    tx.execute("DELETE FROM chat_stream_chunks WHERE generation_id=?1", params![generation_id])?;
    tx.commit()?;
    Ok(())
}

/// Persist a terminal state and its final assistant text. Returns the durable message when
/// the generation is still active; already-terminal generations are left untouched.
pub fn finalize_generation(
    db: &mut Connection,
    generation_id: &str,
    state: &str,
    text: &str,
    reasoning: Option<&str>,
    message_status: &str,
    error: Option<&str>,
    sequence: i64,
) -> Result<Option<ChatMessage>> {
    let tx = db.transaction()?;
    let row: Option<(String, i64)> = tx
        .query_row(
            "SELECT conversation_id,sequence FROM chat_generations WHERE id=?1",
            params![generation_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((conversation_id, previous_sequence)) = row else {
        return Err(Error::Missing);
    };
    let changed = tx.execute(
        &format!(
            "UPDATE chat_generations SET state=?2,error=?3,sequence=?4,updated_at={NOW}
             WHERE id=?1 AND state IN ('generating','cancelling')"
        ),
        params![generation_id, state, error, sequence.max(previous_sequence)],
    )?;
    if changed == 0 {
        tx.commit()?;
        return Ok(None);
    }
    let assistant_id: Option<String> = tx
        .query_row(
            "SELECT id FROM chat_messages WHERE conversation_id=?1 AND generation_id=?2
             AND role='assistant' ORDER BY rowid DESC LIMIT 1",
            params![conversation_id, generation_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(ref assistant_id) = assistant_id {
        tx.execute(
            &format!(
                "UPDATE chat_messages SET text=?2,reasoning=?3,status=?4,error=?5,updated_at={NOW} WHERE id=?1"
            ),
            params![assistant_id, text, reasoning, message_status, error],
        )?;
    }
    tx.execute(
        &format!("UPDATE chat_conversations SET updated_at={NOW} WHERE id=?1"),
        params![conversation_id],
    )?;
    tx.execute("DELETE FROM chat_stream_chunks WHERE generation_id=?1", params![generation_id])?;
    tx.commit()?;
    let message = match assistant_id {
        Some(id) => get_message(db, &id)?,
        None => return Ok(None),
    };
    Ok(Some(message))
}

/// Fail an in-flight generation after a worker/session error. Returns the conversation id.
#[cfg_attr(not(test), allow(dead_code))]
pub fn fail_generation(
    db: &mut Connection,
    generation_id: &str,
    error: &str,
) -> Result<Option<String>> {
    let tx = db.transaction()?;
    let conversation_id: Option<String> = tx
        .query_row(
            "SELECT conversation_id FROM chat_generations WHERE id=?1
             AND state IN ('generating','cancelling')",
            params![generation_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(conversation_id) = conversation_id else {
        tx.commit()?;
        return Ok(None);
    };
    materialize_stream(&tx, generation_id)?;
    tx.execute(
        &format!(
            "UPDATE chat_generations SET state='failed',error=?2,updated_at={NOW} WHERE id=?1"
        ),
        params![generation_id, error],
    )?;
    tx.execute(
        &format!(
            "UPDATE chat_messages SET status='failed',error=?3,updated_at={NOW}
             WHERE conversation_id=?1 AND generation_id=?2 AND role='assistant'"
        ),
        params![conversation_id, generation_id, error],
    )?;
    tx.execute(
        &format!("UPDATE chat_conversations SET updated_at={NOW} WHERE id=?1"),
        params![conversation_id],
    )?;
    tx.commit()?;
    Ok(Some(conversation_id))
}

/// Move every generation left mutating by a crash/restart to `failed`.
pub fn recover_interrupted(db: &mut Connection) -> Result<usize> {
    let tx = db.transaction()?;
    let ids: Vec<(String, String)> = {
        let mut statement =
            tx.prepare("SELECT id,conversation_id FROM chat_generations WHERE state IN ('generating','cancelling')")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if ids.is_empty() {
        tx.commit()?;
        return Ok(0);
    }
    for (id, conversation_id) in &ids {
        materialize_stream(&tx, id)?;
        tx.execute(
            &format!(
                "UPDATE chat_generations SET state='failed',error=?2,updated_at={NOW} WHERE id=?1"
            ),
            params![id, INTERRUPTED_ERROR],
        )?;
        tx.execute(
            &format!(
                "UPDATE chat_messages SET status='failed',error=?3,updated_at={NOW}
                 WHERE conversation_id=?1 AND generation_id=?2 AND status='generating'"
            ),
            params![conversation_id, id, INTERRUPTED_ERROR],
        )?;
        tx.execute(
            &format!("UPDATE chat_conversations SET updated_at={NOW} WHERE id=?1"),
            params![conversation_id],
        )?;
    }
    tx.commit()?;
    Ok(ids.len())
}

pub fn import_messages_if_empty(
    db: &mut Connection,
    conversation_id: &str,
    messages: &[ChatMessage],
) -> Result<bool> {
    let tx = db.transaction()?;
    let count: i64 = tx.query_row(
        "SELECT count(*) FROM chat_messages WHERE conversation_id=?1",
        params![conversation_id],
        |row| row.get(0),
    )?;
    if count > 0 {
        tx.commit()?;
        return Ok(false);
    }
    for message in messages {
        tx.execute(
            &format!(
                "INSERT INTO chat_messages(id,conversation_id,role,text,reasoning,status,generation_id,error,created_at,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,{NOW},{NOW})"
            ),
            params![
                message.id,
                conversation_id,
                message.role,
                message.text,
                message.reasoning,
                message.status,
                message.generation_id,
                message.error
            ],
        )?;
    }
    tx.execute(
        &format!("UPDATE chat_conversations SET updated_at={NOW} WHERE id=?1"),
        params![conversation_id],
    )?;
    tx.commit()?;
    Ok(true)
}

fn connection_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConnectionRow> {
    Ok(ConnectionRow {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        name: row.get(2)?,
        model_id: row.get(3)?,
        base_url: row.get(4)?,
        secret: row.get(5)?,
        connected: row.get::<_, i64>(6)? != 0,
    })
}

const CONNECTION_COLUMNS: &str = "id,provider_id,name,model_id,base_url,secret,connected";

pub fn list_connections(db: &Connection) -> Result<Vec<ConnectionInfo>> {
    let sql = format!(
        "SELECT {CONNECTION_COLUMNS} FROM provider_connections ORDER BY created_at ASC, rowid ASC"
    );
    let mut statement = db.prepare(&sql)?;
    let rows = statement.query_map([], connection_from_row)?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .map(ConnectionRow::info)
        .collect())
}

pub fn get_connection(db: &Connection, id: &str) -> Result<ConnectionRow> {
    let sql = format!("SELECT {CONNECTION_COLUMNS} FROM provider_connections WHERE id=?1");
    db.query_row(&sql, params![id], connection_from_row)
        .optional()?
        .ok_or(Error::Missing)
}

/// Insert or refresh the single connection for a provider/name pair. Returns its stable id.
pub fn upsert_connection(
    db: &mut Connection,
    provider_id: &str,
    name: &str,
    model_id: Option<&str>,
    base_url: Option<&str>,
    secret: Option<&[u8]>,
) -> Result<String> {
    let tx = db.transaction()?;
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM provider_connections WHERE provider_id=?1 AND name=?2",
            params![provider_id, name],
            |row| row.get(0),
        )
        .optional()?;
    let id = match existing {
        Some(id) => {
            tx.execute(
                &format!(
                    "UPDATE provider_connections SET model_id=?2,base_url=?3,secret=?4,connected=1,
                     updated_at={NOW} WHERE id=?1"
                ),
                params![id, model_id, base_url, secret],
            )?;
            id
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            tx.execute(
                &format!(
                    "INSERT INTO provider_connections(id,provider_id,name,model_id,base_url,secret,connected,created_at,updated_at)
                     VALUES(?1,?2,?3,?4,?5,?6,1,{NOW},{NOW})"
                ),
                params![id, provider_id, name, model_id, base_url, secret],
            )?;
            id
        }
    };
    tx.commit()?;
    Ok(id)
}

pub fn delete_connection(db: &Connection, id: &str) -> Result<bool> {
    Ok(db.execute("DELETE FROM provider_connections WHERE id=?1", params![id])? > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        // Production databases also carry the memory schema, which init migrates.
        db.execute_batch(include_str!("../memory/schema.sql"))
            .unwrap();
        init(&db).unwrap();
        db
    }

    #[test]
    fn schema_upgrades_an_existing_memory_database_in_place() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../memory/schema.sql"))
            .unwrap();
        db.execute(
            "INSERT INTO settings(key,value) VALUES('onboarding','kept')",
            [],
        )
        .unwrap();
        init(&db).unwrap();
        // Running the host DDL twice is also safe on every launch.
        init(&db).unwrap();
        let kept: String = db
            .query_row(
                "SELECT value FROM settings WHERE key='onboarding'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kept, "kept");
        assert_eq!(list_conversations(&db).unwrap().len(), 0);
    }

    fn selection(db: &mut Connection) -> ModelSelection {
        let id = upsert_connection(
            db,
            "openrouter",
            "Personal",
            Some("openrouter/auto"),
            None,
            Some(b"sealed"),
        )
        .unwrap();
        ModelSelection {
            connection_id: id,
            model_id: "openrouter/auto".into(),
        }
    }

    #[test]
    fn conversation_snapshot_and_active_generation_lock() {
        let mut db = database();
        let selection = selection(&mut db);
        let conversation =
            create_conversation(&db, "c1", "New conversation", Some(&selection)).unwrap();
        assert_eq!(conversation.selection.as_ref(), Some(&selection));

        start_generation(
            &mut db,
            NewGeneration {
                id: "g1",
                conversation_id: "c1",
                assistant_id: "a1",
                text: "Hello",
                title: Some("Hello"),
            },
        )
        .unwrap();
        assert_eq!(active_generation(&db, "c1").unwrap().unwrap().id, "g1");
        assert!(matches!(
            start_generation(
                &mut db,
                NewGeneration {
                    id: "g2",
                    conversation_id: "c1",
                    assistant_id: "a2",
                    text: "Again",
                    title: None,
                },
            ),
            Err(Error::Busy)
        ));
        // A different conversation is independent.
        create_conversation(&db, "c2", "Other", None).unwrap();
        start_generation(
            &mut db,
            NewGeneration {
                id: "g3",
                conversation_id: "c2",
                assistant_id: "a3",
                text: "Elsewhere",
                title: None,
            },
        )
        .unwrap();
        assert_eq!(count_active_generations(&db).unwrap(), 2);

        update_progress(&mut db, "g1", 4, "generating", "Hello there", None).unwrap();
        let message = finalize_generation(
            &mut db,
            "g1",
            "completed",
            "Hello there",
            None,
            "completed",
            None,
            5,
        )
        .unwrap()
        .unwrap();
        assert_eq!(message.status, "completed");
        assert_eq!(message.text, "Hello there");
        // Terminal generations cannot be finalized twice.
        assert!(finalize_generation(
            &mut db,
            "g1",
            "failed",
            "late",
            None,
            "failed",
            Some("late"),
            9
        )
        .unwrap()
        .is_none());

        let rows = list_messages(&db, "c1").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].role, "user");
        assert_eq!(rows[0].text, "Hello");
        assert_eq!(rows[1].role, "assistant");
        let info = get_conversation(&db, "c1").unwrap();
        assert_eq!(info.title, "Hello");
    }

    #[test]
    fn retry_reuses_generation_and_recovery_fails_interrupted_rows() {
        let mut db = database();
        create_conversation(&db, "c1", "New conversation", None).unwrap();
        start_generation(
            &mut db,
            NewGeneration {
                id: "g1",
                conversation_id: "c1",
                assistant_id: "a1",
                text: "Try this",
                title: None,
            },
        )
        .unwrap();
        fail_generation(&mut db, "g1", "network down").unwrap();
        let plan = begin_retry(&mut db, "c1", "g1", "a2").unwrap();
        assert_eq!(plan.generation.id, "g1");
        assert_eq!(plan.assistant_id, "a1");
        assert_eq!(plan.user_text, "Try this");
        assert_eq!(
            active_generation(&db, "c1").unwrap().unwrap().state,
            "generating"
        );
        assert!(matches!(
            begin_retry(&mut db, "c1", "g1", "a3"),
            Err(Error::Busy)
        ));

        assert_eq!(recover_interrupted(&mut db).unwrap(), 1);
        assert!(active_generation(&db, "c1").unwrap().is_none());
        let generation = get_generation(&db, "g1").unwrap();
        assert_eq!(generation.state, "failed");
        assert_eq!(generation.error.as_deref(), Some(INTERRUPTED_ERROR));
        let message = get_message(&db, "a1").unwrap();
        assert_eq!(message.status, "failed");
        assert_eq!(message.error.as_deref(), Some(INTERRUPTED_ERROR));
    }

    #[test]
    fn journal_recovers_deltas_resets_and_checkpoints_without_rewriting_each_chunk() {
        let mut db = database();
        create_conversation(&db, "c", "Stream", None).unwrap();
        start_generation(&mut db, NewGeneration { id: "g", conversation_id: "c", assistant_id: "a", text: "Prompt", title: None }).unwrap();
        assert!(append_progress(&mut db, "g", 1, "generating", "Hello", "Think", false).unwrap());
        assert!(!append_progress(&mut db, "g", 1, "generating", "duplicate", "", false).unwrap());
        append_progress(&mut db, "g", 2, "generating", " 世界", " more", false).unwrap();
        let stored: String = db.query_row("SELECT text FROM chat_messages WHERE id='a'", [], |row| row.get(0)).unwrap();
        assert_eq!(stored, "");
        assert_eq!(get_message(&db, "a").unwrap().text, "Hello 世界");
        assert_eq!(message_page(&db, "c", None, None).unwrap().messages[1].reasoning.as_deref(), Some("Think more"));
        update_progress(&mut db, "g", 2, "generating", "Hello 世界", Some("Think more")).unwrap();
        append_progress(&mut db, "g", 3, "generating", "", "", true).unwrap();
        append_progress(&mut db, "g", 4, "cancelling", "Retried", "new", false).unwrap();
        assert_eq!(get_message(&db, "a").unwrap().text, "Retried");
        assert_eq!(recover_interrupted(&mut db).unwrap(), 1);
        let recovered = get_message(&db, "a").unwrap();
        assert_eq!(recovered.text, "Retried");
        assert_eq!(recovered.reasoning.as_deref(), Some("new"));
        assert_eq!(recovered.status, "failed");
        assert_eq!(db.query_row("SELECT count(*) FROM chat_stream_chunks", [], |row| row.get::<_,i64>(0)).unwrap(), 0);
        assert!(!append_progress(&mut db, "g", 5, "generating", "late", "", false).unwrap());
        begin_retry(&mut db, "c", "g", "retry-a").unwrap();
        assert_eq!(get_message(&db, "a").unwrap().reasoning, None);
        append_progress(&mut db, "g", 5, "generating", "Final", "", false).unwrap();
        finalize_generation(&mut db, "g", "completed", "Final", None, "completed", None, 6).unwrap();
        assert_eq!(get_message(&db, "a").unwrap().text, "Final");
        assert_eq!(db.query_row("SELECT count(*) FROM chat_stream_chunks", [], |row| row.get::<_,i64>(0)).unwrap(), 0);
    }

    #[test]
    fn history_pages_are_bounded_ordered_and_scoped_to_the_conversation() {
        let mut db = database();
        create_conversation(&db, "c", "Long chat", None).unwrap();
        create_conversation(&db, "other", "Other", None).unwrap();
        let messages = (0..10_000).map(|index| ChatMessage {
            id: format!("m{index}"), role: if index % 2 == 0 { "user" } else { "assistant" }.into(),
            text: format!("Message {index}"), reasoning: None, status: "completed".into(),
            generation_id: None, error: None, attachments: None,
        }).collect::<Vec<_>>();
        import_messages_if_empty(&mut db, "c", &messages).unwrap();
        let latest = message_page(&db, "c", None, None).unwrap();
        assert_eq!(latest.messages.len(), 40);
        assert_eq!(latest.messages[0].id, "m9960");
        assert_eq!(latest.messages[39].id, "m9999");
        assert_eq!(latest.before.as_deref(), Some("m9960"));
        assert!(latest.after.is_none());
        let older = message_page(&db, "c", latest.before.as_deref(), None).unwrap();
        assert_eq!(older.messages[0].id, "m9920");
        assert_eq!(older.after.as_deref(), Some("m9959"));
        let newer = message_page(&db, "c", None, older.after.as_deref()).unwrap();
        assert_eq!(newer.messages[0].id, "m9960");
        let first = message_page(&db, "c", Some("m40"), None).unwrap();
        assert_eq!(first.messages[0].id, "m0");
        assert!(first.before.is_none());
        assert!(matches!(message_page(&db, "other", Some("m40"), None), Err(Error::Missing)));
    }

    #[test]
    fn snapshot_json_uses_generation_id_and_round_trips() {
        let snapshot = ConversationSnapshot {
            conversation: ConversationInfo {
                id: "c1".into(),
                title: "Hello".into(),
                session_id: Some("s1".into()),
                session_path: Some("session.jsonl".into()),
                created_at: "2025-01-01T00:00:00.000Z".into(),
                updated_at: "2025-01-01T00:00:01.000Z".into(),
                selection: Some(ModelSelection {
                    connection_id: "conn".into(),
                    model_id: "model".into(),
                }),
                workspace_id: "general".into(),
                archived: false,
            },
            before: None,
            after: None,
            messages: vec![ChatMessage {
                id: "m1".into(),
                role: "assistant".into(),
                text: "partial".into(),
                reasoning: None,
                status: "generating".into(),
                generation_id: Some("g1".into()),
                error: None,
                attachments: None,
            }],
            generation_id: Some("g1".into()),
            state: "generating".into(),
            sequence: 4,
            error: None,
            thinking_level: None,
        };
        let value = serde_json::to_value(&snapshot).unwrap();
        // The UI Stop control reads snapshot.generationId; serializing generation_id would
        // make a reopened in-flight conversation impossible to cancel.
        assert_eq!(value["generationId"], "g1");
        assert!(value.get("generation_id").is_none());
        assert_eq!(value["conversation"]["sessionPath"], "session.jsonl");
        assert_eq!(value["messages"][0]["generationId"], "g1");
        let round_trip: ConversationSnapshot = serde_json::from_value(value).unwrap();
        assert_eq!(round_trip.generation_id.as_deref(), Some("g1"));
        assert_eq!(round_trip.sequence, 4);
        assert_eq!(round_trip.messages[0].generation_id.as_deref(), Some("g1"));
    }

    #[test]
    fn connection_upsert_is_stable_and_secret_stays_out_of_info() {
        let mut db = database();
        let first = upsert_connection(
            &mut db,
            "custom",
            "Local",
            Some("llama"),
            Some("http://127.0.0.1:11434/v1"),
            Some(b"sealed-one"),
        )
        .unwrap();
        let second = upsert_connection(
            &mut db,
            "custom",
            "Local",
            Some("qwen"),
            Some("http://127.0.0.1:1234/v1"),
            Some(b"sealed-two"),
        )
        .unwrap();
        assert_eq!(first, second);
        let row = get_connection(&db, &first).unwrap();
        assert_eq!(row.secret.as_deref(), Some(b"sealed-two".as_slice()));
        assert_eq!(row.model_id.as_deref(), Some("qwen"));
        let info = &list_connections(&db).unwrap()[0];
        assert_eq!(info.provider_id, "custom");
        assert!(info.connected);
        let json = serde_json::to_string(info).unwrap();
        assert!(!json.contains("sealed"));
        assert!(!json.contains("secret"));
        assert!(delete_connection(&db, &first).unwrap());
        assert!(list_connections(&db).unwrap().is_empty());
    }
}
