-- Store the full inference output so completed jobs can be replayed after restart.
ALTER TABLE inference_jobs
    ADD COLUMN IF NOT EXISTS result_text TEXT;
