# API Keys — Server-Side: Auth & Rate Limiting

> SSOT | **Last Updated**: 2026-03-28

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
| `crates/veronex/src/infrastructure/outbound/persistence/caching_api_key_repo.rs` | `CachingApiKeyRepo` — TtlCache 60s wrapper (hot-path) |
| `crates/veronex/src/infrastructure/inbound/http/key_handlers.rs` | CRUD handlers |
| `crates/veronex/src/infrastructure/inbound/http/middleware/rate_limiter.rs` | RPM/TPM middleware |
| `crates/veronex/src/main.rs` | Bootstrap key creation on startup (planned) |

`api_key_repo` in `AppState` is wired as `CachingApiKeyRepo(PostgresApiKeyRepository)`.
`get_by_hash()` (hot path) hits in-memory cache; all writes call `invalidate_all()`.
→ See `infra/hot-path-caching.md` for full caching strategy.

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
    key_hash        VARCHAR(64)  NOT NULL UNIQUE, -- BLAKE2b-256
    key_prefix      VARCHAR(16)  NOT NULL,
    tenant_id       VARCHAR(128) NOT NULL,
    is_test_key     BOOLEAN      NOT NULL DEFAULT false,
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
    pub key_prefix: String,                  // "vnx_01ARZ3NDEK…"
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

