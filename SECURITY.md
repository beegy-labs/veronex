# Security Policy

## Reporting a Vulnerability

Do **not** create a public GitHub issue. Instead:

1. Email: security@beegy.dev
2. Or use GitHub Security Advisories (Repository → Security → "Report a vulnerability")

We will respond within **48 hours** and coordinate the fix and disclosure.

## Security Measures

| Area | Implementation |
|------|----------------|
| Authentication | JWT (HS256) with rolling refresh + Valkey revocation blocklist |
| API Keys | BLAKE2b-256 hashed storage (never plaintext) |
| Passwords | Argon2id (memory-hard) |
| Encrypted fields | Gemini API keys — AES-256-GCM |
| RBAC | super / admin / user roles |
| Rate limiting | RPM sliding window + TPM budget per API key (Valkey) |
| SQL | sqlx compile-time verification + parameterized queries |
| Input | Header injection, prompt injection, XSS mitigations |

## Audit & Logging

- All API key operations, auth events, and admin actions logged
- OTel → Redpanda → ClickHouse pipeline (90-day hot retention)

## Vulnerability Disclosure

1. Private report received
2. Validate and triage (≤48h)
3. Fix developed and tested
4. Patch released
5. Public disclosure (7 days after patch, credit optional)

**Contact**: security@beegy.dev
