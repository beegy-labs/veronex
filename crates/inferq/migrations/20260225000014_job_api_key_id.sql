-- Track which API key submitted each inference job.
-- Nullable: existing rows stay NULL; cloud/internal callers may also be NULL.
ALTER TABLE inference_jobs
    ADD COLUMN IF NOT EXISTS api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL;
