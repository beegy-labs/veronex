# Security

> CDD Tier 1 — Security indicator | **Last Updated**: 2026-03-07

## Quick Navigation

| Action | Read |
|--------|------|
| Core security policy | `SECURITY.md` (root) |
| Server-side security specs | `docs/llm/auth/security.md` |
| Authentication | `docs/llm/auth/jwt-sessions.md` |
| API Keys | `docs/llm/auth/api-keys.md` |

## Current Security Features

- **JWT (HS256)**: Token auth + rolling refresh + Valkey revocation
- **API Keys**: BLAKE2b-256 hashed storage (cascade delete on account deletion planned — FK missing CASCADE)
- **Passwords**: Argon2id hashing
- **Rate Limiting**: RPM + TPM via Valkey (sliding window), fail-closed (503 on Valkey error)
- **RBAC**: super/admin roles
- **Encrypted Fields**: Gemini API keys (stored as-is — PoC, AES-256-GCM planned)
- **Circuit Breaker**: `CircuitBreakerMap` — per-provider failure gating
- **CORS**: `CORS_ALLOWED_ORIGINS` env var — origin allowlist for production
- **Security Headers**: HSTS, X-Content-Type-Options, X-Frame-Options, Referrer-Policy
- **SSRF Protection**: Provider URL validation blocks metadata endpoints
- **Input Validation**: Prompt 1MB, model name 256B limits

## Security Enhancements (Planned)

| Priority | Task | Path |
|----------|------|------|
| P0 | Gemini API key encryption (AES-256-GCM) | docs/llm/auth/security.md |
| P0 | Secret management (Vault) | docs/llm/auth/security.md |
| P1 | api_keys ON DELETE CASCADE | migration |
| P1 | DDoS protection | docs/llm/auth/security.md |

## Reporting

**Email**: security@beegy.dev  
**PGP**: Available on request  
**Response**: ≤48 hours

---

**SSOT**: `SECURITY.md`
