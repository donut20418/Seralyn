-- MyAI Phase 1 Initial Schema
-- Source of truth for all conversation data

CREATE TABLE IF NOT EXISTS conversations (
    id              TEXT PRIMARY KEY,
    title           TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now')),
    archived        INTEGER NOT NULL DEFAULT 0,
    metadata_json   TEXT
);

CREATE TABLE IF NOT EXISTS messages (
    id                  TEXT PRIMARY KEY,
    conversation_id     TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    parent_id           TEXT REFERENCES messages(id),
    role                TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system', 'tool')),
    content             TEXT NOT NULL,
    provider            TEXT CHECK(provider IN ('claude', 'codex', 'gemini') OR provider IS NULL),
    model               TEXT,
    provider_session_id TEXT,
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    token_estimate      INTEGER,
    metadata_json       TEXT
);
CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_messages_parent ON messages(parent_id);

CREATE TABLE IF NOT EXISTS provider_sessions (
    id                  TEXT PRIMARY KEY,
    conversation_id     TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    provider            TEXT NOT NULL CHECK(provider IN ('claude', 'codex', 'gemini')),
    provider_session_id TEXT,
    model               TEXT,
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at        TEXT,
    status              TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'closed', 'error')),
    metadata_json       TEXT
);
CREATE INDEX IF NOT EXISTS idx_provider_sessions_conversation ON provider_sessions(conversation_id);

CREATE TABLE IF NOT EXISTS tool_calls (
    id                  TEXT PRIMARY KEY,
    message_id          TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    tool_name           TEXT NOT NULL,
    tool_input          TEXT,
    tool_result         TEXT,
    status              TEXT NOT NULL CHECK(status IN ('pending', 'approved', 'denied', 'completed', 'error')),
    created_at          TEXT NOT NULL DEFAULT (datetime('now')),
    metadata_json       TEXT
);
CREATE INDEX IF NOT EXISTS idx_tool_calls_message ON tool_calls(message_id);

CREATE TABLE IF NOT EXISTS usage_snapshots (
    id                  TEXT PRIMARY KEY,
    conversation_id     TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    provider_session_id TEXT REFERENCES provider_sessions(id),
    input_tokens        INTEGER,
    output_tokens       INTEGER,
    cache_read_tokens   INTEGER,
    cache_write_tokens  INTEGER,
    reasoning_tokens    INTEGER,
    context_tokens      INTEGER,
    context_window      INTEGER,
    confidence          TEXT NOT NULL DEFAULT 'ESTIMATED' CHECK(confidence IN ('EXACT', 'ESTIMATED')),
    created_at          TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_usage_conversation ON usage_snapshots(conversation_id);

CREATE TABLE IF NOT EXISTS settings (
    key     TEXT PRIMARY KEY,
    value   TEXT NOT NULL
);

-- Insert default settings
INSERT OR IGNORE INTO settings (key, value) VALUES ('default_provider', 'claude');
INSERT OR IGNORE INTO settings (key, value) VALUES ('permission_mode', 'workspace');
INSERT OR IGNORE INTO settings (key, value) VALUES ('auto_compact_warning', '0.7');
INSERT OR IGNORE INTO settings (key, value) VALUES ('auto_compact_threshold', '0.8');
INSERT OR IGNORE INTO settings (key, value) VALUES ('auto_compact_critical', '0.9');
