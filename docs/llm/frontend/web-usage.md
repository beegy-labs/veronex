> **SSOT** | **Tier 2** | Last Updated: 2026-03-02

# Web — Usage Page

## Layout (Tabs)

The page is structured as: **KPI row** (always visible) → **Tabs**

| Tab | Contents |
|-----|----------|
| `overview` | Global request+token trend (AreaChart) · Token donut · Analytics KPIs (TPS, avg tokens) · Finish reasons donut · Model distribution bar chart |
| `by-key` | Key breakdown table (clickable rows) · Selected-key detail: hourly charts (tokens, requests) + key model breakdown table |
| `by-model` | Search input · Model breakdown table · Model latency horizontal bar chart |
| `by-provider` | Backend breakdown cards (2-col grid) |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/usage/page.tsx` | Usage page — KPI cards + 4-tab breakdown |
| `web/lib/types.ts` | `UsageBreakdown`, `BackendBreakdown`, `KeyBreakdown`, `ModelBreakdown` types |
| `web/lib/api.ts` | `api.usageAggregate()`, `api.usageBreakdown()` |
| `web/messages/en.json` | i18n keys under `usage.*` |

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new breakdown dimension | `usage_handlers.rs` new query + new struct | Add response type, handler, route; update frontend types |
| Change the default time window | `web/app/usage/page.tsx` `?hours=` default | Also update `UsageQuery` default in `usage_handlers.rs` |
| Add new cost column to breakdown table | `web/lib/types.ts` + table component | Extend `BackendBreakdown` / `KeyBreakdown` / `ModelBreakdown` |
| Add new i18n key | `web/messages/en.json` `usage.*` → `ko.json` → `ja.json` | Always add to all 3 locales |

---

## API Endpoints

```
GET /v1/usage?hours=24
    Authorization: Bearer <JWT>
    → UsageAggregate

GET /v1/usage/breakdown?hours=24
    Authorization: Bearer <JWT>
    → UsageBreakdownResponse

GET /v1/usage/{key_id}?hours=24
    Authorization: Bearer <JWT>
    → HourlyUsage[]

GET /v1/usage/{key_id}/jobs?hours=24
    Authorization: Bearer <JWT>
    → UsageJob[]
```

Default window: `hours=24`. Supported values: any positive integer (e.g. `72` = 3 days).

---

## Response Types

### `UsageAggregate`

```typescript
interface UsageAggregate {
  request_count:     number
  success_count:     number
  cancelled_count:   number
  error_count:       number
  prompt_tokens:     number
  completion_tokens: number
  total_tokens:      number
}
```

Sourced from ClickHouse via `veronex-analytics`. Does not include cost fields (cost is computed from PostgreSQL token counts via `model_pricing`).

### `UsageBreakdownResponse`

```typescript
interface UsageBreakdownResponse {
  by_backend: BackendBreakdown[]
  by_key:     KeyBreakdown[]
  by_model:   ModelBreakdown[]
  total_cost_usd: number          // sum of all backend costs for the window; 0.0 when no pricing
}

interface BackendBreakdown {
  backend:              string
  request_count:        number
  success_count:        number
  error_count:          number
  prompt_tokens:        number
  completion_tokens:    number
  success_rate:         number    // 0–100 (rounded to 1dp)
  estimated_cost_usd:   number | null
}

interface KeyBreakdown {
  key_id:             string
  key_name:           string
  key_prefix:         string
  request_count:      number
  success_count:      number
  prompt_tokens:      number
  completion_tokens:  number
  success_rate:       number
  estimated_cost_usd: number | null
}

interface ModelBreakdown {
  model_name:         string
  backend:            string
  request_count:      number
  call_pct:           number      // percentage of total requests (0–100, rounded to 1dp)
  prompt_tokens:      number
  completion_tokens:  number
  avg_latency_ms:     number
  estimated_cost_usd: number | null
}
```

Sourced from PostgreSQL (`inference_jobs` + `model_pricing` LATERAL JOIN). Queried directly in `usage_handlers.rs::usage_breakdown()`.

---

## Cost Tracking

Token costs are estimated at query time via a LATERAL JOIN on the `model_pricing` PostgreSQL table. No cost is stored on `inference_jobs`.

For full pricing table schema and LATERAL JOIN logic, see: `docs/llm/backend/model-pricing.md`

### Cost Fields in `UsageBreakdownResponse`

| Field | Source | Display |
|-------|--------|---------|
| `by_backend[].estimated_cost_usd` | Wildcard-only pricing (`model_name = '*'`) aggregated across all jobs for that provider | "Free" (0.0) or `$X.XXXX` shown at bottom of backend card |
| `by_key[].estimated_cost_usd` | Exact-then-wildcard pricing per job row, SUM per key | "—" (null), "Free" (0.0), or `$X.XXXX` table column |
| `by_model[].estimated_cost_usd` | Exact-then-wildcard pricing per job row, SUM per model+backend | same pattern as key column |
| `total_cost_usd` | Sum of `by_backend[].estimated_cost_usd` (filters nulls) | `$X.XXXX` badge in breakdown card header — shown only when > 0 |

### `estimated_cost_usd` Semantics

| Value | Meaning | UI rendering |
|-------|---------|--------------|
| `0.0` | Ollama (self-hosted, no cost) | "Free" |
| `> 0` | Gemini — input + output tokens × per-1M rate | `$0.0000` (4dp) |
| `null` | No pricing row found, or tokens not yet recorded | "—" |

### Ollama Always Shows $0.00

Ollama backends have no rows in `model_pricing`. The backend cost expression short-circuits to `0.0` for any job where `j.backend = 'ollama'`, regardless of model name. The UI renders this as "Free".

### Gemini Cost Computation

For `by_key` and `by_model` breakdowns, the cost is computed per job row:

```
cost = (prompt_tokens / 1,000,000 × input_per_1m)
     + (completion_tokens / 1,000,000 × output_per_1m)
```

The pricing lookup uses exact model name first, then `'*'` wildcard. Results are summed (`SUM(CASE ...)`) in the GROUP BY query.

### `total_cost_usd`

Always a `number` (never null). Computed server-side as:

```rust
let total_cost_usd: f64 = by_backend.iter()
    .filter_map(|b| b.estimated_cost_usd)
    .sum();
```

Backends with `null` cost (unknown providers) are excluded from the sum.

---

## i18n Keys (`messages/en.json`)

```json
"usage": {
  "estimatedCost",   // "Est. Cost"  — table column header
  "totalCost",       // "Total Cost"  — breakdown card header badge label
  "free",            // "Free"  — displayed for Ollama $0.00 breakdown entries
  "hours24",         // "Last 24h"
  "hours72",         // "Last 3 days"
  "hours168"         // "Last 7 days"
}
```
