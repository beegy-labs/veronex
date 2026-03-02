# Auth — RBAC, JWT & Audit Trail

> SSOT | **Last Updated**: 2026-03-02 (rev 4 — audit details always populated; token storage updated to cookies)

## Overview

Veronex has two independent auth layers:

| Layer | Mechanism | Protects |
|-------|-----------|---------|
| **API Key** | `X-API-Key` header (BLAKE2b hash) | `/v1/chat/*`, `/v1/inference/*`, dashboard/key/backend routes |
| **JWT Bearer** | `Authorization: Bearer <token>` (HS256) | `/v1/accounts/*`, `/v1/audit`, `/v1/test/*` |
| **Public** | None | `/v1/auth/*`, `/health`, `/readyz`, `/docs/*` |

Existing API-key routes are **not changed** — only the new JWT routes are added.

---

## Roles

| Role | Description |
|------|-------------|
| `super` | Can create/manage accounts, view audit log |
| `admin` | Normal admin — cannot manage accounts or view audit |

Role is stored in `accounts.role` (`CHECK (role IN ('super', 'admin'))`).

---

## Router Layers (4-layer)

```
Public         /v1/auth/*                    no middleware
API Key Auth   existing routes (unchanged)   api_key_auth + rate_limiter
JWT Auth       /v1/accounts/*, /v1/audit/*   jwt_auth middleware → RequireSuper extractor
JWT Auth       /v1/test/*                    jwt_auth middleware (no rate limit, no RequireSuper)
```

### JWT Middleware (`jwt_auth`)
- Extracts `Authorization: Bearer <token>`
- Decodes via `jsonwebtoken::decode<Claims>(token, HS256, secret)`
- Checks Valkey `veronex:revoked:{jti}` — returns **401** if revoked (O(1) blocklist)
- Calls `session_repo.update_last_used(&jti)` via `tokio::spawn` (non-blocking)
- Inserts `Claims { sub: Uuid, role: String, jti: Uuid, exp: usize }` into request extensions
- Returns **401** on missing/invalid/expired/revoked token

### RequireSuper Extractor
- `FromRequestParts` — reads `Claims` from extensions
- Returns **403** if `claims.role != "super"`
- Usage: `RequireSuper(claims): RequireSuper` as handler arg

---

## Accounts Table

```sql
CREATE TABLE accounts (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    username      VARCHAR(64) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,          -- Argon2id
    name          VARCHAR(128) NOT NULL,
    email         VARCHAR(255),
    role          VARCHAR(16)  NOT NULL DEFAULT 'admin'
                  CHECK (role IN ('super', 'admin')),
    department    VARCHAR(128),
    position      VARCHAR(128),
    is_active     BOOLEAN NOT NULL DEFAULT true,
    created_by    UUID REFERENCES accounts(id),
    last_login_at TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at    TIMESTAMPTZ                     -- soft-delete
);
CREATE INDEX idx_accounts_username ON accounts(username) WHERE deleted_at IS NULL;
```

Migration: `000034_accounts.sql`

---

## First-Run Setup Flow

On fresh install (no accounts in DB), the first super admin account is created via:

```
GET  /v1/setup/status  → { "needs_setup": true | false }
POST /v1/setup         → { access_token, refresh_token, ... }  (same as login response)
```

- Both endpoints have **no auth** — they are accessible before any account exists
- `POST /v1/setup` returns **409 Conflict** if any account already exists (idempotent guard)
- Frontend `AppShell` calls `GET /v1/setup/status` on every load:
  - `needs_setup: true` → redirect to `/setup` (from any page)
  - `/setup` page when `needs_setup: false` → redirect to `/login`

**Optional CI bootstrap** (pre-seed without UI flow):
```bash
BOOTSTRAP_SUPER_USER=admin    # If set, create super account on first boot
BOOTSTRAP_SUPER_PASS=secret   # Both must be set; omit for setup-flow-only
```
When both env vars are set, account is created idempotently on startup.
When omitted (default for docker-compose), setup flow is the only path.

## Password Hashing

- **Algorithm**: Argon2id (`argon2 = "0.5"` crate)
- **Hash format**: PHC string (`$argon2id$v=19$m=...$salt$hash`)
- API keys still use BLAKE2b-256 (unchanged)

---

## JWT

