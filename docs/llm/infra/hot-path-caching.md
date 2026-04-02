# Hot-Path Caching Strategy

> SSOT | **Last Updated**: 2026-03-26

Caching layer to eliminate per-request RDBMS queries on the inference API hot path (`POST /v1/chat/completions`).
Scale target: 10K providers, 1M TPS.

---

## Hot-Path Query Overview

| Priority | Location | Query | Cache | Status |
|----------|----------|-------|-------|--------|
| P1 | `infer_auth` middleware | `SELECT FROM api_keys WHERE key_hash = $1` | TtlCache 60s | Done |
| P1 | `openai_handlers.rs:293` | `SELECT FROM lab_settings WHERE id = 1` (image) | TtlCache 30s | Done |
| P1 | `openai_handlers.rs:604` | `SELECT FROM lab_settings WHERE id = 1` (MCP) | TtlCache 30s | Done |
| P1 | `bridge.rs` `run_loop()` | `SELECT server_id FROM mcp_key_access` | Valkey 60s | Done |
| P1 | `openai_handlers.rs` MCP pre-check | `COUNT(*) FROM mcp_key_access` | Valkey 60s | Done |
| -- | `inference_jobs` INSERT | Required write per request | Not cacheable (consistency required) | Kept |

---

## Cache Implementations

### CachingApiKeyRepo

- **Location**: `infrastructure/outbound/persistence/caching_api_key_repo.rs`
- **TTL**: 60s (in-memory TtlCache per instance)
- **Cache key**: `key_hash` (BLAKE2b-256 -- not sensitive)
- **Cache value**: `Option<ApiKey>` (None is also cached -- negative cache)
- **Invalidation**: revoke / set_active / soft_delete / regenerate / update_fields / set_tier -> `invalidate_all()`
- **Write path (create)**: No cache update (auto-populated on next auth)

**Effect**: Zero DB queries when the same API key authenticates repeatedly within 60s.

```rust
// bootstrap/repositories.rs
let api_key_repo: Arc<dyn ApiKeyRepository> =
    Arc::new(CachingApiKeyRepo::new(Arc::new(
        PostgresApiKeyRepository::new(pg_pool.clone()),
    )));
```

### CachingLabSettingsRepo

- **Location**: `infrastructure/outbound/persistence/caching_lab_settings_repo.rs`
- **TTL**: 30s (in-memory TtlCache per instance)
- **Cache key**: `()` (global singleton -- `lab_settings` table has a single row `id=1`)
- **Invalidation**: `invalidate_all()` after `update()` -- immediate reflection on settings change

**Effect**: Image and MCP request lab_settings DB lookups drop to zero within 30s TTL.

```rust
// bootstrap/repositories.rs
let lab_settings_repo: Arc<dyn LabSettingsRepository> =
    Arc::new(CachingLabSettingsRepo::new(Arc::new(
        PostgresLabSettingsRepository::new(pg_pool.clone()),
    )));
```

### MCP ACL Valkey Cache

- **Location**: `infrastructure/outbound/mcp/bridge.rs` `fetch_mcp_acl()`
- **Valkey key**: `veronex:mcp:acl:{api_key_id}` (TTL 60s)
- **Value**: JSON UUID array -- list of allowed MCP server IDs
- **Empty arrays are cached** -- blocks repeated DB lookups for unauthorized keys (negative cache)
- **Invalidation**: Explicit `DEL` call on grant/revoke in `key_mcp_access_handlers.rs`

---

## TtlCache Pattern (Shared)

`infrastructure/outbound/persistence/ttl_cache.rs` -- shared by all caching wrappers.

| Property | Details |
|----------|---------|
| Implementation | `RwLock<HashMap<K, (V, Instant)>>` |
| Read path | Read lock (fast path) |
| Miss path | Write lock + double-check re-entry |
| Thundering herd | Blocked by double-check |
| Multi-instance | Independent cache per instance -- eventual consistency |

**Existing TtlCache users**:

| Wrapper | TTL | Purpose |
|---------|-----|---------|
| `CachingOllamaModelRepo` | 10s | Model->provider mapping (dispatch hot path) |
| `CachingModelSelection` | 30s | Model activation status |
| `CachingProviderRegistry` | 5s | Provider list snapshot |
| `CachingApiKeyRepo` | 60s | API key auth (inference hot path) |
| `CachingLabSettingsRepo` | 30s | Experimental feature settings (image/MCP hot path) |

---

## Long-Term Direction (Not Implemented)

| Item | Details |
|------|---------|
| `inference_jobs` | Currently PG -- considering migrating routing data to Valkey and analytics data to ClickHouse (long-term) |
| API key Valkey cache | Currently in-memory TtlCache -- upgrade to Valkey when cross-instance instant invalidation is needed |
| Kafka -> ClickHouse | `inference_jobs` writes suit ClickHouse, but worker read path needs Valkey |
