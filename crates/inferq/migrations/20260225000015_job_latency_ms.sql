-- Store actual inference latency (started_at → completed_at) in ms.
-- Computed and persisted when a job completes so it's available without
-- re-calculating from timestamps on every read.
ALTER TABLE inference_jobs
    ADD COLUMN IF NOT EXISTS latency_ms INTEGER;
