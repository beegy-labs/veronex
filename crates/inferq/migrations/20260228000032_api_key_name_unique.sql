-- Enforce unique API key names per tenant (soft-delete aware).
-- Uses a partial index so deleted keys do not block name reuse.
CREATE UNIQUE INDEX IF NOT EXISTS uq_api_keys_tenant_name
  ON api_keys (tenant_id, lower(name))
  WHERE deleted_at IS NULL;
