CREATE TABLE IF NOT EXISTS inference_jobs (
    id           UUID        PRIMARY KEY,
    prompt       TEXT        NOT NULL,
    model_name   VARCHAR(255) NOT NULL,
    backend      VARCHAR(32) NOT NULL,
    status       VARCHAR(32) NOT NULL DEFAULT 'pending',
    error        TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at   TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS ix_inference_jobs_status     ON inference_jobs(status);
CREATE INDEX IF NOT EXISTS ix_inference_jobs_created_at ON inference_jobs(created_at DESC);
