# Web — Jobs Page

> SSOT | **Last Updated**: 2026-03-02 (rev4: 3 tabs — API Jobs / Test Runs / Network Flow; GroupSessionsPanel; session grouping manual trigger)

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
| `web/app/jobs/page.tsx` | Jobs page — `GroupSessionsPanel` + 3 tabs (`JobsSection` × 2, `NetworkFlowTab`) |
| `web/components/job-table.tsx` | Jobs table + detail modal (fully i18n) |
| `web/lib/types.ts` | `Job`, `JobDetail` types (include `source` field) |
| `web/lib/api.ts` | `api.jobs()`, `api.jobDetail()`, `api.triggerSessionGrouping()` |
| `web/messages/en.json` | i18n keys under `jobs.*` |

## `source` Field

The `Job` and `JobDetail` types carry `source: 'api' | 'test'`. This field is set by the backend at job creation time based on the Valkey queue used:

| Value | Queue | Meaning |
|-------|-------|---------|
| `'api'`  | `veronex:queue:jobs` | Submitted via the OpenAI-compatible API |
| `'test'` | `veronex:queue:jobs:test` | Submitted via the web Test panel |

The Jobs page filters by source (`?source=api` / `?source=test`) per tab. The Overview page recent-jobs mini-table shows all sources without filtering.

---

## /jobs — Page Layout

```
┌─ Page header ──────────────────────────────────────────────────────────┐
│  Jobs  (subtitle)                                                        │
├─ GroupSessionsPanel ───────────────────────────────────────────────────┤
│  🔀 Group Sessions                                                       │
│  "Assign conversation IDs to completed jobs before the selected date."   │
│  [date input: yesterday] [Group Now]  → success/error/already-running   │
├─ [API Jobs] [Test Runs] [Network Flow]  ← shadcn/ui Tabs               │
├────────────────────────────────────────────────────────────────────────┤
│ Tab: "API Jobs"  (default)                                               │
│   [search] [status filter]      ← queries ?source=api                   │
│   <JobTable>   pagination                                                │
├────────────────────────────────────────────────────────────────────────┤
│ Tab: "Test Runs"                                                         │
│   <ApiTestPanel> — submits with source=test                              │
│   ── border-t ──                                                         │
│   [search] [status filter]      ← queries ?source=test                   │
│   <JobTable>   pagination                                                │
├────────────────────────────────────────────────────────────────────────┤
│ Tab: "Network Flow"                                                      │
│   <NetworkFlowTab backends={backends} />                                 │
│   (same component as /flow page — real-time pipeline visualization)     │
└─────────────────────────────────────────────────────────────────────────┘
```

### GroupSessionsPanel

위치: 페이지 헤더 아래, 탭 위. 항상 표시.

```tsx
<Card>
  <CardHeader>
    <GitMerge icon /> Group Sessions
    <CardDescription>jobs.groupSessionsDesc</CardDescription>
  </CardHeader>
  <CardContent>
    <label>{t('jobs.groupBeforeDate')}</label>
    <Input type="date" default={yesterday} />
    <Button onClick={handleGroup}>
      {loading ? t('jobs.grouping') : t('jobs.groupNow')}
    </Button>
    {message && <span className={success ? green : red}>{message.text}</span>}
  </CardContent>
</Card>
```

**동작**:
- 기본값: 어제 날짜 (`new Date() - 1day`)
- 버튼 클릭 → `api.triggerSessionGrouping(date)` → `POST /v1/dashboard/session-grouping/trigger`
- **202**: `jobs.groupingSuccess` (green)
- **409**: `jobs.groupingAlreadyRunning` (red) — 이미 실행 중
- 기타 오류: `jobs.groupingError` (red)

**i18n keys** (`jobs.*`):
```json
"groupSessions", "groupSessionsDesc", "groupBeforeDate",
"groupNow", "grouping", "groupingSuccess", "groupingAlreadyRunning", "groupingError"
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

### Network Flow Tab

```tsx
// backendsQuery provides provider node list
const { data: backends } = useQuery(backendsQuery())

<TabsContent value="flow">
  <NetworkFlowTab backends={backends ?? []} />
