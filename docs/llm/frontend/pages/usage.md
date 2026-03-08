# Web — Usage Page

> SSOT | Tier 2 | Last Updated: 2026-03-08

## Layout (Tabs)

KPI row (always visible) then tabs:

| Tab | Contents |
|-----|----------|
| `overview` | Global request+token trend (AreaChart, dual Y-axis: left=requests, right=tokens), token donut, analytics KPIs (TPS, avg tokens), finish reasons donut, model distribution bar |
| `by-key` | Key breakdown table (clickable), selected-key detail: hourly charts (dual Y-axis: left=prompt, right=completion tokens) + key model breakdown |
| `by-model` | Search input, model breakdown table, model latency horizontal bar |
| `by-provider` | Provider breakdown cards (2-col grid) |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/usage/page.tsx` | Usage page (KPI cards + 4-tab breakdown) |
| `web/app/usage/components/token-donut.tsx` | `TokenDonut` — prompt/completion donut chart |
| `web/app/usage/components/finish-reasons-card.tsx` | `FinishReasonsCard` — finish reason distribution |
| `web/app/usage/components/provider-breakdown.tsx` | `ProviderBreakdownSection` — per-provider cards |
| `web/app/usage/components/breakdown-tables.tsx` | `KeyBreakdownTable`, `ModelBreakdownTable` |
| `web/app/usage/components/model-latency-chart.tsx` | `ModelLatencyChart` — horizontal bar chart |
| `web/lib/types.ts` | `UsageBreakdown`, `ProviderBreakdown`, `KeyBreakdown`, `ModelBreakdown` |
| `web/lib/api.ts` | `usageAggregate()`, `usageBreakdown()` |
| `web/messages/en.json` | i18n keys under `usage.*` |

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add breakdown dimension | `usage_handlers.rs` | New query + struct + handler + route; update frontend types |
| Change default time window | `page.tsx` `?hours=` default | Also update `UsageQuery` default in `usage_handlers.rs` |
| Add cost column | `types.ts` + table component | Extend `ProviderBreakdown` / `KeyBreakdown` / `ModelBreakdown` |
| Add i18n key | `en.json` `usage.*` -> `ko.json` -> `ja.json` | Add to all 3 locales |

## API Endpoints

| Method | Path | Auth | Response |
|--------|------|------|----------|
| GET | `/v1/usage?hours=24` | JWT | `UsageAggregate` |
| GET | `/v1/usage/breakdown?hours=24` | JWT | `UsageBreakdownResponse` |
| GET | `/v1/usage/{key_id}?hours=24` | JWT | `HourlyUsage[]` |
| GET | `/v1/usage/{key_id}/jobs?hours=24` | JWT | `UsageJob[]` |
| GET | `/v1/usage/{key_id}/models` | JWT | `ModelBreakdown[]` |

Default: `hours=24`. Any positive integer supported.

## Response Types

### UsageAggregate

| Field | Type |
|-------|------|
| `request_count` | number |
| `success_count` | number |
| `cancelled_count` | number |
| `error_count` | number |
| `prompt_tokens` | number |
| `completion_tokens` | number |
| `total_tokens` | number |

Primary source: ClickHouse via `veronex-analytics`. Fallback: PostgreSQL `inference_jobs` (when ClickHouse returns empty results or is unavailable). No cost fields (cost computed from PostgreSQL via `model_pricing`).

### UsageBreakdownResponse

Top-level: `by_providers: ProviderBreakdown[]`, `by_key: KeyBreakdown[]`, `by_model: ModelBreakdown[]`, `total_cost_usd: number`

### ProviderBreakdown

| Field | Type | Notes |
|-------|------|-------|
| `provider_type` | string | |
| `request_count` | number | |
| `success_count` | number | |
| `error_count` | number | |
| `prompt_tokens` | number | |
| `completion_tokens` | number | |
| `success_rate` | number | 0-100, 1dp |
| `estimated_cost_usd` | number/null | |

### KeyBreakdown

| Field | Type | Notes |
|-------|------|-------|
| `key_id`, `key_name`, `key_prefix` | string | Key identifiers |
| `request_count`, `success_count` | number | |
| `prompt_tokens`, `completion_tokens` | number | |
| `success_rate` | number | 0-100 |
| `estimated_cost_usd` | number/null | |

### ModelBreakdown

| Field | Type | Notes |
|-------|------|-------|
| `model_name`, `provider_type` | string | |
| `request_count` | number | |
| `call_pct` | number | % of total reqs, 0-100, 1dp |
| `prompt_tokens`, `completion_tokens` | number | |
| `avg_latency_ms` | number | |
| `estimated_cost_usd` | number/null | |

Sourced from PostgreSQL (`inference_jobs` + `model_pricing` LATERAL JOIN) in `usage_handlers.rs::usage_breakdown()`.

## Cost Tracking

Token costs estimated at query time via LATERAL JOIN on `model_pricing`. No cost stored on `inference_jobs`. Full pricing schema: `docs/llm/inference/model-pricing.md`

### Cost Display Rules

| `estimated_cost_usd` | Meaning | UI |
|-----------------------|---------|----|
| `0.0` | Ollama (self-hosted) | "Free" |
| `> 0` | Gemini (input+output tokens x per-1M rate) | `$0.0000` (4dp) |
| `null` | No pricing row or tokens not recorded | "--" |

### Cost Fields per Breakdown

| Field | Source |
|-------|--------|
| `by_providers[].estimated_cost_usd` | Wildcard pricing (`model_name='*'`) aggregated per provider |
| `by_key[].estimated_cost_usd` | Exact-then-wildcard pricing, SUM per key |
| `by_model[].estimated_cost_usd` | Exact-then-wildcard pricing, SUM per model+provider |
| `total_cost_usd` | Sum of `by_providers[]` costs (nulls filtered). Shown as badge when > 0 |

Ollama: no `model_pricing` rows; provider cost short-circuits to `0.0`. Gemini: `cost = (prompt/1M x input_rate) + (completion/1M x output_rate)`, exact name first then `*` wildcard.

## i18n Keys

`usage.*`: estimatedCost, totalCost, free, hours24, hours72, hours168
