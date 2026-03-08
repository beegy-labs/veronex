# Security Policy

> **Last Updated**: 2026-03-02

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability, please disclose it responsibly:

1. **Do NOT** create a public GitHub issue
2. Email: security@beegy.dev
3. OR use GitHub Security Advisories (Repository → Security → "Report a vulnerability")

We will respond within **48 hours** and work with you to coordinate the fix and disclosure.

## Security Measures

### Current Implementations

#### 1. Authentication & Authorization
- **JWT (HS256)**: Token-based authentication with rolling refresh
- **API Keys**: BLAKE2b-256 hashed storage (never stored in plaintext)
- **RBAC**: Role-based access control (super, admin, user)
- **Session Management**: Valkey-based session revocation blocklist

#### 2. Data Protection
- **Passwords**: Argon2id hashing (memory-hard, parallelism-resistant)
- **API Keys**: BLAKE2b-256 cryptographic hashing
- **Encrypted Fields**: Gemini API keys stored encrypted (AES-256-GCM)
- **Database**: PostgreSQL 18 with native encryption support

#### 3. Rate Limiting
- **RPM (Requests Per Minute)**: Sliding window rate limiting via Valkey
- **TPM (Tokens Per Minute)**: Token-based rate limiting
- **Fail-open**: Rate limiting disabled if Valkey unavailable (logs warning)

#### 4. Input Validation
- **Type-safe SQL**: sqlx compile-time SQL verification
- **Parameterized Queries**: All queries use prepared statements
- **Request Validation**: Struct validation before processing

### Planned Enhancements

| Priority | Enhancement | Timeline |
|----------|-------------|----------|
| P0 | Secret management (HashiCorp Vault) | 2026 Q2 |
| P0 | Circuit breaker pattern | 2026 Q2 |
| P1 | DDoS protection (Cloudflare/WAF) | 2026 Q2 |
| P1 | SQL injection prevention (additional tests) | 2026 Q3 |
| P1 | XSS protection headers | 2026 Q3 |
| P2 | Security audit (third-party) | 2026 Q4 |
| P2 | Penetration testing | 2026 Q4 |

## Security Headers (Planned)

The following HTTP security headers will be added:

```http
# Current
X-Content-Type-Options: nosniff
X-Frame-Options: DENY

# Planned
Strict-Transport-Security: max-age=31536000; includeSubDomains
Content-Security-Policy: default-src 'self'
X-XSS-Protection: 1; mode=block
Referrer-Policy: strict-origin-when-cross-origin
Permissions-Policy: geolocation=(), microphone=(), camera=()
```

## Audit & Logging

### Current Audit Capabilities
- **API Key Usage**: All API key operations logged
- **Authentication Events**: Login, logout, token refresh
- **Rate Limiting**: Rate limit violations tracked
- **Admin Actions**: Superuser operations recorded

### Audit Data Retention
- **Hot Storage** (ClickHouse): 90 days
- **Cold Storage**: 1 year (S3/backup)

## Compliance

Current implementation supports:

- [x] GDPR: User data deletion via API
- [x] Privacy: No personal data in logs
- [ ] SOC 2: In progress (2026 Q4)
- [ ] ISO 27001: Planned (2027)

## Security Contact

- **Email**: security@beegy.dev
- **PGP Key**: Available on request
- **Response Time**: ≤48 hours

## Security Updates

Security patches will be released as:

- **Patch releases**: Critical security fixes
- **Minor releases**: Non-critical improvements
- **Upgrade Path**: SemVer compatible

---

## Vulnerability Disclosure Policy

We follow a **coordinated disclosure** model:

1. Reporter discloses vulnerability privately
2. We validate and triage (within 48h)
3. Fix is developed and tested
4. Patch is released
5. Public disclosure (7 days after patch)
6. Credit is given (optional)

---

*For security questions or to report vulnerabilities, contact security@beegy.dev*
