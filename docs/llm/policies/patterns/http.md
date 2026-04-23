# Code Patterns: Rust — HTTP Handlers & Errors

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns.md`](../patterns.md)

## Axum 0.8 Handler Signature

```rust
// Read — path param decoded from "job_3X4aB..." → JobId automatically
pub async fn get_thing(
  State(state): State<AppState>, Path(jid): Path<JobId>,
) -> Result<Json<ThingSummary>, AppError> {
  let thing = state.thing_repo.get(&jid.0).await?.ok_or(AppError::NotFound)?;
  Ok(Json(to_summary(&thing)))
}
// Create -- returns 201; response id encoded as "job_3X4aB..."
pub async fn create_thing(
  State(state): State<AppState>, Json(req): Json<CreateThingRequest>,
) -> Result<(StatusCode, Json<ThingSummary>), AppError> {
  let row = state.thing_repo.create(req.into()).await?;
  Ok((StatusCode::CREATED, Json(ThingSummary { id: JobId::from_uuid(row.id), .. })))
}
// Delete -- returns 204
pub async fn delete_thing(
  State(state): State<AppState>, Path(jid): Path<JobId>,
) -> Result<StatusCode, AppError> {
  state.thing_repo.delete(&jid.0).await?;   // .0 extracts inner Uuid for DB
  Ok(StatusCode::NO_CONTENT)
}
```

| Rule | Detail |
|------|--------|
| `Path<EntityId>` | Use typed entity ID — never `Path<Uuid>` or `Path<String>` + manual parse |
| Response `id` field | Always typed ID (e.g. `id: JobId`) — never raw `Uuid` or `String` |
| `.0` for DB calls | Extract inner UUID with `.0` for `sqlx` binds and repo calls |
| POST create → 201 | Return `(StatusCode::CREATED, Json(...))` — not implicit 200 |
| RequireXxx first | Sensitive handlers must declare a `RequireXxx` extractor before `State` |

→ Full ID encoding policy: `policies/id-encoding.md`

## AppError (thiserror v2) + Problem Details (RFC 9457)

`thiserror` errors + `IntoResponse` impl; handlers use `?`.
Full definition: `infrastructure/inbound/http/error.rs`

```rust
#[derive(Debug, thiserror::Error)]
pub enum AppError {
  NotFound(String),        // 404
  BadRequest(String),      // 400
  Unauthorized(String),    // 401
  Forbidden(String),       // 403
  Conflict(String),        // 409
  TooManyRequests { retry_after: u64 }, // 429
  BadGateway(String),      // 502
  ServiceUnavailable(String), // 503
  UnprocessableEntity(String), // 422
  NotImplemented(String),  // 501
  Internal(anyhow::Error), // 500
}
```

### Error Crate Allocation (strict boundaries)

| Location | Crate | Rule |
|----------|-------|------|
| Domain layer | `thiserror` domain enums only | No `anyhow`; errors are part of the contract |
| Application layer | `thiserror` use-case enums | Map from repository errors into domain-speak |
| Infrastructure adapters | `anyhow::Result` internal | Convert to `AppError` only at the inbound HTTP edge |
| `main.rs` / `bootstrap.rs` | `anyhow::Result` | Top-level startup / signal handling |

Never import `anyhow` into `domain/` or `application/`. Never expose `anyhow::Error` from a public API boundary.

### Problem Details Response Body (RFC 9457)

All 4xx and 5xx responses serialize to `application/problem+json`:

```rust
#[derive(serde::Serialize)]
struct ProblemDetails<'a> {
    #[serde(rename = "type")] typ: &'a str,    // URI — defaults to "about:blank"
    title: &'a str,                            // short, stable summary
    status: u16,
    detail: Option<String>,                    // human-readable, safe to show
    instance: Option<String>,                  // request-specific, e.g. trace id
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, title, detail) = match &self { /* ... */ };
        let body = ProblemDetails {
            typ: "about:blank",
            title,
            status: status.as_u16(),
            detail: Some(detail),
            instance: tracing::Span::current().field("trace_id").map(String::from),
        };
        (status, [(CONTENT_TYPE, "application/problem+json")], Json(body)).into_response()
    }
}
```

Rules:
- `detail` must never contain internal paths, SQL fragments, or upstream error messages. Sanitize.
- `instance` carries the current trace id when tracing is active — makes client-reported bugs traceable to spans.
- `Internal(anyhow::Error)` maps to `title: "Internal Server Error"` with a generic `detail`. Log the full chain via `tracing::error!`.

## Image Inference — 3-Endpoint Support

All three inference formats support image forwarding to Ollama vision models:

| Endpoint | Image source | Extraction |
|----------|-------------|------------|
| `/v1/chat/completions` | `messages[].content[]` array with `type: "image_url"` | `openai_handlers.rs`: `ContentPart.extract_base64_images()` parses `data:...;base64,{data}` from `image_url.url` |
| `/api/chat` | `images` field on request body (Ollama native) | `ollama_compat_handlers.rs`: forwarded from parsed messages |
| `/api/generate` | `images` field on request body | `ollama_compat_handlers.rs`: forwarded directly |

`stream_chat()` in `ollama/adapter.rs` injects images into the last user message (Ollama expects per-message images, not top-level). OpenAI `images` field and content-array images are merged before injection.

## Input Validation

All handlers validate input lengths before processing:
- Prompt/message content: `MAX_PROMPT_BYTES` (1MB) in `constants.rs`
- Model name: `MAX_MODEL_NAME_BYTES` (256) in `constants.rs`
- Error messages: `ERR_MODEL_INVALID`, `ERR_PROMPT_TOO_LARGE` in `constants.rs` — shared across all API formats
- Password: `MIN_PASSWORD_LEN` (8) in `auth_handlers.rs`
- Validation applied per API format (native, OpenAI, Gemini, Ollama)

Shared validation functions in `inference_helpers.rs`:
- `validate_content_length(messages)` — checks total content bytes against `MAX_PROMPT_BYTES`
- `validate_model_name(model)` — checks model name length against `MAX_MODEL_NAME_BYTES`

Native `submit_inference()` in `handlers.rs` delegates to these helpers. Format-specific handlers (OpenAI, Gemini, Ollama) call them directly.

## Shared Handler Helpers

Reusable functions in the HTTP handler layer to avoid duplication:

| Function | File | Purpose |
|----------|------|---------|
| `validate_username()` | `handlers.rs` | Alphanumeric + `_.-`, max 64 chars |
| `validate_content_length()` | `inference_helpers.rs` | Content size validation (SSOT) |
| `validate_model_name()` | `inference_helpers.rs` | Model name length validation (SSOT) |
| `resolve_tenant_id()` | `key_handlers.rs` | Account lookup → username (pub(super)). `list_keys`: super admin uses `list_all()`, others use `list_by_tenant(username)` |
| `convert_tool_call()` | `openai_handlers.rs` | Tool call JSON for streaming + non-streaming |
| `SyncSettingsResponse::from_settings()` | `dashboard_handlers.rs` | Capacity settings → response |
| `filter_by_model_selection()` | `provider_router.rs` | HashSet-based O(1) model filtering (DRY) |

## Cookie TTL Constants

Auth cookie Max-Age values are centralized in `constants.rs`:

| Constant | Value | Must match |
|----------|-------|------------|
| `ACCESS_TOKEN_MAX_AGE` | 3600s (1h) | JWT access token expiry |
| `REFRESH_TOKEN_MAX_AGE` | 604800s (7d) | Session expiry |

Used by `set_auth_cookies()` in `auth_handlers.rs`. Never hardcode cookie TTLs.

## SSE Error Sanitization

Use `sanitize_sse_error()` from `handlers.rs` for all SSE/NDJSON error output:
- Replaces database/network details with generic messages
- Escapes `\r\n` to prevent SSE frame injection
- Truncates to 200 characters

```rust
let err = json!({"error": {"message": sanitize_sse_error(&e)}});
```

