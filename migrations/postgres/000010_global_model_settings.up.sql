-- Global model enable/disable settings.
-- When is_enabled = false, the model is blocked on ALL providers regardless
-- of per-provider selected_models state. Per-provider state is preserved
-- and restored when global setting is re-enabled.
CREATE TABLE IF NOT EXISTS global_model_settings (
    model_name  TEXT        PRIMARY KEY,
    is_enabled  BOOLEAN     NOT NULL DEFAULT true,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- API key → provider access control.
-- When no rows exist for a key, all providers are accessible (default allow-all).
-- When rows exist, only providers with is_allowed = true are routable for that key.
CREATE TABLE IF NOT EXISTS api_key_provider_access (
    api_key_id   UUID    NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    provider_id  UUID    NOT NULL REFERENCES llm_providers(id) ON DELETE CASCADE,
    is_allowed   BOOLEAN NOT NULL DEFAULT true,
    PRIMARY KEY (api_key_id, provider_id)
);

-- Add model_manage permission to super role.
UPDATE roles
SET permissions = array_append(permissions, 'model_manage')
WHERE name = 'super'
  AND NOT ('model_manage' = ANY(permissions));

-- ── Trigram indexes for 10K+ scale ILIKE search ───────────────────────────────
-- Required by paginated list endpoints: ?search=... uses ILIKE which needs
-- pg_trgm GIN indexes to avoid full table scans at scale.
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX IF NOT EXISTS idx_ollama_models_name_trgm
    ON ollama_models USING GIN (model_name gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_llm_providers_name_trgm
    ON llm_providers USING GIN (name gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_llm_providers_url_trgm
    ON llm_providers USING GIN (url gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_accounts_name_trgm
    ON accounts USING GIN (name gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_accounts_username_trgm
    ON accounts USING GIN (username gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_api_keys_name_trgm
    ON api_keys USING GIN (name gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_gpu_servers_name_trgm
    ON gpu_servers USING GIN (name gin_trgm_ops);

-- Composite index for providers_info_for_model_page LEFT JOIN
CREATE INDEX IF NOT EXISTS idx_provider_selected_models_lookup
    ON provider_selected_models (provider_id, model_name);
