# Code Patterns: Rust — Tracing, Metrics & Auditing

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns.md`](../patterns.md)

## tracing + OpenTelemetry

Minimum: OpenTelemetry Rust ≥ 0.30 (Metrics SDK graduated stable at 0.30). Current codebase runs **0.31**. All `opentelemetry*` crates use the **same minor version** — mixed-version pins cause runtime type mismatches.

### Workspace Version Rule

```toml
# [workspace.dependencies]  — bump all four together when upgrading
opentelemetry           = "0.31"
opentelemetry_sdk       = "0.31"
opentelemetry-otlp      = { version = "0.31", features = ["grpc-tonic", "tls-roots"] }
tracing-opentelemetry   = "0.32"   # matches OTel 0.31 at time of writing
```

Upgrade rule: bump all four on the same PR, verify exporters still connect, and update this doc with the new minor.

### Span Naming — OTel Semantic Convention

| Span kind | Name format | Example |
|-----------|-------------|---------|
| HTTP server | `{METHOD} {route_template}` | `POST /v1/providers` |
| HTTP client | `{METHOD}` + attribute `http.url` | `POST` with `http.url="http://ollama/api/chat"` |
| DB query | `{operation} {table}` | `SELECT llm_providers` |
| Message queue | `{op} {destination}` | `publish otel-metrics` |

Use literal route templates (`{id}` placeholder), never concrete values — prevents high-cardinality span names.

### Handler Instrumentation

```rust
#[instrument(
    skip(state),
    fields(provider_id = %id, http.route = "/v1/providers/:id"),
    name = "GET /v1/providers/:id",
)]
pub async fn get_provider(
  State(state): State<AppState>, Path(id): Path<Uuid>,
) -> Result<Json<ProviderSummary>, AppError> { ... }
```

### Span Propagation into Spawned Tasks

```rust
let span = tracing::info_span!("run_job", job_id = %job_id);
tokio::spawn(async move { run_job(state, job_id).await }.instrument(span));
```

Spawned tasks without `.instrument(span)` lose trace context → appear as orphan spans in the backend. **Every `tokio::spawn` in a request handler MUST propagate a span**.

### Resource Attributes (required on every service)

```rust
Resource::new([
    KeyValue::new("service.name",           env!("CARGO_PKG_NAME")),
    KeyValue::new("service.version",        env!("CARGO_PKG_VERSION")),
    KeyValue::new("deployment.environment", env::var("DEPLOY_ENV").unwrap_or_else(|_| "dev".into())),
])
```

### Metric Instrument Selection

| Instrument | Use |
|------------|-----|
| `Counter<u64>` | Monotonic rates — requests, errors, tokens emitted |
| `UpDownCounter<i64>` | In-flight gauges — active connections, queue depth |
| `Histogram<f64>` | Latency, size distributions (request/response bytes) |
| `Gauge<f64>` (async) | Periodically-sampled values — VRAM used, temp |

Never use `Counter` for a value that can decrease. Never use `Histogram` for a single value that should be a gauge.

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

**Common omission:** `emit_audit` is frequently missing from handlers added without consulting this doc. A 2026-04-07 audit found it absent in 9 handlers (across `key_mcp_access_handlers`, `key_provider_access_handlers`, `ollama_model_handlers`, `mcp_handlers`, `gemini_model_handlers`, `dashboard_handlers`). Run the quarterly audit grep after adding any mutating handler.

**Prerequisite:** the handler must capture `RequireXxx(claims)` — not `RequireXxx(_)` — to have `claims` available for `emit_audit`. If no `RequireXxx` extractor exists at all, the handler has a missing auth guard (P1 security issue).

