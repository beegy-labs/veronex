ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS account_id UUID REFERENCES accounts(id);
ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS is_test_key BOOLEAN NOT NULL DEFAULT false;
CREATE UNIQUE INDEX IF NOT EXISTS uq_api_keys_account_test
    ON api_keys (account_id) WHERE is_test_key = true AND deleted_at IS NULL;
