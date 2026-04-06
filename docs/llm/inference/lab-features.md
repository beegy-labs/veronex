# Lab Features (Experimental)

> SSOT | **Last Updated**: 2026-04-06 (rev 5 — context compression + multi-turn + vision + handoff fields)

Lab features are experimental capabilities that are **disabled by default**.
They must be explicitly enabled in Settings → Lab Features.

Gating experimental features behind a flag prevents unstable behavior from
affecting production inference while development is ongoing.

---

## Current Lab Features

| Feature | Default | Status |
|---------|---------|--------|
| `gemini_function_calling` | `false` | In development |
| `mcp_orchestrator_model` | `null` | Stable |
| `context_compression_enabled` | `false` | Lab |
| `compression_model` | `null` | Lab |
| `context_budget_ratio` | `0.75` | Lab |
| `compression_trigger_turns` | `1` | Lab |
| `recent_verbatim_window` | `3` | Lab |
| `compression_timeout_secs` | `30` | Lab |
| `multiturn_min_params` | `7` | Lab |
| `multiturn_min_ctx` | `16384` | Lab |
| `multiturn_allowed_models` | `[]` (all) | Lab |
| `vision_model` | `null` | Lab |
| `handoff_enabled` | `true` | Lab |
| `handoff_threshold` | `0.85` | Lab |

---

## `gemini_function_calling`

**Scope**: This flag is the SSOT for all Gemini integration visibility.
Disabling it hides Gemini everywhere — not only function calling — because
the Gemini-compatible API is itself experimental.

**When disabled** (default) — all of the following are suppressed:

| Layer | Behaviour |
|-------|-----------|
| **API** | Requests with `tools[]` → `501 Not Implemented` |
| **Nav** | Gemini child item hidden from the Providers nav group |
| **Providers page** | `?s=gemini` falls back to `?s=ollama`; `GeminiTab` not rendered |
| **Overview dashboard** | Provider Status KPI counts Ollama only; "API Services" row hidden; Gemini legend hidden |
| **Network Flow panel** | Gemini octagon node, Queue→Gemini path, and Gemini response arc removed |

**When enabled**:
- `tools[].functionDeclarations` are converted to Ollama format and forwarded with the job.
- The model can return `functionCall` parts in its response.
- Tool calls are streamed back to the client in Gemini SSE format.
- Tool calls are stored in `tool_calls_json` for training data.
- All Gemini UI (nav item, providers tab, dashboard stats, flow panel) is visible.

---

## `mcp_orchestrator_model`

**Type**: `Option<String>` (DB: `TEXT`, nullable)

**Scope**: Overrides the model used in `mcp_ollama_chat()` for all MCP tool-call loops.
When set, every MCP request uses this model regardless of the `model` field in the client request.

**When `null`** (default): `req.model` is used as-is — no override.

**When set** (e.g. `"qwen3:8b"`): the specified model is used for all MCP orchestration.

**Why it's a lab feature / configurable per deployment**:
- Different Ollama deployments have different models available.
- Multilingual workloads (Korean/Japanese/English) require a model with strong CJK support.
- Recommended: `qwen3:8b` — 128K context, Hermes tool-calling format, explicit CJK support.
- Clients should not need to know which model handles MCP; the orchestrator choice is an ops decision.

**How it's applied** (`openai_handlers.rs` → `mcp_ollama_chat()`):

```rust
let orchestrator_model = state.lab_settings_repo.get().await
    .ok()
    .and_then(|lab| lab.mcp_orchestrator_model)
    .unwrap_or_else(|| req.model.clone());
// orchestrator_model is passed to bridge.run_loop()
```

**UI**: MCP page (`/mcp`) → `OrchestratorModelSelector` card at top.
Dropdown is populated from `GET /v1/dashboard/capacity/settings` → `available_models.ollama`.

---

## DB Schema

```sql
-- lab_settings table (singleton, id=1 enforced by CHECK constraint)
CREATE TABLE lab_settings (
    id                          INT         PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    gemini_function_calling     BOOLEAN     NOT NULL DEFAULT false,
    max_images_per_request      INT         NOT NULL DEFAULT 4,
    max_image_b64_bytes         INT         NOT NULL DEFAULT 2097152,
    mcp_orchestrator_model      TEXT,
    context_compression_enabled BOOLEAN     NOT NULL DEFAULT false,
    compression_model           TEXT,
    context_budget_ratio        REAL        NOT NULL DEFAULT 0.75,
    compression_trigger_turns   INT         NOT NULL DEFAULT 1,
    recent_verbatim_window      INT         NOT NULL DEFAULT 3,
    compression_timeout_secs    INT         NOT NULL DEFAULT 30,
    multiturn_min_params        INT         NOT NULL DEFAULT 7,
    multiturn_min_ctx           INT         NOT NULL DEFAULT 16384,
    multiturn_allowed_models    TEXT[]      NOT NULL DEFAULT '{}',
    vision_model                TEXT,
    handoff_enabled             BOOLEAN     NOT NULL DEFAULT true,
    handoff_threshold           REAL        NOT NULL DEFAULT 0.85,
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO lab_settings DEFAULT VALUES;
```

Migrations:
- `000005_lab_settings_image.up.sql` — initial table
- `000012_lab_mcp_orchestrator_model.up.sql` — adds `mcp_orchestrator_model`
- `000013_lab_context_compression.up.sql` — adds compression + multi-turn + vision fields
- `000014_lab_handoff_threshold.up.sql` — adds `handoff_threshold`

