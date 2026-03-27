# JWT Sessions: Endpoints & Environment

> SSOT | **Last Updated**: 2026-03-24 | Classification: Operational
> Auth endpoints, test run endpoints, account endpoints, and environment variables.

## Auth Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/v1/setup/status` | None | `{ needs_setup }` |
| POST | `/v1/setup` | None | Create first super + issue session; 409 if exists |
| POST | `/v1/auth/login` | None | Verify Argon2id, issue access+refresh tokens |
| POST | `/v1/auth/logout` | None | Revoke session by refresh token; 204 |
| POST | `/v1/auth/refresh` | None | Revoke old session, issue new access+refresh (server-side); only `access_token` returned to client. Note: new refresh hash stored but not sent -- client's original refresh token is invalidated after one refresh |
| POST | `/v1/auth/reset-password` | None | Validate Valkey token, save new hash; 204 |

## Test Run Endpoints (JWT Bearer)

Logged-in accounts run inference without an API key. Jobs tracked by `account_id`, `source=Test`, excluded from API metrics. See [openai-compat.md](../inference/openai-compat.md) for request/response format details.

| Method | Path | Description |
|--------|------|-------------|
| POST | `/v1/test/completions` | OpenAI SSE stream |
| GET | `/v1/test/jobs/{job_id}/stream` | SSE reconnect |
| POST | `/v1/test/api/chat` | Ollama NDJSON stream |
| POST | `/v1/test/api/generate` | Ollama NDJSON stream |
| POST | `/v1/test/v1beta/models/{*path}` | Gemini SSE stream |

All test routes: `api_key_id=NULL`, `account_id=claims.sub`, queue=`veronex:queue:jobs:test` (low-priority).

Full request/response specs: [jwt-sessions-impl.md](jwt-sessions-impl.md#test-run-endpoint-details)

## Account Endpoints (RequireSuper)

All require `Authorization: Bearer <super-token>`.

| Method | Path | Description |
|--------|------|-------------|
| GET | `/v1/accounts` | List all active accounts |
| POST | `/v1/accounts` | Create account + auto-generate test API key |
| PATCH | `/v1/accounts/{id}` | Update name/email/department/position |
| DELETE | `/v1/accounts/{id}` | Soft-delete |
| PATCH | `/v1/accounts/{id}/active` | Toggle `is_active` |
| POST | `/v1/accounts/{id}/reset-link` | Generate 24h reset token |
| GET | `/v1/accounts/{id}/sessions` | List active sessions |
| DELETE | `/v1/accounts/{id}/sessions` | Revoke all sessions |
| DELETE | `/v1/sessions/{session_id}` | Revoke specific session |

Full request/response specs: [jwt-sessions-impl.md](jwt-sessions-impl.md#account-endpoint-details)

## Environment Variables

```bash
JWT_SECRET=change-me-in-production   # HS256 signing key -- MUST change in production
# BOOTSTRAP_SUPER_USER=admin         # optional: pre-seed super account
# BOOTSTRAP_SUPER_PASS=secret        # optional: both must be set
```

## Implementation Files

| File | Role |
|------|------|
| `infrastructure/inbound/http/auth_handlers.rs` | setup + login/logout/refresh/reset-password |
| `infrastructure/inbound/http/test_handlers.rs` | test completions + stream (JWT, no rate limit) |
| `infrastructure/inbound/http/account_handlers.rs` | CRUD + reset-link + session endpoints |
| `infrastructure/inbound/http/audit_handlers.rs` | GET /v1/audit |
| `infrastructure/inbound/http/middleware/jwt_auth.rs` | jwt_auth + RequireSuper |
| `domain/entities/{account,session}.rs` | Account + Session entities |
| `application/ports/outbound/{account,session}_repository.rs` | Repository ports |
| `application/ports/outbound/audit_port.rs` | AuditPort + AuditEvent |
| `infrastructure/outbound/persistence/{account,session}_repository.rs` | Postgres impls |
| `infrastructure/outbound/observability/http_audit_adapter.rs` | HttpAuditAdapter |
| `web/lib/auth.ts` | Token cookie helpers |
| `web/lib/auth-guard.ts` | Refresh mutex + redirect logic |
| `web/lib/api-client.ts` | ApiClient (auto 401 refresh+retry) |
