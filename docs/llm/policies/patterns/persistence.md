# Code Patterns: Rust — sqlx & Database Patterns

> SSOT | **Last Updated**: 2026-04-22 | Classification: Operational
> Parent index: [`../patterns.md`](../patterns.md)

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

Search uses ILIKE with pg_trgm GIN indexes (`docker/postgres/init.sql`). Default limits vary per endpoint (20–100); max 1000.

## Pool Configuration

```rust
PgPoolOptions::new()
  .max_connections(max_conns)        // (vCPU × 2) + 1, cap at 20; read from PG_POOL_MAX env
  .min_connections(2)                // warm floor — avoids cold-start latency on bursts
  .acquire_timeout(Duration::from_secs(5))   // fail fast — shorter than HTTP timeout
  .idle_timeout(Duration::from_secs(600))    // recycle idle conns that may have gone stale
  .max_lifetime(Duration::from_secs(1800))   // force rotation to avoid long-lived state
  .statement_cache_capacity(512)     // reduce parse/plan overhead; 0 if behind PgBouncer tx-mode
  .test_before_acquire(false)        // skip ping on acquire — saves one round-trip per query
  .connect(url).await?
```

| Rule | Detail |
|------|--------|
| `acquire_timeout` | 5s — must be shorter than HTTP timeout so callers fail fast |
| `statement_cache_capacity` | 512 for fixed SQL queries; set to 0 if behind PgBouncer in transaction-pooling mode |
| `test_before_acquire(false)` | Default `true` adds a ping on every acquire. Safe to disable when `max_lifetime` + `idle_timeout` handle stale connections |

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
| Internal / cross-provider batch queries (model lists, capacity profiles, heartbeats) | 10000 |
| Role membership counts | 200 |
| Aggregate / GROUP BY | Commensurate with distinct count (e.g. 8760 for hourly, 10 for statuses) |

**Recurrence:** a 2026-04-07 audit found missing LIMITs in 10+ repositories (`account_repository`, `api_key_repository`, `provider_registry`, `gemini_policy_repository`, `session_repository`, `model_capacity_repository`, `gpu_server_registry`, `global_model_settings`, `ollama_model_repository`, `provider_model_selection`). Run the quarterly audit grep after adding any `fetch_all` query.

## L1 (Valkey) + L2 (Postgres) Cached Lookup Pattern

When two or more functions share the shape "Valkey cache hit → return; cache miss → single-row SQL → repopulate cache", extract a generic helper instead of duplicating the body. Canonical example: `mcp::bridge::cached_mcp_int_lookup`.

```rust
async fn cached_mcp_int_lookup(
    state: &AppState,
    vk_key: String,
    sql: &'static str,
    key_id: Uuid,
    log_label: &'static str,
) -> Option<i16> {
    if let Some(ref pool) = state.valkey_pool
        && let Ok(Some(cached)) = pool.get::<Option<String>, _>(&vk_key).await
    {
        if cached == "null" { return None; }
        if let Ok(v) = cached.parse::<i16>() { return Some(v); }
    }
    let result: Option<i16> = sqlx::query_scalar(sql)
        .bind(key_id).fetch_optional(&state.pg_pool).await.ok().flatten();
    if let Some(ref pool) = state.valkey_pool {
        let val = result.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string());
        let _ = pool.set::<(), _, _>(&vk_key, val,
            Some(Expiration::EX(MCP_KEY_CACHE_TTL_SECS)), None, false).await;
    }
    result
}

// Callers become 1-line wrappers
async fn fetch_mcp_cap_points(state: &AppState, key_id: Uuid) -> Option<u8> {
    cached_mcp_int_lookup(state, mcp_key_cap_points(key_id),
        "SELECT mcp_cap_points FROM api_keys WHERE id = $1",
        key_id, "cap_points").await.map(|v| v as u8)
}
```

**Sentinel for NULL**: cache `"null"` string distinguishes "row exists but column is NULL" from "row absent". Required for invalidation correctness — mutation paths must `kv_del` not `kv_set` when clearing.

## Batch MGET over collection

Replace `for id in collection { pool.get(key(id)).await }` (N round-trips) with a single `pool.mget(keys)` call (1 round-trip). Canonical example: `inference_helpers::lookup_model_max_ctx`.

```rust
let keys: Vec<String> = providers.iter()
    .filter(|p| p.is_ollama())
    .map(|p| ollama_model_ctx(p.id, model_name))
    .collect();
let raw: Vec<Option<String>> = pool.mget(keys).await.ok()?;
raw.iter().flatten()
    .filter_map(|s| serde_json::from_str::<Value>(s).ok())
    .find_map(|v| v["configured_ctx"].as_u64().filter(|&n| n > 0))
    .map(|n| n as u32)
```

`find_map` preserves "first match" early-break semantics; the MGET still issues exactly one round-trip regardless.

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

