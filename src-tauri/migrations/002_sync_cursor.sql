-- Migration 002: Add Sync Cursor columns for cross-provider context sync
-- Adds seq to messages and synced_through_seq to provider_sessions

ALTER TABLE messages ADD COLUMN seq INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_messages_conversation_seq ON messages(conversation_id, seq);

ALTER TABLE provider_sessions ADD COLUMN synced_through_seq INTEGER NOT NULL DEFAULT 0;
