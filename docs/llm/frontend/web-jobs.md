# Web — Jobs Page

> SSOT | **Last Updated**: 2026-02-28 (rev: tab layout + key name search)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new column to jobs table | `web/components/job-table.tsx` columns + `web/lib/types.ts` `Job` | Add column def + i18n key `jobs.*` |
| Add new status filter option | `web/app/jobs/page.tsx` `STATUS_OPTIONS` in `JobsSection` | Matches `JobStatus` enum on backend |
| Change pagination page size | `web/app/jobs/page.tsx` → `PAGE_SIZE` constant | |
| Add new i18n key to jobs | `web/messages/en.json` `jobs.*` → `web/messages/ko.json` → `web/messages/ja.json` | Always add to all 3 locales |
| Change duration format breakpoints | `web/components/job-table.tsx` `formatDuration()` | Change ms thresholds |
| Add another tab to the Jobs page | `web/app/jobs/page.tsx` → add `<TabsTrigger>` + `<TabsContent>` + new `<JobsSection source="...">` | Extend `JobsSectionProps.source` type |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/jobs/page.tsx` | Jobs page — tabs with `JobsSection` + `ApiTestPanel` |
| `web/components/job-table.tsx` | Jobs table + detail modal (fully i18n) |
| `web/lib/types.ts` | `Job`, `JobDetail` types (include `source` field) |
| `web/lib/api.ts` | `api.jobs()`, `api.jobDetail()` |
| `web/messages/en.json` | i18n keys under `jobs.*` |

## `source` Field

The `Job` and `JobDetail` types carry `source: 'api' | 'test'`. This field is set by the backend at job creation time based on the Valkey queue used:

| Value | Queue | Meaning |
|-------|-------|---------|
| `'api'`  | `veronex:queue:jobs` | Submitted via the OpenAI-compatible API |
| `'test'` | `veronex:queue:jobs:test` | Submitted via the web Test panel |

The Jobs page filters by source (`?source=api` / `?source=test`) per tab. The Overview page recent-jobs mini-table shows all sources without filtering.

---

## /jobs — Tab Layout

The Jobs page is split into two tabs managed by `activeTab: 'test' | 'api'` state:

```
┌─ Page header ─────────────────────────────────────────────────────────┐
│  Jobs  (subtitle)                                                       │
├─ [API Jobs] [Test Runs]  ← shadcn/ui Tabs (API Jobs is default)       │
├───────────────────────────────────────────────────────────────────────┤
│ Tab: "API Jobs"  (default)                                              │
│   [search] [status filter]      ← queries ?source=api                  │
│   <JobTable>   pagination                                               │
├───────────────────────────────────────────────────────────────────────┤
│ Tab: "Test Runs"                                                        │
│   <ApiTestPanel> — submits with source=test                             │
│   ── border-t ──                                                        │
│   [search] [status filter]      ← queries ?source=test                  │
│   <JobTable>   pagination                                               │
└────────────────────────────────────────────────────────────────────────┘
```

### `handleRetry` — Cross-Tab Navigation

When the user clicks ▶ Retry on a job in either tab:
1. `setActiveTab('test')` — switches to the Test Runs tab
2. `setTimeout(50ms)` → `testPanelRef.current?.scrollIntoView()` — scrolls to ApiTestPanel
3. `setRetryParams(params)` — pre-fills panel with the job's model + prompt + backend

```tsx
function handleRetry(params: RetryParams) {
  setRetryParams(params)
  setActiveTab('test')
  setTimeout(() => {
    testPanelRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }, 50)
}
```

### `JobsSection` Component (`jobs/page.tsx`)

Reusable component, rendered once per tab (no `title` prop — tabs provide the label):

```tsx
// Test Runs tab
<JobsSection source="test" onRetry={handleRetry} />

// API Jobs tab
<JobsSection source="api" onRetry={handleRetry} />
```

Each section maintains independent state:
- `page`, `status`, `search`, `query` — no shared state between tabs
- Query key: `['dashboard-jobs', source, page, status, query]`
- API call: `GET /v1/dashboard/jobs?source={source}&limit=50&offset=...&status=...&q=...`
- `refetchInterval: 30_000`
- Pagination: `buildPageSlots()` — up to 7 slots with ellipsis, `PAGE_SIZE = 50`

---

## JobTable Columns (`job-table.tsx`)

```
┌──────────────────────────────────────────────────────────────────────┐
│ ID      Model    Backend  API Key   Status     Created   TTFT  Latency│
│ 3a9f…  llama3   gpu-1    dev-key   ✓complete  Feb 25   142ms  1.2s  │
└──────────────────────────────────────────────────────────────────────┘
```

- Status filter: all | pending | running | completed | failed | cancelled
- Search (`q=`): case-insensitive substring match on **prompt text OR api key name**
  - Backend: `j.prompt ILIKE $2 OR k.name ILIKE $2` (LEFT JOIN api_keys)
  - Placeholder: "Search prompt or key…"
- Retry button (▶): pre-fills test panel with job's model + prompt + backend (via `onRetry`)

### Duration Format — `formatDuration(ms)`

| Range | Format |
|-------|--------|
| < 1000ms | `"842ms"` |
| 1000ms – 60s | `"2.3s"` |
| ≥ 60s | `"2m 5s"` |

### Job Detail Modal

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

## Types (`web/lib/types.ts`)

```typescript
export interface Job {
  id: string
  model_name: string
  backend: string
  status: string
  source: 'api' | 'test'
  created_at: string
  completed_at?: string
  latency_ms?: number
  ttft_ms?: number
  completion_tokens?: number
  prompt_tokens?: number
  cached_tokens?: number
  tps?: number
  api_key_name?: string
}

export interface JobDetail extends Job {
  started_at?: string
  prompt: string
  result_text?: string
  error?: string
}
```

---

## i18n Keys (messages/en.json)

### jobs.*
```json
"title", "description",
"testRuns",              // "Test Runs"  — tab label
"apiJobs",              // "API Jobs"   — tab label
"allStatuses", "filterByStatus",
"searchPlaceholder",    // "Search prompt or key…"  (matches prompt OR key name)
"searchingFor", "clearSearch",
"totalLabel", "loadingJobs", "failedJobs", "noJobs",
"statuses.pending", "statuses.running", "statuses.completed",
"statuses.failed", "statuses.cancelled"
```
