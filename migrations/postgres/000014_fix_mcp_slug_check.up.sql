-- Tighten the mcp_servers slug constraint to match application-layer validation:
-- slug must start with a lowercase ASCII letter (not a digit or underscore).
-- The application already enforces ^[a-z][a-z0-9_]* — this aligns the DB check.

ALTER TABLE mcp_servers DROP CONSTRAINT IF EXISTS mcp_servers_slug_check;
ALTER TABLE mcp_servers ADD CONSTRAINT mcp_servers_slug_check
    CHECK (slug ~ '^[a-z][a-z0-9_]*$');
