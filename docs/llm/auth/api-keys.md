# API Keys — Server-Side: Auth & Rate Limiting

> SSOT | **Last Updated**: 2026-03-22

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new API key field | `migrations/` + `domain/entities/api_key.rs` + `persistence/api_key_repository.rs` + `key_handlers.rs` `KeySummary` |
| Change auth rejection logic | `persistence/api_key_repository.rs` → `get_by_hash()` WHERE clause |
| Add new rate limit type (e.g. requests/day) | `middleware/rate_limiter.rs` → add new Valkey check before handler |
| Change RPM window duration | `middleware/rate_limiter.rs` → `RPM_WINDOW_MS` constant + `RATE_LIMIT_SCRIPT` Lua body |
| Change bootstrap key behavior | `main.rs` → bootstrap key creation block (planned, not yet implemented) |
| Add field to CreateKeyRequest | `key_handlers.rs` → `CreateKeyRequest` struct + `create_key()` handler |
| Auto-create test key for new account | `account_handlers.rs` → `create_account()` | key_type="test" |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/domain/entities/api_key.rs` | `ApiKey` entity |
| `crates/veronex/src/application/ports/outbound/api_key_repository.rs` | `ApiKeyRepository` trait |
| `crates/veronex/src/infrastructure/outbound/persistence/api_key_repository.rs` | `PostgresApiKeyRepository` impl |
| `crates/veronex/src/infrastructure/inbound/http/key_handlers.rs` | CRUD handlers |
| `crates/veronex/src/infrastructure/inbound/http/middleware/rate_limiter.rs` | RPM/TPM middleware |
| `crates/veronex/src/main.rs` | Bootstrap key creation on startup (planned) |

---

## Entity

```rust
// domain/entities/api_key.rs
pub struct ApiKey {
    pub id: Uuid,
    pub key_hash: String,                    // BLAKE2b-256, #[serde(skip_serializing)] #[ts(skip)]
    pub key_prefix: String,                  // First 12 chars for display
    pub tenant_id: String,
    pub name: String,
    pub is_active: bool,
    pub rate_limit_rpm: i32,                 // 0 = unlimited
    pub rate_limit_tpm: i32,                 // 0 = unlimited
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,   // #[ts(skip)] — internal only
    pub key_type: KeyType,                   // #[ts(skip)] — internal only (Standard | Test)
    pub tier: KeyTier,                       // Free | Paid — domain enum (migration 000038)
    pub account_id: Option<Uuid>,            // FK → accounts(id), set on create
}
```

`account_id` tracks who created the key. Super admin list view batch-resolves `account_id` → username for the `created_by` display field.

### Key Type vs Tier

| Field | Values | Visibility | Purpose |
|-------|--------|-----------|---------|
| `key_type` | `"standard"` \| `"test"` | **Internal only** — never surfaced to API/frontend | Distinguishes account test keys from production keys; test keys excluded from `GET /v1/keys` |
| `tier` | `"free"` \| `"paid"` | Exposed via `KeySummary` | Billing tier for future usage exclusion. Default: `"paid"` |

Default `tier`: `"paid"` (DB DEFAULT + `fn default_tier() -> String` in Rust).

## DB Schema

```sql
CREATE TABLE api_keys (
    id              UUID         PRIMARY KEY,
    key_hash        TEXT         NOT NULL UNIQUE, -- BLAKE2b-256
    key_prefix      VARCHAR(12)  NOT NULL,
    tenant_id       VARCHAR(255) NOT NULL DEFAULT 'default',
    name            VARCHAR(255) NOT NULL,
    is_active       BOOLEAN      NOT NULL DEFAULT true,
    rate_limit_rpm  INTEGER      NOT NULL DEFAULT 0,
    rate_limit_tpm  INTEGER      NOT NULL DEFAULT 0,
    key_type        TEXT         NOT NULL DEFAULT 'standard', -- migration 000033
    tier            TEXT         NOT NULL DEFAULT 'paid',     -- migration 000038
    account_id      UUID REFERENCES accounts(id),         -- migration 000035, tracks creator
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ  -- migration 000021
);

