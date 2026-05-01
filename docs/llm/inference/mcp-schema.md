# MCP — DB Schema

> SSOT | **Last Updated**: 2026-03-28
> MCP integration overview, run_loop, protections: `inference/mcp.md`

Migration: `docker/postgres/init.sql` (consolidated init)

```sql
-- MCP server registry
CREATE TABLE mcp_servers (
    id           UUID         PRIMARY KEY DEFAULT uuidv7(),
    name         VARCHAR(128) NOT NULL UNIQUE,
    slug         VARCHAR(64)  NOT NULL UNIQUE CHECK (slug ~ '^[a-z][a-z0-9_]*$'),
    url          TEXT         NOT NULL,
    is_enabled   BOOLEAN      NOT NULL DEFAULT true,
    timeout_secs SMALLINT     NOT NULL DEFAULT 30 CHECK (timeout_secs BETWEEN 1 AND 300),
    metadata      JSONB        NOT NULL DEFAULT '{}',
    tool_count    SMALLINT     NOT NULL DEFAULT 0,
    tools_summary JSONB        NOT NULL DEFAULT '[]',
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- Tool capability snapshot (cache from tools/list)
CREATE TABLE mcp_server_tools (
    server_id       UUID  NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name       TEXT  NOT NULL,
    namespaced_name TEXT  NOT NULL,  -- "mcp_{slug}_{tool_name}"
    description     TEXT,
    input_schema    JSONB NOT NULL DEFAULT '{}',
    annotations     JSONB NOT NULL DEFAULT '{}',
    discovered_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (server_id, tool_name)
);

-- Per-API-key access control (default deny; insert row to grant)
CREATE TABLE mcp_key_access (
    api_key_id UUID    NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    server_id  UUID    NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    is_allowed BOOLEAN     NOT NULL DEFAULT true,
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    top_k      SMALLINT    CHECK (top_k BETWEEN 1 AND 64),
    PRIMARY KEY (api_key_id, server_id)
);

-- Audit log for every tool call in an agentic loop.
-- job_id uses ON DELETE SET NULL because the bridge deletes intermediate
-- per-round inference_jobs after the loop completes (only the head job is
-- kept). CASCADE would wipe rounds 1..N-1's audit rows.
CREATE TABLE mcp_loop_tool_calls (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    mcp_loop_id     UUID        NOT NULL,
    job_id          UUID        REFERENCES inference_jobs(id) ON DELETE SET NULL,
    loop_round      SMALLINT    NOT NULL,
    server_id       UUID        NOT NULL,
    tool_name       TEXT        NOT NULL,
    namespaced_name TEXT        NOT NULL,
    args_json       JSONB       NOT NULL,
    result_text     TEXT,
    outcome         TEXT        NOT NULL,  -- success|error|timeout|cache_hit|circuit_open
    cache_hit       BOOLEAN     NOT NULL DEFAULT false,
    latency_ms      INT,
    result_bytes    INT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Global MCP settings singleton (id always = 1)
CREATE TABLE mcp_settings (
    id                        INT          PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    routing_cache_ttl_secs    INTEGER      NOT NULL DEFAULT 300,
    tool_schema_refresh_secs  INTEGER      NOT NULL DEFAULT 3600,
    embedding_model           VARCHAR(128) NOT NULL DEFAULT 'nomic-embed-text',
    max_tools_per_request     INTEGER      NOT NULL DEFAULT 20 CHECK (max_tools_per_request BETWEEN 1 AND 200),
    max_routing_cache_entries INTEGER      NOT NULL DEFAULT 1000,
    updated_at                TIMESTAMPTZ  NOT NULL DEFAULT now()
);
```

`GET /v1/mcp/settings` / `PATCH /v1/mcp/settings` — handler: `mcp_handlers::get_mcp_settings`, `patch_mcp_settings`.
