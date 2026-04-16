-- ============================================================
-- Veronex complete database schema (consolidated init)
-- Last updated: 2026-04-16
-- ============================================================

-- ── Idempotent schema migrations ─────────────────────────────────────────────
-- Applied before CREATE TABLE statements below. On fresh installs every ALTER
-- is a no-op (nothing to alter); on existing DBs these transition the schema
-- so subsequent CREATE TABLE statements error out (table already exists) and
-- psql (without ON_ERROR_STOP) continues past them, leaving the migrated
-- schema in place.
DROP INDEX IF EXISTS idx_llm_providers_is_active;
ALTER TABLE IF EXISTS llm_providers DROP COLUMN IF EXISTS is_active;
ALTER TABLE IF EXISTS gpu_servers ADD COLUMN IF NOT EXISTS gpu_vendor VARCHAR(32) NOT NULL DEFAULT '';
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.referential_constraints
         WHERE constraint_name = 'inference_jobs_provider_id_fkey'
           AND delete_rule != 'SET NULL'
    ) THEN
        ALTER TABLE inference_jobs DROP CONSTRAINT inference_jobs_provider_id_fkey;
        ALTER TABLE inference_jobs
            ADD CONSTRAINT inference_jobs_provider_id_fkey
            FOREIGN KEY (provider_id) REFERENCES llm_providers(id) ON DELETE SET NULL;
    END IF;
END$$;

-- ── Accounts ──────────────────────────────────────────────────────────────────

CREATE TABLE accounts (
    id            UUID        PRIMARY KEY DEFAULT uuidv7(),
    username      VARCHAR(64) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    name          VARCHAR(128) NOT NULL,
    email         VARCHAR(255),
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
    tier           TEXT        NOT NULL DEFAULT 'paid',
    mcp_cap_points SMALLINT    NOT NULL DEFAULT 3 CHECK (mcp_cap_points BETWEEN 0 AND 10)
);

CREATE INDEX idx_api_keys_tenant ON api_keys(tenant_id);
CREATE INDEX idx_api_keys_hash   ON api_keys(key_hash);

CREATE UNIQUE INDEX uq_api_keys_account_test
    ON api_keys (account_id)
    WHERE is_test_key = true AND deleted_at IS NULL;

-- ── GPU Servers ───────────────────────────────────────────────────────────────

