> **SSOT** | **Tier 2** | Last Updated: 2026-03-02

# Model Pricing

Token-level cost estimation for inference jobs. Costs are computed at query time via a PostgreSQL LATERAL JOIN — no cost is stored in `inference_jobs` itself.

## Table Schema (migration 000047)

```sql
CREATE TABLE model_pricing (
    provider      TEXT    NOT NULL,
    model_name    TEXT    NOT NULL,   -- exact model name OR '*' for wildcard fallback
    input_per_1m  FLOAT8  NOT NULL DEFAULT 0,  -- USD per 1M prompt tokens
    output_per_1m FLOAT8  NOT NULL DEFAULT 0,  -- USD per 1M completion tokens
    currency      TEXT    NOT NULL DEFAULT 'USD',
    notes         TEXT,
    PRIMARY KEY (provider, model_name)
);
```

- `provider` matches `inference_jobs.backend` (e.g. `'gemini'`, `'ollama'`).
- `model_name` is either an exact model name (e.g. `'gemini-2.0-flash'`) or `'*'` as a default fallback when no exact match exists.
- Lookup priority: **exact name first**, then `'*'` wildcard.

## Ollama — Always $0.00

Ollama has **no rows** in `model_pricing`. The cost expression short-circuits:

```sql
CASE
    WHEN j.backend = 'ollama' THEN 0.0
    ...
END
```

Self-hosted inference has no per-token API cost. The UI displays `"$0.00 (self-hosted)"`.

## Gemini — Seeded Pricing (2026-03)

| Model | Input / 1M tokens | Output / 1M tokens |
|-------|------------------:|-------------------:|
| `gemini-2.0-flash` | $0.10 | $0.40 |
| `gemini-2.0-flash-lite` | $0.075 | $0.30 |
| `gemini-2.0-flash-thinking-exp` | $0.10 | $0.40 |
| `gemini-2.0-flash-thinking-exp-01-21` | $0.10 | $0.40 |
| `gemini-2.0-pro-exp` | $1.25 | $10.00 |
| `gemini-2.5-pro-preview-03-25` | $1.25 | $10.00 |
| `gemini-1.5-flash` | $0.075 | $0.30 |
| `gemini-1.5-flash-8b` | $0.0375 | $0.15 |
| `gemini-1.5-pro` | $1.25 | $5.00 |
| `gemini-1.0-pro` | $0.50 | $1.50 |
| `*` (wildcard fallback) | $0.10 | $0.40 |

Source: Google AI Studio pricing, March 2026.

## LATERAL JOIN Pattern

Used in every query that returns cost fields:

```sql
LEFT JOIN LATERAL (
    SELECT input_per_1m, output_per_1m
    FROM model_pricing
    WHERE provider = j.backend
      AND (model_name = j.model_name OR model_name = '*')
    ORDER BY CASE WHEN model_name = j.model_name THEN 0 ELSE 1 END
    LIMIT 1
) pricing ON true
```

The `ORDER BY CASE` ensures exact model name wins over the `'*'` wildcard. The `LEFT JOIN ... ON true` pattern means `pricing.*` is `NULL` when no row matches (unknown provider).

## Cost Expression

```sql
CASE
    WHEN j.backend = 'ollama' THEN 0.0
    WHEN pricing.input_per_1m IS NOT NULL
         AND j.prompt_tokens IS NOT NULL
         AND j.completion_tokens IS NOT NULL THEN
        (j.prompt_tokens::float8 / 1000000.0 * pricing.input_per_1m) +
        (j.completion_tokens::float8 / 1000000.0 * pricing.output_per_1m)
    ELSE NULL
END AS estimated_cost_usd
```

Result semantics:

| Value | Meaning |
|-------|---------|
| `0.0` | Ollama (self-hosted, no cost) |
| `> 0` | Gemini — computed from token counts × per-1M rate |
| `NULL` | No pricing data (unknown provider, or tokens not yet recorded) |

## Endpoints That Return Cost Fields

### Job List and Detail

`GET /v1/dashboard/jobs` → `JobSummary[]`
- `estimated_cost_usd: number | null` — per-job cost; `0.0` for Ollama

`GET /v1/dashboard/jobs/{id}` → `JobDetail`
- `estimated_cost_usd: number | null` — same computation as job list

### Usage Breakdown

`GET /v1/usage/breakdown` → `UsageBreakdownResponse`

| Field | Description |
|-------|-------------|
| `by_backend[].estimated_cost_usd` | Aggregate cost for that provider over the window |
| `by_key[].estimated_cost_usd` | Aggregate cost per API key (per-job SUM, exact model lookup) |
| `by_model[].estimated_cost_usd` | Aggregate cost per model+backend combination |
| `total_cost_usd` | Sum of all backend costs (scalar, always a number) |

Note: `by_backend` uses the `'*'` wildcard only (no per-model lookup). `by_key` and `by_model` use the full exact-then-wildcard lookup per job row.

## NULL Handling

- `estimated_cost_usd` is `Option<f64>` in Rust / `number | null` in TypeScript.
- It is `NULL` when either: (a) no pricing row matches the provider, or (b) `prompt_tokens` or `completion_tokens` is `NULL` (job not yet completed).
- `total_cost_usd` in `UsageBreakdownResponse` is always a `f64` (defaults to `0.0` when no backends have pricing).

## Updating Pricing

Insert or `UPDATE` rows directly in `model_pricing`. No application restart required — the LATERAL JOIN reads current table state on every query. To add a new provider:

1. Insert rows with `provider = '<new_provider>'` and appropriate model names.
2. Add a `'*'` wildcard row as a fallback for unrecognized model names.
3. The Ollama `CASE` guard is hardcoded; other providers automatically pick up pricing rows.

## Related Docs

- Backend jobs SSOT: `docs/llm/backend/jobs.md` — Token Cost Measurement section
- Frontend cost display: `docs/llm/frontend/web-usage.md` — Cost Tracking section
- Frontend jobs UI: `docs/llm/frontend/web-jobs.md` — Extended Job Fields section
