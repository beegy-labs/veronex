# SDD: Lifecycle ↔ Inference num_ctx SSOT Alignment

> Status: planned | Change type: **Fix** (SoD — Phase 1 lifecycle and Phase 2 inference must use the same num_ctx) | Created: 2026-04-30 | Owner: TBD
> CDD basis: `docs/llm/inference/` · `docs/llm/providers/ollama.md` · `docs/llm/policies/architecture.md`
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row TBD
> **Resume rule**: every section is self-contained.

---

## §0 Quick-resume State

| Tier | Status | PR | Commit |
| ---- | ------ | -- | ------ |
| A — `ModelLifecyclePort::ensure_ready` accepts `num_ctx` | [ ] | — | — |
| B — `ollama/lifecycle.rs::probe_load` sends `options.num_ctx` to ollama | [ ] | — | — |
| C — `runner.rs` resolves `num_ctx` from sync SSOT before calling `ensure_ready` (same source as `adapter.rs::stream_chat`) | [ ] | — | — |
| D — `model_effective_num_ctx` fabricate values aligned to sync canonical (200k → 200_000 to match Modelfile, etc.) | [ ] | — | — |
| E — `capacity/analyzer.rs` measurement calls (lines 497, 596) use SSOT num_ctx OR explicit co-load proof | [ ] | — | — |
| F — Tests (signature change + body assertion + fabricate-equals-sync sentinel + double-load-avoidance live trace) | [ ] | — | — |
| CDD-sync — multiple files (see §7.1) | [ ] | — | — |
| Live verify (dev) — single cold-load on 200K MCP request, no second runner | [ ] | — | — |

---

## §1 Problem (verified 2026-04-30 on dev `develop-35fcc34`)

200K-context MCP request to `qwen3-coder-next-200k:latest` produced **two ollama runner subprocesses** for the same model — total 220 + 232 = 452 s instead of a single 220 s cold-load.

ollama log evidence:

```
07:08:11.633  starting runner #1 (port 44151)         ← lifecycle probe /api/generate
07:08:11.645  load request KvSize:200000              ← Modelfile PARAMETER (probe sent NO num_ctx)
07:08:16.452  commit
07:11:52.323  runner #1 ready (220s)

07:11:52.756  starting runner #2 (port 41757)         ← chat /api/chat
07:11:52.779  load request KvSize:204800              ← veronex sent num_ctx=204800
07:12:03.430  commit, "loaded runners count=2"
07:15:44.838  runner #2 ready (232s)
```

ollama scheduler treats the same model with different `KvSize` as separate runner subprocesses (`OLLAMA_NUM_PARALLEL=1` per env).

The **4800-token mismatch** (200000 vs 204800) is the entire reason for the double cold-load.

---

## §2 Root cause — Phase 1 lifecycle bypasses the sync SSOT

S14 (`inference-lifecycle-sod.md`) split the provider work into two ports with two separate impls:

| Port | Impl file | Sends num_ctx? | Source |
|------|-----------|---------------|--------|
| `ModelLifecyclePort::ensure_ready` | `ollama/lifecycle.rs::probe_load` | **❌ No** | (ollama falls back to Modelfile PARAMETER) |
| `InferenceProviderPort::stream_tokens` | `ollama/adapter.rs::stream_chat` | ✅ Yes | `lookup_ctx` (Valkey) → fallback `model_effective_num_ctx` |

**The two ports must agree on num_ctx for a given model**, otherwise ollama spawns separate runners. The sync chain that should provide the canonical num_ctx is already in place:

```
[Sync — capacity::analyzer (background)]
   ollama /api/show → Modelfile parameters → extract num_ctx
        ├─→ Postgres `model_capacity.configured_ctx`
        └─→ Valkey cache `ollama_model_ctx(provider_id, model)` (TTL 600s)

[Reads — already wired]
   adapter.rs::lookup_ctx (chat path) ✅
   openai_handlers.rs:988 (multi-turn gate) ✅
   ollama_compat_handlers.rs:384 / :496 ✅

[Reads — MISSING]
   lifecycle.rs::probe_load ❌
```

`lifecycle.rs::probe_load` is the only place in the inference call chain that does not consult the sync SSOT. Adding `num_ctx` to its `/api/generate` body — sourced from the same sync chain — closes the gap.

---

## §3 Solution

### §3.1 Two-line invariant

For any model `M` reaching ollama via veronex:

1. `lifecycle.ensure_ready(M)` and `adapter.stream_tokens(<job using M>)` MUST send identical `options.num_ctx` to ollama.
2. Both values MUST come from the same lookup path (sync SSOT first, deterministic fallback second).

### §3.2 Port signature change

```rust
// crates/veronex/src/application/ports/outbound/model_lifecycle.rs
#[async_trait]
pub trait ModelLifecyclePort: Send + Sync {
    async fn ensure_ready(
        &self,
        model: &str,
        num_ctx: u32,                              // NEW
    ) -> Result<LifecycleOutcome, LifecycleError>;
}
```

