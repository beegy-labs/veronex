-- Singleton row that holds the admin Gemini API key used for global model sync.
CREATE TABLE gemini_sync_config (
    id                INTEGER     PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    api_key_encrypted TEXT        NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
