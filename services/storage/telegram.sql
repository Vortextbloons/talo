CREATE TABLE IF NOT EXISTS telegram_connection (
  id TEXT PRIMARY KEY,
  secret BLOB NOT NULL,
  bot_username TEXT NOT NULL,
  user_id INTEGER,
  chat_id INTEGER,
  account_name TEXT,
  next_update INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS telegram_inbox (
  connection_id TEXT NOT NULL,
  update_id INTEGER NOT NULL,
  generation_id TEXT NOT NULL UNIQUE,
  text TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'received',
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  PRIMARY KEY(connection_id,update_id)
);
CREATE TABLE IF NOT EXISTS telegram_origins (
  id TEXT PRIMARY KEY,
  connection_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  delivered INTEGER NOT NULL DEFAULT 0,
  last_state TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS telegram_outbox (
  id TEXT PRIMARY KEY,
  connection_id TEXT NOT NULL,
  method TEXT NOT NULL,
  payload TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  message_id INTEGER,
  card_kind TEXT,
  card_id TEXT,
  resolved INTEGER NOT NULL DEFAULT 0,
  retry_at INTEGER NOT NULL DEFAULT 0
);
