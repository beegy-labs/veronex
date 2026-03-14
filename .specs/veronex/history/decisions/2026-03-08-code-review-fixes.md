# Code Review Fix Handoff — 2026-03-08

> Cross-check document for reviewers. All fixes applied on `feat/api-key-usage` branch.

## Verification Commands

```bash
cargo test --workspace --all-targets     # 205 passed, 0 failed
cargo clippy --all-targets --all-features -- -D warnings  # 0 errors
cd web && npm test -- --run              # 40 passed
cd web && npm run build                  # clean
```

---

## Phase 1 — Security & Correctness (12 items)

### 1. JWT Revocation: Fail-Open → Fail-Closed
**File**: `crates/veronex/src/infrastructure/inbound/http/middleware/jwt_auth.rs`
**Change**: `.unwrap_or(false)` → `.map_err(|e| AppError::ServiceUnavailable(...))?`
**Verify**: Valkey down → 503 (not silent pass-through). Check `is_revoked` error path.

### 2-3. Refresh Token TOCTOU → Atomic SET NX
**File**: `crates/veronex/src/infrastructure/inbound/http/auth_handlers.rs`
**Change**: Replaced separate `is_refresh_token_used()` + `blocklist_refresh_token()` with `atomic_claim_refresh_token()` using `SET NX + EX`.
**Verify**: Single Valkey round-trip. `SetOptions::NX` (not `SetPolicy`). Returns `Ok(false)` on replay. Fail-closed on Valkey error.

### 4. Audit SQL Injection → Whitelist
**File**: `crates/veronex-analytics/src/handlers/audit.rs`
**Change**: Added `ALLOWED_ACTIONS` and `ALLOWED_RESOURCE_TYPES` arrays. Validates before query interpolation. Let-chain syntax for clippy.
**Verify**: Unknown action/resource_type → 400. Whitelist matches domain actions. 4 unit tests.

### 5. Analytics Hours Validation
**Files**: `usage.rs`, `performance.rs` (veronex-analytics)
**Change**: `if q.hours == 0 || q.hours > 8760 { return Err(BAD_REQUEST); }` on all 5 handlers.
**Verify**: hours=0, hours=8761 → 400. hours=1, hours=8760 → OK. Tests use `is_valid_hours()` helper (not tautological assertions).

### 6-7. ts-rs Sensitive Field Exclusion
**Files**: `domain/entities/account.rs`, `domain/entities/api_key.rs`, `web/lib/generated/Account.ts`, `web/lib/generated/ApiKey.ts`
**Change**:
- Account: `#[ts(skip)]` on `password_hash` (already had `skip_serializing`), `created_by`, `deleted_at`
- ApiKey: `#[serde(skip_serializing)] #[ts(skip)]` on `key_hash`; `#[ts(skip)]` on `deleted_at`, `key_type`
- Generated TS files updated to remove excluded fields
- ApiKey serde test updated: roundtrip → serialization-only (since `key_hash` no longer serializes)
**Verify**: `Account.ts` has no `password_hash`, `created_by`, `deleted_at`. `ApiKey.ts` has no `key_hash`, `deleted_at`, `key_type`. `tsc --noEmit` clean. No frontend imports from generated types.

### 8. Bootstrap VALKEY_URL → Config SSOT
**File**: `crates/veronex/src/bootstrap/background.rs`
**Change**: Replaced `std::env::var("VALKEY_URL").unwrap()` (2 occurrences) with `config.valkey_url.as_deref().expect(...)`. Config already has `valkey_url: Option<String>`.
**Verify**: No `std::env::var("VALKEY_URL")` in background.rs. Uses `config` parameter that's already passed to the function.

### 10. Clippy Tautological Tests
**Files**: `performance.rs`, `usage.rs`, `audit.rs` (analytics)
**Change**: Replaced `assert!(0_u32 == 0)` style tautologies with `is_valid_hours()` helper + meaningful assertions. Audit tests use local constants (not `use super::*` which was unused).
**Verify**: `cargo clippy -p veronex-analytics --all-targets -- -D warnings` clean.

### 12. CI Web Build Step
**File**: `.github/workflows/ci.yml`
**Change**: Added `npm run build` step after unit tests in `check-web` job.
**Verify**: Build failures now block PR merge.

---

## Phase 2 — Architecture & Performance (6 items)

