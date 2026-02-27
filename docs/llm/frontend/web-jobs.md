# Web — Jobs, Usage & Performance Pages

> SSOT | **Last Updated**: 2026-02-27

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new column to jobs table | `web/components/job-table.tsx` columns + `web/lib/types.ts` `JobSummary` | Add column def + i18n key `jobs.*` |
| Add new status filter option | `web/components/job-table.tsx` status filter options | Matches `JobStatus` enum on backend |
| Add new stat card to overview | `web/app/overview/page.tsx` + `infrastructure/inbound/http/handlers.rs` `dashboard_stats()` | Extend SQL query + response struct + card in TSX |
| Change duration format breakpoints | `web/components/job-table.tsx` `formatDuration()` | Change ms thresholds (`< 1000`, `< 60000`) |
| Add new chart to performance page | `web/app/performance/page.tsx` + ClickHouse SQL in `handlers.rs` `dashboard_performance()` | Extend `PerformanceStats` struct + add Recharts component |
| Add new i18n key to jobs | `web/messages/en.json` `jobs.*` → `web/messages/ko.json` → `web/messages/ja.json` | Always add to all 3 locales |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/jobs/page.tsx` | Jobs list page |
| `web/components/job-table.tsx` | Jobs table + detail modal (fully i18n) |
| `web/app/overview/page.tsx` | Stats + request trend + jobs-by-status chart |
| `web/app/usage/page.tsx` | Token consumption charts (ClickHouse) |
| `web/app/performance/page.tsx` | P50/P95/P99 latency charts (ClickHouse) |
| `web/lib/api.ts` | `api.jobs()`, `api.jobDetail()`, `api.stats()`, `api.usage()`, `api.performance()` |
| `web/messages/en.json` | i18n keys under `jobs.*`, `overview.*`, `usage.*`, `performance.*` |

---

## /jobs — Jobs List (job-table.tsx)

### Columns

```
[Search prompt…]  [Status: All ▼]

┌──────────────────────────────────────────────────────────────────────┐
│ ID      Model    Backend  API Key   Status     Created   TTFT  Latency│
│ 3a9f…  llama3   gpu-1    dev-key   ✓complete  Feb 25   142ms  1.2s  │
│ 8b2c…  gemini   cloud    prod-key  ✓complete  Feb 25   380ms  2m 3s │
└──────────────────────────────────────────────────────────────────────┘
```

- Status filter: all | pending | running | completed | failed | cancelled
- Prompt search: `q=` parameter → `prompt ILIKE '%{q}%'`
- Pagination: limit/offset

### Duration Format — `formatDuration(ms)`

| Range | Format |
|-------|--------|
| < 1000ms | `"842ms"` |
| 1000ms – 60s | `"2.3s"` |
| ≥ 60s | `"2m 5s"` |

### Job Detail Modal (job-table.tsx)

```
┌─ 3a9fbcd… · ✓completed ──────────────────────────────────────────────┐
│ llama3 · gpu-ollama-1                                                  │
│ Created: Feb 25, 14:32  Started: 14:32  Completed: 14:32              │
│ Latency: 1.2s  TTFT: 142ms  TPS: 44.3 tok/s                          │
│ Tokens: 53  API Key: dev-key                                           │
├────────────────────────────────────────────────────────────────────────┤
│ PROMPT                                                                  │
│ <prompt text>                                                           │
├────────────────────────────────────────────────────────────────────────┤
│ RESULT                                                                  │
│ <result text>                                                           │
└────────────────────────────────────────────────────────────────────────┘
```

- `useQuery({ queryKey: ['job-detail', jobId], enabled: !!jobId && open })`
- Prompt/Result: `<pre>` + monospace + `whitespace-pre-wrap` + `max-h-52 overflow-y-auto`
- TPS displayed when `completion_tokens` and `latency_ms - ttft_ms` are available

---

## /overview — Dashboard Overview (overview/page.tsx)

Stats cards: total keys, active keys, total jobs, jobs last 24h
Charts:
- Request trend (line chart, hourly) — ClickHouse `inference_logs`
- Jobs by status (bar chart) — Postgres `inference_jobs`
- Model usage breakdown

Query key: `['stats']`

---

## /usage — Token Consumption (usage/page.tsx)

- Aggregate: `GET /v1/usage?hours=24` → total tokens, requests by model
- Per-key: `GET /v1/usage/{key_id}?hours=24` → hourly breakdown
- Charts: stacked bar (prompt vs completion tokens per hour) — Recharts
- All colors from `var(--theme-*)` tokens

Query keys: `['usage']`, `['usage', keyId]`

---

## /performance — Latency Analytics (performance/page.tsx)

- `GET /v1/dashboard/performance?hours=24`
- Shows: avg, P50, P95, P99 latency; success rate; total tokens; hourly throughput
- Charts: line chart (latency percentiles), bar chart (throughput)
- Use `var(--theme-primary)`, `var(--theme-text-secondary)` for chart colors

Query key: `['performance']`

---

## i18n Keys (messages/en.json)

### jobs.*
```json
"title", "search", "statusFilter", "id", "model", "backend", "apiKey",
"status", "created", "ttft", "latency", "tps", "tokens",
"pending", "running", "completed", "failed", "cancelled",
"detailTitle", "prompt", "result", "started", "completedAt", "error",
"noJobs", "loadingJobs"
```

### overview.*
```json
"title", "totalKeys", "activeKeys", "totalJobs", "last24h",
"requestTrend", "jobsByStatus", "modelUsage"
```

### usage.*
```json
"title", "totalTokens", "totalRequests", "promptTokens", "completionTokens",
"byHour", "byModel", "selectKey"
```

### performance.*
```json
"title", "avgLatency", "p50", "p95", "p99", "successRate",
"totalTokens", "throughput", "hourly"
```
