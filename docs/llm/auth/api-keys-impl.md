# API Keys: Auth Flow & Impl

> SSOT | **Last Updated**: 2026-03-24 | Classification: Operational
> Authentication flow, rate limiting, soft-delete, audit, and web UI for API keys.

## Authentication Flow

Middleware accepts three header formats:

1. `X-API-Key: <key>`
2. `Authorization: Bearer <key>` (OpenAI SDK compatible)
3. `x-goog-api-key: <key>` (Gemini CLI compatible)

```
1. Extract key from headers (X-API-Key → Authorization: Bearer → x-goog-api-key)
2. BLAKE2b-256 hash → lookup WHERE key_hash = ? AND deleted_at IS NULL
3. Reject if: not found | is_active = false | expires_at < now()
4. Pass ApiKey entity to handler via extensions (id stored in job record as api_key_id)
```

Note: `deleted_at` filtering happens at the SQL query level (`WHERE deleted_at IS NULL`), not in middleware logic.

**Refresh token hashing**: Uses domain-separated BLAKE2b with `veronex-refresh-token-v1:` prefix to prevent cross-protocol hash collisions.

**Bootstrap key** — **Status: Planned** — `BOOTSTRAP_API_KEY` env var support is not yet implemented in `main.rs`. The Helm chart defines the env var but the Rust code does not read or use it.

---

## Rate Limiting (middleware/rate_limiter.rs)

```
RPM: Sorted set  veronex:ratelimit:rpm:{key_id}
     ZADD now() uuid; ZCOUNT window=60s → count ≥ rpm_limit → 429
     Valkey TTL = 62s (2s buffer for clock skew)

TPM: Counter     veronex:ratelimit:tpm:{key_id}:{minute}
     INCR; EXPIRE 120s → count + estimated_tokens ≥ tpm_limit → 429
     TPM incremented AFTER job completes (actual completion_tokens)
```

Fail-closed: Valkey error → returns 503 Service Unavailable, job rejected.

---

## Soft-Delete Behavior

- `DELETE /v1/keys/{id}` → sets `deleted_at = NOW()`, row kept
- Hidden from `GET /v1/keys` (WHERE deleted_at IS NULL)
- Rejected at auth check
- Historical jobs preserved: `inference_jobs.api_key_id` → NULL on hard delete (FK ON DELETE SET NULL)
- ClickHouse `inference_logs.api_key_id` is String (UUID), preserved after soft-delete

### Cascade Delete

When an account is soft-deleted, all associated API keys are automatically soft-deleted via `soft_delete_by_tenant(tenant_id)`.

---

## Audit Trail

All key operations emit audit events to ClickHouse via OTel:

| Action | resource_type | When |
|--------|---------------|------|
| `create` | `api_key` | Key created |
| `update` | `api_key` | is_active or tier changed |
| `delete` | `api_key` | Key soft-deleted |
| `regenerate` | `api_key` | Key regenerated (new hash) |

Per-key history: `GET /v1/audit?resource_type=api_key&resource_id={key_id}` returns all audit events for a specific key. The web UI shows a History button per key row.

---

## API Key Provider Access

Per-key provider allow/deny control. When no rows exist for a key, all providers are accessible (default allow-all). When rows exist, only providers with `is_allowed = true` are routable for that key.

DB: `api_key_provider_access (api_key_id UUID FK, provider_id UUID FK, is_allowed BOOL, PK(api_key_id, provider_id))` — `docker/postgres/init.sql`.

| Endpoint | Auth | Body | Response |
|----------|------|------|----------|
| `GET /v1/keys/{key_id}/providers` | `RequireSettingsManage` | — | `Vec<{ provider_id, provider_name, is_allowed }>` |
| `PATCH /v1/keys/{key_id}/providers/{provider_id}` | `RequireSettingsManage` | `{ is_allowed: bool }` | 200 |

Handler: `key_provider_access_handlers.rs`

---

## Key Regeneration

`POST /v1/keys/{id}/regenerate` generates a new BLAKE2b-256 hash and prefix for an existing key. The key ID is preserved so historical usage data remains linked. The old key is invalidated immediately. The new plaintext is returned once (same as `CreateKeyResponse`).

RBAC: super admin can regenerate any key; non-super can only regenerate their own tenant's keys.

```rust
// key_handlers.rs — regenerate_key()
let (_new_id, plaintext, new_hash, new_prefix) = generate_api_key();
state.api_key_repo.regenerate(&uuid, &new_hash, &new_prefix).await?;
// Returns CreateKeyResponse with new plaintext
```

---

## DashboardStats — key counts (test keys excluded)

```rust
// dashboard_handlers.rs
pub struct DashboardStats {
    pub total_keys: i64,   // key_type != 'test' AND deleted_at IS NULL AND tenant_id = 'default'
    pub active_keys: i64,  // key_type = 'standard' AND is_active AND deleted_at IS NULL AND tenant_id = 'default'
    // ...
}
```

SQL:
```sql
COUNT(*) FILTER (WHERE deleted_at IS NULL AND key_type != 'test' AND tenant_id = 'default')              AS total_keys,
COUNT(*) FILTER (WHERE is_active = true AND deleted_at IS NULL AND key_type = 'standard' AND tenant_id = 'default') AS active_keys
```

Tenant scope: scoped to `tenant_id = 'default'` to match `GET /v1/keys` (`list_by_tenant("default")`). Without this filter, keys with other tenant IDs inflate the count.

Job count queries also exclude test-source jobs:
```sql
WHERE source != 'test'
```

---

## Web UI

→ See `docs/llm/frontend/pages/keys.md`
