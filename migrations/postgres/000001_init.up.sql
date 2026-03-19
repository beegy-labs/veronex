-- ============================================================
-- Veronex complete database schema (consolidated init)
-- Last updated: 2026-03-03
-- ============================================================

-- ── Accounts ──────────────────────────────────────────────────────────────────

CREATE TABLE accounts (
    id            UUID        PRIMARY KEY DEFAULT uuidv7(),
    username      VARCHAR(64) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    name          VARCHAR(128) NOT NULL,
    email         VARCHAR(255),
    role          VARCHAR(16)  NOT NULL DEFAULT 'admin'
                  CHECK (role IN ('super', 'admin')),
    department    VARCHAR(128),
    position      VARCHAR(128),
    is_active     BOOLEAN     NOT NULL DEFAULT true,
    created_by    UUID REFERENCES accounts(id),
    last_login_at TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ
);

CREATE INDEX idx_accounts_username ON accounts(username) WHERE deleted_at IS NULL;

-- ── Account Sessions ──────────────────────────────────────────────────────────

CREATE TABLE account_sessions (
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

CREATE INDEX idx_sessions_account_active
    ON account_sessions (account_id, created_at DESC)
    WHERE revoked_at IS NULL;

CREATE INDEX idx_sessions_jti ON account_sessions (jti);

CREATE INDEX idx_sessions_refresh_hash
    ON account_sessions (refresh_token_hash)
    WHERE refresh_token_hash IS NOT NULL;

-- ── API Keys ──────────────────────────────────────────────────────────────────

CREATE TABLE api_keys (
    id             UUID        PRIMARY KEY DEFAULT uuidv7(),
    key_hash       VARCHAR(64) NOT NULL UNIQUE,
    key_prefix     VARCHAR(16) NOT NULL,
    tenant_id      VARCHAR(128) NOT NULL,
    name           VARCHAR(255) NOT NULL,
    is_active      BOOLEAN     NOT NULL DEFAULT TRUE,
    rate_limit_rpm INTEGER     NOT NULL DEFAULT 0,
    rate_limit_tpm INTEGER     NOT NULL DEFAULT 0,
    expires_at     TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at     TIMESTAMPTZ,
    key_type       TEXT        NOT NULL DEFAULT 'standard',
    account_id     UUID        REFERENCES accounts(id),
    is_test_key    BOOLEAN     NOT NULL DEFAULT false,
    tier           TEXT        NOT NULL DEFAULT 'paid'
);

CREATE INDEX ix_api_keys_tenant ON api_keys(tenant_id);
CREATE INDEX ix_api_keys_hash   ON api_keys(key_hash);

CREATE UNIQUE INDEX uq_api_keys_account_test
    ON api_keys (account_id)
    WHERE is_test_key = true AND deleted_at IS NULL;

-- ── GPU Servers ───────────────────────────────────────────────────────────────

CREATE TABLE gpu_servers (
    id                UUID         PRIMARY KEY DEFAULT uuidv7(),
    name              VARCHAR(255) NOT NULL,
    node_exporter_url TEXT,
    registered_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- ── LLM Providers ─────────────────────────────────────────────────────────────

CREATE TABLE llm_providers (
    id                UUID        PRIMARY KEY DEFAULT uuidv7(),
    name              VARCHAR(255) NOT NULL,
    provider_type     VARCHAR(32) NOT NULL,
    url               TEXT        NOT NULL DEFAULT '',
    api_key_encrypted TEXT,
    is_active         BOOLEAN     NOT NULL DEFAULT true,
    total_vram_mb     BIGINT      NOT NULL DEFAULT 0,
    status            VARCHAR(32) NOT NULL DEFAULT 'offline',
    registered_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    gpu_index         SMALLINT,
    server_id         UUID        REFERENCES gpu_servers(id) ON DELETE SET NULL,
    is_free_tier      BOOLEAN     NOT NULL DEFAULT false
);

CREATE INDEX ix_llm_providers_is_active ON llm_providers(is_active);
CREATE INDEX ix_llm_providers_status    ON llm_providers(status);

-- ── Inference Jobs ────────────────────────────────────────────────────────────

CREATE TABLE inference_jobs (
    id                   UUID        PRIMARY KEY DEFAULT uuidv7(),
    prompt               TEXT        NOT NULL,
    model_name           VARCHAR(255) NOT NULL,
    provider_type        VARCHAR(32) NOT NULL,
    status               VARCHAR(32) NOT NULL DEFAULT 'pending',
    error                TEXT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at           TIMESTAMPTZ,
    completed_at         TIMESTAMPTZ,
    result_text          TEXT,
    api_key_id           UUID        REFERENCES api_keys(id) ON DELETE SET NULL,
    latency_ms           INTEGER,
    ttft_ms              INTEGER,
    completion_tokens    INTEGER,
    prompt_tokens        INTEGER,
    cached_tokens        INTEGER,
    source               VARCHAR(8)  NOT NULL DEFAULT 'api',
    account_id           UUID        REFERENCES accounts(id),
    provider_id          UUID        REFERENCES llm_providers(id),
    api_format           TEXT        NOT NULL DEFAULT 'openai_compat',
    request_path         TEXT,
    conversation_id      TEXT,
    tool_calls_json      JSONB,
    messages_json        JSONB,
    queue_time_ms        INT,
    cancelled_at         TIMESTAMPTZ,
    messages_hash        TEXT,
    messages_prefix_hash TEXT
);

CREATE INDEX ix_inference_jobs_status     ON inference_jobs(status);
CREATE INDEX ix_inference_jobs_created_at ON inference_jobs(created_at DESC);
CREATE INDEX idx_inference_jobs_source    ON inference_jobs(source);

CREATE INDEX idx_inference_jobs_conversation_id
    ON inference_jobs(conversation_id)
    WHERE conversation_id IS NOT NULL;

CREATE INDEX idx_inference_jobs_tool_calls
    ON inference_jobs USING GIN (tool_calls_json)
    WHERE tool_calls_json IS NOT NULL;

CREATE INDEX idx_inference_jobs_messages_hash
    ON inference_jobs (api_key_id, messages_hash)
    WHERE messages_hash IS NOT NULL;

CREATE INDEX idx_inference_jobs_session_ungrouped
    ON inference_jobs (api_key_id, messages_prefix_hash, created_at)
    WHERE conversation_id IS NULL
      AND messages_prefix_hash IS NOT NULL
      AND messages_prefix_hash != '';

CREATE INDEX idx_inference_jobs_provider_capacity
    ON inference_jobs(provider_id, model_name, created_at DESC)
    WHERE status = 'completed';

-- ── Provider Selected Models ──────────────────────────────────────────────────

CREATE TABLE provider_selected_models (
    provider_id UUID    NOT NULL REFERENCES llm_providers(id) ON DELETE CASCADE,
    model_name  TEXT    NOT NULL,
    is_enabled  BOOLEAN NOT NULL DEFAULT true,
    added_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (provider_id, model_name)
);

-- ── Gemini Rate Limit Policies ────────────────────────────────────────────────

CREATE TABLE gemini_rate_limit_policies (
    id                     UUID    PRIMARY KEY DEFAULT uuidv7(),
    model_name             TEXT    NOT NULL UNIQUE,
    rpm_limit              INTEGER NOT NULL DEFAULT 0,
    rpd_limit              INTEGER NOT NULL DEFAULT 0,
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    available_on_free_tier BOOLEAN NOT NULL DEFAULT true
);

INSERT INTO gemini_rate_limit_policies (model_name, rpm_limit, rpd_limit, available_on_free_tier) VALUES
    ('gemini-2.5-flash',         5,  20, true),
    ('gemini-2.5-flash-lite',   10,  20, true),
    ('gemini-3-flash-preview',   5,  20, true),
    ('gemini-2.5-pro',           5,  25, false),
    ('*',                       10, 250, false);

-- ── Gemini Sync Config ────────────────────────────────────────────────────────

CREATE TABLE gemini_sync_config (
    id                INTEGER     PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    api_key_encrypted TEXT        NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── Gemini Models ─────────────────────────────────────────────────────────────

CREATE TABLE gemini_models (
    model_name TEXT        PRIMARY KEY,
    synced_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── Ollama Models ─────────────────────────────────────────────────────────────

CREATE TABLE ollama_models (
    model_name  TEXT NOT NULL,
    provider_id UUID NOT NULL REFERENCES llm_providers(id) ON DELETE CASCADE,
    synced_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (model_name, provider_id)
);

-- ── Ollama Sync Jobs ──────────────────────────────────────────────────────────

CREATE TABLE ollama_sync_jobs (
    id              UUID        PRIMARY KEY DEFAULT uuidv7(),
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at    TIMESTAMPTZ,
    status          TEXT        NOT NULL DEFAULT 'running',
    total_providers INT         NOT NULL DEFAULT 0,
    done_providers  INT         NOT NULL DEFAULT 0,
    results         JSONB       NOT NULL DEFAULT '[]'::jsonb
);

-- ── Model VRAM Profiles ───────────────────────────────────────────────────────

CREATE TABLE model_vram_profiles (
    provider_id       UUID     NOT NULL REFERENCES llm_providers(id) ON DELETE CASCADE,
    model_name        TEXT     NOT NULL,
    weight_mb         INT      NOT NULL DEFAULT 0,
    weight_estimated  BOOLEAN  NOT NULL DEFAULT true,
    kv_per_request_mb INT      NOT NULL DEFAULT 0,
    num_layers        SMALLINT NOT NULL DEFAULT 0,
    num_kv_heads      SMALLINT NOT NULL DEFAULT 0,
    head_dim          SMALLINT NOT NULL DEFAULT 0,
    configured_ctx    INT      NOT NULL DEFAULT 0,
    failure_count     SMALLINT NOT NULL DEFAULT 0,
    llm_concern       TEXT,
    llm_reason        TEXT,
    max_concurrent    INT      NOT NULL DEFAULT 0,
    baseline_tps      INT      NOT NULL DEFAULT 0,
    baseline_p95_ms   INT      NOT NULL DEFAULT 0,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (provider_id, model_name)
);

-- ── Capacity Settings ─────────────────────────────────────────────────────────

CREATE TABLE capacity_settings (
    id                  INT     PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    analyzer_model      TEXT    NOT NULL DEFAULT '',
    sync_enabled        BOOLEAN NOT NULL DEFAULT true,
    sync_interval_secs  INT     NOT NULL DEFAULT 300,
    probe_permits       INT     NOT NULL DEFAULT 1,
    probe_rate          INT     NOT NULL DEFAULT 3,
    last_run_at         TIMESTAMPTZ,
    last_run_status     TEXT,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Model Pricing ─────────────────────────────────────────────────────────────

CREATE TABLE model_pricing (
    provider      TEXT   NOT NULL,
    model_name    TEXT   NOT NULL,
    input_per_1m  FLOAT8 NOT NULL DEFAULT 0,
    output_per_1m FLOAT8 NOT NULL DEFAULT 0,
    currency      TEXT   NOT NULL DEFAULT 'USD',
    notes         TEXT,
    PRIMARY KEY (provider, model_name)
);

-- ── Lab Settings ──────────────────────────────────────────────────────────────

CREATE TABLE lab_settings (
    id                      INT     PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    gemini_function_calling BOOLEAN NOT NULL DEFAULT false,
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO lab_settings (id) VALUES (1) ON CONFLICT DO NOTHING;