### §3.3 Ollama lifecycle adapter

```rust
// crates/veronex/src/infrastructure/outbound/ollama/lifecycle.rs::probe_load
.json(&serde_json::json!({
    "model": model,
    "prompt": "",
    "num_predict": 0,
    "keep_alive": keep_alive,
    "options": { "num_ctx": num_ctx },         // NEW
}))
```

### §3.4 Runner integration

```rust
// crates/veronex/src/application/use_cases/inference/runner.rs
// before ensure_ready:
let num_ctx = resolve_num_ctx(state, &job.model_name).await; // same helper used by adapter
let outcome = lifecycle_port.ensure_ready(&job.model_name, num_ctx).await?;
// then:
let stream = inference_port.stream_tokens(&job).await?;      // same num_ctx via lookup_ctx
```

`resolve_num_ctx` is the shared helper — Valkey lookup → fallback `model_effective_num_ctx`. Existing `adapter.rs::lookup_ctx` is the canonical implementation; either reuse it (extract to a shared module) or duplicate the lookup chain in runner with identical semantics.

### §3.5 `model_effective_num_ctx` returns must equal sync values

CDD `providers/ollama-impl.md` §3 documents `options.num_ctx` per request as the design contract. The fabricate fallback exists for cache-miss safety. **Currently the fabricate values disagree with sync values for the same model**:

| Source | Value for `qwen3-coder-next-200k` | Origin |
|--------|-----------------------------------|--------|
| `model_effective_num_ctx` (name-pattern) | **204_800** (200 × 1024) | hardcoded in `adapter.rs:54` |
| Modelfile `PARAMETER num_ctx` (sync source) | **200_000** | model author |
| `OLLAMA_CONTEXT_LENGTH` env (operator) | **204_800** | platform-gitops |

`adapter.rs::lookup_ctx` returns 200_000 on Valkey HIT, 204_800 on MISS. The lifecycle fix alone (passing num_ctx through) does NOT close this — if a request lands during a cache miss window, both paths now agree on 204_800 (good), but a request right after a successful sync will see 200_000 (also consistent within that request). The mismatch reappears across consecutive requests if MISS/HIT alternate around the 600 s TTL.

Per user direction ("ollama 동기화에 설정된 값이 context_length 보장"), **sync IS the SSOT**. The fix:

- `model_effective_num_ctx` returned values MUST match what sync would store for that model. For ollama models, sync extracts Modelfile `PARAMETER num_ctx` → so fabricate must return the same canonical value (e.g., `200k → 200_000` not `204_800`).
- Equivalently: fabricate is the "what would sync return if it had run successfully" prediction. Drift between fabricate and sync is the bug.

### §3.6 Capacity analyzer measurement calls (separate path, same SoD violation)

`capacity/analyzer.rs:497` and `:596` issue measurement calls to ollama with hardcoded `options.num_ctx: 512` and `1024`. Per `OLLAMA_NUM_PARALLEL=1` semantics, these can also trigger separate runner subprocesses for the same model. Within scope:

- Measurement calls MUST also use the SSOT-resolved num_ctx (same helper as runner/adapter), OR
- Measurement calls explicitly use a model-specific small value AND ensure ollama can co-load (out of this SDD; track separately if not already known)

Documented here so the analyzer change is not forgotten when cleaning up `model_effective_num_ctx`.

---

## §4 Files

| File | Change |
|---|---|
| `application/ports/outbound/model_lifecycle.rs` | `ensure_ready` signature: add `num_ctx: u32`. Trait + mock impl. |
| `infrastructure/outbound/ollama/lifecycle.rs::probe_load` | Accept `num_ctx`, include `options.num_ctx` in `/api/generate` body. |
| `infrastructure/outbound/ollama/lifecycle.rs` (caller of probe_load: `drive_probe`?) | Plumb `num_ctx` through. |
| `infrastructure/outbound/ollama/adapter.rs` | Implements the new ensure_ready signature; reuses internal num_ctx resolution. |
| `application/use_cases/inference/runner.rs` | Before `ensure_ready`, resolve `num_ctx` via the same lookup the adapter uses. Pass to `ensure_ready`. |
| `application/use_cases/inference/runner.rs` (or new `helpers.rs`) | Extract `resolve_num_ctx(state, model) -> u32` so runner and adapter call the same code. |
| `infrastructure/outbound/ollama/adapter.rs::lookup_ctx` (refactor target) | Move out of adapter or expose so runner can call it. |
| `infrastructure/inbound/http/test_support.rs` | Mock provider's `ensure_ready` signature update. |

---

## §5 Tests

