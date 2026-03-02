# Security

> SSOT | **Last Updated**: 2026-03-02 (security policy: SECURITY.md)

## Security Overview

Veronex implements a defense-in-depth security strategy with multiple layers of protection for the LLM inference gateway.

---

## Authentication & Authorization

### JWT Authentication

**Implementation**: `infrastructure/inbound/http/middleware/jwt_auth.rs`

| Feature | Details |
|----------|----------|
| Algorithm | HS256 |
| Claims | `sub` (UUID), `role`, `jti`, `exp` |
| Revocation | Valkey `veronex:revoked:{jti}` blocklist |
| Refresh | Rolling refresh via new session creation |

**Flow**:
1. Client sends `Authorization: Bearer <token>`
2. Middleware validates JWT signature and expiration
3. Check Valkey for revoked JTI → return 401 if revoked
4. Insert Claims into request extensions
5. Non-blocking session update (async)

**Extractor**:
```rust
// Require super-admin role
pub struct RequireSuper(pub Claims);

// Usage
async fn admin_handler(RequireSuper(_): RequireSuper) -> Result<...> {
    // Only super role can access
}
```

### API Key Authentication

**Implementation**: `infrastructure/inbound/http/middleware/api_key_auth.rs`

| Feature | Details |
|----------|----------|
| Hashing | BLAKE2b-256 |
| Headers | `X-API-Key`, `Authorization: Bearer`, `x-goog-api-key` |
| Expiry | Optional `expires_at` field |
| Soft delete | `deleted_at` field check |

**Flow**:
1. Extract raw key from headers (any of 3 formats)
2. Compute BLAKE2b-256 hash
3. Query DB by hash → check `is_active` + `deleted_at` + `expires_at`
4. Insert ApiKey into extensions

**Excluded Paths**: `/health`, `/readyz` (no auth required)

### RBAC Roles

| Role | Permissions |
|------|-------------|
| `super` | All endpoints (including `/v1/accounts/*`, `/v1/audit/*`) |
| `admin` | Tenant-level resources (keys, servers, backends) |
| `user` | Inference endpoints only |

---

## Rate Limiting

**Implementation**: `infrastructure/inbound/http/middleware/rate_limiter.rs`

### RPM (Requests Per Minute)

- **Algorithm**: Sliding window via Valkey sorted sets
- **Script**: Atomic Lua eval (1 RTT)
- **TTL**: 62 seconds

```lua
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
redis.call('ZADD', KEYS[1], ARGV[2], ARGV[3])
redis.call('EXPIRE', KEYS[1], 62)
return redis.call('ZCARD', KEYS[1])
```

### TPM (Tokens Per Minute)

- **Algorithm**: Per-minute counter in Valkey
- **Granularity**: `veronex:ratelimit:tpm:{key_id}:{minute}`
- **Increment**: By InferenceUseCase after job completion

### Fail-Open

- Valkey unavailable → request allowed (log warning)
- Rate limit exceeded → `429 TOO_MANY_REQUESTS` + `Retry-After` header

---

## Data Protection

### Passwords

- **Hash**: Argon2id (memory-hard, parallelism-resistant)
- **Salt**: Random 16-byte salt per password
- **Storage**: Plain text in DB (never in logs)

```rust
let salt = SaltString::generate(&mut OsRng);
Argon2::default()
    .hash_password(password.as_bytes(), &salt)
    .map(|h| h.to_string())
```

### API Keys

- **Hash**: BLAKE2b-256 (cryptographic hash)
- **Storage**: Hash only in DB
- **Prefix**: Truncated hash for UI display (`key_prefix`)

### Encrypted Fields

- **Gemini API Keys**: AES-256-GCM encrypted
- **Key**: Generated at runtime (not persisted)
- **IV**: Per-key random IV

**Note**: Encrypted fields require runtime key rotation strategy (future).

---

## Security Headers (Planned)

### Current Headers (HTTP/2)

```rust
X-Content-Type-Options: nosniff
X-Frame-Options: DENY
```

### Planned Headers (HTTP/1.1 → HTTP/2 migration)

```http
# Transport Security
Strict-Transport-Security: max-age=31536000; includeSubDomains

# Content Security
Content-Security-Policy: default-src 'self'; frame-ancestors 'none'

# XSS Protection
X-XSS-Protection: 1; mode=block

# Referrer Policy
Referrer-Policy: strict-origin-when-cross-origin

# Permissions Policy
Permissions-Policy: geolocation=(), microphone=(), camera=()
```

**Implementation**: Add middleware in `infrastructure/inbound/http/middleware/security_headers.rs`

---

## Audit & Logging

### Audit Events

| Event | Field | Format |
|------|------|------|
| API Key Usage | `api_key_id`, `endpoint`, `status` | JSONB |
| Authentication | `action`, `account_id`, `ip` | JSONB |
| Rate Limit | `key_id`, `limit_type`, `limit`, `retry_after` | JSONB |
| Admin Action | `action`, `target_id`, `actor_id` | JSONB |

### Observability Pipeline

```
Audit Events → HttpAuditAdapter → veronex-analytics
            → ClickHouse → OTel → Redpanda
```

---

## Secure Coding Practices

### Input Validation

- **Type-safe SQL**: sqlx compile-time verification
- **Parameterized Queries**: All queries use prepared statements
- **Struct Validation**: Request structs before processing

### Error Handling

- **No leakage**: Internal errors hidden from client
- **Logging**: Full context in server logs only
- **Fail-open**: Graceful degradation on non-critical failures

### Secrets Management

**Current**: Environment variables only

**Planned**: HashiCorp Vault integration

```rust
// Current (INSECURE for production)
JWT_SECRET: std::env::var("JWT_SECRET")?

// Planned (SECURE)
JWT_SECRET: vault_client.get_secret("veronex/jwt_secret")?
```

---

## Vulnerability Response

### Reporting

- **Email**: security@beegy.dev
- **PGP**: Available on request
- **Response**: ≤48 hours

### Disclosure Policy

1. Private disclosure (no public issue)
2. Validation & triage (48h)
3. Fix development & testing
4. Patch release
5. Public disclosure (7 days post-patch)
6. Credit (optional)

---

## Compliance

### Current

- [x] GDPR: User data deletion API
- [x] Privacy: No PII in logs
- [x] Encryption: TLS 1.3 (in production)

### In Progress

- [ ] SOC 2 Type II (2026 Q4)
- [ ] ISO 27001 (2027)

---

## Security Checklist

### Pre-Deployment

- [ ] All secrets in environment variables (no hardcoding)
- [ ] JWT_SECRET > 32 characters
- [ ] API keys hashed (BLAKE2b-256)
- [ ] Passwords hashed (Argon2id)
- [ ] Rate limiting enabled (RPM/TPM)
- [ ] TLS enabled (HTTPS only)

### Post-Deployment

- [ ] Monitor audit logs
- [ ] Review rate limit violations
- [ ] Rotate secrets periodically
- [ ] Audit API key usage

---

## Known Limitations

1. **No rate limit on JWT endpoints** → Plan to add per-account limiting
2. **No DDoS protection at application layer** → Use WAF/Cloudflare
3. **No SQL injection prevention tests** → Add integration tests
4. **No XSS protection headers** → Add middleware (planned)

---

## Security References

- **OWASP Top 10**: Referenced in design decisions
- **CWE/SANS**: Vulnerability categories tracked
- **RFC 6750**: OAuth 2.0 Bearer token usage
- **NIST SP 800-63B**: Authentication guidelines

---

**SSOT**: This document
