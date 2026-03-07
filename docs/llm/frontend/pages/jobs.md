# Web -- Jobs Page

> SSOT | **Last Updated**: 2026-03-04 (rev5: split to jobs.md + jobs-impl.md)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new column to jobs table | `web/components/job-table.tsx` columns + `web/lib/types.ts` `Job` | Add column def + i18n key `jobs.*` |
| Add new status filter option | `web/app/jobs/page.tsx` `STATUS_OPTIONS` in `JobsSection` | Matches `JobStatus` enum on backend |
| Change pagination page size | `web/app/jobs/page.tsx` -> `PAGE_SIZE` constant | |
| Add new i18n key to jobs | `web/messages/en.json` `jobs.*` -> `web/messages/ko.json` -> `web/messages/ja.json` | Always add to all 3 locales |
| Change duration format breakpoints | `web/components/job-table.tsx` `formatDuration()` | Change ms thresholds |
| Add another tab to the Jobs page | `web/app/jobs/page.tsx` -> add `<TabsTrigger>` + `<TabsContent>` + new `<JobsSection source="...">` | Extend `JobsSectionProps.source` type |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/jobs/page.tsx` | Jobs page -- `GroupSessionsPanel` + 3 tabs (`JobsSection` x 2, `NetworkFlowTab`) |
| `web/components/job-table.tsx` | Jobs table + detail modal (fully i18n) |
| `web/lib/types.ts` | `Job`, `JobDetail` types (include `source` field) |
| `web/lib/api.ts` | `api.jobs()`, `api.jobDetail()`, `api.triggerSessionGrouping()` |
| `web/messages/en.json` | i18n keys under `jobs.*` |

## `source` Field

The `Job` and `JobDetail` types carry `source: 'api' | 'test'`. This field is set by the backend at job creation time based on the Valkey queue used:

| Value | Queue | Meaning |
|-------|-------|---------|
| `'api'` | `veronex:queue:jobs` | Submitted via the OpenAI-compatible API |
| `'test'` | `veronex:queue:jobs:test` | Submitted via the web Test panel |

The Jobs page filters by source (`?source=api` / `?source=test`) per tab. The Overview page recent-jobs mini-table shows all sources without filtering.

---

## /jobs -- Page Layout

```
+-- Page header --------------------------------------------------------+
|  Jobs  (subtitle)                                                     |
+-- GroupSessionsPanel -------------------------------------------------+
|  Group Sessions                                                       |
|  "Assign conversation IDs to completed jobs before the selected date."|
|  [date input: yesterday] [Group Now]  -> success/error/already-running|
+-- [API Jobs] [Test Runs] [Network Flow]  <- shadcn/ui Tabs -----------+
|                                                                       |
| Tab: "API Jobs" (default)                                             |
|   [search] [status filter]      <- queries ?source=api                |
|   <JobTable>   pagination                                             |
|                                                                       |
| Tab: "Test Runs"                                                      |
|   <ApiTestPanel> -- submits with source=test                          |
|   -- border-t --                                                      |
|   [search] [status filter]      <- queries ?source=test               |
|   <JobTable>   pagination                                             |
|                                                                       |
| Tab: "Network Flow"                                                   |
|   <NetworkFlowTab providers={providers} />                              |
|   (same component as /flow page -- real-time pipeline visualization)  |
+-----------------------------------------------------------------------+
```

See `jobs-impl.md` for GroupSessionsPanel internals, handleRetry cross-tab navigation, and Network Flow details.

## `JobsSection` Component

Reusable component in `jobs/page.tsx`, rendered once per tab:

```tsx
<JobsSection source="api" onRetry={handleRetry} />
<JobsSection source="test" onRetry={handleRetry} />
```

Each section maintains independent state:
- `page`, `status`, `search`, `query` -- no shared state between tabs
- Query key: `['dashboard-jobs', source, page, status, query]`
- API call: `GET /v1/dashboard/jobs?source={source}&limit=50&offset=...&status=...&q=...`
- `refetchInterval: 30_000`
- Pagination: `buildPageSlots()` -- up to 7 slots with ellipsis, `PAGE_SIZE = 50`

---

## JobTable Columns

```
+--------------------------------------------------------------------------+
| ID      Model    Provider API Key   Status           Created   TTFT  Latency|
| 3a9f..  llama3   gpu-1    dev-key   complete         Feb 25   142ms  1.2s  |
+--------------------------------------------------------------------------+
```

- Status filter: all | pending | running | completed | failed | cancelled
- Search (`q=`): case-insensitive substring match on prompt text OR api key name
- Retry button: pre-fills test panel with job's model + prompt + provider (via `onRetry`)
- Wrench icon next to status badge when `has_tool_calls = true`

See `jobs-impl.md` for duration format, job detail modal, and extended field specs.

---

## Types (`web/lib/types.ts`)

| Interface | Key fields | Notes |
|-----------|-----------|-------|
| `ToolCall` | `id`, `function.name`, `function.arguments` | Used in both `Job` and `JobDetail` |
| `Job` | `id`, `model_name`, `provider_type`, `status`, `source`, timing fields, `has_tool_calls`, `estimated_cost_usd` | List view -- `has_tool_calls` computed by backend SQL |
| `ChatMessage` | `role`, `content`, `tool_calls` | Roles: system/user/assistant/tool |
| `JobDetail` | All `Job` fields + `prompt`, `result_text`, `error`, `tool_calls_json`, `message_count`, `messages_json` | Modal view -- `tool_calls_json` rendered when `result_text` is null |

See `jobs-impl.md` for full type definitions and extended field specs.

---

## i18n Keys (`messages/en.json`)

### jobs.*
```json
"title", "description",
"testRuns",              // "Test Runs"  -- tab label
"apiJobs",               // "API Jobs"   -- tab label
"networkFlow",           // "Network Flow"  -- tab label
"allStatuses", "filterByStatus",
"searchPlaceholder",     // "Search prompt or key..."
"searchingFor", "clearSearch",
"totalLabel", "loadingJobs", "failedJobs", "noJobs",
"statuses.pending", "statuses.running", "statuses.completed",
"statuses.failed", "statuses.cancelled",
"toolCalls",             // "Tool Calls"
"agentToolCall",         // description shown below Tool Calls label
"conversationTurns",     // "Conversation turns"
"estimatedCost",         // "Est. Cost"
"conversationHistory",   // "Conversation History"
"groupSessions",         // "Group Sessions"
"groupSessionsDesc",     // "Assign conversation IDs..."
"groupBeforeDate",       // "Group jobs before"
"groupNow",              // "Group Now"
"grouping",              // "Grouping..."
"groupingSuccess",       // "Session grouping triggered"
"groupingAlreadyRunning",// "Already running, please wait"
"groupingError"          // "Failed to trigger grouping"
```

### usage.*
```json
"estimatedCost",  // "Est. Cost"  -- table column header
"totalCost"       // "Total Cost"  -- breakdown header badge label
```