| # | Test | Module |
|---|---|---|
| 1 | `probe_load` includes `options.num_ctx: <expected>` in request body — `wiremock` with body matcher | ollama lifecycle unit |
| 2 | `runner` resolves num_ctx via `resolve_num_ctx` before calling `ensure_ready` and passes the same value used by `stream_tokens` | runner unit |
| 3 | `model_effective_num_ctx("qwen3-coder-next-200k:latest") == 204_800` (sentinel — operator intent must not drift from `OLLAMA_CONTEXT_LENGTH` env) | adapter unit (already exists, keep) |
| 4 | (regression sentinel) test that asserts probe_load body does NOT omit `options.num_ctx` | ollama lifecycle unit |
| 5 | All existing phase-aware timeout invariants (S19/S19.1) unchanged — mock now supplies `num_ctx` arg | bridge unit |

---

## §6 Live verification (dev cluster)

### §6.1 Setup
- Image rolled out to `develop-<this PR sha>`
- Force model unload: `kubectl exec ollama-0 -- ollama stop qwen3-coder-next-200k:latest`

### §6.2 PASS conditions

| # | Check |
|---|---|
| L1 | Submit MCP-active 200K-context chat request via CF-bypass (`https://veronex-api-dev.girok.dev/v1/chat/completions`) |
| L2 | ollama log shows **exactly ONE** `starting runner` for `qwen3-coder-next-200k` during the request window |
| L3 | ollama log: `loaded runners count=1` (or 2 only if a second model like `qwen3:8b` is co-resident) — never 2 instances of the same model |
| L4 | Both ollama log load requests show **identical KvSize** (the value `model_effective_num_ctx` returns or what Valkey cache holds) |
| L5 | Bridge log: `lifecycle.ensure_ready outcome=LoadCompleted duration_ms=N` ONCE; `MCP round complete round=0` follows shortly after |
| L6 | Total request duration ≤ 250 s (one cold-load + Phase 2 prefill + tool round) — was 452 s pre-fix |
| L7 | Conversation `result_text` non-empty (S20 stream-tap delivers final round) |

### §6.3 FAIL signals
- Two `starting runner` for the same model → fix incomplete
- `KvSize` mismatch in load requests → resolve_num_ctx not consistent between paths
- Phase 1 LoadCompleted then no Phase 2 round → stream-tap regression (S20)

---

## §7 CDD sync (post-impl)

### §7.1 Files to update

| CDD file | Edit |
|---------|------|
| `docs/llm/flows/model-lifecycle.md:47` | `provider.ensure_ready(model)` → `provider.ensure_ready(model, num_ctx)` in the call-flow diagram. Update the surrounding prose to call out that Phase 1 and Phase 2 share `num_ctx` source. |
| `docs/llm/flows/model-lifecycle.md:60` | Probe body: add `options.num_ctx` line to the snippet. |
| `docs/llm/providers/ollama-impl.md` §3 (line 43–67) | Add: lifecycle probe is included in "Every Ollama request includes options.num_ctx". State explicitly that fabricate values MUST equal sync canonical values to avoid runner-spawn drift. |
| `docs/llm/inference/job-lifecycle.md:120` | Phase 1 invocation row: show `num_ctx` as Phase 1's input alongside `model`. |
| `docs/llm/inference/capacity.md:178` | Note that `configured_ctx` (sync source) and `model_effective_num_ctx` (fabricate) MUST agree per model; cross-reference this SDD. |
| `.ai/components.md` (if it lists ports) | `ModelLifecyclePort::ensure_ready` signature update. |

### §7.2 Cross-reference

Once this SDD is archived, `flows/model-lifecycle.md` and `providers/ollama-impl.md` should reference it from the "References" or "History" footers.

---

## §8 Out of Scope

- `OLLAMA_CONTEXT_LENGTH=204800` env value review — operator decision, platform-gitops territory. Per user direction, sync (Modelfile) is the SSOT, env is a server-wide floor; aligning fabricate to sync supersedes any need to change env.
- Modelfile PARAMETER value (model author's choice). Future model rebuilds may change values; veronex consumes whatever sync returns.
- Capacity analyzer scheduling / trigger frequency. If sync misses are common, this fix still holds (Phase 1 / Phase 2 / measurement all use the same fallback). If frequent, that's a separate operability concern.
- ReAct shim path (`run_loop_react`) — uses same ollama adapter; inherits the fix automatically. No additional changes required.
- `OLLAMA_NUM_PARALLEL=1` env semantics — operator decision; raising it would let multiple num_ctx variants coexist per model but at VRAM cost. Out of this SDD.

---

## §9 References

- `.specs/veronex/history/inference-lifecycle-sod.md` (S14) — Phase 1/2 SoD origin (this SDD fixes the SoD num_ctx gap)
- `.specs/veronex/bridge-phase-aware-timing.md` (S19/S19.1) — phase-aware timeouts (independent)
- `.specs/veronex/bridge-mcp-loop-correctness.md` (S20) — fast-path drop + stream-tap (independent)
- ollama env: `OLLAMA_CONTEXT_LENGTH=204800` (platform-gitops `ollama-values.yaml`)
- ollama scheduler: `OLLAMA_NUM_PARALLEL=1` — runners with different KvSize do NOT share
