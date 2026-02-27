# Backends — Gemini: Rate Limiting & Tier Routing

> SSOT | **Last Updated**: 2026-02-27

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Change default RPM/RPD for a model | API: `PUT /v1/gemini/policies/{model}` | Or edit seed in `migrations/000017_*.sql` |
| Add new Gemini tier routing logic | `crates/inferq/src/infrastructure/outbound/backend_router.rs` | `pick_gemini_backend()` function |
| Change `available_on_free_tier` behavior | `backend_router.rs` | `pick_gemini_backend()` → early return block |
| Add new Valkey counter key | `crates/inferq/src/infrastructure/inbound/http/backend_handlers.rs` | rate counter increment after job |
| Add field to GeminiRateLimitPolicy | `domain/entities/` + migration + `persistence/` + `backend_handlers.rs` `UpsertGeminiPolicyRequest` |

## Key Files

| File | Purpose |
|------|---------|
| `crates/inferq/src/domain/entities/gemini_rate_limit_policy.rs` | `GeminiRateLimitPolicy` entity |
| `crates/inferq/src/application/ports/outbound/` | `GeminiPolicyRepository` trait |
| `crates/inferq/src/infrastructure/outbound/persistence/` | `PostgresGeminiPolicyRepository` |
| `crates/inferq/src/infrastructure/outbound/gemini/adapter.rs` | `GeminiAdapter` (streaming) |
| `crates/inferq/src/infrastructure/outbound/backend_router.rs` | `pick_gemini_backend()` |
| `crates/inferq/src/infrastructure/inbound/http/backend_handlers.rs` | Policy CRUD handlers |

---

## GeminiRateLimitPolicy Entity

```rust
pub struct GeminiRateLimitPolicy {
    pub id: Uuid,
    pub model_name: String,           // "gemini-2.5-flash" | "*" (global fallback)
    pub rpm_limit: i32,               // 0 = not enforced
    pub rpd_limit: i32,               // 0 = not enforced
    pub available_on_free_tier: bool, // false → skip free, route to paid directly
    pub updated_at: DateTime<Utc>,
}
```

`model_name = "*"` = global fallback when no model-specific policy exists.

## DB Schema

```sql
CREATE TABLE gemini_rate_limit_policies (
    id                     UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    model_name             VARCHAR(255) NOT NULL UNIQUE, -- "*" = global fallback
    rpm_limit              INTEGER      NOT NULL DEFAULT 0,
    rpd_limit              INTEGER      NOT NULL DEFAULT 0,
    available_on_free_tier BOOLEAN      NOT NULL DEFAULT true,
    updated_at             TIMESTAMPTZ  NOT NULL DEFAULT now()
);
-- migrations: 000017 CREATE + seed, 000019 available_on_free_tier
```

---

## API Endpoints

```
GET  /v1/gemini/policies              → Vec<GeminiPolicySummary>
PUT  /v1/gemini/policies/{model}      UpsertGeminiPolicyRequest → GeminiPolicySummary
```

### UpsertGeminiPolicyRequest

```rust
pub struct UpsertGeminiPolicyRequest {
    pub rpm_limit: i32,
    pub rpd_limit: i32,
    pub available_on_free_tier: bool,
}
```

---

## Core Routing Concept

Rate limits are per **Google Cloud project** — all keys from same project share one pool.
To roll across accounts: register keys from **different Google projects** as separate `LlmBackend` entries.
All `is_free_tier=true` backends share one policy per model.

### `available_on_free_tier` Flag

```
true (default):
  → Route through free backends in RPM/RPD order
  → On exhaustion → paid fallback (unless tier_filter="gemini-free")

false:
  → Skip free backends entirely, route direct to paid
  → RPM/RPD counters NOT incremented
```

### pick_gemini_backend() Sequence (backend_router.rs)

```
1. policy.available_on_free_tier=false → paid direct
   (tier_filter="gemini-free" → error: model not available on free tier)

2. Iterate free backends (is_free_tier=true):
   - RPD exhausted → skip
   - RPM exhausted, RPD OK → wait up to next minute (max 3 retries)
   - Both OK → use this backend, increment counters after job

3. All free RPD exhausted → paid fallback
   (tier_filter="gemini-free" → error: all free tiers exhausted)

4. Valkey unavailable → fail-open (use first available)
```

---

## Valkey Counter Keys

```
veronex:gemini:rpm:{backend_id}:{model}:{minute}     TTL=120s
veronex:gemini:rpd:{backend_id}:{model}:{YYYY-MM-DD} TTL=90000s
```

Counters incremented AFTER job completes, only for `is_free_tier=true` backends.

---

## Per-Model Policy Lookup (SQL)

```sql
SELECT * FROM gemini_rate_limit_policies
WHERE model_name = $1 OR model_name = '*'
ORDER BY CASE WHEN model_name = $1 THEN 0 ELSE 1 END
LIMIT 1
```

---

## 2026 Free Tier Default Limits (seeded in migration 000017)

| Model | RPM | RPD |
|-------|-----|-----|
| gemini-2.5-pro | 5 | 100 |
| gemini-2.5-flash | 10 | 250 |
| gemini-2.5-flash-lite | 15 | 1,000 |

Change via admin web `/backends?s=gemini` — no code change needed.

---

## `backend` Field → Tier Filter Mapping

| `backend` field value | `tier_filter` | Behavior |
|----------------------|--------------|----------|
| `"gemini-free"` | `Some("free")` | Free backends only, paid_backends = [] |
| `"gemini"` | `None` | Auto (free-first, paid-fallback) |

---

## Web UI

→ See `docs/llm/frontend/web-backends.md` → GeminiTab + GeminiSyncSection