| Property | Value |
|----------|-------|
| Algorithm | HS256 |
| `sub` | `account.id` (UUID) |
| `role` | `"super"` \| `"admin"` |
| `jti` | `Uuid::now_v7()` — unique per session, used for revocation |
| `exp` | now + 1 hour |
| Secret | `JWT_SECRET` env var (default: `"change-me-in-production"`) |

Access token issued at login, valid 1h. Frontend calls `POST /v1/auth/refresh` with refresh token.

---

## Sessions (`account_sessions` table)

Refresh tokens and session state stored in **PostgreSQL**, not Valkey:

```sql
CREATE TABLE account_sessions (
    id                 UUID        PRIMARY KEY DEFAULT uuidv7(),
    account_id         UUID        NOT NULL REFERENCES accounts(id),
    jti                UUID        NOT NULL UNIQUE,    -- matches JWT jti claim
    refresh_token_hash VARCHAR(64),                    -- BLAKE2b-256 of raw refresh token
    ip_address         VARCHAR(45),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at       TIMESTAMPTZ,
    expires_at         TIMESTAMPTZ NOT NULL,           -- matches JWT exp
    revoked_at         TIMESTAMPTZ
);
```

Migration: `20260301000036_account_sessions.sql`

**Revocation flow**:
1. Login → `jti = Uuid::now_v7()`, INSERT into `account_sessions`
2. Logout → `session_repo.revoke(&session.id)` + Valkey `SET veronex:revoked:{jti} 1 EX {remaining_ttl}`
3. jwt_auth → `EXISTS veronex:revoked:{jti}` in Valkey → **401** if found (O(1) check)
4. Refresh (rolling) → revoke old session + jti → INSERT new session + new jti → new access token

**Valkey revocation blocklist** (TTL-bounded):
```
veronex:revoked:{jti}  →  "1"  TTL = remaining lifetime of old access token
```
Entries expire automatically — no cleanup needed.

---

## Password Reset

One-time token stored in Valkey:

```
veronex:pwreset:{raw_token}  →  account_id (String)  TTL 24 hours
```

- Super creates link: `POST /v1/accounts/{id}/reset-link` → returns `{ token }`
- Super shares token out-of-band (email, Slack, etc.)
- User resets: `POST /v1/auth/reset-password { token, new_password }` — token deleted immediately (1-use)

---

## Auth Endpoints

### `GET /v1/setup/status`
```json
Response: { "needs_setup": true | false }
```
No auth. Returns `true` when no accounts exist. Frontend polls this on every load.

### `POST /v1/setup`
```json
Request:  { "username": "string", "password": "string" }
Response: { "access_token": "...", "token_type": "Bearer",
            "account_id": "uuid", "username": "...", "role": "super",
            "refresh_token": "..." }
```
No auth. Creates the first super account + issues session. Returns **409** if any account exists.
Returns **422** if username is blank or password is shorter than 8 characters.

### `POST /v1/auth/login`
```json
Request:  { "username": "string", "password": "string" }
Response: { "access_token": "...", "token_type": "Bearer",
            "account_id": "uuid", "username": "...", "role": "super|admin",
            "refresh_token": "raw-uuid-v4-string" }
```
- Verifies Argon2id hash
- Updates `last_login_at`
- Generates `jti = Uuid::now_v7()`, issues access token (jti in claims)
- Generates raw refresh token, stores BLAKE2b-256 hash in `account_sessions`
- Returns **401** if account not found, wrong password, or `is_active=false`

### `POST /v1/auth/logout`
```json
Request:  { "refresh_token": "string" }
Response: 204 No Content
```
Looks up session by `refresh_token_hash` → sets `revoked_at` in DB + adds `jti` to Valkey blocklist with remaining TTL.

### `POST /v1/auth/refresh`
```json
Request:  { "refresh_token": "string" }
Response: { "access_token": "...", "token_type": "Bearer" }
```
Rolling rotation: revokes old session (DB + Valkey blocklist) → creates new session with new `jti` → issues new access token.
Returns **401** if token not found, session revoked, or account inactive.

### `POST /v1/auth/reset-password`
```json
Request:  { "token": "string", "new_password": "string" }
Response: 204 No Content
```
Validates Valkey token, deletes it, saves new Argon2id hash. Returns **401** if token invalid/expired.

---

## Test Run Endpoints (JWT Bearer — any logged-in account)

