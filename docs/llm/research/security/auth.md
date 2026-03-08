# Authentication & Sessions — 2026 Research

> **Last Researched**: 2026-03-01 | **Source**: JWT best practices + verified in production
> **Status**: ✅ Verified — used in `crates/veronex/src/infrastructure/inbound/http/middleware/`

---

## JWT Claims Structure

```rust
#[derive(Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,   // account_id (UUIDv7)
    pub role: String,  // "super" | "admin" | "viewer"
    pub jti: String,   // UUIDv7 — unique per session (for revocation)
    pub exp: usize,    // Unix timestamp
}
```

**`jti` is required** for session revocation. Without it, JWTs cannot be individually invalidated before expiry.

---

## Session Lifecycle

```
Login → INSERT account_sessions (jti, refresh_token_hash, ip, expires_at)
     → Return { access_token, refresh_token }

Refresh → Verify refresh_token (BLAKE2b hash compare)
       → Revoke old session (revoked_at = now)
       → INSERT new session (rolling refresh)
       → Return new { access_token, refresh_token }

Logout → Mark session revoked_at = now
      → SET veronex:revoked:{jti} in Valkey (TTL = remaining access token lifetime)
```

---

## Revocation — Valkey Blocklist

Access tokens are short-lived but still need instant revocation for logout.
Solution: Valkey key per revoked JTI.

```rust
// On logout:
let ttl = claims.exp - now_unix();
valkey.set_ex(&format!("veronex:revoked:{}", claims.jti), "1", ttl).await?;

// In auth middleware:
if valkey.exists(&format!("veronex:revoked:{}", claims.jti)).await? {
    return Err(AppError::Unauthorized);
}
```

TTL = remaining token lifetime → key auto-expires when token would have expired anyway.

---

## Refresh Token Storage

```sql
-- account_sessions table
refresh_token_hash BYTEA NOT NULL   -- BLAKE2b-256 hash of the raw token
```

**Never store raw refresh tokens.** Store BLAKE2b hash, compare on verify.
BLAKE2b is fast (keyed MAC variant available) and suitable for non-secret data hashing.

```rust
use blake2::{Blake2b256, Digest};
let hash = Blake2b256::digest(raw_token.as_bytes());
```

---

## Frontend Auto-Refresh Pattern

```ts
// api-client.ts — intercept 401, try refresh, retry once
async function fetchWithAuth(url: string, opts: RequestInit) {
  let res = await fetch(url, withAuth(opts))
  if (res.status === 401) {
    const refreshed = await tryRefresh()
    if (refreshed) {
      res = await fetch(url, withAuth(opts))   // retry with new token
    } else {
      redirectToLogin()
    }
  }
  return res
}
```

---

## API Key Auth (separate from JWT)

```
Authorization: Bearer <api_key>

// Middleware:
1. Extract raw key from header
2. BLAKE2b hash it
3. Lookup in api_keys table by hash
4. Check active=true, expires_at, rate limits
```

Two auth systems coexist: API keys (for inference) + JWT sessions (for dashboard).
Never mix them — separate middleware layers.

---

## Anti-Patterns

| Anti-Pattern | Problem | Fix |
|-------------|---------|-----|
| JWT without `jti` | Cannot revoke individual sessions | Add `jti: UUIDv7` to claims |
| Store raw refresh token in DB | Exposure → account takeover | Store BLAKE2b hash |
| Long-lived access tokens | Window for replay after logout | Short expiry (15m) + Valkey revocation |
| Single-use refresh without rolling | Poor UX (logout on any 401) | Rolling refresh: revoke old + issue new |
| Checking JWT only (no blocklist) | Logout doesn't work until expiry | Check Valkey blocklist in middleware |

---

## Sources

- OWASP JWT cheat sheet: https://cheatsheetseries.owasp.org/cheatsheets/JSON_Web_Token_for_Java_Cheat_Sheet.html
- RFC 7519 (JWT), RFC 7617 (Bearer)
- Verified: `crates/veronex/src/infrastructure/inbound/http/middleware/jwt_auth.rs`
- Verified: `crates/veronex/src/infrastructure/outbound/persistence/session_repository.rs`
