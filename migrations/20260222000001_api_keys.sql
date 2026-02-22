CREATE TABLE api_keys (
    id              UUID PRIMARY KEY,
    key_hash        VARCHAR(64) NOT NULL UNIQUE,
    key_prefix      VARCHAR(16) NOT NULL,
    tenant_id       VARCHAR(128) NOT NULL,
    name            VARCHAR(255) NOT NULL,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    rate_limit_rpm  INTEGER NOT NULL DEFAULT 0,
    rate_limit_tpm  INTEGER NOT NULL DEFAULT 0,
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX ix_api_keys_tenant ON api_keys(tenant_id);
CREATE INDEX ix_api_keys_hash   ON api_keys(key_hash);
