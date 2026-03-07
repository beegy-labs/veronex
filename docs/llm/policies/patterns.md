# Code Patterns: Rust -- 2026 Reference

> SSOT | **Last Updated**: 2026-03-07
> Rust Edition 2024 · Axum 0.8 · sqlx 0.8
> Frontend patterns -> `policies/patterns-frontend.md`

## Axum 0.8 Handler Signature

```rust
// Read
pub async fn get_thing(
  State(state): State<AppState>, Path(id): Path<Uuid>,
) -> Result<Json<ThingSummary>, AppError> {
  let thing = state.thing_repo.get(id).await?.ok_or(AppError::NotFound)?;
  Ok(Json(to_summary(&thing)))
}
// Create -- returns 201
pub async fn create_thing(
  State(state): State<AppState>, Json(req): Json<CreateThingRequest>,
) -> Result<(StatusCode, Json<ThingSummary>), AppError> {
  Ok((StatusCode::CREATED, Json(to_summary(&state.thing_repo.create(req.into()).await?))))
}
// Delete -- returns 204
pub async fn delete_thing(
  State(state): State<AppState>, Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
  state.thing_repo.delete(id).await?;
  Ok(StatusCode::NO_CONTENT)
}
```

## AppError (thiserror v2)

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

## sqlx -- Compile-Time SQL

```rust
#[derive(sqlx::FromRow)]
struct ProviderRow { id: Uuid, name: String, provider_type: String }
let row = sqlx::query_as!(ProviderRow,
  "SELECT id, name, provider_type FROM llm_providers WHERE id = $1", id
).fetch_optional(&self.pool).await?;
// Never SELECT * -- column order breaks with JOINs
```

## async-trait (Required)

`#[async_trait]` still required for `Arc<dyn Trait>`. Rust 1.75+ async fn in trait is object-safe with `impl Trait` only, not `dyn Trait`.

```rust
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
  async fn get_by_hash(&self, hash: &str) -> anyhow::Result<Option<ApiKey>>;
}
```

## tracing + OpenTelemetry

```rust
#[instrument(skip(state), fields(provider_id = %id))]
pub async fn get_provider(
  State(state): State<AppState>, Path(id): Path<Uuid>,
) -> Result<Json<ProviderSummary>, AppError> { ... }
// Propagate span into spawned tasks
let span = tracing::info_span!("run_job", job_id = %job_id);
tokio::spawn(async move { run_job(state, job_id).await }.instrument(span));
```

## DashMap (not `Mutex<HashMap>`)

```rust
let jobs: Arc<DashMap<Uuid, JobEntry>> = Arc::new(DashMap::new());
jobs.insert(id, entry);
let value = jobs.get(&id).map(|r| r.clone());  // clone, drop Ref before .await
let notify = {
  let mut entry = jobs.get_mut(&id).ok_or(NotFound)?;
  entry.tokens.push(token);
  entry.notify.clone()
};  // RefMut dropped here
notify.notify_one();
```

Never hold `Ref`/`RefMut` across `.await` -- it locks the shard.

## Valkey Key Registry

All `veronex:*` key patterns MUST be defined in `infrastructure/outbound/valkey_keys.rs`.
This is the single source of truth — never hardcode key strings elsewhere.

## Valkey Lua Eval

Multi-step Valkey ops must be atomic. Single `EVAL` instead of multiple round-trips.

```rust
const RATE_LIMIT_SCRIPT: &str = r#"
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', ARGV[1])
redis.call('ZADD', KEYS[1], ARGV[2], ARGV[3])
redis.call('EXPIRE', KEYS[1], 62)
return redis.call('ZCARD', KEYS[1])
"#;
let count: u64 = pool.next()
  .eval(RATE_LIMIT_SCRIPT, vec![key], vec![window_start, now_ms, member]).await?;
```

## Performance Patterns

> Full research: `research/backend/rust-perf-2026.md`

**Enum `as_str()`** -- zero-allocation. Never `format!("{:?}", e).to_lowercase()`.

```rust
impl FinishReason {
  pub fn as_str(&self) -> &'static str {
    match self { Self::Stop => "stop", Self::Length => "length", ... }
  }
}
```