</TabsContent>
```

`NetworkFlowTab` renders **`ProviderFlowPanel`** + **`LiveFeed`** — the real-time pipeline visualization previously at the standalone `/flow` page.

- **Hook**: `useInferenceStream()` — 5 s TanStack Query polling of `GET /v1/dashboard/jobs?limit=50`
- **Phase model**: `enqueue` (API→Queue, amber) / `dispatch` (Queue→Provider[→GPU], blue) / `response` (bypass arc, green/red, dimmed)
- **SVG layout**: 450×260, 4 columns — API(cx=56) → Queue(cx=172) → Provider(cx=288) → GPU(cx=404)
- **Animation**: CSS Motion Path bee-fly (1400ms, BEE_STAGGER_MS=700, MAX_BEES=30)
- **LiveFeed**: shows `enqueue`-phase events (job arrivals), scrollable newest-first

> The `/flow` route still exists (`web/app/flow/page.tsx`) but is not linked in the nav.
> Network Flow is accessible only as the third tab of the Jobs page.

**i18n key**: `jobs.networkFlow` — "Network Flow" tab label

---

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
┌──────────────────────────────────────────────────────────────────────────┐
│ ID      Model    Backend  API Key   Status           Created   TTFT  Latency│
│ 3a9f…  llama3   gpu-1    dev-key   ✓complete        Feb 25   142ms  1.2s  │
│ 7f2a…  qwen3    gpu-1    —         ✓complete 🔧     Feb 25   320ms  4.1s  │
└──────────────────────────────────────────────────────────────────────────┘
```

- Status cell shows a `Wrench` icon (🔧) next to the badge when `has_tool_calls = true`
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
│ Created: Feb 25 14:32  Started: 14:32  Completed: 14:32               │
│ Latency: 1.2s   TTFT: 142ms   TPS: 44.3 tok/s                        │
│ Input tokens: 53  Output tokens: 12  Total tokens: 65                 │
│ Conversation turns: 26  Est. Cost: $0.000012                          │
│ API Key: dev-key  Endpoint: /v1/chat/completions                      │
├────────────────────────────────────────────────────────────────────────┤
│ PROMPT                                                                  │
│ <last user message — NOT full context>                                  │
├────────────────────────────────────────────────────────────────────────┤
│ [Text response]: RESULT                                                 │
│   <result_text>                                                         │
│ [Tool call response]: TOOL CALLS                                        │
│   "The model responded with a tool call — no text output was generated."│
│   ┌──────────────────────────┐                                         │
│   │ 🔧 list_directory  call_abc│                                        │
│   │  {"path": "/src"}         │                                         │
│   └──────────────────────────┘                                         │
└────────────────────────────────────────────────────────────────────────┘
```

**Result section branching logic** (priority order):
1. `status === 'failed'` → show **Error** section (red)
2. `tool_calls_json.length > 0 && !result_text` → show **Tool Calls** section (info blue)
   - Lists each call: function name + id + arguments `<pre>` block
3. otherwise → show **Result** section (green); falls back to `t('jobs.noResult')` / `t('jobs.processing')`

**MetaItems shown** (timing row):
`Created` · `Started` · `Completed` · `Latency` · `TTFT` · `TPS` · `Input tokens` (tooltip: full context) · `Output tokens` · `Cached tokens` (if > 0) · `Total tokens` · `API Key` / `Runner` (account) · `Endpoint` · `Conversation turns` (if > 1) · `Est. Cost` (if not null)

- `useQuery({ queryKey: ['job-detail', jobId], enabled: !!jobId && open })`
- TPS displayed when `completion_tokens` and `latency_ms - ttft_ms` are available
- Est. Cost: `"$0.00 (self-hosted)"` for Ollama, `"$0.000xxx"` for Gemini

---

## Types (`web/lib/types.ts`)

```typescript
export interface ToolCall {
  id?: string
  function?: {
    name: string
    index?: number
    arguments?: Record<string, unknown> | string
  }
}

