# Code Patterns — 2026 Reference

> SSOT | **Last Updated**: 2026-02-27
> Rust Edition 2024 · Axum 0.8 · sqlx 0.8 · Next.js 15 · React 19 · TanStack Query v5

---

## Rust: Axum 0.8 Handler Signature

Every handler follows this exact signature pattern:

```rust
// Read — returns single resource
pub async fn get_thing(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ThingSummary>, AppError> {
    let thing = state.thing_repo.get(id).await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(to_summary(&thing)))
}

// Create — returns 201 + body
pub async fn create_thing(
    State(state): State<AppState>,
    Json(req): Json<CreateThingRequest>,
) -> Result<(StatusCode, Json<ThingSummary>), AppError> {
    let thing = state.thing_repo.create(req.into()).await?;
    Ok((StatusCode::CREATED, Json(to_summary(&thing))))
}

// Delete — returns 204 No Content
pub async fn delete_thing(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    state.thing_repo.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
```

---

## Rust: Error Handling — AppError (thiserror v2)

2026 standard: define domain errors with `thiserror` → implement `IntoResponse` → handlers use `?` cleanly.

> `thiserror = "2"` is already in `Cargo.toml` but not yet fully adopted. All new handlers should use this pattern.

```rust
// infrastructure/inbound/http/error.rs  ← create this file
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match &self {
            Self::NotFound      => (StatusCode::NOT_FOUND, self.to_string()),
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            Self::Unauthorized  => (StatusCode::UNAUTHORIZED, self.to_string()),
            Self::Internal(e)   => {
                tracing::error!("internal: {e:#}"); // preserve context, hide from client
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error".into())
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

// Optional type alias
pub type ApiResult<T> = Result<Json<T>, AppError>;
```

**Current codebase**: handlers use `impl IntoResponse` + manual `StatusCode` tuples (~50 repetitions).
To migrate: create `error.rs` above → change handler return types to `Result<T, AppError>`.

---

## Rust: sqlx — Compile-Time SQL Verification

```rust
// ✅ Recommended: query_as! + FromRow
// Requires DATABASE_URL in .env at compile time
#[derive(sqlx::FromRow)]
struct BackendRow {
    id: Uuid,
    name: String,
    backend_type: String,
    // ... one field per DB column
}

let row = sqlx::query_as!(
    BackendRow,
    "SELECT id, name, backend_type FROM llm_backends WHERE id = $1",
    id
)
.fetch_optional(&self.pool)
.await?;

// ⚠️ Never use SELECT * — column order breaks with JOINs

// Current codebase: uses query() + manual row_to_entity() mapping.
// New repositories should use query_as! pattern.
```

---

## Rust: async-trait (Required — Do Not Remove)

```rust
// #[async_trait] is STILL required for Arc<dyn Trait> (trait objects)
// Rust 1.75+ async fn in trait is only object-safe with `impl Trait`, not `dyn Trait`
// This project uses Arc<dyn ApiKeyRepository> → keep #[async_trait]

#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    async fn get_by_hash(&self, hash: &str) -> anyhow::Result<Option<ApiKey>>;
}

// ❌ Removing #[async_trait] breaks Arc<dyn ApiKeyRepository> at compile time
```

---

## Rust: tracing + OpenTelemetry

2026 standard: `tracing` crate is the de facto Rust instrumentation standard. Combine with OTel for distributed traces.

```rust
use tracing::{info, error, instrument};

// Add #[instrument] to important handlers and background tasks
#[instrument(skip(state), fields(backend_id = %id))]
pub async fn get_backend(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<BackendSummary>, AppError> {
    info!("fetching backend");
    let b = state.backend_registry.get(id).await?
        .ok_or(AppError::NotFound)?;
    Ok(Json(to_summary(&b)))
}

// Propagate span into spawned tasks
let span = tracing::info_span!("run_job", job_id = %job_id);
tokio::spawn(async move { run_job(state, job_id).await }.instrument(span));

// OTEL_EXPORTER_OTLP_ENDPOINT env → enables gRPC exporter → traces to ClickHouse
```

---

## Rust: Adding a New Port + Adapter

Strict order to respect hexagonal dependency rule:

