-- Global pool of Gemini models fetched via the admin API key.
CREATE TABLE gemini_models (
    model_name TEXT        PRIMARY KEY,
    synced_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
