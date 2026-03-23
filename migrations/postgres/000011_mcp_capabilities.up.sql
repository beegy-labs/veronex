-- MCP server registry and tool capability tracking.
-- Stores MCP server configurations that Veronex connects to as a client.

CREATE TABLE IF NOT EXISTS mcp_servers (
    id           UUID        PRIMARY KEY DEFAULT uuidv7(),
    name         VARCHAR(128) NOT NULL UNIQUE,
    -- Slug is used as the namespace prefix for tool names: mcp_{slug}_{tool}
    slug         VARCHAR(64)  NOT NULL UNIQUE
                 CHECK (slug ~ '^[a-z0-9_]+$'),
    url          TEXT         NOT NULL,
    is_enabled   BOOLEAN      NOT NULL DEFAULT true,
    -- Maximum timeout (seconds) for a single tool call to this server.
    timeout_secs SMALLINT     NOT NULL DEFAULT 30
                 CHECK (timeout_secs BETWEEN 1 AND 300),
    -- Arbitrary metadata (description, tags, contact, etc.).
    metadata     JSONB        NOT NULL DEFAULT '{}',
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled
    ON mcp_servers (is_enabled)
    WHERE is_enabled = true;

-- Snapshot of tool definitions discovered via tools/list on last connect.
-- This is a cache / audit log — the live state lives in Valkey.
CREATE TABLE IF NOT EXISTS mcp_server_tools (
    server_id    UUID         NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name    TEXT         NOT NULL,
    -- namespaced_name = "mcp_{server_slug}_{tool_name}"
    namespaced_name TEXT      NOT NULL,
    description  TEXT,
    input_schema JSONB        NOT NULL DEFAULT '{}',
    annotations  JSONB        NOT NULL DEFAULT '{}',
    discovered_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (server_id, tool_name)
);

CREATE INDEX IF NOT EXISTS idx_mcp_server_tools_namespaced
    ON mcp_server_tools (namespaced_name);

-- Per-API-key MCP server access control.
-- When no rows exist for a key, MCP is disabled for that key (default deny).
-- Insert a row with is_allowed = true to grant access.
CREATE TABLE IF NOT EXISTS mcp_key_access (
    api_key_id  UUID    NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    server_id   UUID    NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    is_allowed  BOOLEAN NOT NULL DEFAULT true,
    granted_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (api_key_id, server_id)
);

CREATE INDEX IF NOT EXISTS idx_mcp_key_access_key
    ON mcp_key_access (api_key_id)
    WHERE is_allowed = true;
