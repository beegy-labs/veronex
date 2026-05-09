# SDD: Bridge Phase-Aware Timing (single-timer race fix)

> Status: implementation complete | Change type: **Fix** (architectural — Phase 1/2 separation visible to bridge) | Created: 2026-04-29 | Shipped: in PR #120 + S19.1 hotfix (this PR) | Live verify: re-running after S19.1
> CDD basis: `docs/llm/inference/job-lifecycle.md` · `docs/llm/inference/mcp.md` · S14 `inference-lifecycle-sod.md` (architectural lineage)
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row S19 (to add)

---

## §0 Quick-resume State

| Tier | Status | PR | Commit |
| ---- | ------ | -- | ------ |
| A — `StreamToken.is_phase_boundary` field + `phase_boundary()` constructor | [x] done | #120 | `bbf823c` |
| B — Runner emits boundary token after `ensure_ready` | [x] done | #120 | `bbf823c` |
| C — Bridge `collect_round` phase-aware timer + new constants | [x] done | #120 | `bbf823c` |
| D — Tests (StreamToken + collect_round phase transitions) | [x] done | #120 | `bbf823c` |
| CDD-sync (`mcp.md` Phase-aware timing row + `job-lifecycle.md` post-`ensure_ready` boundary) | [x] done | #120 | (#120 follow-up) |
| **S19.1 — TOKEN_FIRST_TIMEOUT 60→300s + ROUND_TOTAL_TIMEOUT 720→1500s + gateway-bound invariant** | [x] done | this PR | (this commit) |
| **CF-bypass infra for long-stream path** (platform-gitops PR #598/#599/#600/#601) | [x] done | platform-gitops | — |
| Live verify (dev) — re-run after S19.1 image rollout | [ ] pending | — | — |

---

## §1 Problem (verified 2026-04-29)

`conv_3386OgDfDKkJvamF9X1Dr` (job `019dda00`, model `qwen3-coder-next-200k:latest`):
- 16:09:24 — bridge submits round-0 of MCP loop.
- 16:13:24 (T+240 s) — bridge log: `MCP round failed round=0 error=model is still loading (first-token timeout 240s exceeded). Retry in a moment.` Bridge **gives up**.
- 16:13:32 (T+248 s) — the underlying inference job (`019dda00`) finally completes with `has_tool_calls=t`, `completion_tokens=116`. Tool result was ready, **but bridge already cancelled** → round-1 (final answer) never submitted → `conversation.turns[0].result = null`.

Net 8-second race lost the entire MCP loop.

---

## §2 Root Cause — leaky abstraction in S14's lifecycle SoD

S14 (`inference-lifecycle-sod.md`) split provider work into Phase 1 (`ensure_ready` — model load + KV alloc + warmup, can run 60–250 s on 200K-context models) and Phase 2 (`stream_tokens` — sub-second per token). The split was **applied inside `runner.rs`**.

`bridge.rs::collect_round`, however, has no awareness of phases. It runs a single `FIRST_TOKEN_TIMEOUT = 240 s` from job submission until first token. For a 200K-context cold start, Phase 1 alone can occupy nearly the entire budget, leaving Phase 2 with no slack — when Phase 1 takes 248 s, the bridge's timer fires 8 s before the first Phase 2 token.

The S14 architecture is the right shape; the `bridge.rs::collect_round` consumer simply hasn't been updated to honor it.

---

## §3 Solution — propagate Phase signal through the existing token stream

The `StreamToken` channel is the canonical "current job state" channel between runner and bridge. Adding a Phase boundary signal to that same channel keeps the SoD intact (no new bus, no new domain enum) and lets bridge separate Phase 1 timing from Phase 2 timing.

### §3.1 Choice rationale (vs alternatives)

| Option | Verdict |
|---|---|
| **A. `StreamToken::phase_boundary()` sentinel** | **Chosen** — minimal invasiveness; reuses existing channel; one new bool field |
| B. Extend `JobStatus` enum (`Loading` / `Ready`) | Rejected — domain-enum ripple touches DB schema + every match arm |
| C. Separate lifecycle event channel (tokio broadcast) | Rejected — new abstraction without proportional benefit |

### §3.2 Phase-aware constants (final, post-S19.1)

| Const | Value | Phase | Rationale |
|---|---|---|---|
| `LIFECYCLE_TIMEOUT` | **600 s** | Phase 1 | 200K cold-load measured ≤ 250 s. 600 s = ~2.4× headroom for future 300K+ models or congested VRAM scheduler. Does not race actual cold-load. |
| `TOKEN_FIRST_TIMEOUT` | **300 s** | Phase 2 first token | Originally 60 s — live verify on `qwen3-coder-next-200k:latest` showed `model_hung_post_load` firing during prefill on a 200K-context session with ~5K MCP-injected prompt tokens. Phase 2 first token is **NOT** sub-second when prefill is large. 300 s clears observed prefill with ~2× headroom for 300K-context futures. |
| `STREAM_IDLE_TIMEOUT` | **45 s** | Phase 2 streaming | Per-token gap on warm models is sub-second. Unchanged. |
| `ROUND_TOTAL_TIMEOUT` | **1500 s** | round-wide cap | = 600 (Phase 1) + 300 (Phase 2 first token) + 600 streaming budget. Must be **strictly less** than the upstream Cilium HTTPRoute `timeouts.request` (1800 s set in platform-gitops `cilium-gateway-values.yaml#veronex-api-direct-dev-route`) — bridge always chooses its own outcome before the route layer fires. Locked by `tests::round_total_under_gateway_request_timeout`. |

#### Two-layer timeout architecture (S19.1)

The end-to-end stream is bounded by two timers in series:

```
Client ── 1800s ──── Cilium HTTPRoute (timeouts.request = 1800s, platform-gitops)
                           │
                           └─→ Bridge round (ROUND_TOTAL_TIMEOUT = 1500s)
                                       │
                                       ├─ Phase 1: LIFECYCLE_TIMEOUT = 600s
                                       └─ Phase 2: TOKEN_FIRST_TIMEOUT = 300s, then STREAM_IDLE_TIMEOUT = 45s
```

Invariant `ROUND_TOTAL_TIMEOUT < gateway_request_timeout` ensures the bridge's named `RoundError` variant always surfaces before the gateway returns a generic 5xx. CF Edge / CF Tunnel (100 s idle) is **out of this picture** — the streaming path uses CF-bypass `*.girok.dev` direct-route. See `.add/domain-integration.md` (this repo) and `.add/add-direct-domain.md` (platform-gitops).

### §3.3 Algorithm (collect_round)

```text
in_phase_1 = true        # until phase_boundary token observed
received_any_token = false

loop:
    phase_timeout =
        LIFECYCLE_TIMEOUT       if in_phase_1
        else TOKEN_FIRST_TIMEOUT if !received_any_token
        else STREAM_IDLE_TIMEOUT

    token = stream.next() with timeout phase_timeout
    
    if token.is_phase_boundary:
        in_phase_1 = false
        received_any_token = false   # restart Phase 2 first-token countdown
        continue (don't push to caller)
    
    received_any_token = true
    # rest unchanged...
```

When `MCP_LIFECYCLE_PHASE` is OFF (legacy path), runner does not emit a boundary token. Bridge stays in `in_phase_1 = true` for the entire round, falling back to a single 600 s timeout — strictly more permissive than the prior 240 s. No regression possible.

---

## §4 Files

| File | Change |
|---|---|
| `crates/veronex/src/domain/value_objects.rs` | Add `is_phase_boundary: bool` field to `StreamToken`; add `StreamToken::phase_boundary()` constructor; existing `text` / `done` set false |
| `crates/veronex/src/infrastructure/outbound/ollama/adapter.rs` | All `StreamToken { ... }` literal constructors gain `is_phase_boundary: false` |
| `crates/veronex/src/infrastructure/outbound/gemini/adapter.rs` | Same — `is_phase_boundary: false` |
| `crates/veronex/src/infrastructure/inbound/http/test_support.rs` | Mock stream — `is_phase_boundary: false` |
| `crates/veronex/src/application/use_cases/inference/runner.rs` | After `ensure_ready` succeeds (gated by `mcp_lifecycle_phase_enabled`), push `StreamToken::phase_boundary()` into `JobEntry.tokens` and notify; doc-comment cites SDD |
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` | Replace `FIRST_TOKEN_TIMEOUT = 240` with `LIFECYCLE_TIMEOUT = 600 + TOKEN_FIRST_TIMEOUT = 60`; bump `ROUND_TOTAL_TIMEOUT = 720`; rewrite `collect_round` per §3.3 |

---

## §5 Tests

| # | Test | Module |
|---|---|---|
| 1 | `StreamToken::phase_boundary()` produces token with `is_phase_boundary=true`, no value, no finish_reason | domain |
| 2 | `StreamToken::text()` / `done()` produce `is_phase_boundary=false` | domain |
| 3 | `collect_round` boundary received → in_phase_1 transitions; received_any_token reset | bridge unit |
| 4 | `collect_round` Phase 1 timeout fires only after LIFECYCLE_TIMEOUT (mock stream that yields nothing for > 240s but < 600s should NOT timeout pre-fix; should timeout pre-fix; post-fix should not) | bridge unit |
| 5 | `collect_round` no boundary received (legacy path) → single timer = LIFECYCLE_TIMEOUT applies (no regression vs old behavior) | bridge unit |
| 6 | `collect_round` after boundary, Phase 2 first-token timeout uses TOKEN_FIRST_TIMEOUT (60 s) | bridge unit |

---

## §6 Live verification (dev cluster)

### §6.1 Setup
- `MCP_LIFECYCLE_PHASE=on` (already enabled on dev)
- Force cold-load by scaling stateful-set or sending `keep_alive=0s` to ollama
- Issue chat to `qwen3-coder-next-200k:latest` with MCP-active prompt (e.g. "오늘 마이크론 주식 분석해줘")

### §6.2 PASS conditions

| # | Check |
|---|---|
| L1 | bridge log `lifecycle.ensure_ready outcome=LoadCompleted duration_ms=N` emits within ~250 s |
| L2 | bridge does NOT emit `model is still loading` warning during the cold-load window |
| L3 | After Phase 1 completes, first token arrives within `TOKEN_FIRST_TIMEOUT` (300 s) — measured by elapsed since boundary log |
| L4 | Round-0 completes (tool_call produced) — same observable as today, but reliable instead of racing |
| L5 | Round-1 (final text) submitted, completes, `result_text` populated in S3 |
| L6 | Conversation detail GET returns non-null `result` |

### §6.3 Negative — non-MCP / non-lifecycle paths unchanged

`MCP_LIFECYCLE_PHASE=off` test: runner does not emit boundary; bridge stays in Phase 1 for the entire round; 600 s cap applies to whole round. Same correctness as today, just more lenient timeout.

---

## §7 CDD-sync

- `docs/llm/inference/mcp.md` "Response framing" section — replace single FIRST_TOKEN_TIMEOUT description with the phase-aware table from §3.2.
- `docs/llm/inference/job-lifecycle.md` "Phase 1 / Phase 2" — clarify that bridge now distinguishes the two phases via `StreamToken::is_phase_boundary`.

---

## §8 Resume rule recap

If `StreamToken::is_phase_boundary` doesn't exist yet → Tier A. If runner doesn't emit boundary post-`ensure_ready` → Tier B. If bridge `collect_round` has the old single FIRST_TOKEN_TIMEOUT → Tier C. If §6.2 unverified → live verify pending.