```
1. domain/entities/new_entity.rs              ← pure struct, no I/O
2. application/ports/outbound/new_port.rs     ← #[async_trait] trait; add to mod.rs
3. migrations/YYYYMMDDHHMMSS_description.sql  ← DB migration
4. infrastructure/outbound/persistence/new.rs ← impl the trait; add to mod.rs
5. infrastructure/inbound/http/state.rs       ← add Arc<dyn NewPort> field
6. main.rs                                    ← init + inject into AppState
7. infrastructure/inbound/http/new_handlers.rs ← use Result<T, AppError>
8. infrastructure/inbound/http/router.rs      ← register routes inside auth middleware
9. docs/llm/backend/new_feature.md            ← CDD doc
```

---

## Frontend: TanStack Query v5

```typescript
// Read — include all fetch dependencies in queryKey
const { data, isPending } = useQuery({
  queryKey: ['backends'],
  queryFn: () => api.backends(),
  staleTime: 30_000,          // reuse cache for 30s before background refetch
})

// Conditional fetch — only when prerequisites are met
const { data } = useQuery({
  queryKey: ['job-detail', jobId],
  queryFn: () => api.jobDetail(jobId!),
  enabled: !!jobId && open,   // fetch only when modal is open
})

// Mutation — always invalidate related query on success
const mutation = useMutation({
  mutationFn: (id: string) => api.deleteBackend(id),
  onSuccess: () => queryClient.invalidateQueries({ queryKey: ['backends'] }),
  onError: (e: Error) => console.error(e.message),
})
mutation.mutate(id)            // fire-and-forget
await mutation.mutateAsync(id) // await inside async handler
```

---

## Frontend: React 19 — useOptimistic

2026 standard: apply optimistic updates to all toggle/switch mutations for perceived speed.

```typescript
import { useOptimistic } from 'react'

// useOptimistic(currentValue, updater)
const [optimisticEnabled, setOptimistic] = useOptimistic(
  model.is_enabled,
  (_, newValue: boolean) => newValue
)

const mutation = useMutation({
  mutationFn: (v: boolean) => api.setModelEnabled(backendId, model.model_name, v),
  onError: () => setOptimistic(model.is_enabled), // auto-revert on failure
})

<Switch
  checked={optimisticEnabled}
  onCheckedChange={(v) => { setOptimistic(v); mutation.mutate(v) }}
/>
// UI responds instantly → server syncs in background → reverts if error
```

---

## Frontend: TypeScript + Zod (API Boundary Validation)

2026 standard: TypeScript enforces compile-time types; Zod validates untrusted API responses at runtime.

```typescript
// web/lib/types.ts
import { z } from 'zod'

// Define schema first, infer type from it
export const BackendSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  backend_type: z.enum(['ollama', 'gemini']),
  status: z.enum(['online', 'offline', 'degraded']),
  is_active: z.boolean(),
})
export type Backend = z.infer<typeof BackendSchema>

// Use safeParse to handle errors gracefully (no throws)
const result = BackendSchema.safeParse(apiResponse)
if (!result.success) console.error(result.error.issues)

// Branded types prevent wrong-ID bugs
const BackendIdSchema = z.string().uuid().brand<'BackendId'>()
type BackendId = z.infer<typeof BackendIdSchema>
```

Apply Zod at entry points: API responses, form inputs, env vars.

---

## Frontend: Tailwind v4 Color Rules

```tsx
// ✅ Use @theme-generated utilities (from tokens.css @theme inline block)
<div className="bg-bg-card text-text-primary border border-border rounded-md p-4">

// ✅ Inline dynamic values via CSS vars
<span style={{ color: 'var(--theme-text-secondary)' }}>

// ✅ Status colors (per design spec, both modes)
const STATUS_COLOR: Record<JobStatus, string> = {
  completed: 'text-emerald-400',  // #34d399
  failed:    'text-rose-400',     // #fb7185
  pending:   'text-amber-400',    // #fbbf24
  running:   'text-blue-400',     // #60a5fa
  cancelled: 'text-slate-400',
}

// ❌ Never: hardcoded hex in style prop
// ❌ Never: non-theme Tailwind color classes (text-slate-700 etc.)
```

---

## Frontend: Adding a New Page

```
1. web/lib/types.ts               ← add TypeScript types (+ Zod schema)
2. web/lib/api.ts                 ← add API functions using req<T>()
3. web/app/new-page/page.tsx      ← 'use client' + useQuery + UI
4. web/components/nav.tsx         ← add navItems entry
5. web/messages/en.json           ← add i18n keys (source of truth)
6. web/messages/ko.json           ← Korean translation
7. web/messages/ja.json           ← Japanese translation
8. docs/llm/frontend/web-*.md     ← update CDD doc
```
