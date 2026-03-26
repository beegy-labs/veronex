-- ============================================================
-- Migration: S3 conversation store
-- Move large content columns to S3/MinIO (ConversationRecord).
-- Only prompt_preview (≤200 chars) and has_tool_calls remain in Postgres.
-- ============================================================

-- 1. Add lightweight metadata columns
ALTER TABLE inference_jobs
    ADD COLUMN IF NOT EXISTS prompt_preview VARCHAR(200);

ALTER TABLE inference_jobs
    ADD COLUMN IF NOT EXISTS has_tool_calls BOOLEAN NOT NULL DEFAULT FALSE;

-- 2. Backfill from existing data (must happen before DROP)
UPDATE inference_jobs
SET prompt_preview = LEFT(prompt, 200)
WHERE prompt_preview IS NULL AND prompt IS NOT NULL;

UPDATE inference_jobs
SET has_tool_calls = TRUE
WHERE tool_calls_json IS NOT NULL;

-- 3. Drop large content columns (now stored in S3)
ALTER TABLE inference_jobs DROP COLUMN IF EXISTS prompt;
ALTER TABLE inference_jobs DROP COLUMN IF EXISTS result_text;
ALTER TABLE inference_jobs DROP COLUMN IF EXISTS messages_json;
ALTER TABLE inference_jobs DROP COLUMN IF EXISTS tool_calls_json;
