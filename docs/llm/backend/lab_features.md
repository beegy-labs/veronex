# Lab Features (Experimental)

> SSOT | **Last Updated**: 2026-03-02

Lab features are experimental capabilities that are **disabled by default**.
They must be explicitly enabled in Settings → Lab Features.

Gating experimental features behind a flag prevents unstable behavior from
affecting production inference while development is ongoing.

---

## Current Lab Features

| Feature | Default | Status |
|---------|---------|--------|
| `gemini_function_calling` | `false` | 🔬 In development |

---

## `gemini_function_calling`

**What it does**: Enables tool use (function calling) through the Gemini-compatible
endpoint (`POST /v1beta/models/*`).

**When disabled** (default):
- Requests that include `tools[]` return `501 Not Implemented` with:
  ```json
  {
    "error": {
      "code": 501,
      "message": "Gemini function calling is a lab (experimental) feature. Enable it in Settings → Lab Features → Gemini function calling.",
      "status": "UNIMPLEMENTED"
    }
  }
  ```
- Requests without `tools[]` are unaffected — text generation works normally.

**When enabled**:
- `tools[].functionDeclarations` are converted to Ollama format and forwarded with the job.
- The model can return `functionCall` parts in its response.
- Tool calls are streamed back to the client in Gemini SSE format.
- Tool calls are stored in `tool_calls_json` for training data.

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

```
GET /v1/dashboard/lab
    Authorization: Bearer <JWT>
    → {
        "gemini_function_calling": false,
        "updated_at": "2026-03-02T..."
      }

PATCH /v1/dashboard/lab
    Authorization: Bearer <JWT>
    Body: { "gemini_function_calling": true }
    → updated LabSettings
```

Both endpoints require JWT Bearer auth (dashboard-only — not accessible via API key).

---

## Frontend

Lab features are exposed in the **Settings dialog** (nav footer → ⚙️ Settings):

- **Lab Features** section with a `FlaskConical` icon and "Lab" badge.
- Toggle switch per feature (disabled state = grayed out while loading).
- i18n keys: `common.labFeatures`, `common.labFeaturesDesc`,
  `common.labGeminiFunctionCalling`, `common.labGeminiFunctionCallingDesc`
- State loaded from API when the dialog opens; PATCH on toggle.

---

## Adding a New Lab Feature

1. Add a `BOOLEAN NOT NULL DEFAULT false` column to `lab_settings` (new migration).
2. Add the field to `LabSettings` struct and both `get()` / `update()` trait methods.
3. Update `PostgresLabSettingsRepository` `get()` and `update()` SQL.
4. Add the check in the relevant handler(s).
5. Add i18n keys (en / ko / ja).
6. Add a toggle in the Settings dialog (`nav.tsx`).
7. Document here.
