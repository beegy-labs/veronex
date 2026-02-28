ALTER TABLE inference_jobs
    ADD COLUMN IF NOT EXISTS prompt_tokens INTEGER;
