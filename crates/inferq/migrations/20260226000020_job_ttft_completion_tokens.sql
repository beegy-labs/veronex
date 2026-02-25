ALTER TABLE inference_jobs
    ADD COLUMN IF NOT EXISTS ttft_ms INTEGER,
    ADD COLUMN IF NOT EXISTS completion_tokens INTEGER;
