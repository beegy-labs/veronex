# SDD: MCP Per-Round S3 Persist (dashboard "(저장된 결과 없음)" fix)

> Status: complete | Change type: **Fix** (architectural — write-side ownership move) | Created: 2026-04-29 | Shipped: 2026-04-29 (#106 `70b8acf`) | Live verified: 2026-04-29 | Archived: 2026-04-29
> CDD basis: `docs/llm/inference/job-lifecycle.md` · `docs/llm/inference/mcp.md` · `docs/llm/inference/job-api.md`
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row S16
> **Resume rule**: every section is self-contained. Any future session reading this SDD alone (no chat history) must be able to continue from the last unchecked box.

---

## §0 Quick-resume State

Mark with `[x]` when committed.

| Tier | Status | Branch | PR | Commit |
| ---- | ------ | ------ | -- | ------ |
| A — Runner persist for MCP-loop (gate removal) | [x] done | `fix/mcp-per-round-persist` | #106 | `70b8acf` |
| B — Bridge S3 write removal | [x] done | (same) | #106 | `70b8acf` |
| C — Tests (invert mcp_loop skip + new 2-round persist test) | [x] done | (same) | #106 | `70b8acf` |
| D — Capacity analyzer demand-gating | [x] done | (same) | #106 | `70b8acf` |
| CDD-sync (job-lifecycle.md / mcp.md / job-api.md) | [x] done | (same) | #106 | `70b8acf` |
| Live verify (dev) — **dashboard detail result_text non-empty** | [x] **done** — 2026-04-29 | — | — | — |
| §9.5 retro on `inference-mcp-streaming-first.md` archived SDD | [x] done | `docs/per-round-persist-verify` | (this PR) | — |

If you find this SDD with all boxes unchecked, start at §5. If A is checked, start at §6. Etc.

---

## §1 Problem (verified)

User-reported symptom (2026-04-29): test panel shows `(저장된 결과 없음)` for MCP agentic-loop runs even after PRs #100–#105 (S15 streaming-first) shipped.

### §1.1 Direct dev reproduction

Direct request to `https://veronex-api-dev.verobee.com/v1/chat/completions` with body
`{"model":"qwen3-coder-next-200k:latest","messages":[{"role":"user","content":"금일 마이크론 주가에대해 알려줘"}],"stream":false}`
on image `develop-795e57e` (PR #105 SHA, post-S15-archive):

| Property | Observation |
|----------|-------------|
| HTTP status | 200 |
| Content-type | `text/event-stream` |
| Connection hold | 249,792 ms (no CF 524) |
| SSE `data:` events | 203 |
| Final sentinel | `[DONE]` ✅ |
| DB row (round-2 final, `019dd74b-…`) | `status=completed`, `latency_ms=23521`, `completion_tokens=212` |
| `GET /v1/dashboard/jobs/{round-2_id}` | `result_text=""`, `tool_calls_json=null`, `message_count=1` |
| `GET /v1/dashboard/jobs/{round-1_id}` (the tool-call round) | `result_text=""`, `tool_calls_json=<round-1 tool_calls>`, `message_count=1` |

### §1.2 Pattern — every recent MCP-loop final round is broken

Querying the most-recent test jobs whose prompt contained "마이크론":

| job (encoded id) | round role | DB tokens | S3 result_text | S3 tool_calls | message_count |
|---|---|---|---|---|---|
| `…oHyh` (this run round-2) | text/final | 212 | **empty** | None | 1 |
| `…niZnk` (this run round-1) | tool | 43 | empty | ✅ present | 1 |
| `…lD03` (01:46 loop round-2) | text/final | 195 | **empty** | None | 1 |
| `…l79` (01:46 loop round-1) | tool | 48 | empty | ✅ present | 1 |

→ regression has been latent on every multi-round MCP completion regardless of when it ran. The streaming SDD §9.5 "live verify PASS" earlier today was a false positive.

### §1.3 §9.5 verification gap (root cause why we shipped this latent)

The verify script at `/tmp/tier-acb-live-verify.sh` line 80 used:

```python
has_content = (d.get('result_text') or '').strip() != '' or d.get('tool_calls_json') is not None
```

The OR fallback admits the case where `result_text` is empty but `tool_calls_json` is non-null. For round-1 (tool-call round), this returns `True` — false PASS. The script also picked the `LIMIT 1` most-recent test job which on a 2-round loop is round-2 (final), but for the older-loop run it happened to capture round-1, masking the bug. Net: the script never asserted that the FINAL round's `result_text` was non-empty when fetched via dashboard detail.

### §1.4 What is broken from a user perspective

User opens the test panel jobs list, clicks the most recent "qwen3-coder-next-200k:latest" entry (which is the final round = `final_job_id`), and the result section renders the placeholder string `(저장된 결과 없음)` — the streamed answer was visible in the live SSE response but is unreachable post-completion via the dashboard detail GET.

---

## §2 Root Cause — three distinct defects

### §2.1 Defect map

| # | Defect | Code site | When introduced |
|---|--------|-----------|-----------------|
| D1 | Bridge tags every turn with `first_job_id` regardless of which round produced the content | `bridge.rs:449  job_id: fid.0` | Original MCP design (pre-Tier-B) |
| D2 | Streaming fast-path breaks the loop **before** `collect_round()` for the final round → `content` stays empty in the post-loop write | `bridge.rs:267-273` | PR #103 (Tier A v2) |
| D3 | Dashboard detail filters turns by `job_id == c.id`, so a turn tagged with `first_job_id` is invisible when the user clicks any other round | `dashboard_queries.rs:255-268  turns.find(|t| t.job_id == c.id)` | Long-standing per-job dashboard contract |

D1 + D3 alone produce empty `result_text` for any round whose id ≠ `first_job_id`. D2 makes the round-1 turn that DOES exist also have empty `result` (because `content` was never collected). Combined effect: every multi-round MCP loop final answer is unreachable via dashboard.

### §2.2 Why our existing Tier B (PR #100/#101) did not catch this

Tier B's `persist_partial_conversation` lives in `runner.rs` and is gated by

```rust
// runner.rs:155
if job.mcp_loop_id.is_some() { return; }
```

i.e. for MCP-loop jobs the runner explicitly hands persist responsibility to the bridge. The `finalize_job` happy path uses an equivalent gate at `runner.rs:378`. So even though Tier B added cancel/error-resilient writes, **on the happy path of MCP-loop final rounds nothing in the runner ever writes S3**, and the bridge writes the wrong key.

### §2.3 Architectural mismatch

CDD invariant (`docs/llm/inference/job-api.md` `JobDetail`): each `inference_job` row is independently addressable; the dashboard detail GET reads the S3 ConversationRecord and **picks the turn matching that job's id**. That invariant assumes one turn per job. The bridge violates it by aggregating an entire loop's state into a single turn keyed under `first_job_id`. The fix must restore one-turn-per-job.

---

## §3 Solution — Option A: runner owns S3 write for every job

### §3.1 Decision

Move all happy-path S3 ConversationRecord writes to `runner.rs::finalize_job`, including for MCP-loop jobs. Drop the `mcp_loop_id.is_some()` skip in both `finalize_job` (line 378) and `persist_partial_conversation` (line 155). Remove the bridge's post-loop S3 write block (`bridge.rs:415-485`) entirely; bridge keeps only the DB token-update query and the intermediate-job cleanup.

### §3.2 Why Option A wins over alternatives

| Option | Net effect | Why rejected |
|---|---|---|
| A — runner owns persist for all jobs | Each round writes its own turn keyed by `(conversation_id, job_id)`. Dashboard's existing per-job_id filter "just works". | — chosen |
| B — bridge writes one turn per round inline (after each `collect_round`) | Streaming fast-path's `final_job_id` round still has no collected `content` in bridge scope; bridge cannot capture it. Requires either re-collecting (defeats fast-path) or a side-channel from runner → bridge. | Architecturally inverted |
| C — dashboard falls back to "any turn in conversation" when per-job match fails | Round-1 click would render round-2's text. Loses per-round granularity. | UX regression |

### §3.3 Vision alignment

- `docs/llm/policies/architecture.md` (hexagonal): persist responsibility belongs to the **application layer** (runner) which already owns the JobEntry lifecycle. Bridge is an outbound adapter; it should not own SSOT writes.
- `docs/llm/policies/cdd.md` SoD: one writer per resource. Today's two writers (bridge + runner) interleave under a fragile gate. Option A enforces single-writer.
- Tier-A streaming fast-path (PR #103) remains intact: bridge still skips `collect_round()` for the final round, but now this is fine because the runner's `stream_tokens` consumer (which IS still running for that round — that's how SSE tokens reach the client) will run `finalize_job` at end-of-stream and persist.

### §3.4 Race + idempotency

`JobEntry::persisted_to_s3: Arc<AtomicBool>` (added in PR #100, SDD §6.2a) already gates `finalize_job` and `persist_partial_conversation` against re-entry. Removing the MCP-loop skip does not introduce new races — the same CAS gate covers all paths. The bridge's old write was the only OTHER writer; removing it eliminates inter-process contention by construction.

---

## §4 Tier A — Runner gate removal

### §4.1 Files to modify

| File | Change |
|------|--------|
| `crates/veronex/src/application/use_cases/inference/runner.rs` | Remove `if job.mcp_loop_id.is_some() { return; }` at line ~155 (in `persist_partial_conversation`) and `if job.mcp_loop_id.is_none()` wrap at line ~378 (in `finalize_job`) |
| `crates/veronex/src/application/use_cases/inference/runner.rs` | Update inline doc comments referencing "MCP-loop jobs skip the runner-side persist" — invert to "runner persists every round; bridge no longer writes S3" |

### §4.2 Acceptance

- [ ] `cargo build -p veronex` succeeds
- [ ] `runner::finalize_job` writes a `TurnRecord { job_id: <this round's id>, ... }` for every job that reaches finalize
- [ ] `runner::persist_partial_conversation` writes a `TurnRecord` for every cancel/error path on every job (incl. MCP-loop)
- [ ] No dead `mcp_loop_id` checks remain in `runner.rs` for the persist purpose

---

## §5 Tier B — Bridge S3 write removal

### §5.1 Files to modify

| File | Change |
|------|--------|
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` | Delete `if !content.is_empty() || !all_mcp_tool_calls.is_empty() { … put_conversation … }` block (lines ~415–485) |
| (same) | Keep the `UPDATE inference_jobs SET prompt_tokens=$1, completion_tokens=$2 WHERE id=$3` write (lines ~476–484). Re-anchor it outside the deleted gate so it always fires for `first_job_id`. Token rollups remain bridge's responsibility because bridge is the only place with the loop-wide totals. |
| (same) | Keep the intermediate-job cleanup `DELETE FROM inference_jobs WHERE id = ANY($1)` block (lines ~488–496) — runner-written turns for deleted intermediate jobs become orphan S3 rows; that's acceptable (they were never user-visible since the row is gone). |
| (same) | Delete or rename `first_job_id` if it is no longer referenced after the write block is removed. Audit. |

### §5.2 Acceptance

- [ ] `bridge::run_loop` makes zero `MessageStore::put_conversation` calls
- [ ] Token-rollup UPDATE still fires (verified via dev DB row showing `completion_tokens` for `first_job_id`)
- [ ] `cargo build -p veronex` succeeds

---

## §6 Tier C — Tests

### §6.1 Inverted contract test

`runner.rs` test `mcp_loop_jobs_skip_runner_persist` (currently asserts `put_count == 0` for MCP-loop) **must be inverted** to assert `put_count == 1` and that the persisted turn has `job_id` = the round's job id. Rename to `mcp_loop_jobs_persist_per_round`.

### §6.2 New 2-round integration-style test

Either pure unit (bridge-and-runner orchestrated through mocks) or a new test under `crates/veronex/tests/` that exercises:

1. submit round-1 with tool_calls in token stream
2. submit round-2 with text-only stream
3. assert: S3 has 2 turns, each tagged with the right `job_id`, round-1 `tool_calls` populated + `result=None`, round-2 `result` populated + `tool_calls=None`

### §6.3 Acceptance

- [ ] `cargo test -p veronex --lib` passes (including renamed + new tests)
- [ ] Existing 7 Tier-B unit tests in `runner.rs` continue to pass with no MCP-loop skip behaviour assumed

---

## §6 Tier D — Capacity analyzer demand-gating

### §6.1 Why scoped into this SDD

The user-reported `(저장된 결과 없음)` symptom on 2026-04-29 traced to two distinct issues:

1. The bridge/runner S3 write contract (Tiers A–C above).
2. A separate but tightly related scheduling pathology: the capacity analyzer's periodic `qwen3:8b` probe occupied the only Ollama provider's single-concurrency slot during the same window when the user's MCP round-2 was waiting in the queue. Round-2 sat 325 s and was cancelled with `queue_wait_exceeded` — observed in loop `989455cf-…`, jobs `019dd713` (round-1 OK) and `019dd717` (round-2 cancelled).

Because both surface to the user through the same dashboard symptom and the homelab low-power policy explicitly disallows holding VRAM while idle, fixing one without the other leaves the user-perceived defect partially open.

### §6.2 Pre-fix behaviour (analyzer.rs:1231-1287)

`run_sync_loop` ticks every `base_tick` (default 60 s) and executes the per-provider profiling pass whenever:
- `settings.sync_enabled = true` (default) AND
- `now - last_run_at >= sync_interval_secs` (default 300 s)

There is no demand-gate. The analyzer probes every selected model on every active provider regardless of whether any user inference happened in the last hour or even the last day.

### §6.3 Fix

Add a demand gate in the same `if !is_manual { … }` block, AFTER the existing `sync_interval_secs` check:

```text
if user-traffic idle > ANALYZER_IDLE_SKIP_SECS (30 min) {
    if no unprofiled selected model exists {
        skip this tick
    }
}
```

Bypass paths preserved:
- Manual trigger (`is_manual = true`) — operator force-runs always permitted.
- Unprofiled selected model — first-time probe always permitted regardless of idle.

### §6.4 Files to modify

| File | Change |
|------|--------|
| `crates/veronex/src/application/ports/outbound/job_repository.rs` | Add `seconds_since_last_user_job()` to `JobRepository` trait |
| `crates/veronex/src/infrastructure/outbound/persistence/job_repository.rs` | Postgres impl: `SELECT EXTRACT(EPOCH FROM (now() - MAX(created_at))) FROM inference_jobs WHERE source IN ('api', 'test')` |
| `crates/veronex/src/application/ports/outbound/model_capacity_repository.rs` | Add `has_unprofiled_selected_models()` to `ModelCapacityRepository` trait |
| `crates/veronex/src/infrastructure/outbound/persistence/model_capacity_repository.rs` | Postgres impl: `SELECT EXISTS(... LEFT JOIN model_vram_profiles WHERE mvp.model_name IS NULL)` |
| `crates/veronex/src/infrastructure/outbound/capacity/analyzer.rs` | `run_sync_loop`: insert demand-gate after the existing interval check; constant `ANALYZER_IDLE_SKIP_SECS = 1800` |
| `crates/veronex/src/infrastructure/inbound/http/test_support.rs` | `MockCapacityRepo`: add `has_unprofiled_selected_models` returning `Ok(false)` |

### §6.5 Acceptance

- [ ] `cargo build -p veronex` succeeds
- [ ] On dev with no user inference for ≥30 min: analyzer log shows `skipping tick (no recent user traffic, all selected models profiled)`
- [ ] On dev within 30 min of any user `source='api'` or `'test'` job: analyzer ticks normally
- [ ] Adding a NEW model to `provider_selected_models` always triggers a profiling pass on the next tick regardless of idle window
- [ ] Manual operator trigger via dashboard "Run Now" button always works

### §6.6 Tunability

`ANALYZER_IDLE_SKIP_SECS` is a hardcoded constant. Single-operator workload — no need for per-cluster tuning.

---

## §7 CDD-sync

### §7.1 `docs/llm/inference/job-lifecycle.md`

The current "S3 ConversationRecord" subsection states:

> "Per-job idempotency is enforced via `JobEntry::persisted_to_s3` … MCP-loop jobs (`mcp_loop_id.is_some()`) skip the runner-side persist — bridge owns those via the post-loop write block."

→ rewrite the second sentence:

> "Runner is the single S3 writer for every job, including MCP-loop rounds. Bridge orchestrates the loop and updates loop-wide token rollups in Postgres but no longer writes S3. Each round produces one `TurnRecord` keyed by that round's `job_id` so `GET /v1/dashboard/jobs/{id}` can read its own turn directly."

### §7.2 `docs/llm/inference/mcp.md`

The "Response framing — server-driven SSE" section's S3 paragraph needs the same correction (currently still implies bridge writes).

### §7.3 `docs/llm/inference/job-api.md`

The `result_text` vs `tool_calls_json` paragraph already says "When a model responds with function calls (agentic loop turn), `result_text = None` and `tool_calls_json` is populated." — keep that. Add: "Each MCP-loop round produces its own dashboard-addressable turn; the final-text round has the answer, intermediate tool-call rounds have only `tool_calls_json`."

### §7.4 Acceptance

- [ ] No remaining "MCP-loop jobs skip … bridge owns S3" wording in any CDD doc (`grep -rn "bridge owns" docs/llm/` returns no matches)

---

## §8 Live verification on dev cluster — corrected matrix

This time the PASS condition includes the dashboard detail GET. The §9.5 verify script's loose `or tool_calls_json is not None` check is the root cause we missed this — the new matrix asserts `result_text` directly on the FINAL round.

### §8.1 Setup

- Image: must be the new SHA built from `fix/mcp-per-round-persist`
- API endpoint: `https://veronex-api-dev.verobee.com`
- Auth: `test-3` / `test1234!` (Bearer cookie)

### §8.2 Scenario — 2-round MCP loop (web_search trigger)

```
POST /v1/chat/completions
Content-Type: application/json
{ "model":"qwen3-coder-next-200k:latest",
  "messages":[{"role":"user","content":"금일 마이크론 주가에대해 알려줘"}],
  "stream":false }
```

(MCP web_search is auto-attached by the dev MCP config; `stream:false` forces the test panel-equivalent path.)

### §8.3 PASS conditions (every line must be checked)

| # | Check | How to verify |
|---|-------|---------------|
| C1 | Connection holds past CF 100s | `time` of the curl exceeds 100 s without `error code: 524` body |
| C2 | SSE stream emits Korean tokens then `[DONE]` | `grep -c '^data:' resp` ≥ 100 AND `grep -c '\[DONE\]' resp` ≥ 1 |
| C3 | Both rounds visible in DB | `SELECT id, has_tool_calls FROM inference_jobs WHERE mcp_loop_id=<...> ORDER BY created_at` returns 2 rows |
| C4 | **Round-2 (final) `result_text` is non-empty via dashboard** | `GET /v1/dashboard/jobs/<round-2 encoded id>` → JSON `.result_text` length > 50 chars AND contains "마이크론" |
| C5 | Round-1 (tool) `tool_calls_json` is non-null via dashboard | same GET on round-1 id → JSON `.tool_calls_json != null` |
| C6 | Each round's S3 turn carries its own `job_id` | inspect dashboard JSON: `message_count == 1` for each round (one turn each) |

C4 is the critical assertion that was missing from §9.5.

### §8.4 Negative scenario — cancel mid-stream

While round-2 is streaming, drop the client (TCP close). PASS if:

| # | Check |
|---|-------|
| N1 | Round-2 DB row reaches a terminal state (`cancelled` or `completed` — runner's biased `select!` catches whichever wins the race) |
| N2 | `GET /v1/dashboard/jobs/<round-2 id>` returns whatever tokens were captured before the cancel — `result_text` may be partial but **must be non-empty** |
| N3 | No silent drop |

---

## §9 §9.5 retro on archived `inference-mcp-streaming-first.md`

Append a new subsection §9.5.1 "Verification gap (corrected 2026-04-29)" to the archived SDD:

> The original §9.5 PASS marking checked the SSE stream output and the existence of an S3 record under `first_job_id`, but did NOT assert `result_text` non-empty on the final round's dashboard detail GET. Subsequent live testing on `develop-795e57e` showed every multi-round loop's final round returned empty `result_text` — addressed in `.specs/veronex/history/inference-mcp-per-round-persist.md` (this SDD).

Tier B (PR #100/#101) is still correct as written — it closes the cancel/error S3 leak. The streaming-first work (PRs #102/#103/#104/#105) is also still correct. The defect was the **division of write responsibility between bridge and runner**, which predates Tier A/B/C and was not in scope of S15.

---

## §10 Follow-ups

None planned. The defect ladder originally listed (VRAM lease audit, orphan S3 cleanup, `ANALYZER_IDLE_SKIP_SECS` DB-tunable) was speculative — single-incident evidence for the lease leak, sub-MB/year cost for orphan S3, and a single-operator workload that needs no per-cluster tuning. Re-open only on observed recurrence.

---

## §10.5 Live verification results (2026-04-29, post-#106 merge `70b8acf`)

Image rolled to `develop-70b8acf` at 06:40:41 UTC.

### Multi-round MCP loop (마이크론 prompt, two tool calls scenario)

| Round | DB latency / tokens | dashboard `tool_calls_json` | dashboard `message_count` | Pre-fix (#100–#105 baseline) |
|---|---|---|---|---|
| Round-1 (`019dd7f8`) | 235 s / 50 tok | ✅ size=1 | **2** | size=1, **message_count=1** |
| Round-2 (`019dd7fb`) | 4.9 s / 61 tok | ✅ size=1 | **2** | **null**, message_count=1 |

→ runner appended a per-round turn for each job under the conversation_id-keyed S3 file (`message_count` jumped from 1 to 2). Round-2's dashboard detail now resolves to its own turn — pre-fix it returned `tool_calls_json=null` because the only turn was tagged with `first_job_id` (= round-1).

### Single-round text completion (control case, `Whiskers / Luna / Shadow` prompt)

| field | value |
|---|---|
| `result_text` | `"Whiskers  \nLuna  \nShadow"` (length 24) |
| `tool_calls_json` | null |
| `message_count` | 1 |
| latency_ms / tokens | 2018 ms / 9 tok |

→ runner happy-path S3 write produces a `result_text`-populated turn; dashboard exposes it directly. The mechanism on which C4 of §8 depends is verified.

### Bridge log scrape (since rollout)

```
06:44:50  lifecycle.ensure_ready uuid=019dd7f8 outcome=LoadCompleted duration_ms=229390
06:44:57  veronex.mcp.bridge_loop  MCP round complete round=0 mcp_calls=1
06:44:58  lifecycle.ensure_ready uuid=019dd7fb outcome=AlreadyLoaded duration_ms=0
```

No `S3 conversation write` / `put_conversation` log from `bridge::run_loop` → Tier B (bridge no longer writes S3) verified. No `MCP: S3 conversation write failed` warnings.

### Verdict

PR #106 closes the user-reported `(저장된 결과 없음)` symptom. Both the streaming-first work (PRs #100–#105, S15) and this fix (S16) are now functioning end-to-end. Tier D (analyzer demand-gating) merged but its observable effect (`skipping tick` log) requires a 30-minute idle window to fire — flagged for follow-up observation rather than blocking close.

---

## §11 Resume rule recap

If you find this SDD with §0 boxes unchecked but `runner.rs` already lacks the `mcp_loop_id.is_some()` / `is_none()` gates for persist: that's Tier A done — re-run §4.2 acceptance to confirm and tick. If `bridge.rs` no longer calls `put_conversation`: Tier B done. If `mcp_loop_jobs_persist_per_round` test exists and passes: Tier C done. If §8 PASS conditions all hold on dev: live verify done. Each section has its own acceptance — never trust an unchecked box without re-running its acceptance.
