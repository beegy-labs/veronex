-- Full prompt now stored in S3; inference_jobs only stores prompt_preview.
ALTER TABLE inference_jobs ALTER COLUMN prompt DROP NOT NULL;
ALTER TABLE inference_jobs ALTER COLUMN prompt SET DEFAULT '';
