-- Add key_type to api_keys: 'standard' (default) or 'test'
ALTER TABLE api_keys
    ADD COLUMN key_type TEXT NOT NULL DEFAULT 'standard';
