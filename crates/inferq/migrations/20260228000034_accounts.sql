CREATE TABLE IF NOT EXISTS accounts (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    username      VARCHAR(64) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    name          VARCHAR(128) NOT NULL,
    email         VARCHAR(255),
    role          VARCHAR(16)  NOT NULL DEFAULT 'admin'
                  CHECK (role IN ('super', 'admin')),
    department    VARCHAR(128),
    position      VARCHAR(128),
    is_active     BOOLEAN NOT NULL DEFAULT true,
    created_by    UUID REFERENCES accounts(id),
    last_login_at TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_accounts_username ON accounts(username) WHERE deleted_at IS NULL;
