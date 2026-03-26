# Authentication Flows

> **Last Updated**: 2026-03-26

---

## API Key Auth (`InferCaller` / management endpoints)

```
Incoming request
  │
  ├── Extract key from headers (priority order):
  │     1. X-API-Key: <key>
  │     2. Authorization: Bearer <key>
  │     3. x-goog-api-key: <key>
  │
  ▼
BLAKE2b-256 hash(key)
  │
  ▼
SELECT * FROM api_keys
  WHERE key_hash = ? AND deleted_at IS NULL
  │
  ├── Not found         → 401 Unauthorized
  ├── is_active = false → 401 Unauthorized
  ├── expires_at < now  → 401 Unauthorized
  └── OK → ApiKey entity injected via axum Extension
       │
       ▼
  Rate limit check (Valkey)
       │
       ├── RPM: ZADD + ZCOUNT window=60s
       │     count ≥ rpm_limit → 429 Too Many Requests
       │
       ├── TPM: INCR counter key=tpm:{key_id}:{minute}
       │     count + estimated_tokens ≥ tpm_limit → 429
       │
       └── Valkey error → 503 (fail-closed)
       │
       ▼
  Handler executes
       │
       ▼
  TPM update (post-completion): INCR by actual completion_tokens
```

---

## JWT Session Auth (admin / settings endpoints)

```
POST /auth/login
  │
  ├── lookup account by username
  ├── bcrypt verify(password, hash)
  │     └── fail → 401
  │
  └── issue JWT (HS256, exp=configured TTL)
        payload: { account_id, role, session_id }
        │
        ▼
  Client stores JWT

Subsequent requests
  │  Authorization: Bearer <jwt>
  ▼
jwt_auth middleware
  │
  ├── verify HS256 signature (JWT_SECRET env)
  ├── check exp claim
  │     └── expired → 401
  ├── check role claim against RequireXxx extractor
  │     └── insufficient role → 403
  └── Claims injected via axum Extension
```

---

## `InferCaller` — Dual Auth (inference endpoints)

```
POST /v1/chat/completions  /api/chat  /api/generate
  │
  ▼
infer_auth::InferCaller extractor
  │
  ├── Try JWT Bearer first
  │     └── valid JWT → InferCaller::Session { account_id, role }
  │           MCP ACL: None (bypass — session users have full access)
  │
  └── Try API Key
        └── valid API key → InferCaller::ApiKey { key_id, tier, ... }
              MCP ACL: Some(HashSet<server_id>) from mcp_key_access
              Provider ACL: from api_key_provider_access
```

---

## API Key Provider ACL

```
Dispatch time: select_provider()
  │
  ├── fetch allowed provider IDs for key (api_key_provider_access)
  │     ├── No rows for key → all providers allowed (default allow-all)
  │     └── Rows exist      → only is_allowed=true providers routable
  │
  └── filter candidate providers by allowlist
```

---

## API Key MCP ACL

```
bridge.run_loop() — called per inference request
  │
  ├── API key caller?
  │     └── SELECT server_id FROM mcp_key_access
  │           WHERE api_key_id = ? AND is_allowed = true
  │           │
  │           ├── 0 rows → Some({}) — empty set — deny all MCP servers
  │           └── N rows → Some({id1, id2, ...}) — allowlist
  │
  └── JWT session caller?
        └── None — bypass ACL entirely
  │
  ▼
tool_cache.get_all(allowed_servers)
  └── filters tool list to only allowed servers
      empty allowlist → no tools injected → MCP bridge inactive
```

---

## Valkey Rate Limit Key Schema

```
RPM:  veronex:ratelimit:rpm:{key_id}
      Type: sorted set  |  TTL: 62s  |  Score: unix_ms

TPM:  veronex:ratelimit:tpm:{key_id}:{minute}
      Type: string (counter)  |  TTL: 120s  |  Unit: tokens
```

---

## Files

| File | Purpose |
|------|---------|
| `infrastructure/inbound/http/middleware/api_key_auth.rs` | API key extraction + DB lookup |
| `infrastructure/inbound/http/middleware/jwt_auth.rs` | JWT verification + role extractors |
| `infrastructure/inbound/http/middleware/infer_auth.rs` | `InferCaller` dual-auth extractor |
| `infrastructure/inbound/http/middleware/rate_limiter.rs` | RPM/TPM Valkey rate limiting |
| `infrastructure/inbound/http/key_mcp_access_handlers.rs` | MCP ACL management REST API |
| `infrastructure/inbound/http/key_provider_access_handlers.rs` | Provider ACL management REST API |
