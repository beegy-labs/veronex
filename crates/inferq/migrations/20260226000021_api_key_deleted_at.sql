-- Soft-delete support for API keys.
-- deleted_at IS NOT NULL → key is hidden from list and blocked from auth.
ALTER TABLE api_keys
    ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ;
