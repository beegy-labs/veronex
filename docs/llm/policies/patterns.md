# Code Patterns: Rust -- 2026 Reference

> SSOT | **Last Updated**: 2026-03-26 | Classification: Operational | Exception: >200 lines (pattern registry)
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

| Rule | Detail |
|------|--------|
| `Path<Uuid>` | Always typed for UUID path segments — never `Path<String>` + `Uuid::parse_str` |
| POST create → 201 | Return `(StatusCode::CREATED, Json(...))` — not implicit 200 |
| RequireXxx first | Sensitive handlers must declare a `RequireXxx` extractor before `State` |

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

## Pagination Pattern

Shared params struct in `handlers.rs` (single definition, imported by all list handlers):

```rust
// infrastructure/inbound/http/handlers.rs
pub struct ListPageParams {
    pub search: Option<String>,
    pub page: Option<i64>,
    pub limit: Option<i64>,
}
```

Handler signature:
```rust
pub async fn list_things(
    State(state): State<AppState>,
    Query(params): Query<ListPageParams>,
) -> Result<Json<serde_json::Value>, AppError> {
    let search = params.search.as_deref().unwrap_or("").trim().to_string();
    let limit = params.limit.unwrap_or(DEFAULT).clamp(1, MAX);
    let page = params.page.unwrap_or(1).max(1);
    let offset = (page - 1) * limit;
    let (items, total) = state.repo.list_page(&search, limit, offset).await?;
    Ok(Json(serde_json::json!({ "things": items, "total": total, "page": page, "limit": limit })))
}
```

Response shape: `{ <plural_name>: [...], total: i64, page: i64, limit: i64 }`

Search uses ILIKE with pg_trgm GIN indexes (migration 000010). Default limits vary per endpoint (20–100); max 1000.

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

## Valkey Error Observability

All Valkey I/O results MUST be handled — never silently discard errors.

```rust
// CORRECT — match with tracing
match pool.mget::<Vec<Option<String>>, _>(keys).await {
    Ok(vals) => vals,
    Err(e) => { tracing::warn!(error = %e, "mcp: failed to fetch heartbeats from Valkey"); vec![] }
}

// CORRECT — unwrap_or_else with tracing
pool.set(key, value, Some(Expiration::EX(ttl)), None, false).await
    .unwrap_or_else(|e| tracing::warn!(error = %e, key, "Valkey SET failed"));

// WRONG
let _ = pool.set(...).await;          // ✗ silent discard
pool.get(key).await.unwrap_or(None)   // ✗ no error logging
```

Security-critical paths (refresh token claims, JTI revocation) must fail-closed: Valkey error → `AppError::ServiceUnavailable`, not silent success.

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
| `HEALTH_CHECK_INTERVAL_SECS` | 30s | Health checker loop interval |
| `STATS_TICK_INTERVAL` | 1s | FlowStats broadcast cadence |

**Health checker** (`health_checker.rs` — health check specific):

| Constant | Value | Purpose |
|----------|-------|---------|
| `OLLAMA_HEALTH_TIMEOUT` | 5s | Ollama `/api/version` health check |
| `GEMINI_HEALTH_TIMEOUT` | 10s | Gemini API key validation |
| `NODE_EXPORTER_METRICS_TIMEOUT` | 5s | node-exporter metrics scrape |

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

### Stats Ticker — Sliding Window Counters

`FlowStats` uses 60 x 1-second sliding-window buckets (not ring-buffer event scanning):

| Field | Computation | Buckets |
|-------|-------------|---------|
| `incoming` | sum of last 10 buckets | req/s = incoming/10 |
| `incoming_60s` | sum of all 60 buckets | = req/m |
| `completed` | sum of all 60 buckets | terminal events |

A separate task counts broadcast events (`pending` -> incoming, terminal -> completed) into the current bucket. The ticker rotates buckets every second, clears the new slot, and always broadcasts -- no PartialEq skip. Clients rely on receiving stats every second.

`queued`/`running` sourced from DashMap (`get_live_counts()`) with DB fallback (single indexed query) when DashMap is empty (e.g. after restart). Not Valkey LLEN -- pops too fast for accurate reads.

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

