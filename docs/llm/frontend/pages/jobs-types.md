# Web -- Jobs: Types & Extended Fields

> SSOT | **Last Updated**: 2026-03-16 (companion to `jobs.md`, `jobs-impl.md`)

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
  provider_type: string
  status: 'pending' | 'running' | 'completed' | 'failed' | 'cancelled'
  source: 'api' | 'test' | 'analyzer'
  created_at: string
  completed_at: string | null
  latency_ms: number | null
  ttft_ms: number | null
  prompt_tokens: number | null
  completion_tokens: number | null
  cached_tokens: number | null
  tps: number | null
  api_key_name: string | null
  account_name: string | null
  request_path: string | null
  has_tool_calls: boolean
  estimated_cost_usd: number | null
  provider_name: string | null
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
  prompt: string
  result_text: string | null
  error: string | null
  tool_calls_json: ToolCall[] | null
  message_count: number | null
  messages_json: ChatMessage[] | null
  estimated_cost_usd: number | null
  provider_name: string | null
  image_keys: string[] | null
  image_urls: string[] | null
}
```

---

## Extended Job Fields

### `has_tool_calls: boolean`

Present on `Job` (list). `true` when `tool_calls_json IS NOT NULL` in DB. Computed by backend SQL. UI: `Wrench` icon next to status badge.

### `tool_calls_json: ToolCall[] | null`

Present on `JobDetail` only. Raw function calls from model. `null` for text-only responses. Backend type: `Option<serde_json::Value>` (JSONB). When present + no `result_text`, modal shows Tool Calls section (blue info card).

### `message_count: number | null`

Present on `JobDetail`. Computed: `COALESCE(jsonb_array_length(j.messages_json), 0)`. `null` for pre-migration jobs. UI: "Conversation turns" MetaItem when > 1.

### `messages_json: ChatMessage[] | null`

Present on `JobDetail`. Storage: MinIO/S3 primary (`messages/{job_id}.json`), DB fallback for legacy jobs.

| Layer | Detail |
|-------|--------|
| Port | `MessageStore` trait (`application/ports/outbound/message_store.rs`) |
| Adapter | `S3MessageStore` (`infrastructure/outbound/s3/message_store.rs`) |
| Put | Called from `submit()` before queueing |
| Get | Called from `get_job_detail()`, S3 first then DB fallback |
| Config | `aws-sdk-s3 = "1"`, `force_path_style(true)` for MinIO |
| Init | `ensure_bucket()` on startup (handles `BucketAlreadyExists`) |

### Image Storage (ImageStore)

| Layer | Detail |
|-------|--------|
| Port | `ImageStore` trait (`application/ports/outbound/image_store.rs`) |
| Adapter | `S3ImageStore` (`infrastructure/outbound/s3/image_store.rs`) |
| Put | `put(job_id, index, webp, thumb)` — stores full + 128px thumbnail |
| Get | `url(key)` — presigned/direct URL for S3 object |
| Keys | `images/{job_id}/{index}.webp` (full), `images/{job_id}/{index}_thumb.webp` (thumb) |
| Bucket | `S3_IMAGE_BUCKET` env var (default: `veronex-images`) — separate from messages |
| Init | `ensure_bucket()` on startup |

### ConversationHistory Component

Located in `web/components/job-table.tsx`. Collapsible panel showing full message history.

| Role | Badge Color |
|------|-------------|
| system | grey |
| user | blue |
| assistant | green |
| tool | yellow |

`tool_calls` shown when `content` is null. i18n: `jobs.conversationHistory`.

### `estimated_cost_usd: number | null`

Present on both `Job` and `JobDetail`. Computed via LATERAL JOIN on `model_pricing` (not stored). `0.0` = Ollama (self-hosted), `> 0` = Gemini, `null` = no pricing data.

See `docs/llm/inference/model-pricing.md` for pricing schema.

### `provider_name: string | null`

Present on both `Job` and `JobDetail`. The human-readable name of the provider (Ollama server) that processed the job. Resolved via LEFT JOIN on `llm_providers`. `null` when provider has been deleted or job is still pending.

### `image_keys: string[] | null` / `image_urls: string[] | null`

Present on `JobDetail` only. `image_keys` are S3 object keys (`images/{job_id}/{index}.webp`). `image_urls` are constructed from `image_keys` + `S3_IMAGE_PUBLIC_URL` env var (e.g. `http://localhost:9010/veronex-images/{key}`). UI renders a thumbnail gallery in the job detail modal when present. Stored as `TEXT[]` column on `inference_jobs`.

---

## Usage Page -- Cost Display

`GET /v1/usage/breakdown` shows costs in:

| Location | Field | Display |
|----------|-------|---------|
| Provider breakdown cards | `estimated_cost_usd` | "Free" (0.0) or `$X.XXXX` |
| API Key breakdown table | `estimated_cost_usd` | "---" (null), "Free" (0.0), or `$X.XXXX` |
| Model breakdown table | `estimated_cost_usd` | same pattern |
| Breakdown card header | `total_cost_usd` | `$X.XXXX` badge (shown only when > 0) |

```typescript
interface UsageBreakdown {
  by_provider: ProviderBreakdown[]  // + estimated_cost_usd
  by_key: KeyBreakdown[]            // + estimated_cost_usd
  by_model: ModelBreakdown[]        // + estimated_cost_usd
  total_cost_usd: number
}
```
