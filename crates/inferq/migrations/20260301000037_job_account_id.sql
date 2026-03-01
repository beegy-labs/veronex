-- Add account_id to inference_jobs for Test Run tracking.
-- Test Run jobs: account_id = JWT subject, api_key_id = NULL.
-- API key jobs:  api_key_id = key.id,  account_id = NULL.
ALTER TABLE inference_jobs
  ADD COLUMN account_id UUID REFERENCES accounts(id);
