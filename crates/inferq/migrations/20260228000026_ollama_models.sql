CREATE TABLE ollama_models (
    model_name TEXT NOT NULL,
    backend_id UUID NOT NULL REFERENCES llm_backends(id) ON DELETE CASCADE,
    synced_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (model_name, backend_id)
);
