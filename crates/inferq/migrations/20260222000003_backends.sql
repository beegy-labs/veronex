-- LLM backend registry
-- Stores Ollama servers and Gemini API credentials registered at runtime.

CREATE TABLE IF NOT EXISTS llm_backends (
    id              UUID        PRIMARY KEY,
    name            VARCHAR(255) NOT NULL,
    backend_type    VARCHAR(32)  NOT NULL,
    url             TEXT        NOT NULL DEFAULT '',
    api_key_encrypted TEXT,
    is_active       BOOLEAN     NOT NULL DEFAULT true,
    total_vram_mb   BIGINT      NOT NULL DEFAULT 0,
    status          VARCHAR(32)  NOT NULL DEFAULT 'offline',
    registered_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS ix_llm_backends_is_active ON llm_backends(is_active);
CREATE INDEX IF NOT EXISTS ix_llm_backends_status    ON llm_backends(status);
