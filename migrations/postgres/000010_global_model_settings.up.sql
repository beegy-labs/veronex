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
