# Security

> CDD Tier 1 — Security indicator | **Last Updated**: 2026-03-03

## Quick Navigation

| Action | Read |
|--------|------|
| Core security policy | `SECURITY.md` (root) |
| Backend security specs | `docs/llm/backend/security.md` |
| Authentication | `docs/llm/backend/auth.md` |
| API Keys | `docs/llm/backend/api_keys.md` |

## Current Security Features

- **JWT (HS256)**: Token auth + rolling refresh + Valkey revocation
- **API Keys**: BLAKE2b-256 hashed storage
- **Passwords**: Argon2id hashing
- **Rate Limiting**: RPM + TPM via Valkey (sliding window)
- **RBAC**: super/admin roles
- **Encrypted Fields**: Gemini API keys (AES-256-GCM)
- **Circuit Breaker**: `CircuitBreakerMap` — per-backend failure gating
- **CORS**: `CORS_ALLOWED_ORIGINS` env var — origin allowlist for production

## Security Enhancements (Planned)

| Priority | Task | Path |
|----------|------|------|
| P0 | Secret management (Vault) | docs/llm/backend/security.md |
| P1 | DDoS protection | docs/llm/backend/security.md |
| P1 | Security headers (XSS, CSP) | docs/llm/backend/security.md |

## Reporting

**Email**: security@beegy.dev  
**PGP**: Available on request  
**Response**: ≤48 hours

---

**SSOT**: `SECURITY.md`
