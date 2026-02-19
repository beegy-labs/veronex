# Task 10: API Key & Usage Tracking

> Rate limiting infrastructure included; limits set to unlimited (0 = no limit) initially.
> Stack: PostgreSQL (keys) + ClickHouse (usage analytics) + Valkey (rate limit counters)

## Steps

### Phase 1 — Domain Model

- [ ] `ApiKey` entity:

```python
@dataclass
class ApiKey:
    id: UUID          # UUIDv7 (PK)
    key_hash: str     # BLAKE2b-256 hex — never store plaintext
    key_prefix: str   # first 12 chars for display: "iq_01ARZ3N..."
    tenant_id: str
    name: str         # human label
    is_active: bool
    rate_limit_rpm: int   # requests per minute; 0 = unlimited
    rate_limit_tpm: int   # tokens per minute;   0 = unlimited
    expires_at: datetime | None
    created_at: datetime
```

- [ ] `ApiKeyCreated` value object (returned once at creation — plaintext):

```python
@dataclass
class ApiKeyCreated:
    id: UUID
    key: str          # "iq_<base62(uuidv7_bytes)>" — shown once, never stored
    key_prefix: str
    tenant_id: str
    created_at: datetime
```

### Phase 2 — Key Generation

- [ ] `src/domain/services/api_key_generator.py`:

```python
import hashlib
import uuid6           # pip install uuid6 (Python 3.13; stdlib in 3.14)
import base62          # pip install python-base62

PREFIX = "iq_"

def generate_api_key() -> tuple[str, str, str]:
    """
    Returns (key_id_str, plaintext_key, key_hash).
    key_id:       UUIDv7 string (stored as PK)
    plaintext:    "iq_<base62>" — returned to caller once, never stored
    key_hash:     BLAKE2b-256 hex — stored in DB for validation
    """
    raw: uuid6.UUID = uuid6.uuid7()
    encoded = base62.encodebytes(raw.bytes)          # 22 chars
    plaintext = f"{PREFIX}{encoded}"                 # "iq_..." ~25 chars
    key_hash = hashlib.blake2b(
        plaintext.encode(), digest_size=32
    ).hexdigest()                                    # 64 hex chars
    key_prefix = plaintext[:12]                      # "iq_01ARZ3N..."
    return str(raw), plaintext, key_hash, key_prefix
```

### Phase 3 — IApiKeyRepository Port

- [ ] `application/ports/outbound/api_key_repository.py`:

```python
class IApiKeyRepository(Protocol):
    async def create(self, key: ApiKey) -> None: ...
    async def get_by_hash(self, key_hash: str) -> ApiKey | None: ...
    async def list_by_tenant(self, tenant_id: str) -> list[ApiKey]: ...
    async def revoke(self, key_id: UUID) -> None: ...
```

### Phase 4 — PostgreSQL Schema

- [ ] `api_keys` table migration:

```sql
CREATE TABLE api_keys (
    id          UUID PRIMARY KEY,          -- UUIDv7
    key_hash    VARCHAR(64) NOT NULL UNIQUE,
    key_prefix  VARCHAR(16) NOT NULL,
    tenant_id   VARCHAR(128) NOT NULL,
    name        VARCHAR(255) NOT NULL,
    is_active   BOOLEAN NOT NULL DEFAULT TRUE,
    rate_limit_rpm  INTEGER NOT NULL DEFAULT 0,  -- 0 = unlimited
    rate_limit_tpm  INTEGER NOT NULL DEFAULT 0,  -- 0 = unlimited
    expires_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ix_api_keys_tenant ON api_keys(tenant_id);
CREATE INDEX ix_api_keys_hash   ON api_keys(key_hash);
```

### Phase 5 — Auth Middleware (FastAPI)

- [ ] `src/infrastructure/inbound/http/middleware/api_key_auth.py`:

