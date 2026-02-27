-- Track which models are explicitly selected (enabled/disabled) for a backend.
-- Used by paid Gemini backends to restrict routing to a subset of available models.
CREATE TABLE backend_selected_models (
    backend_id  UUID    NOT NULL REFERENCES llm_backends(id) ON DELETE CASCADE,
    model_name  TEXT    NOT NULL,
    is_enabled  BOOLEAN NOT NULL DEFAULT true,
    added_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (backend_id, model_name)
);
