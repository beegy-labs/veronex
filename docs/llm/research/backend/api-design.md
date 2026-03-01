# API Design — 2026 Research

> **Last Researched**: 2026-03-02 | **Source**: Implementation patterns + web search
> **Status**: ✅ Verified — all topics researched and documented

---

## Conventions (verified in this codebase)

### URL Structure

```
GET    /v1/resource              # list
POST   /v1/resource              # create
GET    /v1/resource/{id}         # get one
PATCH  /v1/resource/{id}         # partial update
DELETE /v1/resource/{id}         # delete

# Nested resources
GET    /v1/servers/{id}/metrics          # server-scoped resource
GET    /v1/ollama/backends/{id}/models   # sub-resource list
POST   /v1/ollama/models/sync            # action (verb as last segment)
```

### Error Shape

```json
{ "error": "human-readable message" }
```

All errors return `{ "error": "..." }` with appropriate HTTP status code.

### Pagination

```
GET /v1/dashboard/jobs?limit=50&offset=0&status=completed
```

Response includes `jobs: []` + `total: N`.

### Async Actions (202)

Long-running operations return 202 immediately:
```
POST /v1/ollama/models/sync   → 202 Accepted  { "message": "sync started" }
GET  /v1/ollama/sync/status   → { "status": "running" | "completed", "results": [...] }
```

---

---

## OpenAPI 3.1 Best Practices (2026)

### Nullable Fields

OpenAPI 3.0 used `nullable: true` (non-standard). **OpenAPI 3.1 uses JSON Schema `type` arrays:**

```yaml
# OLD (3.0 — DO NOT USE)
completed_at:
  type: string
  nullable: true

# NEW (3.1 — CORRECT)
completed_at:
  type: [string, "null"]
  format: date-time
```

### Discriminator for Polymorphic Types

Use `discriminator` + `oneOf` when a field type varies by a tag:

```yaml
components:
  schemas:
    BackendConfig:
      oneOf:
        - $ref: '#/components/schemas/OllamaConfig'
        - $ref: '#/components/schemas/GeminiConfig'
      discriminator:
        propertyName: backend_type
        mapping:
          ollama: '#/components/schemas/OllamaConfig'
          gemini: '#/components/schemas/GeminiConfig'
```

### Schema Annotations

Always include `description`, `example`, and `format`:

```yaml
api_key_id:
  type: [string, "null"]
  format: uuid
  description: "UUIDv7 of the API key used. Null for test runs."
  example: "0194e3b0-..."
```

### `$schema` Declaration

OpenAPI 3.1 is a valid JSON Schema 2020-12 dialect. Add at doc root:

```yaml
openapi: "3.1.0"
info:
  title: Veronex API
  version: "1.0.0"
```

**This codebase**: `openapi.json` is hand-maintained. When adding a new endpoint, update `openapi.json` alongside the router. The file is served at `GET /docs/openapi.json`.

---

## API Versioning Strategy

| Strategy | Example | Verdict |
|----------|---------|---------|
| URL path (current) | `/v1/chat/completions` | ✅ Use this — easy to debug, CDN-friendly, visible in logs |
| Header (`API-Version: 2`) | `POST /chat/completions` + header | ❌ Hard to test in browser, invisible in logs |
| Accept header | `Accept: application/vnd.api.v2+json` | ❌ Complex, rarely used in practice |

**Decision:** URL path versioning (`/v1/`) is the pragmatic 2026 choice for:
- OpenAI compatibility (`/v1/chat/completions`)
- Easy curl testing without header flags
- Reverse proxy routing by path prefix
- Clear changelog boundary (v1 → v2 = breaking change)

**This codebase:** All endpoints are `/v1/`. A v2 router would be merged alongside v1 in `router.rs`.

---

## Rate Limit Response Headers

When a rate limit is hit (429 Too Many Requests), return these standard headers:

```
HTTP/1.1 429 Too Many Requests
X-RateLimit-Limit: 60          # requests allowed per window
X-RateLimit-Remaining: 0       # remaining in current window
X-RateLimit-Reset: 1709481600  # Unix timestamp when window resets
Retry-After: 30                # seconds until retry is safe
Content-Type: application/json

{ "error": "rate limit exceeded" }
```

**This codebase:** `rate_limiter.rs` returns 429 with `{ "error": ... }` body. The `X-RateLimit-*` headers are not currently emitted — a future enhancement if clients need backoff info.

---

## Pagination: Offset vs Cursor

### Offset Pagination (current codebase)

```
GET /v1/dashboard/jobs?limit=50&offset=0
→ { jobs: [...], total: 450 }
```

**Pros:** Simple, supports random page access, easy to implement with SQL `LIMIT/OFFSET`.

**Cons:** Records shift if new data is inserted — page N may show duplicates or skip records. Degraded performance on large `OFFSET` values (full table scan to skip rows).

### Cursor Pagination (2026 best practice for large datasets)

```
GET /v1/dashboard/jobs?limit=50&after=0194e3b0-...  # UUIDv7 cursor
→ { jobs: [...], next_cursor: "0194e3c1-..." | null }
```

**Pros:** Stable (no duplicates/skips), O(log N) with index, scales to millions of rows.

**Cons:** No random page access, harder to implement.

**Decision for this codebase:**

| Table | Strategy | Reason |
|-------|----------|--------|
| `inference_jobs` | Offset (current) | Jobs page shows ≤50 recent; `total` needed for UI pagination |
| `audit_events` | Offset (current) | Same rationale |
| Future: high-volume stream | Cursor | Switch if `total` > 100k and performance degrades |

Cursor pagination is not needed yet — `OFFSET` on a 10k-row table with `created_at` index is fast enough.

---

## Sources

- Verified: `crates/inferq/src/infrastructure/inbound/http/`
- Web search: OpenAPI 3.1 best practices 2026, rate limit headers RFC, cursor pagination
