-- Global model enable/disable settings.
-- When is_enabled = false, the model is blocked on ALL providers regardless
-- of per-provider selected_models state. Per-provider state is preserved
-- and restored when global setting is re-enabled.
CREATE TABLE IF NOT EXISTS global_model_settings (
    model_name  TEXT        PRIMARY KEY,
    is_enabled  BOOLEAN     NOT NULL DEFAULT true,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
