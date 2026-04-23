# LLM Gateway Security Audit (OWASP API + LLM 2025)

> ADD Execution — P0/P1/P2 security greps | **Last Updated**: 2026-04-22
> Parent: `best-practices.md`. Expensive resource = GPU time + model slots; every check evaluates monopoly risk.


The expensive resource in an LLM gateway is GPU time and model slots, not CPU or memory.
Every check evaluates: can an attacker monopolize the GPU fleet cheaply?

### P0 — GPU Slot Monopoly / Memory DoS (always run)

```bash
# HTTP body size limit — without DefaultBodyLimit, a 500MB JSON payload is fully buffered in memory
grep -rn "DefaultBodyLimit\|RequestBodyLimitLayer" crates/veronex/src/

# max_tokens server-side cap — passing client value uncapped to upstream allows GPU monopoly
grep -rn "max_tokens" crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs | grep -v "//\|clamp\|min\|MAX"

# messages array length cap — unbounded messages array = context bomb
grep -rn "messages.*len()\|MAX_MESSAGES" crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs
```

### P1 — Slot Exhaustion / Header Hardening (run when changing infra/handlers)

```bash
# Per-key concurrent connection limit — RPM alone cannot defend against Slowloris
grep -rn "concurrent\|semaphore\|in_flight" crates/veronex/src/infrastructure/inbound/http/middleware/

# SSE streaming timeout — CancelOnDrop alone is insufficient
grep -rn "timeout\|Duration" crates/veronex/src/infrastructure/inbound/http/streaming.rs

# Response header hardening
grep -rn "nosniff\|no-store\|X-Frame-Options\|X-Content-Type" crates/veronex/src/

# Global router timeout
grep -rn "TimeoutLayer\|tower_http::timeout" crates/veronex/src/main.rs

# MCP tool call argument exfiltration — user data must not appear in outbound tool call URLs
grep -rn "format!.*namespaced\|format!.*tool_name\|format!.*args" crates/veronex/src/infrastructure/outbound/mcp/bridge.rs
```

### P2 — Defense in Depth (run during security review)

```bash
# system message override — check if client system messages can overwrite tenant prompts
grep -rn '"system"\|role.*system' crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs

# Internal error exposure — check if upstream Ollama/Gemini errors are forwarded verbatim to clients
grep -rn "e\.to_string()\|err\.to_string()\|error.*format!" crates/veronex/src/infrastructure/inbound/http/ | grep -v "//\|tracing\|warn\|debug"

# JSON injection — format!() for JSON assembly (must use serde_json::json! instead)
grep -rn 'format!.*\\"error\\"' crates/veronex/src/

# Log injection — user input interpolated via format!() in tracing fields
grep -rn 'tracing::.*format!' crates/veronex/src/
```

### Completed (reference only)

| Item | Date |
|------|------|
| SQL injection (sqlx parameterized) | baseline |
| API key hashing (BLAKE2b-256) | baseline |
| Password hashing (Argon2id) | baseline |
| SSRF defense (provider URL validation) | baseline |
| Header injection (cookie sanitize) | 2026-03-28 |
| Prompt injection (JSON safe build) | 2026-03-28 |
| XSS (mermaid SVG strip) | 2026-03-28 |
| Index naming consistency (idx_ prefix) | 2026-03-28 |
| Missing FK indexes (4 added) | 2026-03-28 |

---

