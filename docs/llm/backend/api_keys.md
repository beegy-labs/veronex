# API Keys — Backend: Auth & Rate Limiting

> SSOT | **Last Updated**: 2026-03-02 (rev: RPM rate limiter uses atomic Lua eval — 1 RTT instead of 4)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new API key field | `migrations/` + `domain/entities/api_key.rs` + `persistence/api_key_repository.rs` + `key_handlers.rs` `KeySummary` |
| Change auth rejection logic | `persistence/api_key_repository.rs` → `get_by_hash()` WHERE clause |
| Add new rate limit type (e.g. requests/day) | `middleware/rate_limiter.rs` → add new Valkey check before handler |
| Change RPM window duration | `middleware/rate_limiter.rs` → `RPM_WINDOW_MS` constant + `RATE_LIMIT_SCRIPT` Lua body |
| Change bootstrap key behavior | `main.rs` → bootstrap key creation block |
| Add field to CreateKeyRequest | `key_handlers.rs` → `CreateKeyRequest` struct + `create_key()` handler |
| Auto-create test key for new account | `account_handlers.rs` → `create_account()` | key_type="test", is_test_key=true |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/domain/entities/api_key.rs` | `ApiKey` entity |
| `crates/inferq/src/application/ports/outbound/api_key_repository.rs` | `ApiKeyRepository` trait |
| `crates/inferq/src/infrastructure/outbound/persistence/api_key_repository.rs` | `PostgresApiKeyRepository` impl |
| `crates/inferq/src/infrastructure/inbound/http/key_handlers.rs` | CRUD handlers |
| `crates/inferq/src/infrastructure/inbound/http/middleware/rate_limiter.rs` | RPM/TPM middleware |
| `crates/inferq/src/main.rs` | Bootstrap key creation on startup |

---

## Entity

```rust
// domain/entities/api_key.rs
pub struct ApiKey {
    pub id: Uuid,
    pub key_hash: String,                    // BLAKE2b-256, never stored plaintext
    pub key_prefix: String,                  // First 12 chars for display
    pub tenant_id: String,
    pub name: String,
    pub is_active: bool,
    pub rate_limit_rpm: i32,                 // 0 = unlimited
    pub rate_limit_tpm: i32,                 // 0 = unlimited
    pub key_type: String,                    // "standard" | "test" — internal only (migration 000033)
    pub tier: String,                        // "free" | "paid" — billing tier (migration 000038)
    pub account_id: Option<Uuid>,            // FK → accounts (migration 000035; NULL for pre-auth keys)
    pub is_test_key: bool,                   // true = auto-created per-account test key (migration 000035)
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,   // NULL = active, NOT NULL = soft-deleted
}
```

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
    account_id      UUID REFERENCES accounts(id),            -- migration 000035 (NULL for pre-auth keys)
    is_test_key     BOOLEAN      NOT NULL DEFAULT false,      -- migration 000035 (true = per-account test key)
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ  -- migration 000021
);

-- No unique index on name. Names are labels; uniqueness is provided by `id` (UUIDv7).
-- (migration 000032 added uq_api_keys_tenant_name; migration 000040 dropped it)
-- One test key per account: uq_api_keys_account_test ON (account_id) WHERE is_test_key=true AND deleted_at IS NULL
```

- migrations: 000001 CREATE, 000021 deleted_at, 000033 key_type column, 000035 account_id + is_test_key, 000038 tier column, 000040 drop name unique index

---

## API Endpoints

```
POST   /v1/keys        CreateKeyRequest → CreateKeyResponse (plaintext shown once)
                       Names are non-unique labels; unique id = UUIDv7
GET    /v1/keys        → Vec<KeySummary> (excludes soft-deleted; excludes key_type = 'test')
DELETE /v1/keys/{id}   → 204 (soft-delete: sets deleted_at = NOW())
PATCH  /v1/keys/{id}   ToggleKeyRequest { is_active: bool } → 204
```

`GET /v1/keys` filters out test keys (`key_type = 'test'`) server-side — these are account-owned internal keys and are never listed.

### Request / Response Structs

```rust
// key_handlers.rs
pub struct CreateKeyRequest {
    pub tenant_id: String,           // default: "default"
    pub name: String,
    pub rate_limit_rpm: Option<i32>, // 0 = unlimited
    pub rate_limit_tpm: Option<i32>,
    #[serde(default = "default_tier")]
    pub tier: String,                // "free" | "paid" — default "paid"
    pub expires_at: Option<DateTime<Utc>>,
}
// Note: key_type is NOT accepted from client — always "standard" for user-created keys

pub struct CreateKeyResponse {
    pub id: Uuid,
    pub key: String,        // Full plaintext — shown ONCE
    pub key_prefix: String, // "vnx_abc123de…"
    pub tenant_id: String,
    pub created_at: String,
}

pub struct KeySummary {
    pub id: String,
    pub key_prefix: String,
    pub tenant_id: String,
    pub name: String,
    pub is_active: bool,
    pub rate_limit_rpm: i32,
    pub rate_limit_tpm: i32,
    pub tier: String,               // "free" | "paid"
    pub expires_at: Option<String>,
    pub created_at: String,
}
```

### DashboardStats — key counts (test keys excluded)

```rust
// dashboard_handlers.rs
pub struct DashboardStats {
    pub total_keys: i64,   // key_type != 'test' AND deleted_at IS NULL
    pub active_keys: i64,  // key_type != 'test' AND is_active AND deleted_at IS NULL
    // ...
}
```

SQL:
```sql
COUNT(*) FILTER (WHERE deleted_at IS NULL AND key_type != 'test')              AS total_keys,
COUNT(*) FILTER (WHERE is_active AND deleted_at IS NULL AND key_type != 'test') AS active_keys
```

Job count queries also exclude test-source jobs:
```sql
-- total jobs / 24h jobs / status breakdown:
WHERE source != 'test'
```

---

## Authentication Flow

Every protected endpoint reads `X-API-Key` header:

```
1. Extract header value
2. BLAKE2b-256 hash → lookup WHERE key_hash = ?
3. Reject if: not found | deleted_at IS NOT NULL | is_active = false | expires_at < now()
4. Pass ApiKey to handler (id stored in job record as api_key_id)
```

**Bootstrap key**: `BOOTSTRAP_API_KEY` env var (default: `veronex-bootstrap-admin-key`) —
auto-created at startup if not found. No rate limits. `tier = "paid"`, `key_type = "standard"`.

---

## Rate Limiting (middleware/rate_limiter.rs)

```
RPM: Sorted set  veronex:ratelimit:rpm:{key_id}:{minute}
     ZADD now() uuid; ZCOUNT window=60s → count ≥ rpm_limit → 429

TPM: Counter     veronex:ratelimit:tpm:{key_id}:{minute}
     INCR; EXPIRE 120s → count + estimated_tokens ≥ tpm_limit → 429
     TPM incremented AFTER job completes (actual completion_tokens)
```

Fail-open: Valkey error → skip rate limit check, job proceeds.

---

## Soft-Delete Behavior

- `DELETE /v1/keys/{id}` → sets `deleted_at = NOW()`, row kept
- Hidden from `GET /v1/keys` (WHERE deleted_at IS NULL)
- Rejected at auth check
- Historical jobs preserved: `inference_jobs.api_key_id` → NULL on hard delete (FK ON DELETE SET NULL)
- ClickHouse `inference_logs.api_key_id` is String (UUID), preserved after soft-delete

---

## Web UI

→ See `docs/llm/frontend/web-keys.md`