**Streaming hash** -- `io::Write` adapter for digest, zero intermediate allocation:

```rust
struct HashWriter<D: Digest>(D);
impl<D: Digest> io::Write for HashWriter<D> {
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> { self.0.update(buf); Ok(buf.len()) }
  fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
// serde_json::to_writer(&mut w, &value)
```

**`Vec::reserve()`** before extend: `accumulated.reserve(arr.len())` then `extend`.

## Domain Services

Pure functions in `domain/services/` — no I/O, no async:

| Service | Function | Purpose |
|---------|----------|---------|
| `password_hashing` | `hash_password(password) → Result<String>` | Argon2id hashing (hexagonal: infra calls domain, not the reverse) |
| `message_hashing` | `hash_messages(msgs) → String` | SHA-256 content hash for deduplication |

## VramPool CAS Safety

`try_reserve()` uses compare-and-swap with `MAX_CAS_RETRIES = 16`:

```rust
for _ in 0..MAX_CAS_RETRIES {
    let current = active_kv.load(Ordering::Acquire);
    if current + kv > kv_budget { return None; }
    if active_kv.compare_exchange_weak(current, current + kv, ...).is_ok() {
        return Some(permit);
    }
}
```

## Timeout & TTL Constants

All timeouts and TTLs are centralized as named constants — never hardcode `Duration::from_secs(N)`.

**Domain layer** (`domain/constants.rs` — importable from all layers):

| Constant | Value | Purpose |
|----------|-------|---------|
| `PROVIDER_REQUEST_TIMEOUT` | 300s | Inference request to Ollama/Gemini |
| `OLLAMA_METADATA_TIMEOUT` | 10s | Ollama `/api/show`, `/api/tags`, `/api/ps` |
| `OLLAMA_HEALTH_CHECK_TIMEOUT` | 5s | Ollama `/api/version` in analyzer |
| `LLM_ANALYSIS_TIMEOUT` | 30s | Single-model LLM analysis |
| `LLM_BATCH_ANALYSIS_TIMEOUT` | 60s | Batch model LLM analysis |
| `NODE_EXPORTER_TIMEOUT` | 5s | Node-exporter metrics fetch |
| `CANCEL_TIMEOUT` | 5s | Job cancellation in CancelGuard |
| `OLLAMA_MODEL_CACHE_TTL` | 10s | Provider-for-model lookup cache |
| `MODEL_SELECTION_CACHE_TTL` | 30s | Provider model-selection enabled list cache |

**Health checker** (`health_checker.rs` — health check specific):

| Constant | Value | Purpose |
|----------|-------|---------|
| `OLLAMA_HEALTH_TIMEOUT` | 5s | Ollama `/api/version` health check |
| `GEMINI_HEALTH_TIMEOUT` | 10s | Gemini API key validation |
| `AGENT_METRICS_TIMEOUT` | 5s | veronex-agent `/api/metrics` poll |

## Background Tasks -- JoinSet + CancellationToken

```rust
let shutdown = CancellationToken::new();
let mut tasks = JoinSet::new();
tasks.spawn(run_health_checker_loop(..., shutdown.child_token()));
axum::serve(listener, app)
  .with_graceful_shutdown(shutdown.clone().cancelled_owned()).await?;
shutdown.cancel();
while let Some(res) = tasks.join_next().await {
  if let Err(e) = res { tracing::warn!("task panicked: {e}"); }
}
```

Loop convention: accept `CancellationToken`, use `select!` to exit cleanly.

## Pool Configuration

```rust
PgPoolOptions::new()
  .max_connections(10).min_connections(2)
  .acquire_timeout(Duration::from_secs(5))
  .idle_timeout(Duration::from_secs(600))
  .max_lifetime(Duration::from_secs(1800))
  .connect(url).await?
```

## Adding a New Port + Adapter