These endpoints allow logged-in accounts to run inference without an API key.
Jobs submitted via these endpoints are tracked by `account_id` (not `api_key_id`).
They are excluded from API usage/performance metrics (source = `Test`).

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/test/completions` | Submit test inference — OpenAI SSE (no API key, no rate limit) |
| `GET` | `/v1/test/jobs/{job_id}/stream` | SSE reconnect for test jobs |
| `POST` | `/v1/test/api/chat` | Ollama NDJSON stream (no API key, no rate limit) |
| `POST` | `/v1/test/api/generate` | Ollama NDJSON stream (no API key, no rate limit) |
| `POST` | `/v1/test/v1beta/models/{*path}` | Gemini SSE (no API key, no rate limit) |

All test routes: `api_key_id=NULL`, `account_id=claims.sub`, `source=Test`.

### POST /v1/test/completions

```json
Request: {
  "model": "llama3.2",
  "messages": [{ "role": "user", "content": "hello" }],
  "backend": "ollama"
}
Response: SSE stream (OpenAI chunk format)
```

- Auth: `Authorization: Bearer <access_token>` (any role)
- No API key, no rate limiting
- `api_key_id = NULL`, `account_id = claims.sub`, `source = Test`
- Job placed in `veronex:queue:jobs:test` (low-priority queue; API queue polled first)
- Returns first SSE chunk with `id` = `job_id` for reconnect

### GET /v1/test/jobs/{job_id}/stream

- Auth: `Authorization: Bearer <access_token>`
- For completed jobs: replays `result_text` as single chunk + `[DONE]`
- For in-progress jobs: attaches to live token stream
- Same OpenAI SSE format as `/v1/jobs/{job_id}/stream`

### inference_jobs — account_id column

```sql
ALTER TABLE inference_jobs
    ADD COLUMN account_id UUID REFERENCES accounts(id);
```

Migration: `20260301000037_job_account_id.sql`

| Column | API Key Test | Test Run |
|--------|-------------|---------|
| `api_key_id` | key.id | NULL |
| `account_id` | NULL | claims.sub |
| `source` | `Api` | `Test` |

Dashboard jobs list (`GET /v1/dashboard/jobs`) JOINs `accounts` table to return `account_name` for test jobs.

---

## Account Endpoints (RequireSuper)

All require `Authorization: Bearer <super-token>`.

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/v1/accounts` | List all active accounts |
| `POST` | `/v1/accounts` | Create account + auto-generate test API key |
| `PATCH` | `/v1/accounts/{id}` | Update name/email/department/position |
| `DELETE` | `/v1/accounts/{id}` | Soft-delete (`deleted_at = now()`) |
| `PATCH` | `/v1/accounts/{id}/active` | Toggle `is_active` |
| `POST` | `/v1/accounts/{id}/reset-link` | Generate 24h password-reset token |
| `GET` | `/v1/accounts/{id}/sessions` | List active sessions for account |
| `DELETE` | `/v1/accounts/{id}/sessions` | Revoke all sessions for account |
| `DELETE` | `/v1/sessions/{session_id}` | Revoke a specific session |

### POST /v1/accounts — Create Account
```json
Request: {
  "username": "string",
  "password": "string",
  "name": "string",
  "email": "string?",
  "role": "admin",          // default "admin"
  "department": "string?",
  "position": "string?"
}
Response: {
  "id": "uuid",
  "username": "string",
  "role": "string",
  "test_api_key": "iq_...",  // plaintext, shown ONCE
  "created_at": "iso8601"
}
```

Test API key auto-created: `key_type="test"`, `tenant_id=username`, `name="{username}-test"`.

### PATCH /v1/accounts/{id}/active
```json
Request: { "is_active": true | false }
Response: 204
```

### POST /v1/accounts/{id}/reset-link
```json
Response: { "token": "uuid-v4-string" }
```

---

## api_keys Changes (Migration 035)

```sql
ALTER TABLE api_keys ADD COLUMN account_id UUID REFERENCES accounts(id);
ALTER TABLE api_keys ADD COLUMN is_test_key BOOLEAN NOT NULL DEFAULT false;
CREATE UNIQUE INDEX uq_api_keys_account_test
    ON api_keys (account_id) WHERE is_test_key = true AND deleted_at IS NULL;
```

One test key per account. `account_id` is `NULL` for keys created before this migration.

---

## Audit Trail

### AuditPort

