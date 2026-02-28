ALTER TABLE inference_jobs
  ADD COLUMN IF NOT EXISTS source VARCHAR(8) NOT NULL DEFAULT 'api';

CREATE INDEX IF NOT EXISTS idx_inference_jobs_source ON inference_jobs(source);