### 13. Circuit Breaker P99 Latency Hybrid
**Files**: `infrastructure/outbound/circuit_breaker.rs`, `domain/constants.rs`, `application/ports/outbound/circuit_breaker_port.rs`
**Change**:
- `ProviderCircuit` now holds `VecDeque<u64>` latency buffer (window=100)
- `record_latency()` method: if P99 > 30s threshold (min 20 samples) → Closed→HalfOpen
- Does NOT override failure-based Open state
- `CircuitBreakerPort` trait: added `record_latency(provider_id, latency_ms)`
- 3 new constants: `CIRCUIT_BREAKER_LATENCY_WINDOW`, `_MIN_SAMPLES`, `_P99_THRESHOLD_MS`
**Verify**: 7 unit tests. Existing failure logic unchanged. Latency is additive only.

### 15. Analytics Code Deduplication
**Files**: `handlers/mod.rs`, `handlers/usage.rs`, `handlers/performance.rs` (analytics)
**Change**: Extracted 5 shared helpers to `mod.rs`:
- `HoursQuery` struct (replaces `UsageQuery` + `PerfQuery`)
- `validate_hours()` → replaces 5 inline checks
- `format_rfc3339()` → replaces 4 repeated format calls
- `success_rate()` → replaces 3 inline calculations
- `ch_query_error()` → replaces 9 repeated map_err blocks
**Verify**: No local `default_hours()` or query structs in usage.rs/performance.rs. 46 analytics tests pass.

### 16. OTel Timestamp Semantics
**File**: `crates/veronex-analytics/src/otel.rs`
**Change**: `emit()` now accepts `event_time: DateTime<Utc>` parameter.
- `timeUnixNano` = original event time (from veronex ingest payload)
- `observedTimeUnixNano` = `SystemTime::now()` (when analytics received it)
**Verify**: Callers in `ingest.rs` pass `req.event_time`. No more `let _ = req.event_time`.

### 17. Ingest Payload Validation
**File**: `crates/veronex-analytics/src/handlers/ingest.rs`
**Change**:
- `ALLOWED_EVENT_TYPES` whitelist: `["inference.completed", "audit.action"]`
- `validate_inference()`: checks `tenant_id`, `model_name`, `provider_type`, `finish_reason`, `status` non-empty
- `validate_audit()`: checks `account_name`, `action`, `resource_type`, `resource_id`, `resource_name` non-empty
- Return type: `Result<StatusCode, StatusCode>` (was plain `StatusCode`)
**Verify**: Empty required field → 400. Unknown event_name → 400. 17 unit tests.

### 18. Model Selection Handler Separation
**Files**: new `model_selection_handlers.rs`, modified `provider_handlers.rs`, `mod.rs`, `router.rs`
**Change**: Extracted `list_selected_models` + `set_model_enabled` + DTOs to new file. `get_provider` helper made `pub(super)`.
**Verify**: Route registration in `router.rs` updated. `provider_handlers.rs` reduced ~100 lines.

### 19. Dashboard Aggregation Endpoint
**Files**: `dashboard_handlers.rs`, `router.rs`
**Change**: New `GET /v1/dashboard/overview` handler. Returns combined `DashboardOverview { stats, performance, capacity, queue_depth, lab }`. Uses `tokio::join!` for parallel queries.
**Verify**: Route registered. Response contains all 5 sections. No N+1 queries.

---

---

## Phase 3 — Clippy Clean + Regression Tests (8 items)

### 20. test_support.rs Duplicate `#![cfg(test)]`
**File**: `infrastructure/inbound/http/test_support.rs`
**Change**: Removed `#![cfg(test)]` — parent `mod.rs` already has `#[cfg(test)] mod test_support;`.

### 21. encryption.rs Doc Indentation
**File**: `domain/services/encryption.rs`
**Change**: Fixed over-indented doc list items (4 → 2 spaces). Clippy `doc_overindented_list_items`.

### 22. Collapsible If (let-chains)
**Files**: `dashboard_handlers.rs` (2), `circuit_breaker.rs` (1), `provider_registry.rs` (3)
**Change**: Nested `if` → let-chain syntax (Rust edition 2024).

### 23. `api_key_auth.rs` Manual Contains
**File**: `infrastructure/inbound/http/middleware/api_key_auth.rs`
**Change**: `.iter().any(|p| path == *p)` → `.contains(&path)` (5 occurrences).

### 24. `provider_handlers.rs` Items After Test Module
**File**: `infrastructure/inbound/http/provider_handlers.rs`
**Change**: Moved test module from mid-file to end of file.