```rust
pub struct AuditEvent {
    pub event_time: DateTime<Utc>,
    pub account_id: Uuid,
    pub account_name: String,
    pub action: String,        // "create"|"update"|"delete"|"login"|"logout"|"reset_password"|"sync"|"trigger"
    pub resource_type: String, // "api_key"|"ollama_backend"|"gemini_backend"|"account"|"gpu_server"|"session"|"lab_settings"|"capacity_settings"
    pub resource_id: String,
    pub resource_name: String,
    pub ip_address: Option<String>,
    pub details: Option<String>, // ALWAYS Some(…) — human-readable description of the action
}
```

**Rule**: every `emit_audit()` call site MUST pass a descriptive `details` string so the
log entry is self-explanatory when read without additional context.

Implemented by `HttpAuditAdapter` — forwards to veronex-analytics.

### Covered Actions

| Handler | Action | resource_type |
|---------|--------|---------------|
| `auth_handlers::login` | `login` | `account` |
| `auth_handlers::logout` | `logout` | `account` |
| `auth_handlers::reset_password` | `reset_password` | `account` |
| `auth_handlers::setup` | `create` | `account` |
| `account_handlers::create_account` | `create` | `account` |
| `account_handlers::update_account` | `update` | `account` |
| `account_handlers::delete_account` | `delete` | `account` |
| `account_handlers::set_account_active` | `update` | `account` |
| `account_handlers::create_reset_link` | `reset_password` | `account` |
| `account_handlers::revoke_session` | `delete` | `session` |
| `account_handlers::revoke_all_account_sessions` | `delete` | `session` |
| `key_handlers::create_key` | `create` | `api_key` |
| `key_handlers::delete_key` | `delete` | `api_key` |
| `key_handlers::toggle_key` | `update` | `api_key` |
| `backend_handlers::register_backend` | `create` | `ollama_backend` / `gemini_backend` |
| `backend_handlers::delete_backend` | `delete` | `ollama_backend` |
| `backend_handlers::update_backend` | `update` | `ollama_backend` / `gemini_backend` |
| `gpu_server_handlers::register_gpu_server` | `create` | `gpu_server` |
| `gpu_server_handlers::update_gpu_server` | `update` | `gpu_server` |
| `gpu_server_handlers::delete_gpu_server` | `delete` | `gpu_server` |
| `gemini_model_handlers::set_sync_config` | `update` | `gemini_backend` |
| `gemini_model_handlers::sync_models` | `sync` | `gemini_backend` |
| `gemini_policy_handlers::upsert_gemini_policy` | `update` | `gemini_backend` |
| `dashboard_handlers::patch_capacity_settings` | `update` | `capacity_settings` |
| `dashboard_handlers::trigger_capacity_sync` | `trigger` | `capacity_settings` |
| `dashboard_handlers::patch_lab_settings` | `update` | `lab_settings` |

### Details Format

Each call site passes a specific human-readable string so audit logs are self-explanatory:

