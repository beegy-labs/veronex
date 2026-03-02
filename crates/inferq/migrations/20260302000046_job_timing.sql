-- Migration 000046: Job timing columns
-- queue_time_ms : created_at → started_at  (time spent waiting in the Valkey queue)
-- cancelled_at  : timestamp when a cancel request was received

ALTER TABLE inference_jobs
    ADD COLUMN IF NOT EXISTS queue_time_ms INT,
    ADD COLUMN IF NOT EXISTS cancelled_at  TIMESTAMPTZ;
