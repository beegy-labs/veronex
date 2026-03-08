# Auth -- Implementation Details

> SSOT | **Last Updated**: 2026-03-08 | Parent: [jwt-sessions.md](jwt-sessions.md)

## Test Run Endpoint Details

### POST /v1/test/completions

```
Auth: Authorization: Bearer <access_token> (any role)
Request:  { "model": "llama3.2", "messages": [...], "provider_type": "ollama" }
Response: SSE stream (OpenAI chunk format)
```

- No API key, no rate limiting
- `api_key_id=NULL`, `account_id=claims.sub`, `source=Test`
- Job placed in `veronex:queue:jobs:test` (low-priority; API queue polled first)
- First SSE chunk `id` = `job_id` for reconnect

### GET /v1/test/jobs/{job_id}/stream

- Auth: Bearer token (any role)
- Completed jobs: replays `result_text` as single chunk + `[DONE]`
- In-progress jobs: attaches to live token stream
- Same OpenAI SSE format as `/v1/jobs/{job_id}/stream`

### inference_jobs -- account_id column

```sql
ALTER TABLE inference_jobs ADD COLUMN account_id UUID REFERENCES accounts(id);
```

Migration: `20260301000037_job_account_id.sql`

| Column | API Key Job | Test Run |
|--------|------------|----------|
| `api_key_id` | key.id | NULL |
| `account_id` | NULL | claims.sub |
| `source` | `Api` | `Test` |

Dashboard jobs list (`GET /v1/dashboard/jobs`) JOINs `accounts` to return `account_name` for test jobs.

## Account Endpoint Details

### POST /v1/accounts -- Create Account

```
Request:  { "username", "password", "name", "email?", "role": "admin", "department?", "position?" }
Response: { "id", "username", "role", "test_api_key": "iq_...", "created_at" }
```

Test API key auto-created: `key_type="test"`, `tenant_id=username`, `name="{username}-test"`.

### PATCH /v1/accounts/{id}/active

```
Request:  { "is_active": true|false }
Response: 204
```

### POST /v1/accounts/{id}/reset-link

```
Response: { "token": "uuid-v4-string" }
```

### api_keys Changes (Migration 035)

`account_id` is now implemented in the `ApiKey` entity. It tracks which account created the key. Super admin list view batch-resolves `account_id` → username for the `created_by` display field. Test keys use `key_type = "test"` (not `is_test_key`).

```sql
ALTER TABLE api_keys ADD COLUMN account_id UUID REFERENCES accounts(id);
```

See [api-keys.md](api-keys.md) for entity definition and regenerate endpoint.

## Audit Trail

### AuditPort

```rust
pub struct AuditEvent {
  pub event_time: DateTime<Utc>,
  pub account_id: Uuid,
  pub account_name: String,
  pub action: String,        // create|update|delete|regenerate|login|logout|reset_password|sync|trigger
  pub resource_type: String, // api_key|ollama_provider|gemini_provider|account|gpu_server|session|lab_settings|capacity_settings
  pub resource_id: String,
  pub resource_name: String,
  pub ip_address: Option<String>,
  pub details: Option<String>, // ALWAYS Some(...) -- human-readable description
}
```

Rule: every `emit_audit()` call MUST pass a descriptive `details` string.

Implemented by `HttpAuditAdapter` -- forwards to veronex-analytics. Fail-open: HTTP error logs `warn!`, request continues.

### Covered Actions

| Handler group | Actions | resource_type |
|---------------|---------|---------------|
| `auth_handlers` | login, logout, reset_password, create (setup) | `account` |
| `account_handlers` | create, update, delete, reset_password | `account` |
| `account_handlers` (sessions) | delete | `session` |
| `key_handlers` | create, delete, update | `api_key` |
| `provider_handlers` | create, delete, update | `ollama_provider` / `gemini_provider` |
| `gpu_server_handlers` | create, update, delete | `gpu_server` |
| `gemini_model_handlers` | update, sync | `gemini_provider` |
| `gemini_policy_handlers` | update | `gemini_provider` |
| `dashboard_handlers` | update, trigger | `capacity_settings`, `lab_settings` |

### Audit Pipeline

```
*_handlers.rs -> AuditPort::record() -> HttpAuditAdapter
  -> POST /internal/ingest/audit (veronex-analytics)
  -> OTel LogRecord (event.name="audit.action")
  -> OTLP gRPC -> OTel Collector -> Redpanda [otel-logs]
  -> kafka_otel_logs_mv (ClickHouse MV) -> otel_logs (MergeTree)
```

### audit_events ClickHouse Table

```sql
CREATE TABLE audit_events (
  event_time    DateTime64(3),
  account_id    UUID,
  account_name  LowCardinality(String),
  action        LowCardinality(String),
  resource_type LowCardinality(String),
  resource_id   String,
  resource_name String,
  ip_address    String,
  details       String
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(event_time)
ORDER BY (event_time, resource_type, resource_id)
TTL toDate(event_time) + INTERVAL 1 YEAR;
```

### Audit Query Endpoint

`GET /v1/audit` -- RequireSuper. Delegates to `analytics_repo.audit_events(filters)` -> veronex-analytics -> ClickHouse `otel_logs WHERE LogAttributes['event.name']='audit.action'`.

Query params: `limit` (default 100, max 1000), `offset` (default 0), `action`, `resource_type`.

## Web Frontend

### Token Storage (Cookies)

| Cookie | Content | Expiry |
|--------|---------|--------|
| `veronex_access_token` | JWT access token | 1h |
| `veronex_refresh_token` | Raw refresh token | 7 days (rolling) |
| `veronex_username` | Display name | 7 days |
| `veronex_role` | `super` / `admin` | 7 days |
| `veronex_account_id` | Account UUID | 7 days |

All cookies: `SameSite=Strict`.

### Auth Helpers (`web/lib/auth.ts`)

| Function | Purpose |
|----------|---------|
| `getAccessToken()` / `getRefreshToken()` | Read from cookies |
| `setTokens(resp)` | Save access+refresh tokens (7-day cookie) |
| `setAccessToken(token)` | Update access token only (after refresh) |
| `clearTokens()` | Clear all auth cookies (logout/forced redirect) |
| `getAuthUser()` | `{ username, role, accountId }` from cookies |
| `isLoggedIn()` | True if access token cookie present |

### Auth Guard (`web/lib/auth-guard.ts`)

- Module-level `refreshMutex` -- concurrent 401s share same refresh promise
- `tryRefresh()` -- attempts token refresh; clears cookies + redirects on failure
- `redirectToLogin()` -- checks `PUBLIC_PATHS = ['/login', '/setup']` before redirecting

### API Client (`web/lib/api-client.ts`)

Singleton `apiClient` for all JWT-protected routes:
- Attaches `Authorization: Bearer <access_token>` automatically
- On 401: `POST /v1/auth/refresh` -> update access token -> retry once
- If refresh fails: `clearTokens()` + redirect to `/login`

### Nav Auth State (`web/components/nav.tsx`)

`useEffect` reads `getAuthUser()` on mount. Shows username + Logout button if logged in. `role === "super"` shows Audit link.

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add auth endpoint | `auth_handlers.rs` | Handler + `router.rs` public block |
| Add account field | new migration + `account.rs` + `account_repository.rs` | Trait + impl |
| Add auditable action | handler file | Call `emit_audit()` with action + `details` string |
| Change JWT expiry | `auth_handlers.rs` `issue_access_token()` | Update `exp` |
| Change refresh TTL | `auth_handlers.rs` login handler | `EXPIRE` call duration |
| Add audit filter | `audit_handlers.rs` | Query param + `WHERE` clause |