## RequirePermission Macro

`define_require_permission!` generates Axum `FromRequestParts` extractors that check JWT claims for a specific permission. Super-admin bypasses all checks.

```rust
// Definition (jwt_auth.rs)
macro_rules! define_require_permission {
    ($name:ident, $perm:expr) => { /* reads Claims, checks role==Super || permissions.contains($perm) */ };
}
define_require_permission!(RequireRoleManage, "role_manage");

// Usage in handlers
pub async fn list_roles(RequireRoleManage(_claims): RequireRoleManage, ...) { ... }
```

| Extractor | Permission | Used by |
|-----------|-----------|---------|
| `RequireRoleManage` | `role_manage` | Role CRUD |
| `RequireAccountManage` | `account_manage` | Account CRUD |
| `RequireProviderManage` | `provider_manage` | Provider CRUD |
| `RequireKeyManage` | `key_manage` | API key CRUD |
| `RequireAuditView` | `audit_view` | Audit log |
| `RequireSettingsManage` | `settings_manage` | System settings |
| `RequireApiTest` | `api_test` | Test inference |
| `RequireDashboardView` | `dashboard_view` | Dashboard data |

## Audit Trail (`emit_audit`)

All mutating handlers (POST/PATCH/DELETE) MUST call `emit_audit()` after the DB write succeeds:

```rust
emit_audit(
    &state, &claims,
    "action_verb",            // e.g. "create", "update", "delete", "grant", "revoke"
    "resource_type",          // e.g. "api_key", "account", "mcp_server", "role"
    &resource_id.to_string(),
    &resource_name,
    "Human-readable description of what changed",
).await;
```

Location: `infrastructure/inbound/http/super::emit_audit` (imported as `super::emit_audit`).
Call after the DB mutation succeeds, before returning the response. Non-blocking — fire-and-forget via `.await` is fine.

## Batch DB Writes (N+1 Prevention)

Never execute N sequential queries in a loop. Use UNNEST for inserts, `ANY($1)` for filters and aggregates.

```rust
// CORRECT — UNNEST batch insert/upsert
sqlx::query(
    "INSERT INTO mcp_server_tools (server_id, name, description)
     SELECT * FROM UNNEST($1::uuid[], $2::text[], $3::text[])
     ON CONFLICT (server_id, name) DO UPDATE SET description = EXCLUDED.description"
)
.bind(&server_ids as &[Uuid])
.bind(&names as &[String])
.bind(&descriptions as &[String])
.execute(&pool).await?;

// CORRECT — ANY($1) batch aggregate
let count_rows: Vec<(Uuid, i64)> = sqlx::query_as(
    "SELECT role_id, COUNT(*)::bigint FROM account_roles
     WHERE role_id = ANY($1) GROUP BY role_id"
)
.bind(&role_ids as &[Uuid])
.fetch_all(&pool).await?;
let count_map: HashMap<Uuid, i64> = count_rows.into_iter().collect();

// WRONG — O(N) round-trips
for id in &ids {
    let n = sqlx::query_scalar("SELECT COUNT(*) FROM account_roles WHERE role_id = $1")
        .bind(id).fetch_one(&pool).await?; // one DB call per row
}
```

**SQL LIMIT**: all `fetch_all` list queries must have an explicit `LIMIT` clause — unbounded SELECT is prohibited at 10K+ provider scale.

| Scope | Minimum LIMIT |
|-------|--------------|
| Admin list queries (servers, keys, accounts) | 500 |
| Role membership counts | 200 |
| Aggregate / GROUP BY | Commensurate with distinct count (e.g. 8760 for hourly, 10 for statuses) |

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

## SQL Multi-Value Filters

Use `string_to_array` + `ANY` for comma-separated status filters instead of `= $1`:

```rust
// CORRECT — supports "pending,running" as single parameter
"j.status = ANY(string_to_array($1, ','))"
// WRONG — only matches a single value
"j.status = $1"
```

Used in `dashboard_queries.rs` for job status filtering (live feed, dashboard jobs).

## SQL Interval Parameterization

Never interpolate user-controlled intervals as strings. Use `make_interval()`:

