CREATE TABLE IF NOT EXISTS account_sessions (
    id                 UUID        PRIMARY KEY DEFAULT uuidv7(),
    account_id         UUID        NOT NULL REFERENCES accounts(id),
    jti                UUID        NOT NULL UNIQUE,
    refresh_token_hash VARCHAR(64),
    ip_address         VARCHAR(45),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at       TIMESTAMPTZ,
    expires_at         TIMESTAMPTZ NOT NULL,
    revoked_at         TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_sessions_account_active
    ON account_sessions (account_id, created_at DESC)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_sessions_jti
    ON account_sessions (jti);

CREATE INDEX IF NOT EXISTS idx_sessions_refresh_hash
    ON account_sessions (refresh_token_hash)
    WHERE refresh_token_hash IS NOT NULL;
