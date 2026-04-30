# SDD: VRAM total SSOT Priority Restoration

> Status: planned | Change type: **Fix** (regression — SoD source-of-truth invariant violated by `4891fbc`) | Created: 2026-04-30 | Owner: TBD
> CDD basis: `docs/llm/inference/capacity.md` (L688–L699 — `provider_vram_budget` + `vram_total_source` + "confirmed total VRAM (0 = unknown → pass-through)")
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row TBD

---

## §0 Quick-resume State

| Tier | Status | PR | Commit |
| ---- | ------ | -- | ------ |
| A — `analyzer.rs::sync_provider` priority chain restored (provider DB → agent mirror → auto-detect pass-through) | [ ] | — | — |
| B — Tests: provider DB value > 0 used; pass-through to APU/DRM when value is 0 | [ ] | — | — |
| CDD-sync — `inference/capacity.md` priority order made explicit | [ ] | — | — |
| Live verify (dev) — dispatcher uses 117760 MB envelope; `qwen3-coder-next-200k` MCP multi-round completes within budget | [ ] | — | — |

---

## §1 Problem (verified 2026-04-30 on dev `develop-6ff3c88`)

User test panel submitted MCP-active 200K request to `qwen3-coder-next-200k:latest`. Job reached round 0 (`mcp_calls=1`) at 09:02:24 then **round 1 sat in queue 5+ minutes without dispatch** → queue_maintenance cancelled with `queue_wait_exceeded — 312s` → bridge `LIFECYCLE_TIMEOUT=600s` fired with `model load did not complete within 600s. Provider may be cold-stuck.` → frontend showed `load failed`.

Root cause: **dispatcher's VRAM gate computed `available_vram_mb=1840` for ollama-1.kr1**, while ollama itself reported `available 69.8 GiB`. Mismatch traced to `analyzer.rs::sync_provider` deriving `total_vram_mb=59915` (close to current `mem_available_mb` of system RAM), instead of using operator-registered `total_vram_mb=117760` (provider DB value).

Concrete numbers:

| Source | Value | Used by |
|--------|-------|---------|
| Provider DB `llm_providers.total_vram_mb` (operator-registered) | **117760 MB** (115 GB) | dashboard provider list |
| `node_drm_memory_vram_size_bytes` (DRM dedicated) | 1024 MB | analyzer (1st priority — APU detection) |
| `node_drm_memory_gtt_size_bytes` (unified memory) | 131072 MB | (not consulted) |
| `mem_available_mb` (node-exporter MemAvailable) | ~60000 MB (variable) | analyzer (APU branch — fed to vram_pool) |
| **vram_pool current `total_vram_mb`** | **~59915 MB** | dispatcher VRAM gate ❌ |

ollama-1.kr1 is an AMD Ryzen AI Max+ 395 APU. The system has 100 GiB RAM and 1 GiB dedicated VRAM with 128 GiB GTT. ollama correctly uses 129 GiB total; veronex uses transient `mem_available_mb` (~60 GB now).

---

## §2 Root cause — `4891fbc` regressed the priority chain

CDD `inference/capacity.md` L688–L699 defines:

> `vram_total_mb` lives in `llm_providers` (managed via provider API) … `vram_total_source` TEXT: `probe / node_exporter / manual` … `total_vram_mb` — confirmed total VRAM (0 = unknown → pass-through)

The intent: **operator-registered value is the SSOT** ("confirmed"). Auto-detection (`probe` / `node_exporter`) is the **fallback** when operator hasn't set a value (0 = unknown → pass-through).

Commit history of `crates/veronex/src/infrastructure/outbound/capacity/analyzer.rs`:

| commit | date | change |
|--------|------|--------|
| `4761cda` (initial v1) | 2025-?? | provider DB value used directly |
| `9b199fa` | 2026-?? | u32 → u64 (type only) |
| `a4da7c3` `fix(capacity): always register APU providers in VramPool even when total_vram=0` | 2026-03-22 | always call `set_total_vram` (even when 0) so APU providers stay in pool — CORRECT scope |
| **`4891fbc` `feat(agent): push capacity state to Valkey, analyzer reads from cache`** | **2026-03-22** | **Inverted priority: `hw.vram_total_mb` (DRM 1024 MB) → 1st, `agent_total_vram_mb` → 2nd, provider DB → 3rd. Comment says "Prefer agent-reported … fall back to provider DB" but the `unwrap_or_else` chain puts DRM ahead** |

