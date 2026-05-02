# Security

> SSOT | **Last Updated**: 2026-05-02 (rev: SetSensitiveRequestHeadersLayer wrapping TraceLayer — Authorization / Cookie / X-API-Key / Proxy-Authorization redacted from trace spans)

## Authentication & Authorization

For full details see:
- **JWT**: [jwt-sessions.md](jwt-sessions.md) (flow, middleware, extractors, sessions, revocation)
- **API Keys**: [api-keys.md](api-keys.md) (hashing, headers, auth flow, rate limiting)

### RBAC Roles

| Role | Permissions |
|------|-------------|
| `super` | All endpoints (including `/v1/accounts/*`, `/v1/audit/*`) |
| `viewer` | `dashboard_view` only |

---

## Rate Limiting

**File**: `infrastructure/inbound/http/middleware/rate_limiter.rs`

| Type | Algorithm | Key Pattern | Details |
|------|-----------|-------------|---------|
| RPM | Sliding window (Valkey sorted set) | `veronex:ratelimit:rpm:{key_id}` | Atomic Lua eval, 62s TTL |
| TPM | Per-minute counter (Valkey) | `veronex:ratelimit:tpm:{key_id}:{minute}` | Reserve 500 at admission, adjust on completion |

**TPM reservation flow**: Rate limiter reserves `TPM_ESTIMATED_TOKENS` (500) at admission and stores the reservation minute in `JobEntry.tpm_reservation_minute`. On completion, `record_tpm` targets the **reservation minute's key** (not current time) to prevent cross-minute drift.

### Login Rate Limiting

| Property | Value |
|----------|-------|
| Scope | Per IP address |
| Limit | 10 attempts per 5 minutes |
| Key pattern | `veronex:login_attempts:{ip}` |
| Storage | Valkey (auto-expires after 5 min) |
| Exceeded | `429 Too Many Requests` |

**Fail-open on INCR error (defaults to count=1)**: Valkey unavailable -> allows the request through with count=1 (log error).
**Exceeded**: `429 TOO_MANY_REQUESTS` + `Retry-After` header.

---

## Data Protection

| Asset | Algorithm | Storage | Notes |
|-------|-----------|---------|-------|
| Passwords | Argon2id | PHC string in DB | Random 16-byte salt per password |
| API Keys | BLAKE2b-256 | Hash only in DB | `key_prefix` for UI display |
| Gemini API Keys | AES-256-GCM | Encrypted in DB | `GEMINI_ENCRYPTION_KEY` env var (≥32 chars), HKDF-derived |

Key rotation strategy for encrypted fields: planned (future). Current encryption uses random 12-byte nonce per value (nonce‖ciphertext stored as base64).

**Gemini API key transport**: Gemini API keys are sent via the `x-goog-api-key` HTTP header (not URL query parameters) to avoid key leakage in access logs.

---

## Security Headers

**File**: inline in `router.rs` via `map_response`

| Header | Value | Status |
|--------|-------|--------|
| `Strict-Transport-Security` | `max-age=31536000; includeSubDomains` | Done |
| `X-Content-Type-Options` | `nosniff` | Done |
| `X-Frame-Options` | `DENY` | Done |
| `Referrer-Policy` | `strict-origin-when-cross-origin` | Done |
| `Content-Security-Policy` | `default-src 'self'; frame-ancestors 'none'` | Planned |
| `X-XSS-Protection` | `1; mode=block` | Planned |
| `Permissions-Policy` | `geolocation=(), microphone=(), camera=()` | Planned |

## Sensitive-Header Redaction (Tower Layer)

`router.rs` wraps `SetSensitiveRequestHeadersLayer` before `TraceLayer` so trace spans never capture credentials. Redacted: `Authorization`, `Cookie`, `Proxy-Authorization`, `x-api-key`. Required `tower-http` feature: `sensitive-headers`. Layer-order rationale: `patterns/middleware.md § Tower Layer Order`.

---

## SSRF Protection

Provider URLs are validated by `validate_provider_url()` in `provider_handlers.rs` before any HTTP requests.

| Rule | Details |
|------|---------|
| Scheme enforcement | Only `http://` and `https://` allowed |
| GCP metadata | Blocks `metadata.google.internal` hostname |
| IPv4 link-local | Blocks `169.254.0.0/16` (AWS metadata `169.254.169.254`) |
| IPv6 link-local | Blocks `fe80::/10` addresses |
| IPv4-mapped IPv6 | Blocks `::ffff:169.254.x.x` (bypass prevention) |
| IPv6 bracket parsing | Correctly handles `[::ffff:169.254.169.254]:port` notation |
| Applied to | Ollama provider URLs, Gemini API base URLs (register + update) |

