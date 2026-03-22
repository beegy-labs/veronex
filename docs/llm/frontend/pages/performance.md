> CDD Layer 2 | **Last Updated**: 2026-03-21

# Web — Performance Page

## Layout

```
Header + TimeRangeSelector
KPI row (5 cards): P50 | P95 | P99 | Success Rate | Errors
Analytics row (4 cards): Avg TPS | Avg Prompt | Avg Completion | Total Requests  (ClickHouse)
Model Performance: table (model, provider, requests, avg latency, success%) + latency bar chart  (ClickHouse)
Key Performance: table (key, requests, success%, tokens, cost)  (PostgreSQL via usageBreakdown)
Chart: Avg Latency / Hour (LineChart + P95 reference line)
Chart: Throughput / Hour (BarChart: total/success/errors)
Chart: Avg TPS / Hour (LineChart — shown only when tps data present)
Chart: Error Rate / Hour (LineChart 0–100%)
```

## Key Files

| File | Purpose |
|------|---------|
| `web/app/performance/page.tsx` | Performance page (KPI cards + charts) |
| `web/app/performance/components/model-latency-section.tsx` | `ModelLatencySection` — model table + latency bar chart |
| `web/app/performance/components/key-performance-section.tsx` | `KeyPerformanceSection` — per-key table |
| `web/lib/queries/dashboard.ts` | `performanceQuery(hours)` — `GET /v1/dashboard/performance` |
| `web/lib/queries/usage.ts` | `usageBreakdownQuery(hours)` — reused for model/key breakdown |
| `web/lib/queries/usage.ts` | `analyticsQuery(hours)` — TPS + model success rates |
| `web/lib/types.ts` | `PerformanceStats`, `HourlyThroughput`, `ModelBreakdown`, `KeyBreakdown` |

## Data Sources

| Section | Source | Requires ClickHouse | PG Fallback |
|---------|--------|---------------------|-------------|
| KPI cards (P50/P95/P99) | `performanceQuery` → `GET /v1/dashboard/performance` | Yes | `PERCENTILE_CONT` on `inference_jobs` |
| Analytics KPIs (TPS) | `analyticsQuery` → `GET /v1/dashboard/analytics` | Yes | aggregates from `inference_jobs` |
| Model latency table | `usageBreakdownQuery.by_model` (avg_latency_ms) + `analyticsQuery.models` (success_rate) | Partial | Yes |
| Key performance table | `usageBreakdownQuery.by_key` | No | N/A (always PG) |
| Hourly charts | `performanceQuery.hourly` | Yes | hourly GROUP BY on `inference_jobs` |

All endpoints accept `from`/`to` ISO-8601 query parameters (see [usage.md](usage.md) "Date Range Parameters" section) in addition to the `hours` param. When `from` is set, `hours` is ignored.

All ClickHouse-dependent endpoints use **ClickHouse primary + PG fallback**: if ClickHouse returns empty results (`total_requests == 0`) or errors, the handler falls back to PostgreSQL `inference_jobs`.

## Model Performance Merge Logic

`modelPerfData` merges two sources:
- `usageBreakdownQuery.by_model[]` → `avg_latency_ms`, `request_count`, `provider_type`
- `analyticsQuery.models[]` → `success_rate` (matched by `model_name`)

Sorted ascending by `avg_latency_ms` (fastest first).

## Success Rate Scale

Backend returns `success_rate` on a **0-100 scale** (not 0-1). Frontend formatters:
- `fmtPct(n)` — renders `Math.round(n)%` directly (no `*100`)
- `successRateCls(rate)` — thresholds at 99/95 (not 0.99/0.95)
- `errorCount` — `total_requests - Math.round(success_rate / 100 * total_requests)`

## TPS Trend Computation

Per-hour TPS is estimated as:
```
tps = total_tokens / (avg_latency_ms / 1000 * request_count)
```
This is an approximation — shown only when `h.total_tokens > 0 && h.request_count > 0`.
The global `avg_tps` from `analyticsQuery` is the authoritative value.

## i18n Keys

```
performance.byModel        → "By Model" section header
performance.byKey          → "By Key" section header
performance.modelLatency   → "Model Avg Latency"
performance.keyPerformance → "Key Performance"
performance.keyCol         → "Key" (table column)
performance.tpsHour        → "Avg TPS / Hour" chart title
```
