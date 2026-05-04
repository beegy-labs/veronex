# Web -- Jobs Page

> SSOT | **Last Updated**: 2026-05-02 (rev8: feature components moved to `app/jobs/components/`, conversation-list with markdown rendering)

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new column to jobs table | `web/app/jobs/components/job-table.tsx` columns + `web/lib/types.ts` `Job` | Add column def + i18n key `jobs.*` |
| Add new status filter option | `web/app/jobs/page.tsx` `STATUS_OPTIONS` in `JobsSection` | Matches `JobStatus` enum on backend |
| Change pagination page size | `web/app/jobs/page.tsx` -> `PAGE_SIZE` constant | |
| Add new i18n key to jobs | `web/messages/en.json` `jobs.*` -> `web/messages/ko.json` -> `web/messages/ja.json` | Always add to all 3 locales |
| Change duration format breakpoints | `web/lib/chart-theme.ts` `fmtMsNullable()` | Change ms thresholds |
| Add another tab to the Jobs page | `web/app/jobs/page.tsx` -> add `<TabsTrigger>` + `<TabsContent>` + new `<JobsSection source="...">` | Extend `JobsSectionProps.source` type |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/jobs/page.tsx` | Jobs page -- `GroupSessionsPanel` + 3 tabs (`JobsSection` x 2, `NetworkFlowTab`) |
| `web/app/jobs/components/job-table.tsx` | Jobs table (imports `JobDetailModal` from `./job-detail-modal`) |
| `web/app/jobs/components/job-detail-modal.tsx` | `JobDetailModal` — timing, tokens, prompt/result, conversation history |
| `web/app/jobs/components/conversation-list.tsx` | Conversation turn list with markdown (react-markdown + remark-gfm) rendering for assistant text and tool-result expand panels |
| `web/app/jobs/components/api-test-panel.tsx` | API test tab (form + runs + conversation panels) |
| `web/lib/types.ts` | `Job`, `JobDetail` types (include `source` field) |
| `web/lib/api.ts` | `api.jobs()`, `api.jobDetail()`, `api.triggerSessionGrouping()` |
| `web/messages/en.json` | i18n keys under `jobs.*` (mirror in `ko.json`, `ja.json`) |

## `source` Field

The `Job` and `JobDetail` types carry `source: 'api' | 'test' | 'analyzer'`. This field is set by the backend at job creation time based on the origin:

| Value | Queue | Meaning |
|-------|-------|---------|
| `'api'` | `veronex:queue:zset` (tier=standard/paid) | Submitted via the OpenAI-compatible API |
| `'test'` | `veronex:queue:zset` (tier=test, lowest score bonus) | Submitted via the web Test panel |
| `'analyzer'` | `veronex:queue:zset` | Submitted by the capacity analyzer (VRAM probing/batch analysis) |

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
+-- [API 호출별] [대화별] [Network Flow]  <- shadcn/ui Tabs -------------+
|                                                                       |
| Tab: "API 호출별" / "Tasks" (default, i18n key: jobs.tasks)           |
|   [search] [status filter]      <- queries ?source=api                |
|   <JobTable>   pagination                                             |
|                                                                       |
| Tab: "대화별" / "Conversations" (i18n key: jobs.conversations)        |
|   <ConversationList>                                                  |
|                                                                       |
| Tab: "Network Flow"                                                   |
|   <NetworkFlowTab providers={providers} />                              |
|   (same component as /flow page -- real-time pipeline visualization)  |
+-----------------------------------------------------------------------+
```

Network Flow notes: Ollama node centers when Gemini is disabled. Stale bee filter removes inactive bees after 2 s. Flow chart badges show live pending/running/req-s counts from `flow_stats` SSE events.

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
- API call: `GET /v1/dashboard/jobs?source={source}&limit=50&offset=...&status=...&q=...&model=...&provider=...`
- `refetchInterval: 30_000`
- Pagination: `buildPageSlots()` -- up to 7 slots with ellipsis, `PAGE_SIZE = 50`

---

## JobTable Columns

```
+----------------------------------------------------------------------------------------------------+
| ID      Conv ID  Model    Provider  Provider Name  API Key  Status    Created  TTFT  Latency      |
| 3a9f..  c7d2..   llama3   ollama    gpu-1          dev-key  complete  Feb 25  142ms  1.2s         |
+----------------------------------------------------------------------------------------------------+
```

- `Conversation ID`: truncated UUID (8 chars) with full UUID in tooltip. Empty cell (muted `—`) when `conversation_id` is null.

- Status filter: all | pending | running | completed | failed | cancelled
- Model filter (`model=`): exact match on model_name
- Provider filter (`provider=`): exact match on provider name (via JOIN)
- Search (`q=`): case-insensitive substring match on prompt text OR api key name
- Retry button: pre-fills test panel with job's model + prompt + provider (via `onRetry`)
- Wrench icon next to status badge when `has_tool_calls = true`

See `jobs-impl.md` for duration format, job detail modal, and extended field specs.

---

## Types (`web/lib/types.ts`)

| Interface | Key fields | Notes |
|-----------|-----------|-------|
| `ToolCall` | `id`, `function.name`, `function.arguments` | Used in both `Job` and `JobDetail` |
| `Job` | `id`, `conversation_id`, `model_name`, `provider_type`, `status`, `source`, timing fields, `has_tool_calls`, `estimated_cost_usd`, `provider_name` | List view -- `has_tool_calls` computed by backend SQL |
| `ChatMessage` | `role`, `content`, `tool_calls` | Roles: system/user/assistant/tool |
| `JobDetail` | All `Job` fields + `prompt`, `result_text`, `error`, `tool_calls_json`, `message_count`, `messages_json`, `image_keys`, `image_urls` | Modal view -- `tool_calls_json` rendered when `result_text` is null; image thumbnail gallery when `image_urls` present |

See `jobs-impl.md` for full type definitions and extended field specs.

---

## i18n Keys (`messages/en.json`)

### jobs.*
```json
"title", "description",
"tasks",                 // "API 호출별" / "Tasks"  -- tab label (was apiJobs)
"conversations",         // "대화별" / "Conversations"  -- tab label (was testRuns)
"networkFlow",           // "Network Flow"  -- tab label
"conversationId",        // "Conversation ID"  -- table column header
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