---

## Input Validation

| Field | Max Size | Applied To |
|-------|----------|------------|
| Prompt/message content | 1 MB | All API formats (native, OpenAI, Gemini, Ollama) |
| Model name | 256 bytes | All API formats |

---

## Audit & Logging

For full audit trail (events, covered actions, details format, pipeline, ClickHouse schema):
see [jwt-sessions.md](jwt-sessions.md) -- Audit Trail section.

| Event Category | Key Fields | Format |
|----------------|------------|--------|
| API Key Usage | `api_key_id`, `endpoint`, `status` | JSONB |
| Authentication | `action`, `account_id`, `ip` | JSONB |
| Rate Limit | `key_id`, `limit_type`, `limit`, `retry_after` | JSONB |
| Admin Action | `action`, `target_id`, `actor_id` | JSONB |

Pipeline: `*_handlers.rs` -> `AuditPort::record()` -> `HttpAuditAdapter` -> veronex-analytics -> ClickHouse (via OTel Collector + Redpanda).

---

## Secure Coding Practices

| Practice | Implementation |
|----------|---------------|
| Parameterized SQL | All queries use `sqlx::query().bind()` — no string interpolation |
| Parameterized queries | All queries use prepared statements |
| Struct validation | Request structs validated before processing |
| No error leakage | Internal errors hidden from client |
| Full logging | Context in server logs only |
| Fail-closed rate limiting | 503 on Valkey errors instead of allowing requests through |
| Fail-closed JWT revocation | Valkey error → 503 (denies access), not fail-open |
| Atomic refresh token claim | SET NX + EX prevents TOCTOU race on token replay |
| ClickHouse audit whitelist | Action/resource_type validated against allowlist before query |
| Analytics input validation | Hours parameter bounded 1..=8760 on all handlers |

### Secrets Management

| Secret | Env Var | Required | Notes |
|--------|---------|----------|-------|
| JWT signing key | `JWT_SECRET` | **Always** | `>=32` chars, `expect()` panics if missing |
| Database URL | `DATABASE_URL` | **Always** | No default — panics if missing |
| S3 access key | `S3_ACCESS_KEY` | **Always** | No default — required for startup |
| S3 secret key | `S3_SECRET_KEY` | **Always** | No default — required for startup |
| CORS origins | `CORS_ALLOWED_ORIGINS` | **Always** | No default — required for startup |
| Gemini encryption key | `GEMINI_ENCRYPTION_KEY` | **Always** | `>=32` chars, HKDF-derived AES-256-GCM key |
| Analytics shared secret | `ANALYTICS_SECRET` | When `ANALYTICS_URL` set | No default — adapters disabled if missing |
| Bootstrap API key | `BOOTSTRAP_API_KEY` | Planned | Defined in Helm, not yet read by Rust |

| Stage | Approach |
|-------|----------|
| Current | Environment variables (no hardcoded defaults for secrets) |
| Planned | HashiCorp Vault integration |

---

## Vulnerability Response

- **Email**: security@beegy.dev
- **PGP**: Available on request
- **Response SLA**: 48 hours

| Step | Action |
|------|--------|
| 1 | Private disclosure (no public issue) |
| 2 | Validation and triage (48h) |
| 3 | Fix development and testing |
| 4 | Patch release |
| 5 | Public disclosure (7 days post-patch) |

---

## Compliance

| Standard | Status |
|----------|--------|
| GDPR (data deletion API) | Done |
| Privacy (no PII in logs) | Done |
| TLS 1.3 | Done |
| SOC 2 Type II | In progress (2026 Q4) |
| ISO 27001 | Planned (2027) |

---

## Security Checklist

| Phase | Check |
|-------|-------|
| Pre-deploy | Secrets in env vars (no hardcoding) |
| Pre-deploy | `JWT_SECRET` > 32 chars |
| Pre-deploy | API keys hashed (BLAKE2b-256) |
| Pre-deploy | Passwords hashed (Argon2id) |
| Pre-deploy | Rate limiting enabled (RPM/TPM) |
| Pre-deploy | TLS enabled (HTTPS only) |
| Post-deploy | Monitor audit logs |
| Post-deploy | Review rate limit violations |
| Post-deploy | Rotate secrets periodically |

---

## Known Limitations

| Limitation | Mitigation |
|------------|------------|
| No per-account rate limit on JWT endpoints | Plan: per-account limiting |
| No DDoS protection at app layer | Use WAF/Cloudflare |

---

## References

| Standard | Usage |
|----------|-------|
| OWASP Top 10 | Design decisions |
| RFC 6750 | Bearer token usage |
| NIST SP 800-63B | Auth guidelines |
