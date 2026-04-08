# Lab Features — Port, API & Frontend

> SSOT | **Last Updated**: 2026-04-06
> Overview and DB schema: `inference/lab-features.md`

---

## Port

```rust
// application/ports/outbound/lab_settings_repository.rs

pub struct LabSettings {
    pub gemini_function_calling: bool,
    pub max_images_per_request: i32,
    pub max_image_b64_bytes: i32,
    pub context_compression_enabled: bool,
    pub compression_model: Option<String>,
    pub context_budget_ratio: f32,
    pub compression_trigger_turns: i32,
    pub recent_verbatim_window: i32,
    pub compression_timeout_secs: i32,
    pub multiturn_min_params: i32,
    pub multiturn_min_ctx: i32,
    pub multiturn_allowed_models: Vec<String>,
    pub vision_model: Option<String>,
    pub handoff_enabled: bool,
    pub handoff_threshold: f32,
    pub updated_at: DateTime<Utc>,
}

pub struct LabSettingsUpdate {
    pub gemini_function_calling: Option<bool>,
    pub max_images_per_request: Option<i32>,
    pub max_image_b64_bytes: Option<i32>,
    pub context_compression_enabled: Option<bool>,
    pub compression_model: Option<Option<String>>,
    pub context_budget_ratio: Option<f32>,
    pub compression_trigger_turns: Option<i32>,
    pub recent_verbatim_window: Option<i32>,
    pub compression_timeout_secs: Option<i32>,
    pub multiturn_min_params: Option<i32>,
    pub multiturn_min_ctx: Option<i32>,
    pub multiturn_allowed_models: Option<Vec<String>>,
    pub vision_model: Option<Option<String>>,
    pub handoff_enabled: Option<bool>,
    pub handoff_threshold: Option<f32>,
}

pub trait LabSettingsRepository: Send + Sync {
    async fn get(&self) -> Result<LabSettings>;
    async fn update(&self, patch: LabSettingsUpdate) -> Result<LabSettings>;
}
```

Implementation: `CachingLabSettingsRepo(PostgresLabSettingsRepository)` — TtlCache 30s wrapper.
`get()` hits in-memory cache (hot path: every image/MCP request). `update()` invalidates cache.
→ See `infra/hot-path-caching.md` for full caching strategy.

Files:
- `infrastructure/outbound/persistence/lab_settings_repository.rs`
- `infrastructure/outbound/persistence/caching_lab_settings_repo.rs`

AppState field: `lab_settings_repo: Arc<dyn LabSettingsRepository>`

---

## API

JWT Bearer–only (dashboard router — not accessible via API key).
Handler: `infrastructure/inbound/http/dashboard_handlers.rs`

### `GET /v1/dashboard/lab`

Returns `LabSettingsResponse` — all fields:

```json
{
  "gemini_function_calling": false,
  "max_images_per_request": 4,
  "max_image_b64_bytes": 2097152,
  "context_compression_enabled": false,
  "compression_model": null,
  "context_budget_ratio": 0.60,
  "compression_trigger_turns": 1,
  "recent_verbatim_window": 1,
  "compression_timeout_secs": 10,
  "multiturn_min_params": 7,
  "multiturn_min_ctx": 16384,
  "multiturn_allowed_models": [],
  "vision_model": null,
  "handoff_enabled": true,
  "handoff_threshold": 0.85,
  "updated_at": "2026-04-06T00:00:00Z"
}
```

### `PATCH /v1/dashboard/lab`

All fields optional — `None`/absent = keep current (COALESCE). For nullable text: `null` = clear, string = set.

```json
{ "context_compression_enabled": true, "compression_model": "qwen2.5:3b" }
```

Returns the full updated `LabSettingsResponse` (same shape as GET).

---

## Frontend

### Provider

`LabSettingsProvider` (`web/components/lab-settings-provider.tsx`) — auto-fetches on mount, exposes `{ labSettings, refetch() }` via `useLabSettings()`.

### Types (`web/lib/types.ts`)

```typescript
export interface LabSettings {
  gemini_function_calling: boolean
  max_images_per_request: number
  max_image_b64_bytes: number
  context_compression_enabled: boolean
  compression_model: string | null
  context_budget_ratio: number
  compression_trigger_turns: number
  recent_verbatim_window: number
  compression_timeout_secs: number
  multiturn_min_params: number
  multiturn_min_ctx: number
  multiturn_allowed_models: string[]
  vision_model: string | null
  handoff_enabled: boolean
  handoff_threshold: number
  updated_at: string
}
```

### Compression UI (`web/components/nav-settings-dialog.tsx`)

Settings → Lab Features:
- `context_compression_enabled` toggle
- `compression_model` dropdown (`CompressionModelSelector` — all Ollama models)
- `handoff_enabled` toggle
- `handoff_threshold` number input (0–1)
- `multiturn_min_params`, `multiturn_min_ctx`, `multiturn_allowed_models` (comma-separated)
- `vision_model` dropdown (`VisionModelSelector` — Ollama models with `is_vision=true`)

Uses `useOptimistic` + `startTransition` for compression/handoff switches.