-- No unique index on name. Names are labels; uniqueness is provided by `id` (UUIDv7).
-- (migration 000032 added uq_api_keys_tenant_name; migration 000040 dropped it)
-- (planned) One test key per account: uq_api_keys_account_test ON (account_id) WHERE is_test_key=true AND deleted_at IS NULL
```

- migrations: 000001 CREATE, 000021 deleted_at, 000033 key_type column, 000035 account_id, 000038 tier column, 000040 drop name unique index

---

## API Endpoints

```
POST   /v1/keys              CreateKeyRequest → CreateKeyResponse (plaintext shown once)
                              Names are non-unique labels; unique id = UUIDv7
GET    /v1/keys?search=&page=1&limit=50 → { keys: Vec<KeySummary>, total: N, page: 1, limit: 50 } (excludes soft-deleted; excludes key_type = 'test')
DELETE /v1/keys/{id}         → 204 (soft-delete: sets deleted_at = NOW())
PATCH  /v1/keys/{id}         PatchKeyRequest { is_active?, tier? } → 204
POST   /v1/keys/{id}/regenerate → CreateKeyResponse (new hash + prefix, same id)
```

`GET /v1/keys` filters out test keys (`key_type = 'test'`) server-side. Scope: **super admin** sees all keys (`list_all()`); non-super users see only their own tenant's keys (`list_by_tenant(username)`). Pagination: ?search= (ILIKE on name), ?page=N, ?limit=N (default 50, max 1000).

### Request / Response Structs

```rust
// key_handlers.rs
pub struct CreateKeyRequest {
    pub tenant_id: String,
    pub name: String,
    #[serde(default)]
    pub rate_limit_rpm: i32,         // 0 = unlimited
    #[serde(default)]
    pub rate_limit_tpm: i32,
    #[serde(default = "default_tier")]
    pub tier: String,                // "free" | "paid" — default "paid"
    pub expires_at: Option<DateTime<Utc>>,
}
// Note: key_type is NOT accepted from client — always "standard" for user-created keys

pub struct CreateKeyResponse {
    pub id: Uuid,
    pub key: String,                         // Full plaintext — shown ONCE
    pub key_prefix: String,                  // "iq_01ARZ3NDEK…"
    pub tenant_id: String,
    pub created_at: DateTime<Utc>,
}

pub struct PatchKeyRequest {
    pub is_active: Option<bool>,
    pub tier: Option<String>,                // "free" | "paid"
}

pub struct KeySummary {
    pub id: Uuid,
    pub key_prefix: String,
    pub tenant_id: String,
    pub name: String,
    pub is_active: bool,
    pub rate_limit_rpm: i32,
    pub rate_limit_tpm: i32,
    pub tier: String,                        // "free" | "paid"
    pub created_by: Option<String>,          // resolved from account_id → username (super admin only)
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
```

### Key Regeneration

`POST /v1/keys/{id}/regenerate` generates a new BLAKE2b-256 hash and prefix for an existing key. The key ID is preserved so historical usage data remains linked. The old key is invalidated immediately. The new plaintext is returned once (same as `CreateKeyResponse`).

RBAC: super admin can regenerate any key; non-super can only regenerate their own tenant's keys.

```rust
// key_handlers.rs — regenerate_key()
let (_new_id, plaintext, new_hash, new_prefix) = generate_api_key();
state.api_key_repo.regenerate(&uuid, &new_hash, &new_prefix).await?;
// Returns CreateKeyResponse with new plaintext
```

### DashboardStats — key counts (test keys excluded)

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

> **Tenant scope**: Dashboard query is explicitly scoped to `tenant_id = 'default'` to match what `GET /v1/keys` returns (`list_by_tenant("default")`). Without this filter, keys created with any other tenant ID (e.g. from tests or the example payload) inflate the count.

Job count queries also exclude test-source jobs:
```sql
-- total jobs / 24h jobs / status breakdown:
WHERE source != 'test'
```

---

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

DB: `api_key_provider_access (api_key_id UUID FK, provider_id UUID FK, is_allowed BOOL, PK(api_key_id, provider_id))` — migration 000010.

| Endpoint | Auth | Body | Response |
|----------|------|------|----------|
| `GET /v1/keys/{key_id}/providers` | `RequireSettingsManage` | — | `Vec<{ provider_id, provider_name, is_allowed }>` |
| `PATCH /v1/keys/{key_id}/providers/{provider_id}` | `RequireSettingsManage` | `{ is_allowed: bool }` | 200 |

Handler: `key_provider_access_handlers.rs`

---

## Web UI

→ See `docs/llm/frontend/pages/keys.md`