| Step | File | Action |
|------|------|--------|
| 1 | `domain/entities/new_entity.rs` | Pure struct, no I/O |
| 2 | `application/ports/outbound/new_port.rs` | `#[async_trait]` trait; add to mod.rs |
| 3 | `migrations/YYYYMMDDHHMMSS_*.sql` | DB migration |
| 4 | `infrastructure/outbound/persistence/new.rs` | Impl trait; add to mod.rs |
| 5 | `infrastructure/inbound/http/state.rs` | `Arc<dyn NewPort>` field |
| 6 | `main.rs` | Init + inject into AppState |
| 7 | `infrastructure/inbound/http/new_handlers.rs` | `Result<T, AppError>` |
| 8 | `infrastructure/inbound/http/router.rs` | Register routes inside auth middleware |
| 9 | `docs/llm/{domain}/new_feature.md` | CDD doc |

## Domain Enum Patterns

Domain enums (`ProviderType`, `JobStatus`, `JobSource`, `ApiFormat`, `KeyTier`) implement conversion methods directly — no wrapper functions in the infrastructure layer.

```rust
// as_str() — zero-allocation display
impl ProviderType {
    pub fn as_str(&self) -> &'static str {
        match self { Self::Ollama => "ollama", Self::Gemini => "gemini" }
    }
}

// FromStr — used by repositories and handlers
impl std::str::FromStr for ProviderType { ... }

// resource_type() — audit resource identifiers
impl ProviderType {
    pub fn resource_type(&self) -> &'static str {
        match self { Self::Ollama => "ollama_provider", Self::Gemini => "gemini_provider" }
    }
}
```

Repositories use `.as_str()` for INSERT/UPDATE and `.parse::<EnumType>()` for SELECT.
Handlers use `.resource_type()` for audit events instead of `match` blocks.

## SQL Column Constants

Each repository defines a `const *_COLS: &str` for SELECT column lists to avoid duplication:
- `API_KEY_COLS` in `api_key_repository.rs`
- `ACCOUNT_COLS` in `account_repository.rs`
- `PROVIDER_COLS` in `provider_registry.rs`
- `JOB_COLS` in `job_repository.rs`
- `SESSION_COLS` in `session_repository.rs`

Use `format!("SELECT {COLS} FROM table WHERE ...")` for queries.

## Shared Persistence Helpers

Reusable constants and functions in `persistence/mod.rs`:

| Item | Type | Purpose |
|------|------|---------|
| `SOFT_DELETE` | `const &str` | `"AND deleted_at IS NULL"` — appended to WHERE clauses |
| `parse_db_enum::<T>(val, col)` | `fn` | Parse DB string → domain enum via `FromStr`, returns `anyhow::Error` with column context |

## SQL Fragment Constants

Repeated SQL fragments (JOINs, subqueries) are extracted as `const` strings:
- `PRICING_LATERAL` in `usage_handlers.rs` — model pricing LATERAL JOIN used by 3 breakdown queries

```rust
const PRICING_LATERAL: &str = "LEFT JOIN LATERAL (...) pricing ON true";
// Used in: key breakdown, model breakdown, per-key model breakdown
format!("SELECT ... FROM inference_jobs j {PRICING_LATERAL} WHERE ...")
```

## SQL Interval Parameterization

Never interpolate user-controlled intervals as strings. Use `make_interval()`:

```rust
// CORRECT — parameterized
"j.created_at >= NOW() - make_interval(hours => $1)"
// WRONG — SQL injection risk
format!("j.created_at >= NOW() - INTERVAL '{interval}'")
```

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
| `resolve_tenant_id()` | `key_handlers.rs` | Account lookup → username (pub(super)) |
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

## Provider URL Validation (SSRF)

`validate_provider_url()` in `provider_handlers.rs` blocks:
- Non-HTTP schemes (file://, ftp://, gopher://)
- Cloud metadata endpoints (GCP `metadata.google.internal`)
- IPv4 link-local (`169.254.0.0/16` — AWS metadata)
- IPv6 link-local (`fe80::/10`)
- IPv4-mapped IPv6 (`::ffff:169.254.169.254`)
- IPv6 bracket notation parsed correctly (`[::ffff:...]:port`)

Called on provider register and update. See `auth/security.md` for full SSRF details.

## SSE Error Sanitization

Use `sanitize_sse_error()` from `handlers.rs` for all SSE/NDJSON error output:
- Replaces database/network details with generic messages
- Escapes `\r\n` to prevent SSE frame injection
- Truncates to 200 characters

```rust
let err = json!({"error": {"message": sanitize_sse_error(&e)}});
```
