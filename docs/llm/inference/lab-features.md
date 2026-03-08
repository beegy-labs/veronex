# Lab Features (Experimental)

> SSOT | **Last Updated**: 2026-03-02 (rev 3 — full API endpoint specs; component file link)

Lab features are experimental capabilities that are **disabled by default**.
They must be explicitly enabled in Settings → Lab Features.

Gating experimental features behind a flag prevents unstable behavior from
affecting production inference while development is ongoing.

---

## Current Lab Features

| Feature | Default | Status |
|---------|---------|--------|
| `gemini_function_calling` | `false` | In development |

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
| **Overview dashboard** | Provider Status KPI (`online/total`) counts Ollama only; "API Services" provider row hidden; Gemini legend hidden from Top Models chart; Gemini models filtered from chart data |
| **Network Flow panel** | Gemini octagon node, Queue→Gemini path, and Gemini response arc removed from SVG |

**When enabled**:
- `tools[].functionDeclarations` are converted to Ollama format and forwarded with the job.
- The model can return `functionCall` parts in its response.
- Tool calls are streamed back to the client in Gemini SSE format.
- Tool calls are stored in `tool_calls_json` for training data.
- All Gemini UI (nav item, providers tab, dashboard stats, flow panel) is visible.

**Why it's a lab feature**:
- Gemini `functionCall` → Ollama tool_calls format conversion is still being validated.
- Multi-turn tool use (functionResponse → next turn) requires client-side handling.
- Streaming `functionCall` parts via SSE is not yet tested across all client SDKs.

---

## DB Schema

```sql
-- lab_settings table (singleton, id=1 enforced by CHECK constraint)
CREATE TABLE lab_settings (
    id                      INT         PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    gemini_function_calling BOOLEAN     NOT NULL DEFAULT false,
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO lab_settings DEFAULT VALUES;
```

Migration: `20260302000044_lab_settings.sql`

---

## Port

```rust
// application/ports/outbound/lab_settings_repository.rs

#[derive(Debug, Clone)]
pub struct LabSettings {
    pub gemini_function_calling: bool,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait LabSettingsRepository: Send + Sync {
    async fn get(&self) -> Result<LabSettings>;
    async fn update(
        &self,
        gemini_function_calling: Option<bool>,
    ) -> Result<LabSettings>;
}
```

Implementation: `PostgresLabSettingsRepository` in `infrastructure/outbound/persistence/lab_settings_repository.rs`

AppState field: `lab_settings_repo: Arc<dyn LabSettingsRepository>`

---

## API

Both endpoints are JWT Bearer–only (dashboard router — not accessible via API key).
Handler: `crates/veronex/src/infrastructure/inbound/http/dashboard_handlers.rs`
(`get_lab_settings` / `patch_lab_settings`)

### `GET /v1/dashboard/lab`

Returns the current lab feature flags.

```
Authorization: Bearer <JWT>
```

Response `200 OK`:

```json
{
  "gemini_function_calling": false,
  "updated_at": "2026-03-02T00:00:00Z"
}
```

### `PATCH /v1/dashboard/lab`

Updates one or more lab feature flags. All body fields are optional; omitted fields are left unchanged.

```
Authorization: Bearer <JWT>
Content-Type: application/json
```

Request body (`PatchLabSettingsBody`):

```json
{ "gemini_function_calling": true }
```

Response `200 OK` — returns the full updated settings object (same shape as GET):

```json
{
  "gemini_function_calling": true,
  "updated_at": "2026-03-02T12:34:56Z"
}
```

Error `500 Internal Server Error` — DB failure; body `{ "error": "<message>" }`.

---

## Frontend — SSOT Architecture

Lab settings are managed through a single React context.
Component file: `web/components/lab-settings-provider.tsx`

```
LabSettingsProvider (web/components/lab-settings-provider.tsx)
  └── auto-fetches GET /v1/dashboard/lab on mount
  └── exposes { labSettings, refetch() } via useLabSettings() hook
  └── mounted in layout.tsx inside QueryClientProvider
```

**Rule**: every component that needs to gate Gemini UI must call
`useLabSettings()` — never read lab settings in local component state.

**Fail-safe default**: when the fetch fails (e.g. login page, 401), all
features default to `false`, mirroring `LabSettings::default()` in Rust.

**Settings dialog** (nav footer → ⚙️ Settings):

- **Lab Features** section with a `FlaskConical` icon and "Lab" badge.
- Toggle reads from `labSettings` in context; after PATCH calls `refetch()`.
- i18n keys: `common.labFeatures`, `common.labFeaturesDesc`,
  `common.labGeminiFunctionCalling`, `common.labGeminiFunctionCallingDesc`

**Components that gate on `gemini_function_calling`**:

| Component | File | Gate |
|-----------|------|------|
| Nav Gemini item | `web/components/nav.tsx` | Hidden from Providers group |
| Providers page | `web/app/providers/page.tsx` | `?s=gemini` → OllamaTab fallback |
| Dashboard tab | `web/app/overview/components/dashboard-tab.tsx` | Provider KPI counts Ollama only (`visibleBs`); API Providers row hidden; Gemini legend + model bar filtered |
| Flow panel | `web/app/overview/components/provider-flow-panel.tsx` | Gemini node + paths hidden |

---

## Adding a New Lab Feature

1. Add a `BOOLEAN NOT NULL DEFAULT false` column to `lab_settings` (new migration).
2. Add the field to `LabSettings` struct and both `get()` / `update()` trait methods.
3. Update `PostgresLabSettingsRepository` `get()` and `update()` SQL.
4. Add the API-level check in the relevant handler(s) (backend gating).
5. Add `useLabSettings()` in every UI component that should be gated.
6. Add i18n keys (en / ko / ja).
7. Add a toggle in the Settings dialog (`nav.tsx`).
8. Document here, including the table of gated components.
