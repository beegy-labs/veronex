# Web -- Jobs Page: Implementation Details

> SSOT | **Last Updated**: 2026-03-04 (companion to `jobs.md`)
> Types and extended fields: `jobs-types.md`

## GroupSessionsPanel

Position: below page header, above tabs. Always visible.

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

**Behavior**:
- Default value: yesterday's date (`new Date() - 1day`)
- Button click -> `api.triggerSessionGrouping(date)` -> `POST /v1/dashboard/session-grouping/trigger`
- **202**: `jobs.groupingSuccess` (green)
- **409**: `jobs.groupingAlreadyRunning` (red) -- already running
- Other errors: `jobs.groupingError` (red)

---

## `handleRetry` -- Cross-Tab Navigation

When the user clicks Retry on a job in either tab:
1. `setActiveTab('test')` -- switches to the Test Runs tab
2. `setTimeout(50ms)` -> `testPanelRef.current?.scrollIntoView()` -- scrolls to ApiTestPanel
3. `setRetryParams(params)` -- pre-fills panel with the job's model + prompt + provider

```tsx
function handleRetry(params: RetryParams) {
  setRetryParams(params)
  setActiveTab('test')
  setTimeout(() => {
    testPanelRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }, 50)
}
```

---

## Network Flow Tab

```tsx
const { data: providers } = useQuery(providersQuery())

<TabsContent value="flow">
  <NetworkFlowTab providers={providers ?? []} />
</TabsContent>
```

`NetworkFlowTab` renders **`ProviderFlowPanel`** + **`LiveFeed`** -- the real-time pipeline visualization previously at the standalone `/flow` page.

- **Hook**: `useInferenceStream()` -- 5 s TanStack Query polling of `GET /v1/dashboard/jobs?limit=50`
- **Phase model**: `enqueue` (API->Queue, amber) / `dispatch` (Queue->Provider[->GPU], blue) / `response` (bypass arc, green/red, dimmed)
- **SVG layout**: 450x260, 4 columns -- API(cx=56) -> Queue(cx=172) -> Provider(cx=288) -> GPU(cx=404)
- **Animation**: CSS Motion Path bee-fly (1400ms, BEE_STAGGER_MS=700, MAX_BEES=30)
- **LiveFeed**: shows `enqueue`-phase events (job arrivals), scrollable newest-first

> The `/flow` route still exists (`web/app/flow/page.tsx`) but is not linked in the nav.
> Network Flow is accessible only as the third tab of the Jobs page.

---

## Duration Format -- `formatDuration(ms)`

| Range | Format |
|-------|--------|
| < 1000ms | `"842ms"` |
| 1000ms -- 60s | `"2.3s"` |
| >= 60s | `"2m 5s"` |

---

## Job Detail Modal

```
+-- 3a9fbcd... completed ------------------------------------------+
| llama3 - gpu-ollama-1                                             |
| Created: Feb 25 14:32  Started: 14:32  Completed: 14:32          |
| Latency: 1.2s   TTFT: 142ms   TPS: 44.3 tok/s                   |
| Input tokens: 53  Output tokens: 12  Total tokens: 65            |
| Conversation turns: 26  Est. Cost: $0.000012                     |
| API Key: dev-key  Endpoint: /v1/chat/completions                 |
+-------------------------------------------------------------------+
| PROMPT                                                            |
| <last user message -- NOT full context>                           |
+-------------------------------------------------------------------+
| [Text response]: RESULT                                           |
|   <result_text>                                                   |
| [Tool call response]: TOOL CALLS                                  |
|   "The model responded with a tool call -- no text output."       |
|   +---------------------------+                                   |
|   | list_directory  call_abc  |                                   |
|   |  {"path": "/src"}         |                                   |
|   +---------------------------+                                   |
+-------------------------------------------------------------------+
```

### Result Section Branching Logic

Priority order:
1. `status === 'failed'` -> show **Error** section (red)
2. `tool_calls_json.length > 0 && !result_text` -> show **Tool Calls** section (info blue). Lists each call: function name + id + arguments `<pre>` block
3. Otherwise -> show **Result** section (green); falls back to `t('jobs.noResult')` / `t('jobs.processing')`

### MetaItems Shown

`Created` | `Started` | `Completed` | `Latency` | `TTFT` | `TPS` | `Input tokens` (tooltip: full context) | `Output tokens` | `Cached tokens` (if > 0) | `Total tokens` | `API Key` / `Runner` (account) | `Endpoint` | `Conversation turns` (if > 1) | `Est. Cost` (if not null)

- `useQuery({ queryKey: ['job-detail', jobId], enabled: !!jobId && open })`
- TPS displayed when `completion_tokens` and `latency_ms - ttft_ms` are available
- Est. Cost: `"$0.00 (self-hosted)"` for Ollama, `"$0.000xxx"` for Gemini

---

## Types & Extended Fields

See `jobs-types.md` for full type definitions (`ToolCall`, `Job`, `ChatMessage`, `JobDetail`) and extended field specs (`has_tool_calls`, `tool_calls_json`, `messages_json`, `estimated_cost_usd`, ConversationHistory component, Usage page cost display).