| Handler | Details string (template) |
|---------|--------------------------|
| `auth_handlers::login` | `"User '{username}' logged in successfully"` |
| `auth_handlers::logout` | `"Session terminated: refresh token revoked and JWT blocklisted"` |
| `auth_handlers::reset_password` | `"Password changed via one-time reset token"` |
| `auth_handlers::setup` | `"First-run setup: super admin account '{username}' created"` |
| `account_handlers::create_account` | `"Account '{username}' (role: {role}) created with auto-generated test API key"` |
| `account_handlers::update_account` | `"Account '{username}' ({id}) profile updated (name/email/department/position)"` |
| `account_handlers::delete_account` | `"Account '{username}' ({id}) soft-deleted (login disabled, data retained)"` |
| `account_handlers::set_account_active` | `"Account {id} is_active set to {bool} (login enabled/disabled)"` |
| `account_handlers::create_reset_link` | `"Password reset link generated for account '{username}' ({id}); token valid 24h"` |
| `account_handlers::revoke_session` | `"Session {session_id} manually revoked by admin"` |
| `account_handlers::revoke_all_account_sessions` | `"All active sessions for account {id} force-revoked by admin"` |
| `key_handlers::create_key` | `"API key '{name}' created for tenant '{tenant}' (tier: {tier}, rpm_limit: {N}, tpm_limit: {N})"` |
| `key_handlers::delete_key` | `"API key {id} soft-deleted (access permanently revoked)"` |
| `key_handlers::toggle_key` | `"API key {id} updated — is_active={bool}, tier={tier}"` (lists only changed fields) |
| `backend_handlers::register_backend` | `"Backend '{name}' registered (type: {ollama/gemini}, initial_status: {status})"` |
| `backend_handlers::delete_backend` | `"Backend '{name}' ({id}) deactivated (soft-deleted, no longer routed)"` |
| `backend_handlers::update_backend` | `"Backend '{name}' ({id}) configuration updated"` |
| `gpu_server_handlers::register_gpu_server` | `"GPU server '{name}' registered (id: {id})"` |
| `gpu_server_handlers::update_gpu_server` | `"GPU server '{name}' ({id}) configuration updated"` |
| `gpu_server_handlers::delete_gpu_server` | `"GPU server {id} permanently deleted"` |
| `gemini_model_handlers::set_sync_config` | `"Gemini admin API key replaced (used for global model list sync)"` |
| `gemini_model_handlers::sync_models` | `"Global Gemini model list synced from API: {N} models discovered"` |
| `gemini_policy_handlers::upsert_gemini_policy` | `"Gemini rate-limit policy for '{model}' upserted: rpm={N}, rpd={N}, free_tier={bool}"` |
| `dashboard_handlers::patch_capacity_settings` | `"Capacity analyzer settings updated: model={:?}, batch_enabled={:?}, batch_interval_secs={:?}"` |
| `dashboard_handlers::trigger_capacity_sync` | `"Manual capacity analysis triggered by admin"` |
| `dashboard_handlers::patch_lab_settings` | `"Lab feature flags updated: gemini_function_calling={:?}"` |

### Pipeline

