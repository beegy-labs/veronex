# Code Review Policy

> SSOT | **Last Updated**: 2026-03-12
> Domain-specific checklists → `.specs/veronex/`

---

## Code Philosophy

Every line of code must satisfy all four properties simultaneously. If any one fails, revise before merging.

| Property | Definition |
|----------|-----------|
| **Consistent** | Follows established patterns in `policies/patterns.md`. No invented conventions. |
| **Concise** | No dead code, no restating comments, no single-use bindings that add no clarity. |
| **Simple** | The minimum complexity needed for the task. No speculative abstractions. |
| **O(1)** | Hot-path reads backed by atomics or cached values. No O(n) scans inside loops. |

---

## SSOT Map — What Governs What

Before reviewing code, identify the governing SSOT for the changed domain. Implementation must match the SSOT, not diverge from it.

| Domain | SSOT (Tier 2) | What it governs |
|--------|--------------|-----------------|
| Architecture | `policies/architecture.md` | Dependency direction, layer rules, port catalog |
| Code patterns | `policies/patterns.md` | Handler signatures, DashMap usage, Valkey Lua, timeout constants |
| Testing | `policies/testing-strategy.md` | Layer responsibility, proptest, TDD decision checklist |
| Security | `auth/security.md` | SSRF blocklist, SSE sanitization, SQL parameterization |
| Inference capacity | `inference/capacity.md` | VRAM, AIMD, OOM ceiling, stable-cycle rule |
| Thermal | `providers/hardware.md` | 5-state machine, drain requirements, forced timeouts |
| Job lifecycle | `inference/job-lifecycle.md` | Path A/B dispatch, queue semantics |
| Deploy / AppState | `infra/deploy.md` | Background loops, composition root wiring |

**Rule**: if implementation contradicts any Tier-2 doc, the doc is right — fix the code or update the doc explicitly (not silently).

---

## A. Architecture Compliance

- **Dependency direction**: `infrastructure → application → domain`. Reverse import = blocker.
- **No business logic in adapters**: handlers orchestrate, use cases decide.
- **Port before adapter**: new feature always defines the trait first, then the impl.
- **Ports are `Arc<dyn Trait>`**: no concrete type leaked into application layer.
- **Valkey key strings**: defined only in `infrastructure/outbound/valkey_keys.rs`. Never hardcoded inline.
- **UUID for PKs**: `Uuid::now_v7()` (app) / `uuidv7()` (PG). `Uuid::new_v4()` only for non-PK random identifiers.

## B. SSOT Alignment

- Every behavioral change must be reflected in the governing Tier-2 doc before or alongside the code change (CDD-first).
- No doc may describe behavior that differs from the implementation.
- When a spec is updated, search for all code paths that implement it and verify consistency.

## C. Performance — O(1) First

**Hot paths** (`try_reserve`, dispatcher loop, gate chain, scoring):

- Aggregated values (`loaded_weight_mb`, `provider_active_requests`, `stable_cycle_count`) **must** be backed by `AtomicU32` / `AtomicU64` cache — never recomputed by scanning a map.
- No O(n) `DashMap` iteration inside a per-request path. Move scans to background loops.
- No `Vec` allocation or `.clone()` inside tight loops without justification.
- `DashMap::Ref` / `RefMut` must not be held across `.await` — it locks the shard.

**Constant-time reads**:

- Enum display: `as_str()` returning `&'static str`. Never `format!("{:?}", e)`.
- SQL column lists: `const *_COLS: &str` to avoid repeated allocation.
- Valkey Lua scripts: multi-step ops in single `EVAL`, not multiple round-trips.

## D. Security

- **SQL injection**: never interpolate user-controlled values into queries. Use `make_interval(hours => $1)`, not `INTERVAL '{n}'`.
- **SSRF**: provider URLs validated by `validate_provider_url()` — blocks link-local, metadata endpoints, non-HTTP schemes.
- **SSE error sanitization**: all error output through `sanitize_sse_error()` — truncates to 200 chars, escapes `\r\n`, strips internals.
- **Secrets**: never hardcoded — environment variables only.
- **Input validation**: `MAX_PROMPT_BYTES` and `MAX_MODEL_NAME_BYTES` enforced at every API format entry point.

## E. Code Patterns

- **Timeouts and TTLs**: named constants only — never `Duration::from_secs(N)` inline.
- **Background tasks**: accept `CancellationToken`, use `tokio::select! { biased; _ = shutdown.cancelled() => break, _ = work => {} }`.
- **Error propagation**: handlers return `Result<T, AppError>`, use `?`. No `.unwrap()` outside tests.
- **Repeated 3+ line patterns**: extract to a named helper. One-off: inline.
- **Functions with 6+ parameters**: use a config struct.
- **`#[allow(...)]`**: only with a comment explaining why.
- **Comments**: explain *why*, not *what*. If the code is self-explanatory, no comment needed.

## F. TDD Policy

Decision order (from `testing-strategy.md`):

```
1. Type system catches it?    → No test needed
2. Pure function?             → Unit test + proptest preferred
3. External dependency?       → Integration (real structs, no internal mocking)
4. User flow?                 → E2E (minimum)
5. Already tested in another layer? → Do not duplicate
```

Enforcement rules:

- **1 test = 1 property**: test name is the spec. No multi-assert omnibus tests.
- **No duplicate assertions**: if invariant A implies B, test only A.
- **Pure functions in `domain/` or `application/`**: must have at least one proptest.
- **New behavioral logic**: must have a unit test exercising the exact edge case.
- **No mocking internal state**: pass real structs into tests.

---

## Output Format

For every finding, use this format — sort by severity before listing:

```
[A|B|C|D|E|F] ✅ / ⚠️ / ❌  file:line
Description — what is wrong or confirmed correct
Fix (if ⚠️/❌): one-line recommendation
```

Severity order: `❌ blocker` → `❌ SSOT violation` → `⚠️ TDD gap` → `⚠️ pattern deviation` → `⚠️ conciseness`

---

## Domain-Specific Checklists

For scheduler / capacity / thermal / placement, use the domain checklist alongside this policy:

| Domain | Checklist location |
|--------|--------------------|
| Scheduler + Capacity | `.ai/code-review.md` (references `.specs/veronex/scheduler.md`) |
