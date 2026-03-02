> **SSOT** | **Tier 2** | Last Updated: 2026-03-02

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
| `web/app/performance/page.tsx` | Performance page |
| `web/lib/queries/dashboard.ts` | `performanceQuery(hours)` — `GET /v1/dashboard/performance` |
| `web/lib/queries/usage.ts` | `usageBreakdownQuery(hours)` — reused for model/key breakdown |
| `web/lib/queries/usage.ts` | `analyticsQuery(hours)` — TPS + model success rates |
| `web/lib/types.ts` | `PerformanceStats`, `HourlyThroughput`, `ModelBreakdown`, `KeyBreakdown` |

## Data Sources

| Section | Source | Requires ClickHouse |
|---------|--------|---------------------|
| KPI cards (P50/P95/P99) | `performanceQuery` → `GET /v1/dashboard/performance` | ✅ Yes |
| Analytics KPIs (TPS) | `analyticsQuery` → `GET /v1/dashboard/analytics` | ✅ Yes |
| Model latency table | `usageBreakdownQuery.by_model` (avg_latency_ms) + `analyticsQuery.models` (success_rate) | Partial |
| Key performance table | `usageBreakdownQuery.by_key` | ❌ No |
| Hourly charts | `performanceQuery.hourly` | ✅ Yes |

## Model Performance Merge Logic

`modelPerfData` merges two sources:
- `usageBreakdownQuery.by_model[]` → `avg_latency_ms`, `request_count`, `backend`
- `analyticsQuery.models[]` → `success_rate` (matched by `model_name`)

Sorted ascending by `avg_latency_ms` (fastest first).

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