CREATE TABLE gpu_servers (
    id                UUID         PRIMARY KEY DEFAULT uuidv7(),
    name              VARCHAR(255) NOT NULL,
    node_exporter_url TEXT,
    gpu_vendor        VARCHAR(32)  NOT NULL DEFAULT '',
    registered_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- ── LLM Providers ─────────────────────────────────────────────────────────────

CREATE TABLE llm_providers (
    id                UUID        PRIMARY KEY DEFAULT uuidv7(),
    name              VARCHAR(255) NOT NULL,
    provider_type     VARCHAR(32) NOT NULL,
    url               TEXT        NOT NULL DEFAULT '',
    api_key_encrypted TEXT,
    total_vram_mb     BIGINT      NOT NULL DEFAULT 0,
    status            VARCHAR(32) NOT NULL DEFAULT 'offline',
    registered_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    gpu_index         SMALLINT,
    server_id         UUID        REFERENCES gpu_servers(id) ON DELETE SET NULL,
    is_free_tier      BOOLEAN     NOT NULL DEFAULT false,
    num_parallel      SMALLINT    NOT NULL DEFAULT 4
);

CREATE INDEX idx_llm_providers_status    ON llm_providers(status);
CREATE UNIQUE INDEX uq_llm_providers_ollama_url ON llm_providers(url) WHERE provider_type = 'ollama';

-- ── Conversations ─────────────────────────────────────────────────────────────
-- source column included (migration 000002)

CREATE TABLE conversations (
    id                      UUID        PRIMARY KEY DEFAULT uuidv7(),
    account_id              UUID        REFERENCES accounts(id) ON DELETE SET NULL,
    api_key_id              UUID        REFERENCES api_keys(id) ON DELETE SET NULL,
    title                   TEXT,
    model_name              VARCHAR(255),
    source                  VARCHAR(8)  NOT NULL DEFAULT 'api',
    turn_count              INT         NOT NULL DEFAULT 0,
    total_prompt_tokens     INT         NOT NULL DEFAULT 0,
    total_completion_tokens INT         NOT NULL DEFAULT 0,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_conversations_account  ON conversations(account_id, updated_at DESC);
CREATE INDEX idx_conversations_api_key  ON conversations(api_key_id, updated_at DESC);
CREATE INDEX idx_conversations_updated  ON conversations(updated_at DESC);
CREATE INDEX idx_conversations_source   ON conversations(source);

-- ── Inference Jobs ────────────────────────────────────────────────────────────

CREATE TABLE inference_jobs (
    id                   UUID        PRIMARY KEY DEFAULT uuidv7(),
    prompt               TEXT        NOT NULL DEFAULT '',
    prompt_preview       TEXT,
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
    provider_id          UUID        REFERENCES llm_providers(id) ON DELETE SET NULL,
    api_format           TEXT        NOT NULL DEFAULT 'openai_compat',
    request_path         TEXT,
    conversation_id      UUID        REFERENCES conversations(id) ON DELETE SET NULL,
    tool_calls_json      JSONB,
    messages_json        JSONB,
    queue_time_ms        INT,
    cancelled_at         TIMESTAMPTZ,
    messages_hash        TEXT,
    messages_prefix_hash TEXT,
    failure_reason       TEXT,
    result_preview       TEXT,
    has_tool_calls       BOOLEAN     NOT NULL DEFAULT false,
    image_keys           TEXT[],
    mcp_loop_id          UUID
);

CREATE INDEX idx_inference_jobs_status     ON inference_jobs(status);
CREATE INDEX idx_inference_jobs_created_at ON inference_jobs(created_at DESC);
CREATE INDEX idx_inference_jobs_source     ON inference_jobs(source);
CREATE INDEX idx_inference_jobs_api_key_id ON inference_jobs(api_key_id, created_at DESC);
CREATE INDEX idx_inference_jobs_account_id ON inference_jobs(account_id, created_at DESC);

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
-- max_ctx column included (migration 000005)

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
    max_ctx           INT      NOT NULL DEFAULT 0,
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
-- compression/multiturn/vision/handoff columns included (migrations 000003-000004)

CREATE TABLE lab_settings (
    id                              INT     PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    gemini_function_calling         BOOLEAN NOT NULL DEFAULT false,
    max_images_per_request          INTEGER NOT NULL DEFAULT 4,
    max_image_b64_bytes             INTEGER NOT NULL DEFAULT 2097152,
    -- context compression (000003)
    context_compression_enabled     BOOLEAN NOT NULL DEFAULT false,
    compression_model               TEXT,
    context_budget_ratio            REAL    NOT NULL DEFAULT 0.60,
    compression_trigger_turns       INT     NOT NULL DEFAULT 1,
    recent_verbatim_window          INT     NOT NULL DEFAULT 1,
    compression_timeout_secs        INT     NOT NULL DEFAULT 10,
    -- multi-turn gate (000003)
    multiturn_min_params            INT     NOT NULL DEFAULT 7,
    multiturn_min_ctx               INT     NOT NULL DEFAULT 16384,
    multiturn_allowed_models        TEXT[]  NOT NULL DEFAULT '{}',
    -- vision (000003)
    vision_model                    TEXT,
    -- session handoff (000003)
    handoff_enabled                 BOOLEAN NOT NULL DEFAULT true,
    -- handoff threshold (000004)
    handoff_threshold               REAL    NOT NULL DEFAULT 0.85,
    updated_at                      TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO lab_settings (id) VALUES (1) ON CONFLICT DO NOTHING;

-- ── MCP Settings ──────────────────────────────────────────────────────────────

CREATE TABLE mcp_settings (
    id                        INT          PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    routing_cache_ttl_secs    INTEGER      NOT NULL DEFAULT 300,
    tool_schema_refresh_secs  INTEGER      NOT NULL DEFAULT 3600,
    embedding_model           VARCHAR(128) NOT NULL DEFAULT 'nomic-embed-text',
    max_tools_per_request     INTEGER      NOT NULL DEFAULT 20 CHECK (max_tools_per_request BETWEEN 1 AND 200),
    max_routing_cache_entries INTEGER      NOT NULL DEFAULT 1000,
    updated_at                TIMESTAMPTZ  NOT NULL DEFAULT now()
);

INSERT INTO mcp_settings (id) VALUES (1) ON CONFLICT DO NOTHING;

-- ── Provider VRAM Budget ──────────────────────────────────────────────────────

CREATE TABLE provider_vram_budget (
    provider_id       UUID        PRIMARY KEY REFERENCES llm_providers(id) ON DELETE CASCADE,
    safety_permil     INT         NOT NULL DEFAULT 100,
    vram_total_source TEXT        NOT NULL DEFAULT 'probe',
    kv_cache_type     TEXT        NOT NULL DEFAULT 'q8_0',
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Roles ─────────────────────────────────────────────────────────────────────

CREATE TABLE roles (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name        VARCHAR(64) NOT NULL UNIQUE,
    permissions TEXT[]      NOT NULL DEFAULT '{}',
    menus       TEXT[]      NOT NULL DEFAULT '{}',
    is_system   BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO roles (name, permissions, menus, is_system) VALUES (
    'super',
    ARRAY['dashboard_view','api_test','provider_manage','key_manage','account_manage','audit_view','settings_manage','role_manage','model_manage'],
    ARRAY['dashboard','flow','jobs','performance','usage','test','providers','servers','keys','accounts','audit','api_docs'],
    TRUE
);

INSERT INTO roles (name, permissions, menus, is_system) VALUES (
    'viewer',
    ARRAY['dashboard_view'],
    ARRAY['dashboard','flow','jobs','performance','usage','api_docs'],
    TRUE
);

-- ── Account Roles ─────────────────────────────────────────────────────────────

CREATE TABLE account_roles (
    account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role_id    UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, role_id)
);

CREATE INDEX idx_account_roles_role ON account_roles(role_id);

-- ── Global Model Settings ─────────────────────────────────────────────────────

CREATE TABLE global_model_settings (
    model_name  TEXT        PRIMARY KEY,
    is_enabled  BOOLEAN     NOT NULL DEFAULT true,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── API Key Provider Access ───────────────────────────────────────────────────

CREATE TABLE api_key_provider_access (
    api_key_id   UUID    NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    provider_id  UUID    NOT NULL REFERENCES llm_providers(id) ON DELETE CASCADE,
    is_allowed   BOOLEAN NOT NULL DEFAULT true,
    PRIMARY KEY (api_key_id, provider_id)
);

CREATE INDEX idx_api_key_provider_access_key ON api_key_provider_access(api_key_id) WHERE is_allowed = true;

-- ── MCP Servers ───────────────────────────────────────────────────────────────

CREATE TABLE mcp_servers (
    id            UUID         PRIMARY KEY DEFAULT uuidv7(),
    name          VARCHAR(128) NOT NULL UNIQUE,
    slug          VARCHAR(64)  NOT NULL UNIQUE
                  CHECK (slug ~ '^[a-z][a-z0-9_]*$'),
    url           TEXT         NOT NULL,
    is_enabled    BOOLEAN      NOT NULL DEFAULT true,
    timeout_secs  SMALLINT     NOT NULL DEFAULT 30
                  CHECK (timeout_secs BETWEEN 1 AND 300),
    metadata      JSONB        NOT NULL DEFAULT '{}',
    tool_count    SMALLINT     NOT NULL DEFAULT 0,
    tools_summary JSONB        NOT NULL DEFAULT '[]',
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX idx_mcp_servers_enabled ON mcp_servers(is_enabled) WHERE is_enabled = true;

-- ── MCP Server Tools ──────────────────────────────────────────────────────────

CREATE TABLE mcp_server_tools (
    server_id       UUID        NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name       TEXT        NOT NULL,
    namespaced_name TEXT        NOT NULL,
    description     TEXT,
    input_schema    JSONB       NOT NULL DEFAULT '{}',
    annotations     JSONB       NOT NULL DEFAULT '{}',
    discovered_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (server_id, tool_name)
);

CREATE INDEX idx_mcp_server_tools_namespaced ON mcp_server_tools(namespaced_name);

-- ── MCP Key Access ────────────────────────────────────────────────────────────

CREATE TABLE mcp_key_access (
    api_key_id  UUID        NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    server_id   UUID        NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    is_allowed  BOOLEAN     NOT NULL DEFAULT true,
    granted_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    top_k       SMALLINT    CHECK (top_k BETWEEN 1 AND 64),
    PRIMARY KEY (api_key_id, server_id)
);

CREATE INDEX idx_mcp_key_access_key ON mcp_key_access(api_key_id) WHERE is_allowed = true;

-- ── MCP Loop Tool Calls ───────────────────────────────────────────────────────

CREATE TABLE mcp_loop_tool_calls (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    mcp_loop_id     UUID        NOT NULL,
    job_id          UUID        NOT NULL REFERENCES inference_jobs(id) ON DELETE CASCADE,
    loop_round      SMALLINT    NOT NULL,
    server_id       UUID        NOT NULL,
    tool_name       TEXT        NOT NULL,
    namespaced_name TEXT        NOT NULL,
    args_json       JSONB       NOT NULL,
    result_text     TEXT,
    outcome         TEXT        NOT NULL,
    cache_hit       BOOLEAN     NOT NULL DEFAULT false,
    latency_ms      INT,
    result_bytes    INT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_mcp_loop_tool_calls_loop   ON mcp_loop_tool_calls(mcp_loop_id);
CREATE INDEX idx_mcp_loop_tool_calls_job    ON mcp_loop_tool_calls(job_id);
CREATE INDEX idx_mcp_loop_tool_calls_server ON mcp_loop_tool_calls(server_id, created_at DESC);

-- ── Trigram indexes ───────────────────────────────────────────────────────────

CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX idx_ollama_models_name_trgm        ON ollama_models USING GIN (model_name gin_trgm_ops);
CREATE INDEX idx_llm_providers_name_trgm        ON llm_providers USING GIN (name gin_trgm_ops);
CREATE INDEX idx_llm_providers_url_trgm         ON llm_providers USING GIN (url gin_trgm_ops);
CREATE INDEX idx_accounts_name_trgm             ON accounts USING GIN (name gin_trgm_ops);
CREATE INDEX idx_accounts_username_trgm         ON accounts USING GIN (username gin_trgm_ops);
CREATE INDEX idx_api_keys_name_trgm             ON api_keys USING GIN (name gin_trgm_ops);
CREATE INDEX idx_gpu_servers_name_trgm          ON gpu_servers USING GIN (name gin_trgm_ops);
CREATE INDEX idx_provider_selected_models_lookup ON provider_selected_models (provider_id, model_name);