export interface Job {
  id: string
  model_name: string
  backend: string
  status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled'
  source: 'api' | 'test'
  created_at: string
  completed_at: string | null
  latency_ms: number | null
  ttft_ms: number | null
  prompt_tokens: number | null
  completion_tokens: number | null
  cached_tokens: number | null
  tps: number | null
  api_key_name: string | null
  account_name: string | null     // test run jobs — runner's display name
  request_path: string | null
  has_tool_calls: boolean         // true when model responded with function calls
  estimated_cost_usd: number | null  // 0.0=Ollama, >0=Gemini, null=no pricing
}

export interface ChatMessage {
  role: 'system' | 'user' | 'assistant' | 'tool'
  content: string | null
  tool_call_id?: string
  name?: string
  tool_calls?: ToolCall[]
}

export interface JobDetail {
  // (all Job fields)
  started_at: string | null
  prompt: string                  // last user message (display only)
  result_text: string | null      // null when tool_calls_json present
  error: string | null
  tool_calls_json: ToolCall[] | null
  message_count: number | null    // JSONB array length of messages_json
  messages_json: ChatMessage[] | null  // full context — loaded from S3; null for single-prompt jobs
  estimated_cost_usd: number | null
}
```

> **`Job.has_tool_calls`**: computed by backend as `(tool_calls_json IS NOT NULL)` — no extra join needed in table rendering.
>
> **`JobDetail.tool_calls_json`**: rendered as a card list in the detail modal when `result_text` is null. Each card shows `function.name`, `id`, and `arguments` in a `<pre>` block.

---

## Extended Job Fields

Fields added to `JobSummary` (list view) and `JobDetail` (modal) beyond the core timing/token metrics:

### `has_tool_calls: boolean` — Job List Indicator

- Present on `Job` (list view).
- `true` when the model responded with function calls (`tool_calls_json IS NOT NULL` in PostgreSQL).
- Computed by the backend SQL — no client-side check needed.
- UI: renders a `Wrench` icon next to the status badge in the jobs table.

### `tool_calls_json: ToolCall[] | null` — Full Tool Call Data

- Present on `JobDetail` only (loaded when detail modal opens).
- Contains the raw function calls the model emitted during that inference turn.
- `null` for text-only responses.
- UI: when `tool_calls_json.length > 0 && !result_text`, the modal renders a **Tool Calls** section (blue info card) listing each call with `function.name`, call `id`, and `arguments` in a `<pre>` block.
- Backend type: `Option<serde_json::Value>` (JSONB column `tool_calls_json`); deserialized as `ToolCall[]` on the frontend.

### `message_count: number | null` — Multi-Turn Context Depth

- Present on `JobDetail` only.
- Computed from the `messages_json` JSONB array length: `COALESCE(jsonb_array_length(j.messages_json), 0)`.
- Represents the number of messages in the full conversation context sent to the model (system + prior turns + current user message).
- `null` for jobs where `messages_json` was not persisted (single-prompt / pre-migration jobs).
- UI: shown in the detail modal as "Conversation turns" MetaItem when value > 1.

### `messages_json: ChatMessage[] | null` — Full Conversation History (S3 Storage)

- Present on `JobDetail` only (loaded when detail modal opens).
- **Storage**: MinIO / AWS S3 is the **mandatory primary store**. Object key: `messages/{job_id}.json`.
  - New jobs: `DB.messages_json = NULL` (cleared before INSERT); S3 is authoritative.
  - Legacy jobs: S3 lookup returns `None` → fallback to `DB.messages_json`.
  - Backend fetches S3 first in `get_job_detail`, falls back to DB column for pre-MinIO jobs.
- **Port**: `MessageStore` trait (`application/ports/outbound/message_store.rs`).
  - `put(job_id, data)` — called from `InferenceUseCaseImpl::submit()` before job is queued.
  - `get(job_id)` — called from `dashboard_handlers::get_job_detail()`.
- **Adapter**: `S3MessageStore` (`infrastructure/outbound/s3/message_store.rs`).
  - Uses `aws-sdk-s3 = "1"` (full default features — requires Tokio runtime for sleep impl).
  - `force_path_style(true)` required for MinIO path-style access.
  - Bucket auto-created on startup via `ensure_bucket()` (handles `BucketAlreadyExists` MinIO quirk).
- UI: `ConversationHistory` component in the detail modal — collapsible accordion.

**`ConversationHistory` Component** (`web/components/job-table.tsx`):
```
[▶ Conversation History  (26 messages)]   ← click to expand
  ┌──────────────────────────────────────────────────────┐
  │ system  │ You are a helpful assistant...              │
  │ user    │ List the files in /src                      │
  │ assistant│ I'll use the list_directory tool...        │
  │ tool    │ [tool_call_id: call_abc] /src/main.rs...    │
  └──────────────────────────────────────────────────────┘
