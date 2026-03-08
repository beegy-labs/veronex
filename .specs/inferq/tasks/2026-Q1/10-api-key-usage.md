# Task 10: API Key & Usage Tracking

> Rate limiting infrastructure included; limits set to unlimited (0 = no limit) initially.
> Stack: PostgreSQL (keys) + ClickHouse (usage analytics) + Valkey (rate limit counters)

## Steps

### Phase 1 — Domain Model

- [x] `ApiKey` entity:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: Uuid,                   // UUIDv7 (PK)
    pub key_hash: String,           // BLAKE2b-256 hex — never store plaintext
    pub key_prefix: String,         // first 12 chars: "iq_01ARZ3N..."
    pub tenant_id: String,
    pub name: String,
    pub is_active: bool,
    pub rate_limit_rpm: i32,        // 0 = unlimited
    pub rate_limit_tpm: i32,        // 0 = unlimited
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
```

- [x] `ApiKeyCreated` value object (returned once at creation — plaintext):

```rust
pub struct ApiKeyCreated {
    pub id: Uuid,
    pub key: String,          // "iq_<base62(uuidv7_bytes)>" — shown once, never stored
    pub key_prefix: String,
    pub tenant_id: String,
    pub created_at: DateTime<Utc>,
}
```

### Phase 2 — Key Generation

- [x] `src/domain/services/api_key_generator.rs`:

```rust
use blake2::{Blake2b256, Digest};
use uuid::Uuid;

const PREFIX: &str = "iq_";

pub fn generate_api_key() -> (Uuid, String, String, String) {
    let id = Uuid::now_v7();
    let encoded = base62::encode(id.as_bytes());           // ~22 chars
    let plaintext = format!("{PREFIX}{encoded}");           // "iq_..." ~25 chars

    let mut hasher = Blake2b256::new();
    hasher.update(plaintext.as_bytes());
    let key_hash = hex::encode(hasher.finalize());          // 64 hex chars

    let key_prefix = plaintext[..12].to_string();

    (id, plaintext, key_hash, key_prefix)
}
```

### Phase 3 — ApiKeyRepository Port

- [x] `application/ports/outbound/api_key_repository.rs`:

```rust
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn create(&self, key: &ApiKey) -> Result<()>;
    async fn get_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>>;
    async fn list_by_tenant(&self, tenant_id: &str) -> Result<Vec<ApiKey>>;
    async fn revoke(&self, key_id: &Uuid) -> Result<()>;
}
```

### Phase 4 — PostgreSQL Schema

- [x] `api_keys` table migration:

```sql
CREATE TABLE api_keys (
    id          UUID PRIMARY KEY,
    key_hash    VARCHAR(64) NOT NULL UNIQUE,
    key_prefix  VARCHAR(16) NOT NULL,
    tenant_id   VARCHAR(128) NOT NULL,
    name        VARCHAR(255) NOT NULL,
    is_active   BOOLEAN NOT NULL DEFAULT TRUE,
    rate_limit_rpm  INTEGER NOT NULL DEFAULT 0,
    rate_limit_tpm  INTEGER NOT NULL DEFAULT 0,
    expires_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ix_api_keys_tenant ON api_keys(tenant_id);
CREATE INDEX ix_api_keys_hash   ON api_keys(key_hash);
```

### Phase 5 — Auth Middleware (Axum Tower layer)

- [x] `src/infrastructure/inbound/http/middleware/api_key_auth.rs`:

```rust
const EXCLUDED_PATHS: &[&str] = &["/health", "/readyz"];

pub async fn api_key_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    if EXCLUDED_PATHS.iter().any(|p| path.starts_with(p)) {
        return Ok(next.run(req).await);
    }

    let raw_key = req.headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let mut hasher = Blake2b256::new();
    hasher.update(raw_key.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    let api_key = state.api_key_repo
        .get_by_hash(&key_hash)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !api_key.is_active {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if let Some(expires) = api_key.expires_at {
        if expires < Utc::now() {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    req.extensions_mut().insert(api_key);
    Ok(next.run(req).await)
}
```

### Phase 6 — Rate Limiting (Valkey, initially unlimited)

- [x] Sliding window via Valkey (ZREMRANGEBYSCORE + ZADD + ZCARD pipeline)
- [x] Skip check when limit == 0 (unlimited)

### Phase 7 — Usage Tracking (ClickHouse)

> `inference_logs` already includes `api_key_id`, `tenant_id`, `finish_reason` columns
> (defined in Task 08 unified schema). No ALTER TABLE needed.

- [x] Usage materialized view:

```sql
CREATE TABLE api_key_usage_hourly (
    hour              DateTime,
    api_key_id        UUID,
    tenant_id         LowCardinality(String),
    request_count     UInt64,
    success_count     UInt64,
    cancelled_count   UInt64,
    error_count       UInt64,
    prompt_tokens     UInt64,
    completion_tokens UInt64,
    total_tokens      UInt64
) ENGINE = AggregatingMergeTree()
PARTITION BY toYYYYMM(hour)
ORDER BY (api_key_id, hour);

CREATE MATERIALIZED VIEW api_key_usage_hourly_mv
TO api_key_usage_hourly AS
SELECT
    toStartOfHour(event_time)           AS hour,
    api_key_id,
    tenant_id,
    count()                              AS request_count,
    countIf(finish_reason = 'stop')      AS success_count,
    countIf(finish_reason = 'cancelled') AS cancelled_count,
    countIf(finish_reason = 'error')     AS error_count,
    sum(prompt_tokens)                   AS prompt_tokens,
    sum(completion_tokens)               AS completion_tokens,
    sum(prompt_tokens + completion_tokens) AS total_tokens
FROM inference_logs
GROUP BY hour, api_key_id, tenant_id;
```

### Phase 8 — Admin & Usage Endpoints

- [x] Key management:
  - `POST   /v1/keys`             → create key (returns plaintext once)
  - `GET    /v1/keys`             → list keys (prefix only, never hash)
  - `DELETE /v1/keys/{id}`        → revoke key

- [x] Usage query:
  - `GET /v1/usage`               → aggregate (requests, tokens, success/fail/cancel)
  - `GET /v1/usage/{key_id}`      → per-key hourly breakdown
  - `GET /v1/usage/{key_id}/jobs` → individual request list (finish_reason included)

## Done

- [x] `generate_api_key()` returns `"iq_<base62(uuidv7)>"` ~25 chars
- [x] Plaintext key shown exactly once; only BLAKE2b hash stored in DB
- [x] `X-API-Key` Tower middleware validates on every request (except health)
- [x] Rate limiter infrastructure ready; all limits default to 0 (unlimited)
- [x] Usage: request_count + token count (prompt + completion) per key
- [x] `finish_reason`: stop / length / cancelled (disconnect) / error
- [x] ClickHouse `api_key_usage_hourly` MV for fast aggregate queries
