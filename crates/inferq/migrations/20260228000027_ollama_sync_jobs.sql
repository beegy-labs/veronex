CREATE TABLE ollama_sync_jobs (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    started_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at   TIMESTAMPTZ,
    status         TEXT        NOT NULL DEFAULT 'running',
    total_backends INT         NOT NULL DEFAULT 0,
    done_backends  INT         NOT NULL DEFAULT 0,
    results        JSONB       NOT NULL DEFAULT '[]'::jsonb
);