```
- Role badge colors: `system`=grey, `user`=blue, `assistant`=green, `tool`=yellow
- `tool_calls` shown when `content` is null (function call turn)
- i18n key: `jobs.conversationHistory`

### `estimated_cost_usd: number | null` — Per-Job Cost

- Present on both `Job` (list) and `JobDetail` (modal).
- Computed at query time via a LATERAL JOIN on `model_pricing` — not stored on the job row.
- `0.0` for Ollama (self-hosted, always free); `> 0` for Gemini; `null` when no pricing data.
- UI: shown in the detail modal as "Est. Cost" MetaItem. Rendered as `"$0.00 (self-hosted)"` for Ollama, `"$0.000xxx"` for Gemini with non-zero cost.
- For cost aggregation across keys and models, see `docs/llm/frontend/web-usage.md`.
- For pricing table schema and LATERAL JOIN logic, see `docs/llm/backend/model-pricing.md`.

---

## Usage Page — Cost Display (`web/app/usage/page.tsx`)

The Usage breakdown section (`GET /v1/usage/breakdown`) shows costs in three places:

| Location | Field | Display |
|----------|-------|---------|
| Backend breakdown cards | `estimated_cost_usd` | "Free" (0.0) or `$X.XXXX` at card bottom |
| API Key breakdown table | `estimated_cost_usd` | "—" (null), "Free" (0.0), or `$X.XXXX` column |
| Model breakdown table | `estimated_cost_usd` | same pattern |
| Breakdown card header | `total_cost_usd` | `$X.XXXX` badge — shown only when > 0 |

`UsageBreakdown` frontend type:
```typescript
interface UsageBreakdown {
  by_backend: BackendBreakdown[]  // + estimated_cost_usd: number | null
  by_key:     KeyBreakdown[]      // + estimated_cost_usd: number | null
  by_model:   ModelBreakdown[]    // + estimated_cost_usd: number | null
  total_cost_usd: number          // sum of backend costs
}
```

---

## i18n Keys (messages/en.json)

### jobs.*
```json
"title", "description",
"testRuns",              // "Test Runs"  — tab label
"apiJobs",              // "API Jobs"   — tab label
"networkFlow",           // "Network Flow"  — tab label
"allStatuses", "filterByStatus",
"searchPlaceholder",    // "Search prompt or key…"  (matches prompt OR key name)
"searchingFor", "clearSearch",
"totalLabel", "loadingJobs", "failedJobs", "noJobs",
"statuses.pending", "statuses.running", "statuses.completed",
"statuses.failed", "statuses.cancelled",
"toolCalls",             // "Tool Calls"  — tool call response section label
"agentToolCall",         // description shown below Tool Calls label
"conversationTurns",     // "Conversation turns"  — MetaItem label
"estimatedCost",         // "Est. Cost"  — MetaItem label
"conversationHistory",   // "Conversation History"  — ConversationHistory accordion header
// GroupSessionsPanel
"groupSessions",         // "Group Sessions"  — panel title
"groupSessionsDesc",     // "Assign conversation IDs to completed jobs before the selected date."
"groupBeforeDate",       // "Group jobs before"  — date input label
"groupNow",              // "Group Now"  — button label (idle)
"grouping",              // "Grouping…"  — button label (loading)
"groupingSuccess",       // "Session grouping triggered"  — 202 feedback
"groupingAlreadyRunning",// "Already running, please wait"  — 409 feedback
"groupingError"          // "Failed to trigger grouping"  — error feedback
```

### usage.*
```json
"estimatedCost",  // "Est. Cost"  — table column header
"totalCost"       // "Total Cost"  — breakdown header badge label
```