```
*_handlers.rs → AuditPort::record() → HttpAuditAdapter
                                           → POST /internal/ingest/audit (veronex-analytics)
                                           → OTel LogRecord (event.name="audit.action")
                                           → OTLP gRPC → OTel Collector
                                           → Redpanda [otel-logs]
                                           → kafka_otel_logs_mv (ClickHouse MV)
                                           → otel_logs (MergeTree, LogAttributes['event.name']='audit.action')
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

`GET /v1/audit` — RequireSuper only.

Query params:
- `limit` (default 100, max 1000)
- `offset` (default 0)
- `action` — filter by action string
- `resource_type` — filter by resource type

### Audit Query

`GET /v1/audit` → delegates to `analytics_repo.audit_events(filters)` → `veronex-analytics GET /internal/audit` → ClickHouse `otel_logs WHERE LogAttributes['event.name']='audit.action'`.

### Fail-Open

`HttpAuditAdapter` HTTP error → `warn!` log + request continues. Audit is best-effort.

---

## Environment Variables

```bash
JWT_SECRET=change-me-in-production   # HS256 signing key — MUST change in production
# BOOTSTRAP_SUPER_USER=admin         # optional: pre-seed super account (CI/automated)
# BOOTSTRAP_SUPER_PASS=secret        # optional: both must be set; omit for setup-flow
```

`JWT_SECRET` must be changed in production. Bootstrap vars are **intentionally omitted**
from docker-compose defaults — use `POST /v1/setup` (first-run UI flow) instead.

---

## Implementation Files

| File | Role |
|------|------|
| `infrastructure/inbound/http/auth_handlers.rs` | `setup_status` + `setup` + login/logout/refresh/reset-password |
| `infrastructure/inbound/http/test_handlers.rs` | `test_completions` + `stream_test_job` (JWT, no rate limit) |
| `migrations/20260301000037_job_account_id.sql` | adds `account_id UUID` column to `inference_jobs` |
| `web/app/setup/page.tsx` | First-run setup form (username + password + confirm) |
| `web/app/layout.tsx` | `AppShell` — checks setup status, redirects /setup↔/login |
| `migrations/000034_accounts.sql` | accounts table |
| `migrations/000035_api_key_account_id.sql` | account_id FK + is_test_key |
| `migrations/20260301000036_account_sessions.sql` | account_sessions table + indexes |
| `domain/entities/account.rs` | Account entity |
| `domain/entities/session.rs` | Session entity |
| `application/ports/outbound/account_repository.rs` | AccountRepository port |
| `application/ports/outbound/session_repository.rs` | SessionRepository port |
| `application/ports/outbound/audit_port.rs` | AuditPort + AuditEvent |
| `infrastructure/outbound/persistence/account_repository.rs` | PostgresAccountRepository |
| `infrastructure/outbound/persistence/session_repository.rs` | PostgresSessionRepository |
| `infrastructure/outbound/observability/http_audit_adapter.rs` | HttpAuditAdapter (replaces RedpandaAuditAdapter) |
| `infrastructure/inbound/http/middleware/jwt_auth.rs` | jwt_auth (jti + Valkey blocklist + last_used) + RequireSuper |
| `infrastructure/inbound/http/auth_handlers.rs` | login / logout / refresh / reset-password |
| `infrastructure/inbound/http/account_handlers.rs` | CRUD + reset-link + session endpoints |
| `infrastructure/inbound/http/audit_handlers.rs` | GET /v1/audit (delegates to analytics_repo) |
| `web/lib/auth.ts` | `getAccessToken`, `setTokens` (saves `refresh_token`), `setAccessToken`, `clearTokens`, `getAuthUser` |
| `web/lib/api-client.ts` | ApiClient SSOT — auto 401→refresh→retry, clears tokens on failure |
| `web/lib/api.ts` | `req()` (API key) + account/session/audit methods via apiClient |
| `web/lib/types.ts` | `Account`, `SessionRecord`, `AuditEvent`, `LoginRequest/Response` (with `refresh_token`) |
| `web/app/login/page.tsx` | Login form → `POST /v1/auth/login` → redirect `/` |
| `web/app/accounts/page.tsx` | Account DataTable + `AccountSessionsModal` (Shield icon per row) |
| `web/app/audit/page.tsx` | Audit log DataTable + action/resource_type filters |
| `web/components/nav.tsx` | Auth user state + Accounts/Audit nav links + logout button |

---

## Web Frontend

### Token Storage

Tokens stored in **cookies** (7-day expiry, `SameSite=Strict`):

```
veronex_access_token   → JWT access token (1h)
veronex_refresh_token  → raw refresh token (rolling, no fixed TTL)
veronex_username       → logged-in username (display name in nav)
veronex_role           → "super" | "admin" (nav access control)
veronex_account_id     → account UUID (session management)
```

Helpers in `web/lib/auth.ts`:
- `getAccessToken()` / `getRefreshToken()` — read from cookies
- `setTokens(resp: LoginResponse)` — saves access_token + refresh_token (7-day cookie)
- `setAccessToken(token)` — updates only access_token (used after token refresh)
- `clearTokens()` — clears all auth cookies (logout, forced redirect to /login)
- `getAuthUser()` → `{ username, role, accountId }` (reads from cookies)
- `isLoggedIn()` — returns true if access token cookie present

**Auth flow SSOT**: `web/lib/auth-guard.ts`
- Module-level mutex `refreshMutex` — concurrent 401s share the same refresh promise
- `tryRefresh()` — attempts token refresh; clears cookies and redirects on failure
- `redirectToLogin()` — checks `PUBLIC_PATHS = ['/login', '/setup']` before redirecting
- Never duplicate auth logic outside `auth-guard.ts`

### API Client (`web/lib/api-client.ts`)

`apiClient` — singleton `ApiClient` for all JWT-protected routes:
- Attaches `Authorization: Bearer <access_token>` automatically
- On **401**: calls `POST /v1/auth/refresh` with refresh_token → updates access_token → retries once
- If refresh fails: `clearTokens()` + `window.location.href = '/login'`
- Methods: `get`, `post`, `patch`, `put`, `delete`

### Nav Auth State

`web/components/nav.tsx` — `useEffect` reads `getAuthUser()` on mount:
- If logged in: shows username + Logout button in nav footer
- Always shows: Accounts link (`Users` icon)
- `role === "super"` only: Audit link (`Shield` icon)

### Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new auth endpoint | `infrastructure/inbound/http/auth_handlers.rs` | Handler + router entry in `router.rs` public block |
| Add new account field | `migrations/` new SQL + `domain/entities/account.rs` + `account_repository.rs` | Update trait + impl |
| Add new auditable action | `account_handlers.rs` (or other handlers) | Call `emit_audit()` with action string + a descriptive `details` string (REQUIRED) |
| Change JWT expiry | `auth_handlers.rs` `issue_access_token()` | Update `exp` calculation |
| Change refresh TTL | `auth_handlers.rs` login handler | `EXPIRE` call duration |
| Add new audit filter | `audit_handlers.rs` | Add query param + `WHERE` clause |
