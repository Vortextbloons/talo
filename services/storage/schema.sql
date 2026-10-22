-- Talo chat and provider metadata.
--
-- These tables are deliberately separate from services/memory's workspace-oriented
-- `sessions`/`messages` tables: chat conversations are app-level, one persistent Pi
-- JSONL session per conversation, and credentials are stored as DPAPI-protected blobs.
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS chat_conversations (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  session_id TEXT,
  session_path TEXT,
  connection_id TEXT,
  model_id TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS chat_conversations_updated
  ON chat_conversations(updated_at DESC);

CREATE TABLE IF NOT EXISTS chat_messages (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK(role IN ('user','assistant')),
  text TEXT NOT NULL DEFAULT '',
  reasoning TEXT,
  status TEXT NOT NULL CHECK(status IN ('completed','generating','cancelled','failed')),
  generation_id TEXT,
  error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS chat_messages_conversation
  ON chat_messages(conversation_id);
CREATE INDEX IF NOT EXISTS chat_messages_generation
  ON chat_messages(generation_id);

-- Images the user attached to a turn. The bytes live inside Pi's agent workspace so the
-- model's own file tools can open them; this table only carries the metadata for display.
CREATE TABLE IF NOT EXISTS chat_attachments (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
  generation_id TEXT,
  name TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  bytes INTEGER NOT NULL,
  path TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS chat_attachments_conversation
  ON chat_attachments(conversation_id);
CREATE INDEX IF NOT EXISTS chat_attachments_generation
  ON chat_attachments(generation_id);

CREATE TABLE IF NOT EXISTS chat_generations (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
  state TEXT NOT NULL,
  sequence INTEGER NOT NULL DEFAULT 0,
  error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS chat_generations_conversation
  ON chat_generations(conversation_id);
-- One mutating generation per conversation. This is the durable half of the
-- generation lock; in-memory state is only a projection of it.
CREATE UNIQUE INDEX IF NOT EXISTS chat_generations_one_active
  ON chat_generations(conversation_id)
  WHERE state IN ('generating','cancelling');

CREATE TABLE IF NOT EXISTS chat_stream_chunks (
  generation_id TEXT NOT NULL REFERENCES chat_generations(id) ON DELETE CASCADE,
  sequence INTEGER NOT NULL,
  text TEXT NOT NULL,
  reasoning TEXT NOT NULL,
  reset INTEGER NOT NULL,
  PRIMARY KEY(generation_id,sequence)
);

CREATE TABLE IF NOT EXISTS provider_connections (
  id TEXT PRIMARY KEY,
  provider_id TEXT NOT NULL,
  name TEXT NOT NULL,
  model_id TEXT,
  base_url TEXT,
  secret BLOB,
  connected INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(provider_id, name)
);

CREATE INDEX IF NOT EXISTS provider_connections_provider
  ON provider_connections(provider_id);