```rust
// CORRECT — parameterized
"j.created_at >= NOW() - make_interval(hours => $1)"
// WRONG — SQL injection risk
format!("j.created_at >= NOW() - INTERVAL '{interval}'")
```

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

## Provider URL Validation (SSRF)

`validate_provider_url()` in `provider_handlers.rs` blocks:
- Non-HTTP schemes (file://, ftp://, gopher://)
- Cloud metadata endpoints (GCP `metadata.google.internal`)
- IPv4 link-local (`169.254.0.0/16` — AWS metadata)
- IPv6 link-local (`fe80::/10`)
- IPv4-mapped IPv6 (`::ffff:169.254.169.254`)
- IPv6 bracket notation parsed correctly (`[::ffff:...]:port`)

Called on provider register and update. See `auth/security.md` for full SSRF details.

## Provider Liveness — Push Model (Heartbeat)

Scale target: 10,000+ providers, tens of thousands req/s.
Do NOT poll providers directly from veronex.
Use the push model: veronex-agent sets a TTL heartbeat; veronex reads via MGET.

| Component | Responsibility |
|-----------|---------------|
| `veronex-agent/src/heartbeat.rs` | `set_online(pool, provider_id, ttl_secs)` after each successful Ollama scrape |
| `valkey_keys::provider_heartbeat(id)` | Key SSOT: `veronex:provider:hb:{uuid}` |
| `health_checker.rs` | MGET all known heartbeat keys → one round-trip; missing key = offline |
| `valkey_keys::PROVIDERS_ONLINE_COUNTER` | `INCR`/`DECR` atomically on status transitions → O(1) dashboard reads |

## Job Counters — Valkey INCR/DECR

O(1) pending/running counts for dashboard. No DB polling in hot path.

| Key | Update | Read |
|-----|--------|------|
| `JOBS_PENDING_COUNTER` | INCR on submit, DECR on dispatch/cancel/fail | stats ticker GET |
| `JOBS_RUNNING_COUNTER` | INCR on dispatch, DECR on complete/fail/cancel | stats ticker GET |

| Safety | Detail |
|--------|--------|
| Double-DECR prevention | Check previous status before DECR |
| Startup reconciliation | DB COUNT → Valkey SET at boot |
| Periodic reconciliation | Every 60s: DB COUNT vs Valkey GET → SET if drift |
| Valkey unavailable | Fallback to DB query |

**TTL rule**: `heartbeat_ttl ≥ 3 × scrape_interval` — survives 2 missed cycles.

**Fallback**: when Valkey is absent, health_checker falls back to semaphore-limited (64) concurrent HTTP probes.

```rust
// Reading liveness — O(1) Valkey instead of N × HTTP
let keys: Vec<String> = active.iter().map(|p| valkey_keys::provider_heartbeat(p.id)).collect();
let values: Result<Vec<Option<String>>, _> = pool.mget(keys).await;
// Some(str) = online, None = TTL expired = offline
```

**Key format test**: `heartbeat::key()` is pure — test it to guard crate-boundary drift.

## Scale Guards — 10K+ Provider Patterns

| Pattern | Location | Detail |
|---------|----------|--------|
| `MAX_SCORING_CANDIDATES = 50` | `dispatcher.rs` | Bounds scoring loop: O(10K) → O(50) |
| `MAX_CONCURRENT_METRICS = 64` | `health_checker.rs` | Semaphore limits concurrent node-exporter polls |
| `MAX_CONCURRENT_PROBES = 64` | `health_checker.rs` | Semaphore limits HTTP health probes (no-Valkey fallback) |
| `pg_class.reltuples` | `dashboard_queries.rs` | O(1) total_jobs estimate instead of COUNT(*) |
| `join_all` parallelism | `dispatcher.rs`, `placement_planner.rs` | Parallel Valkey/DB calls instead of sequential loops |
| `concurrent_http_probes()` | `health_checker.rs` | Bounded parallel HTTP for MGET fallback |
| No-Valkey DB cache | `background.rs` | DB query every 10s (not 1s) when Valkey absent |

## SSE Error Sanitization

Use `sanitize_sse_error()` from `handlers.rs` for all SSE/NDJSON error output:
- Replaces database/network details with generic messages
- Escapes `\r\n` to prevent SSE frame injection
- Truncates to 200 characters

```rust
let err = json!({"error": {"message": sanitize_sse_error(&e)}});
```

## Orphan Sweeper — Agent-Side Crash Recovery

Detects crashed API instances and fails their orphaned jobs. Runs in `veronex-agent`, not in the API server. API servers manage their own INCR/DECR during normal operation; the agent only intervenes when an API server is confirmed dead.

### Separation of Concerns

| Component | Responsibility |
|-----------|---------------|
| API server (`reaper.rs`) | Heartbeat refresh (SET EX 30s, every 10s) + SADD to `INSTANCES_SET` + re-enqueue orphaned jobs (second chance) |
| Agent (`orphan_sweeper.rs`) | Monitor heartbeats, detect death, fail orphaned jobs in DB, DECR counters, SREM from instance set |

### Instance Registry

| Key | Type | Purpose |
|-----|------|---------|
| `veronex:instances` | SET | All API instance IDs (SADD on heartbeat + startup) |
| `veronex:heartbeat:{id}` | STRING EX 30s | Instance liveness (refreshed every 10s) |
| `veronex:suspect:{id}` | STRING EX 180s | Grace period marker (2-min confirmation) |
| `veronex:reaped:{id}` | STRING NX EX 86400s | Prevents duplicate cleanup (24h) |
| `veronex:job:owner:{uuid}` | STRING EX 300s | Maps running job to owning instance |

### 2-Minute Suspect Grace Period

```
Heartbeat missing → SET suspect EX 180 → wait
TTL drops to ≤ 60  → 2+ minutes elapsed → confirmed dead
SET reaped NX      → claim cleanup (single execution)
```

Network blips (< 2 min) do not trigger cleanup. The suspect marker auto-expires after 3 min if the instance recovers.

### Shard Distribution (10K Scale)

| Sweep | Interval | Scope |
|-------|----------|-------|
| Shard sweep | 30s | `hash(instance_id) % replicas == ordinal` — each agent handles its shard |
| Leader sweep | 60s | NX lock — one agent fails jobs from deleted/inactive providers |

### Cleanup Actions

1. Find jobs owned by dead instance (Valkey `processing` list + `job:owner` keys)
2. UPDATE DB: `status = 'failed'`, `failure_reason = 'server_crash'`
3. LREM from processing list, DEL owner key
4. DECR `JOBS_RUNNING_COUNTER` / `JOBS_PENDING_COUNTER`
5. Belt-and-suspenders: DB query for `instance_id` match (catches jobs not in Valkey list)
6. SREM from `INSTANCES_SET`, DEL suspect marker

### Restart Behavior

All agents down then restart: `tokio::time::interval` fires immediately on first tick, triggering an immediate scan and cleanup of any dead instances found.

## Cross-Module Error Sentinel Constants

Use a `const &str` to share error markers across module boundaries instead of duplicating string literals.

```rust
// session.rs — define once
pub(crate) const SESSION_EXPIRED_MARKER: &str = "session expired";

// client.rs — use in error construction
return Err(anyhow!("MCP {SESSION_EXPIRED_MARKER} (404) for {}", session.url));

// bridge.rs — use in match guard
Err(e) if e.to_string().contains(SESSION_EXPIRED_MARKER) => { ... }
```

Prevents silent drift when one side is renamed. `pub(crate)` keeps the sentinel internal.

## Docker Build Cache — `sharing=locked`

All `--mount=type=cache` directives for the Cargo registry and target directory must use `sharing=locked`:

```dockerfile
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release -p my-crate --recipe-path recipe.json
```

Without `sharing=locked`, parallel `docker compose build` services extracting the same crates simultaneously cause `EEXIST (os error 17)` failures. Apply to both `cargo chef cook` and `cargo build` steps in every service Dockerfile.

## Test Code Conventions

| Rule | Rationale |
|------|-----------|
| **Pure function tests** | No external state (env, fs, network, shared mutex) — `cargo test` parallel safe |
| **Avoid duplicate tests** | Merge tests verifying the same property (e.g., if determinism ⊂ uniqueness, keep only the uniqueness test) |
| **1 test = 1 property** | Each test verifies one unique property — the name is the spec |
| **env var tests** | Never call `env::var()` directly → validate parsing logic inline only (prevents race conditions) |
| **DOS boundary values** | Cap tests for `MAX_*` constants are required |

```rust
// Good: pure, individual, non-overlapping
#[test]
fn no_duplicates() {  // uniqueness check (implies determinism)
    for id in &["a", "b", "c"] {
        let owners: Vec<u32> = (0..3).filter(|&o| owns(id, o, 3)).collect();
        assert_eq!(owners.len(), 1);
    }
}

// Bad: duplicate (subset of the above test)
#[test]
fn deterministic_assignment() {  // determinism is trivial once uniqueness is proven
    assert!(owns("a", owner, 3));
}
```

## UTF-8 Safe Truncation

All string truncation must respect UTF-8 char boundaries. Use the shared utility in `veronex_mcp::truncate_at_char_boundary` instead of calling `String::truncate(n)` directly.

```rust
// CORRECT — via shared utility (veronex-mcp crate)
use veronex_mcp::truncate_at_char_boundary;
truncate_at_char_boundary(&mut s, MAX_BYTES);

// CORRECT — inline (when veronex_mcp not in scope)
let boundary = (0..=max_len).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
s.truncate(boundary);

// WRONG — panics on multi-byte char boundaries
s.truncate(MAX_BYTES);
```

The audit grep:
```bash
grep -rn "\.truncate(" crates/ --include="*.rs"
```
Expected: all calls preceded by `is_char_boundary()` reverse-scan or delegated to `truncate_at_char_boundary()`.

## MCP Integration Patterns

### Two-Level Tool Cache (L1 DashMap + L2 Valkey)

```
L1: DashMap<Uuid, CachedTools>  TTL 30s  — per-replica in-process
L2: Valkey SET                  TTL 35s  — cross-replica shared
Lock: Valkey SET NX             TTL 33s  — prevents thundering herd
```

Refresh sequence:
1. Check L1 TTL — if valid, return immediately (zero network)
2. Attempt `SET NX lock` — only one replica fetches at a time
3. Fetch from MCP server via `McpSessionManager`
4. Write to L1 + L2 atomically
5. Release lock

```rust
// SET NX prevents multiple replicas hitting the MCP server simultaneously
let locked: bool = conn.set(&lock_key, "1", Some(Expiration::EX(LOCK_TTL_SECS)), Some(SetOptions::NX), false).await.unwrap_or(false);
if !locked { return; }
```

### MCP Valkey Key Convention (Cross-Crate)

`veronex-mcp` defines its own key strings locally (cross-crate OK, unlike `veronex` which must use `valkey_keys.rs`).
All `veronex-mcp` keys use the `veronex:mcp:` namespace:

| Key | TTL | Purpose |
|-----|-----|---------|
| `veronex:mcp:tools:{server_id}` | 35s | L2 tool list |
| `veronex:mcp:tools:lock:{server_id}` | 33s | Refresh NX lock |
| `veronex:mcp:heartbeat:{server_id}` | set by agent | Server liveness |
| `veronex:mcp:result:{tool}:{args_hash}` | 300s | Result cache |

Rule: cross-crate local key definitions are allowed, but must be guarded with format tests.

### Input Size Guards (OOM/DoS Prevention)

Every entry point that accepts external data must have `MAX_*` constants bounding input size.

Current MCP guards:

| Constant | Value | Location | Purpose |
|----------|-------|----------|---------|
| `MAX_TOOLS_PER_SERVER` | 1,024 | `client.rs` | tools/list response |
| `MAX_TOOL_DESCRIPTION_BYTES` | 4,096 | `client.rs` | Tool description field |
| `MAX_TOOL_SCHEMA_BYTES` | 16,384 | `client.rs` | inputSchema serialized size |
| `MAX_TOOL_RESULT_BYTES` | 32,768 | `bridge.rs` | LLM injection size |
| `MAX_ARGS_FOR_HASH_BYTES` | 4,096 | `bridge.rs` | Loop-detect hashing input |
| `MAX_CANONICAL_DEPTH` | 16 | `result_cache.rs` | JSON recursion depth |
| `MAX_TOOLS_PER_REQUEST` | 32 | `bridge.rs` | Context window protection |

Rule: always pair a `MAX_*` const with a test verifying the boundary does not panic.

### Agentic Loop Duplicate Detection

Prevents infinite loops where the model repeatedly calls the same tool with identical arguments.

```rust
// (tool_name, args_hash) → call count
let mut call_sig_counts: HashMap<(String, String), u8> = HashMap::new();
// ...
let args_hash = quick_args_hash(args_str);
let count = call_sig_counts.entry((name.clone(), args_hash)).or_insert(0);
*count += 1;
if *count >= LOOP_DETECT_THRESHOLD { break; }
```

Bounds: `MAX_ROUNDS(5) × MAX_TOOLS(32) = 160` — the HashMap is fully bounded, not unbounded.

### Canonical JSON for Cache Keys

Args hash for result cache must be order-independent (same args regardless of key ordering).

```rust
fn canonical_json(v: &serde_json::Value, depth: u8) -> String {
    if depth >= MAX_CANONICAL_DEPTH { return "\"...\"".to_owned(); }  // stack overflow guard
    match v {
        serde_json::Value::Object(map) => {
            let mut pairs: Vec<_> = map.iter().collect();
            pairs.sort_by_key(|(k, _)| *k);  // deterministic key order
            // ...
        }
    }
}
// key: SHA-256(tool_name + ":" + canonical_json(args)), first 8 bytes hex-encoded
```

---

## Quarterly Audit Commands

Run these greps to surface violations. Expected results noted per check.

```bash
# P1 — SSRF: validate_provider_url called for URL inputs
grep -rn "url.*String\|String.*url" crates/veronex/src/infrastructure/inbound/ --include="*.rs" -l
# → check each file for validate_provider_url call

# P1 — SQL Interval: must use make_interval(), never format!()
grep -rn "INTERVAL.*format!\|format!.*INTERVAL" crates/ --include="*.rs"
# → expected: 0 results

# P1 — UTF-8 truncation: must use is_char_boundary() or truncate_at_char_boundary()
grep -rn "\.truncate(" crates/ --include="*.rs"
# → all calls must be preceded by is_char_boundary() scan or delegated to truncate_at_char_boundary()

# P1 — Valkey: no silent error discard on I/O
grep -rn "\.await\.unwrap_or\b\|let _.*\.await\|\.await\.ok()" crates/veronex/src/infrastructure/inbound/ --include="*.rs"
# → each result must be checked: Valkey I/O must log at tracing::warn! on error

# P2 — Valkey key hardcoding: all veronex:* keys via valkey_keys.rs only
grep -rn '"veronex:' crates/veronex/src/ | grep -v valkey_keys
# → expected: 0 results

# P2 — Magic Duration: all timeouts via named const
grep -rn "Duration::from_secs([0-9]" crates/ --include="*.rs" | grep -v "const "
# → expected: 0 results

# P2 — O(N) DB scan: COUNT(*) in dashboard hot paths
grep -rn "COUNT(\*)" crates/veronex/src/ --include="*.rs"
# → dashboard_queries.rs must use pg_class.reltuples instead

# P2 — Unbounded SELECT: all fetch_all must have LIMIT
grep -rn "fetch_all" crates/veronex/src/infrastructure/inbound/ --include="*.rs" -B5 | grep -v "LIMIT\|ANY\|UNNEST\|--"
# → each result must have a LIMIT clause in the SQL string above it

# P2 — N+1: fetch_all/fetch_one inside for loop
grep -rn "for.*in.*{" crates/veronex/src/ --include="*.rs" -A6 | grep "fetch_all\|fetch_one\|\.execute("
# → expected: 0 results (use UNNEST or ANY($1) instead)

# P2 — Docker cache: sharing=locked on all cache mounts
grep -rn "mount=type=cache" Dockerfile* **/Dockerfile* 2>/dev/null
# → all --mount=type=cache entries must have sharing=locked

# P3 — Cross-module error sentinel: no duplicated string literals
grep -rn '"session expired"' crates/ --include="*.rs"
# → expected: 1 definition (pub(crate) const), matched via .contains() elsewhere
```
