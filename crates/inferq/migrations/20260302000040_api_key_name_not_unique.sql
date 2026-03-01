-- API key names are labels only — uniqueness is provided by the UUIDv7 primary key.
-- Drop the per-tenant name uniqueness constraint so the same name can be reused.
DROP INDEX IF EXISTS uq_api_keys_tenant_name;