The `4891fbc` change broke the SoD invariant: operator's `manual` registration is silently overridden by transient system measurement. CDD's documented priority is unenforced.

The unused field `vram_total_source` (saved to `provider_vram_budget` but never read for priority decisions) is further evidence of incomplete wiring.

### §2.1 Three-layer safety net (already present, makes restoration safe)

ollama itself + AIMD + safety_permil already provide dynamic correction within whatever envelope the operator sets:

1. **ollama** rejects loads that exceed actual GPU memory (returns 5xx); bridge surfaces `LifecycleError::ProviderError` to caller.
2. **AIMD** (capacity analyzer 30s loop) measures TPS/p95; on degradation, reduces `max_concurrent` per model. Independent of `total_vram_mb`.
3. **`safety_permil`** auto-bumps +50 on OOM (`OOM_SAFETY_BUMP_PERMIL`, CDD L139); gradually decays on stable cycles (`SAFETY_DECAY_PERMIL=10`, CDD L503).

Operator-misconfigured envelope (e.g. registers 200 GB on a 60 GB host) costs **a handful of failed requests** until AIMD + safety_permil settle to a safe operating point. Acceptable trade for SSOT clarity.

---

## §3 Solution

### §3.1 Priority chain restored to CDD intent

```rust
// crates/veronex/src/infrastructure/outbound/capacity/analyzer.rs::sync_provider
//
// SDD: `.specs/veronex/vram-total-ssot-priority-restoration.md` §3.1.
// CDD: `docs/llm/inference/capacity.md` L699 — "confirmed total VRAM (0 = unknown → pass-through)".

let vram_total_mb = if provider_total_vram_mb > 0 {
    // 1st: operator-registered SSOT (CDD: "confirmed total VRAM")
    provider_total_vram_mb as u64
} else if let Some(agent) = agent_total_vram_mb.filter(|&v| v > 0) {
    // 2nd: agent-pushed mirror of provider DB (covers race during analyzer cache miss)
    agent
} else {
    // 3rd: pass-through (CDD: "0 = unknown → pass-through")
    //      auto-detect DRM/APU for unregistered providers (dev / new providers)
    let drm = hw.as_ref().map(|h| h.vram_total_mb as u64).unwrap_or(0);
    let mem_avail = hw.as_ref().map(|h| h.mem_available_mb as u64).unwrap_or(0);
    let is_apu = hw.as_ref().is_some_and(|h| {
        h.gpu_vendor == "amd" && drm > 0 && mem_avail > drm * 2
    });
    if is_apu { mem_avail } else { drm }
};
```

### §3.2 What does NOT change

- `vram_pool.set_total_vram` call site (always called, regression `a4da7c3` preserved — APU providers stay registered even with `total=0`)
- APU drift detection (line 765+ — `last_mem_available_mb` resets AIMD baselines on >15% drift). Independent from VRAM total source.
- AIMD `max_concurrent` learning (independent of `total_vram_mb`)
- `safety_permil` auto-bump on OOM (operates on the envelope, whatever it is)
- `provider_vram_budget.vram_total_source` field — written but unread; keep written for future audit. Future SDD may wire it (e.g. `manual` → reject auto-override; `probe` → allow refresh). Out of this scope.

### §3.3 Operator guidance (out of scope, documented in CDD only)

- Register accurate `total_vram_mb` per `POST /v1/providers` or `PATCH /v1/providers/{id}`.
- For APU: declared envelope ≈ usable unified memory (typically ~80–90% of system RAM if dedicated to ollama).
- For dedicated GPU: declared envelope ≈ GPU VRAM physical size.
- Register 0 / leave unset only for dev / pass-through to ollama enforcement.

---

## §4 Files

| File | Change |
|---|---|
| `crates/veronex/src/infrastructure/outbound/capacity/analyzer.rs` (lines ~732–755) | Replace `unwrap_or_else` chain with explicit if-let priority per §3.1. |
| `crates/veronex/src/infrastructure/outbound/capacity/analyzer.rs` (tests) | Two new unit tests: (a) `provider_total_vram_mb > 0` → that value used; (b) `provider_total_vram_mb = 0` + APU hw → falls back to `mem_available_mb`. Additional sentinel: `provider_total_vram_mb = 0` + non-APU hw → falls back to `drm_vram_mb`. |
| `docs/llm/inference/capacity.md` | Make priority explicit: add a "Priority order" table referencing this SDD. Existing L699 already states intent; reinforce. |

