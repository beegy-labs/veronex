-- Add source column to conversations (api / test / analyzer)
ALTER TABLE conversations
    ADD COLUMN IF NOT EXISTS source VARCHAR(8) NOT NULL DEFAULT 'api';

CREATE INDEX IF NOT EXISTS idx_conversations_source ON conversations(source);
