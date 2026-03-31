# S13 — Dashboard Restructure: Tasks + Conversations

> **Status**: Planning | **Created**: 2026-03-29 | **Branch**: `feat/mcp-integration`

## Goal

Merge api/test/analyzer tabs into single Tasks view. Add Conversations view.
Remove test result overwrite — test panel creates new jobs only.

## Current Structure

```
Jobs Page (web/app/jobs/page.tsx)
├─ Tab: api       → JobsSection(source="api")
├─ Tab: test      → ApiTestPanel + JobsSection(source="test")
├─ Tab: analyzer  → JobsSection(source="analyzer")
└─ Tab: flow      → NetworkFlowTab
```

## Target Structure

```
Jobs Page
├─ Tab: Tasks         → All jobs (source column: api/test/analyzer)
├─ Tab: Conversations → Grouped by conversation_id (from conversations table)
├─ Tab: Test Panel    → ApiTestPanel only (no overwrite, creates new job)
└─ Tab: Flow          → NetworkFlowTab (unchanged)
```

## Backend Changes

### 1. GET /v1/conversations — Conversation list

```
GET /v1/conversations?limit=50&offset=0&account_id=...

Response: {
  conversations: [{
    id: UUID,
    public_id: "conv_32q9...",  // base62
    title: "내 이름은 베로야",  // first user prompt (auto)
    model_name: "qwen3:8b",
    turn_count: 20,
    total_prompt_tokens: 5000,
    total_completion_tokens: 3000,
    created_at: ISO8601,
    updated_at: ISO8601
  }],
  total: 150
}
```

File: `crates/veronex/src/infrastructure/inbound/http/conversation_handlers.rs` (new)
Route: `router.rs`

### 2. GET /v1/conversations/{id} — Conversation detail

```
GET /v1/conversations/32q9...  (base62 public_id)

Response: {
  id: UUID,
  public_id: "conv_32q9...",
  title: "...",
  model_name: "qwen3:8b",
  turn_count: 3,
  total_prompt_tokens: 280,
  total_completion_tokens: 185,
  created_at: ...,
  updated_at: ...,
  turns: [
    {
      id: UUID,
      role: "user",
      prompt_preview: "내 이름은 베로야",
      result_text: "안녕하세요 베로님!",
      model_name: "qwen3:8b",
      prompt_tokens: 50,
      completion_tokens: 30,
      latency_ms: 1200,
      has_tool_calls: false,
      created_at: ...
    },
    ...
  ]
}
```

File: same handler
Data: joins `inference_jobs` WHERE conversation_id = $1, S3 for result

### 3. Auto-title on conversation create

When `conversations.title IS NULL`, set title from first user message:
```sql
UPDATE conversations SET title = LEFT($1, 50)
WHERE id = $2 AND title IS NULL
```

Location: `load_conversation_context()` in `openai_handlers.rs`

### 4. GET /v1/dashboard/jobs — Add source column filter removal

Currently filters by `source` param. Change to return ALL sources with `source` in response.
No separate `source=api` / `source=test` queries — single unified list.

### 5. Remove test overwrite

`ApiTestPanel` currently overwrites previous test result. Change to always create new job.
File: `web/components/api-test-panel.tsx`

## Frontend Changes

### 6. Jobs page tab restructure

File: `web/app/jobs/page.tsx`

```tsx
// Before
<TabsTrigger value="api">API Jobs</TabsTrigger>
<TabsTrigger value="test">Test Runs</TabsTrigger>
<TabsTrigger value="analyzer">Analyzer</TabsTrigger>
<TabsTrigger value="flow">Network Flow</TabsTrigger>

// After
<TabsTrigger value="tasks">Tasks</TabsTrigger>
<TabsTrigger value="conversations">Conversations</TabsTrigger>
<TabsTrigger value="test">Test</TabsTrigger>
<TabsTrigger value="flow">Flow</TabsTrigger>
```

### 7. Tasks tab — unified job list

- Remove `source` filter from JobsSection
- Add `source` column to JobTable (badge: api/test/analyzer)
- Show all jobs in single table

File: `web/components/job-table.tsx` — add source column

### 8. Conversations tab — grouped view

New component: `web/components/conversation-list.tsx`

```
┌─────────────────────────────────────────────────┐
│ Conversations                                    │
├─────────────────────────────────────────────────┤
│ 📝 내 이름은 베로야          qwen3:8b  20 turns │
│    3월 29일 — 280 prompt / 185 completion tokens │
├─────────────────────────────────────────────────┤
│ 🔍 마이크론 주식 전망        qwen3:8b   3 turns │
│    3월 28일 — 5000 / 3000 tokens                │
└─────────────────────────────────────────────────┘

Click → Conversation detail modal/page with turn timeline
```

### 9. Conversation detail view

New component: `web/components/conversation-detail.tsx`

```
┌─────────────────────────────────────────────────┐
│ Conversation: 내 이름은 베로야  (20 turns)       │
├─────────────────────────────────────────────────┤
│ USER   내 이름은 베로야                          │
│ ASST   안녕하세요 베로님! 😊                     │
│ ─────────────────────────────────────────────── │
│ USER   나이 30                                   │
│ ASST   30살이시군요!                             │
│ ─────────────────────────────────────────────── │
│ USER   내 정보 요약해                            │
│ ASST   이름: 베로, 나이: 30, 직업: 개발자...     │
│        🔧 mcp_weather_mcp_web_search (tool call) │
└─────────────────────────────────────────────────┘
```

### 10. Remove GroupSessionsPanel

Session grouping is replaced by explicit `conversation_id`. Remove the manual grouping UI.

### 11. i18n keys

Add to `web/messages/en.json`, `ko.json`, `ja.json`:
```json
{
  "jobs": {
    "tasks": "Tasks",
    "conversations": "Conversations",
    "turnCount": "Turns",
    "totalTokens": "Total Tokens",
    "conversationDetail": "Conversation Detail",
    "source": "Source"
  }
}
```

## Implementation Order

| Step | Task | Type | Files |
|------|------|------|-------|
| 1 | `GET /v1/conversations` endpoint | Backend | conversation_handlers.rs, router.rs |
| 2 | `GET /v1/conversations/{id}` endpoint | Backend | conversation_handlers.rs |
| 3 | Auto-title on create | Backend | openai_handlers.rs |
| 4 | Remove source filter from dashboard jobs API | Backend | dashboard_handlers.rs |
| 5 | conversation-list.tsx component | Frontend | new file |
| 6 | conversation-detail.tsx component | Frontend | new file |
| 7 | job-table.tsx source column | Frontend | modify |
| 8 | jobs/page.tsx tab restructure | Frontend | modify |
| 9 | Remove GroupSessionsPanel | Frontend | modify |
| 10 | Remove test overwrite from ApiTestPanel | Frontend | modify |
| 11 | i18n keys (en/ko/ja) | Frontend | messages/*.json |
| 12 | e2e test for conversations API | Scripts | 12-mcp.sh or new |

## Dependencies

- `conversations` table (DONE — in 000001_init.up.sql)
- `conversation_id` UUID type (DONE — entity + ports + handlers)
- `update_conversation_counters` (DONE — runner.rs + job_repository)
- `load_conversation_context` (DONE — openai_handlers.rs)
