CREATE TABLE library_workspaces (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, folder TEXT,
 archived INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO library_workspaces(id,name) VALUES('general','General');
-- SQLite cannot add a REFERENCES column with a non-NULL default; workspace
-- membership for existing rows is app-enforced with the 'general' default.
ALTER TABLE chat_conversations ADD COLUMN workspace_id TEXT NOT NULL DEFAULT 'general';
ALTER TABLE chat_conversations ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
CREATE UNIQUE INDEX chat_session_path_unique ON chat_conversations(session_path) WHERE session_path IS NOT NULL;
CREATE TABLE library_memories (
 id TEXT PRIMARY KEY, scope TEXT NOT NULL CHECK(scope IN ('global','workspace')),
 workspace_id TEXT REFERENCES library_workspaces(id) ON DELETE CASCADE,
 content TEXT NOT NULL, source TEXT NOT NULL, personal INTEGER NOT NULL DEFAULT 0,
 approved INTEGER NOT NULL DEFAULT 0, superseded_by TEXT REFERENCES library_memories(id),
 created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
 CHECK ((scope='global' AND workspace_id IS NULL) OR (scope='workspace' AND workspace_id IS NOT NULL)),
 CHECK (personal=0 OR approved=1)
);
CREATE VIRTUAL TABLE library_memory_fts USING fts5(content,content='library_memories',content_rowid='rowid');
CREATE TRIGGER library_memory_ai AFTER INSERT ON library_memories BEGIN
 INSERT INTO library_memory_fts(rowid,content) VALUES(new.rowid,new.content); END;
CREATE TRIGGER library_memory_ad AFTER DELETE ON library_memories BEGIN
 INSERT INTO library_memory_fts(library_memory_fts,rowid,content) VALUES('delete',old.rowid,old.content); END;
CREATE TRIGGER library_memory_au AFTER UPDATE ON library_memories BEGIN
 INSERT INTO library_memory_fts(library_memory_fts,rowid,content) VALUES('delete',old.rowid,old.content);
 INSERT INTO library_memory_fts(rowid,content) VALUES(new.rowid,new.content); END;
-- Rebuildable projection only. Pi JSONL remains authoritative.
CREATE VIRTUAL TABLE chat_fts USING fts5(text,content='chat_messages',content_rowid='rowid');
CREATE TRIGGER chat_fts_ai AFTER INSERT ON chat_messages BEGIN
 INSERT INTO chat_fts(rowid,text) VALUES(new.rowid,new.text); END;
CREATE TRIGGER chat_fts_ad AFTER DELETE ON chat_messages BEGIN
 INSERT INTO chat_fts(chat_fts,rowid,text) VALUES('delete',old.rowid,old.text); END;
CREATE TRIGGER chat_fts_au AFTER UPDATE OF text ON chat_messages BEGIN
 INSERT INTO chat_fts(chat_fts,rowid,text) VALUES('delete',old.rowid,old.text);
 INSERT INTO chat_fts(rowid,text) VALUES(new.rowid,new.text); END;
INSERT INTO chat_fts(chat_fts) VALUES('rebuild');
CREATE TABLE context_usage (
 conversation_id TEXT PRIMARY KEY REFERENCES chat_conversations(id) ON DELETE CASCADE,
 compactions INTEGER NOT NULL DEFAULT 0, usage TEXT, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT OR IGNORE INTO settings(key,value) VALUES('memory_enabled','true');
INSERT OR IGNORE INTO settings(key,value) VALUES('active_workspace','general');