```python
from fastapi import Request, HTTPException, status
import hashlib

EXCLUDED_PATHS = {"/health", "/metrics", "/readyz", "/docs", "/openapi.json"}

async def api_key_middleware(request: Request, call_next):
    if request.url.path in EXCLUDED_PATHS:
        return await call_next(request)

    raw_key = request.headers.get("X-API-Key", "")
    if not raw_key:
        raise HTTPException(status.HTTP_401_UNAUTHORIZED, "Missing X-API-Key")

    key_hash = hashlib.blake2b(raw_key.encode(), digest_size=32).hexdigest()
    api_key = await request.app.state.api_key_repo.get_by_hash(key_hash)

    if not api_key or not api_key.is_active:
        raise HTTPException(status.HTTP_401_UNAUTHORIZED, "Invalid API key")
    if api_key.expires_at and api_key.expires_at < datetime.now(timezone.utc):
        raise HTTPException(status.HTTP_401_UNAUTHORIZED, "API key expired")

    # Inject into request state for downstream use
    request.state.api_key = api_key
    request.state.tenant_id = api_key.tenant_id

    return await call_next(request)
```

### Phase 6 — Rate Limiting (Valkey, initially unlimited)

- [ ] `src/infrastructure/outbound/queue/rate_limiter.py` (sliding window):

```python
class RateLimiter:
    """
    Sliding window counter via Valkey.
    Skips check when limit == 0 (unlimited).
    """
    async def check_rpm(self, key_id: str, limit_rpm: int) -> bool:
        if limit_rpm == 0:
            return True   # unlimited
        # sliding window: ZREMRANGEBYSCORE + ZADD + ZCARD in pipeline
        ...

    async def check_tpm(self, key_id: str, limit_tpm: int, tokens: int) -> bool:
        if limit_tpm == 0:
            return True   # unlimited
        ...
```

### Phase 7 — Usage Tracking (ClickHouse)

- [ ] Add columns to `inference_logs` ClickHouse table:

```sql
ALTER TABLE inference_logs
    ADD COLUMN api_key_id   UUID,
    ADD COLUMN finish_reason LowCardinality(String) DEFAULT '',
    -- finish_reason values: "stop" | "length" | "cancelled" (disconnect) | "error"
    ADD COLUMN error_msg    String DEFAULT '';
```

- [ ] Usage materialized view (includes failed/cancelled):

```sql
CREATE TABLE api_key_usage_hourly (
    hour              DateTime,
    api_key_id        UUID,
    tenant_id         LowCardinality(String),
    request_count     UInt64,
    success_count     UInt64,
    cancelled_count   UInt64,   -- SSE disconnect mid-stream
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

- [ ] SSE disconnect → `finish_reason = "cancelled"` 기록
  - Task 06 `stream_inference()`의 `request.is_disconnected()` 감지 시 observability adapter에 `cancelled` 이벤트 전달

### Phase 8 — Admin & Usage Endpoints

- [ ] Key management:
  - `POST   /v1/keys`             → create key (returns plaintext once)
  - `GET    /v1/keys`             → list keys (prefix only, never hash)
  - `DELETE /v1/keys/{id}`        → revoke key

- [ ] Usage query:
  - `GET /v1/usage`               → 전체 집계 (요청수, 토큰수, 성공/실패/취소 분류)
  - `GET /v1/usage/{key_id}`      → key별 시간대 breakdown
  - `GET /v1/usage/{key_id}/jobs` → 개별 요청 목록 (finish_reason 포함)

**응답 예시 `GET /v1/usage/{key_id}`:**
```json
{
  "key_prefix": "iq_01ARZ3N...",
  "period": "2026-02-19T00:00Z / 2026-02-20T00:00Z",
  "summary": {
    "request_count": 142,
    "success_count": 138,
    "cancelled_count": 3,
    "error_count": 1,
    "prompt_tokens": 58420,
    "completion_tokens": 21304,
    "total_tokens": 79724
  }
}
```

## Done

- [ ] `generate_api_key()` returns `"iq_<base62(uuidv7)>"` ~25 chars
- [ ] Plaintext key shown exactly once; only BLAKE2b hash stored in DB
- [ ] `X-API-Key` middleware validates on every request (except health/metrics)
- [ ] Rate limiter infrastructure ready; all limits default to 0 (unlimited)
- [ ] Usage: request_count + token count (prompt + completion) per key
- [ ] `finish_reason`: stop / length / cancelled (disconnect) / error 구분
- [ ] SSE disconnect 시 `cancelled` 로 기록되어 사용량 조회에 포함
- [ ] ClickHouse `api_key_usage_hourly` MV로 빠른 집계 쿼리