---

## §5 Tests

| # | Test | Module |
|---|---|---|
| 1 | Operator value 117760 + APU host (drm=1024, mem_avail=60000) → `vram_total_mb=117760` (operator value wins) | analyzer unit |
| 2 | Operator value 0 + APU host (drm=1024, mem_avail=60000) → `vram_total_mb=60000` (APU pass-through) | analyzer unit |
| 3 | Operator value 0 + non-APU host (drm=24576) → `vram_total_mb=24576` (DRM pass-through) | analyzer unit |
| 4 | Operator value 0 + no hw metrics → `vram_total_mb=0` (full unknown, vram_pool pass-through delegates to ollama) | analyzer unit |
| 5 | Sentinel: no test hardcodes `agent_total_vram_mb` precedence over operator value (regression guard against `4891fbc` recurrence) | analyzer unit |

---

## §6 Live verification (dev cluster)

### §6.1 Setup

- `provider_total_vram_mb` for `ollama-1.kr1` already set to 117760 (registered value) — no provider re-registration required.
- After image rollout (`develop-<this PR sha>`), capacity analyzer's next 30 s sync loop will set `vram_pool.total_vram_mb(provider_id) = 117760`.

### §6.2 PASS conditions

| # | Check |
|---|---|
| L1 | `GET /v1/dashboard/capacity` row `provider_name=ollama-1.kr1` shows `total_vram_mb = 117760` (was ~59915) |
| L2 | `available_vram_mb` = 117760 − used (≥ ~60 GB free, was 1840 MB) |
| L3 | Submit same MCP-active 200K request that previously failed: response completes within `ROUND_TOTAL_TIMEOUT` (1500 s) |
| L4 | Bridge log: round 0 → round 1 → … → final round all dispatch within seconds (no `queue_wait_exceeded`) |
| L5 | Conversation `result_text` non-empty |
| L6 | No `LifecycleError::LoadTimeout` from bridge (within budget) |
| L7 | Frontend test panel: response renders successfully (no "load failed" / no SSE error event) |

---

## §7 Risks & mitigations

| # | Risk | Mitigation |
|---|------|------------|
| 1 | Operator registers value larger than physical capacity | ollama OOM rejects; AIMD reduces `max_concurrent`; `safety_permil` auto-bumps +50; settles to safe point in 1–2 sync cycles (≤60 s) |
| 2 | Hardware change without operator re-registration | Same as #1 — dynamic correction. Operational guidance: re-register on hardware change |
| 3 | APU mem pressure from co-tenants on the host | Operator value is upper bound; ollama enforces actual at runtime; AIMD adapts |
| 4 | Old APU dev installs that never set `total_vram_mb` (relied on auto-detect) | Pass-through path preserved (operator value 0 → APU detection identical to current code) |
| 5 | `agent_total_vram_mb` from cache is stale | Agent push refreshes every scrape (≤30 s); cache TTL 180 s. Acceptable lag |

---

## §8 Out of Scope

- Wiring `vram_total_source` field for runtime priority decisions (currently written, never read). Future SDD if needed.
- Operator UI/API enhancements to suggest a value based on hw_metrics.
- Re-evaluation of APU detection logic itself (kept identical for the pass-through path).
- AIMD parameter tuning (`safety_permil`, `OOM_SAFETY_BUMP_PERMIL`, etc.).

---

## §9 References

- `4891fbc feat(agent): push capacity state to Valkey, analyzer reads from cache` — commit that introduced the regression
- `a4da7c3 fix(capacity): always register APU providers in VramPool even when total_vram=0` — APU registration fix (preserved)
- `docs/llm/inference/capacity.md` — `provider_vram_budget` + `vram_total_source` + "confirmed total VRAM" definitions (L688–L699)
- `docs/llm/providers/ollama-allocation.md` Phase 1 — provider registration flow (operator sets `total_vram_mb`)
- `.specs/veronex/lifecycle-num-ctx-ssot-alignment.md` (S21) — companion SoD restoration (num_ctx alignment), independent fix
