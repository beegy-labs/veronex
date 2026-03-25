ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS mcp_servers_slug_check;
ALTER TABLE mcp_servers ADD CONSTRAINT mcp_servers_slug_check
    CHECK (slug ~ '^[a-z0-9_]+$');