---

## Port

```rust
// application/ports/outbound/lab_settings_repository.rs

#[derive(Debug, Clone)]
pub struct LabSettings {
    pub gemini_function_calling: bool,
    pub max_images_per_request: i32,
    pub max_image_b64_bytes: i32,
    pub mcp_orchestrator_model: Option<String>,
    // Context compression
    pub context_compression_enabled: bool,
    pub compression_model: Option<String>,
    pub context_budget_ratio: f32,
    pub compression_trigger_turns: i32,
    pub recent_verbatim_window: i32,
    pub compression_timeout_secs: i32,
    // Multi-turn eligibility gate
    pub multiturn_min_params: i32,
    pub multiturn_min_ctx: i32,
    pub multiturn_allowed_models: Vec<String>,
    // Vision
    pub vision_model: Option<String>,
    // Session handoff
    pub handoff_enabled: bool,
    pub handoff_threshold: f32,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait LabSettingsRepository: Send + Sync {
    async fn get(&self) -> Result<LabSettings>;
    async fn update(
        &self,
        gemini_function_calling: Option<bool>,
        max_images_per_request: Option<i32>,
        max_image_b64_bytes: Option<i32>,
        /// None = no change, Some(None) = clear, Some(Some(v)) = set
        mcp_orchestrator_model: Option<Option<String>>,
    ) -> Result<LabSettings>;
}
```

Implementation: `CachingLabSettingsRepo(PostgresLabSettingsRepository)` — TtlCache 30s wrapper.
`get()` hits in-memory cache (hot path: every image/MCP request). `update()` invalidates cache.
→ See `infra/hot-path-caching.md` for full caching strategy.

Raw impl: `infrastructure/outbound/persistence/lab_settings_repository.rs`
Cache wrapper: `infrastructure/outbound/persistence/caching_lab_settings_repo.rs`

AppState field: `lab_settings_repo: Arc<dyn LabSettingsRepository>`

---

## API

Both endpoints are JWT Bearer–only (dashboard router — not accessible via API key).
Handler: `crates/veronex/src/infrastructure/inbound/http/dashboard_handlers.rs`
(`get_lab_settings` / `patch_lab_settings`)

### `GET /v1/dashboard/lab`

```json
{
  "gemini_function_calling": false,
  "max_images_per_request": 4,
  "max_image_b64_bytes": 2097152,
  "mcp_orchestrator_model": "qwen3:8b",
  "updated_at": "2026-03-25T00:00:00Z"
}
```

### `PATCH /v1/dashboard/lab`

All fields optional. `mcp_orchestrator_model` is a nullable string:
- Key absent → field unchanged
- `"mcp_orchestrator_model": null` → clears the override (use request model)
- `"mcp_orchestrator_model": "qwen3:8b"` → sets the override

```json
{ "mcp_orchestrator_model": "qwen3:8b" }
```

Returns the full updated settings object (same shape as GET).

---

## Frontend

### Context

`LabSettingsProvider` (`web/components/lab-settings-provider.tsx`) — auto-fetches on mount, exposes `{ labSettings, refetch() }` via `useLabSettings()`. Fail-safe default includes `mcp_orchestrator_model: null`.

### Types (`web/lib/types.ts`)

```typescript
export interface LabSettings {
  gemini_function_calling: boolean
  max_images_per_request: number
  max_image_b64_bytes: number
  mcp_orchestrator_model: string | null
  updated_at: string
}

export interface PatchLabSettings {
  gemini_function_calling?: boolean
  max_images_per_request?: number
  max_image_b64_bytes?: number
  mcp_orchestrator_model?: string | null  // null = clear, string = set, absent = no change
}
```

### Context Compression UI (`web/components/nav-settings-dialog.tsx`)

Compression section in Settings → Lab Features:
- `context_compression_enabled` toggle
- `compression_model` dropdown (`CompressionModelSelector` — all Ollama models)
- `handoff_enabled` toggle
- `handoff_threshold` number input (0–1)
- Multi-turn requirements: `multiturn_min_params`, `multiturn_min_ctx`, `multiturn_allowed_models` (comma-separated input)
- Vision model: `vision_model` dropdown (`VisionModelSelector` — Ollama models with `is_vision=true`)

Uses `useOptimistic` + `startTransition` for compression/handoff switches.

### Orchestrator Model Selector (`web/app/providers/components/mcp-tab.tsx`)

`OrchestratorModelSelector` — card rendered at top of the MCP tab:
- Fetches `GET /v1/dashboard/lab` for current value
- Fetches `GET /v1/dashboard/capacity/settings` for `available_models.ollama` list
- On change: `PATCH /v1/dashboard/lab` with `{ mcp_orchestrator_model: value | null }`
- Shows "saved" flash on success

i18n keys: `mcp.orchestratorModel`, `mcp.orchestratorModelDesc`, `mcp.orchestratorModelNone`, `mcp.orchestratorModelSaved`

---

## Adding a New Lab Feature

1. Add column to `lab_settings` (new migration).
2. Add field to `LabSettings` struct and both `get()` / `update()` trait methods.
3. Update `PostgresLabSettingsRepository` SQL.
4. Add the API-level check in the relevant handler(s) (backend gating).
5. Add `useLabSettings()` in every UI component that should be gated.
6. Add i18n keys (en / ko / ja).
7. Add toggle or control in the relevant settings UI.
8. Document here.