### 25. Test Module `#[allow]` Policies
**Files**: 13 test modules across codebase
**Change**: Added `#[allow(clippy::unwrap_used, clippy::expect_used)]` to all `#[cfg(test)] mod tests` blocks. Test code uses `unwrap()`/`expect()` by design.

### 26. Hours Validation Consistency
**File**: `infrastructure/inbound/http/query_helpers.rs`
**Change**: `hours > 8760` → `hours == 0 || hours > 8760`. Now consistent with analytics validation (1..=8760).

### 27. Security Regression Tests
**File**: `infrastructure/inbound/http/auth_handlers.rs`
**Change**: 5 new unit tests:
- `atomic_claim_requires_valkey` — fail-closed without Valkey
- `revocation_check_fail_closed_error_type` — 503 on Valkey error
- `set_nx_semantics_for_replay_detection` — SET NX first/replay semantics
- `build_session_populates_fields` — session construction correctness
- `build_session_without_ip` — optional IP handling

---

## Phase 4 — Cross-Review Verification (3 items)

### 28. `recover_pending_jobs` Ownership Check
**File**: `application/use_cases/inference/use_case.rs:158-169`
**Change**: Before resetting Running→Pending, checks `kv_get(job_owner_key)`. If another node owns the job, skips recovery with log. Uses let-chain for clippy compliance.
**Risk addressed**: Multi-node environments — prevents double execution when Node A restarts while Node B is processing.
**Verify**: Only jobs with no active owner or self-owned get recovered. Other nodes' jobs are untouched.

### 30. `auth_handlers.rs` Test Fixes
**File**: `infrastructure/inbound/http/auth_handlers.rs`
**Change**:
- `session.is_valid` → `session.revoked_at.is_none()` (Session struct has no `is_valid` field)
- `!result.is_some()` → `result.is_none()` (clippy `nonminimal_bool`)

---

## Skipped with Analysis

| # | Item | Verdict | Rationale |
|---|------|---------|-----------|
| SSE `to_vec()` O(n²) | **Misdiagnosis** | Stream uses `Notify::notified().await` (push model, not polling). `tokens[idx..]` typically yields 1 token per wake. Total cost is O(n), not O(n²). |
| DashMap lock contention | **Misdiagnosis** | `DashMap::get()` is a sharded read lock held only for slice copy (~ns). No contention at practical concurrency levels. |
| `run_job` decomposition | **Deferred (C-group)** | Already split into `runner.rs` + `TokenStreamState` + `helpers` module. Further decomposition is refactoring-only with no behavioral change. |
| `InferenceJob` persistence coupling | **Deferred (C-group)** | `messages: None` pattern is a targeted optimization. Full `JobPersistenceModel` separation is architectural, not urgent. |
| `sqlx::query!` macro | **Intentional design** | Migrations managed in code. 54 queries use `.bind()` (no injection). Macro requires `.sqlx/` offline cache. |
| CI e2e smoke | **Deferred (B-group)** | Requires `playwright install` + Docker service containers. Separate infra PR. |

---

## CDD Updates

| File | Changes |
|------|---------|
| `docs/llm/auth/security.md` | Gemini AES-256-GCM, fail-closed JWT, atomic refresh, audit whitelist, GEMINI_ENCRYPTION_KEY |
| `docs/llm/auth/api-keys.md` | `#[ts(skip)]` annotations |
| `docs/llm/infra/otel-pipeline.md` | Timestamp semantics, ingest validation |

---

## Remaining Work (Next PRs)

### B-Group (Next PR)
| # | Item | Description |
|---|------|-------------|
| B1 | `recover_pending_jobs` distributed lock | SET NX based ownership claim (beyond current check) |
| B2 | CI e2e smoke | Playwright install + smoke subset in CI |
| B3 | Dashboard frontend connection | React Query → `/v1/dashboard/overview` endpoint |
| B4 | `save()` partial update | Use partial UPDATE instead of full row write |

### C-Group (Separate Issues)
| # | Item | Description |
|---|------|-------------|
| C1 | `run_job` state machine | Extract `JobStatusManager` from runner |
| C2 | SSE load testing | Verify streaming at 100+ concurrent connections |
| C3 | S3 lifecycle policy | Auto-delete old messages data |
| C4 | `JobPersistenceModel` | Separate domain/persistence models |
| C5 | Reaper edge cases | Stale job cleanup race conditions |
