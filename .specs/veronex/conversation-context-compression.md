# SDD: Conversation Context Compression (200K-context LLM history management)

> Status: planned (research complete, implementation ready) | Change type: **Add** | Created: 2026-04-29 | Owner: TBD
> CDD basis: `docs/llm/inference/job-lifecycle.md` · `docs/llm/inference/mcp.md` · `docs/llm/inference/capacity.md` · `docs/llm/inference/context-compression.md`
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row S17
> ADD framework: `.add/feature-addition.md` (spec-first) · `.add/implementation.md` (hexagonal, scale 10K providers / 1M TPS)
> **Resume rule**: every section is self-contained.

---

## §0 Quick-resume State

| Tier | Status | Branch | PR | Commit |
| ---- | ------ | ------ | -- | ------ |
| A — Per-model context size lookup (replace hardcoded 32_768) | [ ] | `feat/context-compression` | — | — |
| B — Token counter + budget gate (`tiktoken-rs` integration) | [ ] | (same) | — | — |
| C — Hierarchical compression hook in `bridge::run_loop` entry | [ ] | (same) | — | — |
| D — Apply across all entry points (OpenAI compat / MCP / native) | [ ] | (same) | — | — |
| E — Tests (unit token-count + integration 30+ turn loop) | [ ] | (same) | — | — |
| CDD-sync — `inference/context-compression.md` (existing) updated | [ ] | — | — | — |
| Live verify (dev) — 200K model + 30+ MCP rounds without overflow | [ ] | — | — | — |

---

## §1 Problem (verified 2026-04-29)

User wants the gateway to compress accumulated conversation when context approaches the model's native window (e.g. 262,144 for `qwen3-coder-next-200k:latest`). Current code has partial infrastructure (per-turn async `compress_turn` produces `TurnRecord.compressed`) but three concrete gaps prevent it from working at the 200K boundary:

| # | Defect | Code site | Effect |
|---|--------|-----------|--------|
| D1 | Hardcoded `configured_ctx = 32_768u32` in budget calc | `infrastructure/inbound/http/ollama_compat_handlers.rs:424` | Triggers ~16K instead of ~200K |
| D2 | Only the LAST user message is compressed; accumulated `messages[]` history is forwarded as-is | `compress_input_inline` only mutates `last_user["content"]` | An MCP loop with 30+ rounds grows unbounded |
| D3 | Inline compression wired only in `ollama_compat_handlers`; **MCP/OpenAI-compat routes never invoke it** | `openai_handlers.rs::mcp_ollama_chat`, `bridge::run_loop` entry | The agentic-loop path most likely to hit overflow has no compression |

---

## §2 Solution — three concrete tiers

### §2.1 Design choices (web-research backed)

| Question | Answer | Source |
|---|---|---|
| Token counter library? | **`tiktoken-rs`** crate — has `get_chat_completion_max_tokens` accepting message arrays | [tiktoken-rs](https://github.com/zurawiki/tiktoken-rs), [Markaicode](https://markaicode.com/llm-token-counting-tiktoken-model-limits/) |
| Tokenizer choice for Qwen/Llama (non-OpenAI)? | **`cl100k_base`** approximation. ±10% drift acceptable when budget ratio is conservative (≤0.75) | LiteLLM, [Vellum](https://vellum.ai/blog/count-openai-tokens-programmatically-with-tiktoken-and-vellum) |
| Trigger threshold | **95% of `configured_ctx`** (matches Claude Code's `auto-compact`) | Claude Code's documented behavior (167K of 200K) |
| Always preserve | **System prompt + last 5 turns** (Claude Code default; balance recency vs completeness) | Industry production pattern |
| Compression strategy | **Hierarchical**: replace older turns with their `TurnRecord.compressed` (already produced); if not yet compressed, run inline `compress_input_inline` on that turn synchronously | [LogRocket — LLM context strategies 2026](https://blog.logrocket.com/llm-context-problem-strategies-2026/), [Morph FlashCompact](https://www.morphllm.com/flashcompact) |
| Hook location | **`bridge::run_loop` entry** (covers all MCP-routed flows) + `use_case::submit` pre-flight (covers non-MCP) | Single hook ≈ no drift across paths |
| Failure mode | **Fail-open**: if compression fails, send original; let provider truncate. Surface metric `veronex_context_compression_failure_total` | DCP pattern (Opencode) — never modify session history |

### §2.2 Effects (research-backed)

- 50–80% token reduction with negligible quality loss ([oneuptime — Context Compression 2026](https://oneuptime.com/blog/post/2026-01-30-context-compression/view))
- 2× compressed context can OUTPERFORM uncompressed on long sequences (signal-to-noise improvement, [CompLLM research](https://arxiv.org/pdf/2407.08892))

---

## §3 Tier A — Per-model context size lookup

Replace `let configured_ctx = 32_768u32;` (`ollama_compat_handlers.rs:424`) and identical hardcodes in MCP path with a lookup against `model_vram_profiles.configured_ctx` (already populated by capacity analyzer).

### §3.1 New helper

`crates/veronex/src/application/use_cases/inference/context_lookup.rs` (NEW):

```rust
pub async fn resolve_model_context_size(
    capacity_repo: &dyn ModelCapacityRepository,
    provider_id: Uuid,
    model: &str,
) -> u32 {
    capacity_repo
        .get(provider_id, model)
        .await
        .ok().flatten()
        .map(|p| p.configured_ctx.max(0) as u32)
        .filter(|&c| c >= 4096)             // sanity floor
        .unwrap_or(32_768)                  // legacy fallback for unknown
}
```

### §3.2 Acceptance

- [ ] `cargo build -p veronex` succeeds
- [ ] For `qwen3-coder-next-200k:latest` → returns 262_144 (matches DB `configured_ctx`)
- [ ] For unknown model → returns 32_768 (legacy fallback, identical to today)

---

## §4 Tier B — Token counter + budget gate

### §4.1 Add `tiktoken-rs` dep

`crates/veronex/Cargo.toml`: `tiktoken-rs = "0.6"`.

`tiktoken-rs` `cl100k_base` approximation is sufficient — for production-grade Qwen/Llama counts, error is bounded ±10%; with our 95% threshold + 0.75 conservative budget ratio, drift cannot cause overflow.

### §4.2 New module

`crates/veronex/src/application/use_cases/inference/context_budget.rs` (NEW):

```rust
use tiktoken_rs::cl100k_base;

pub fn count_messages_tokens(messages: &[serde_json::Value]) -> u32 {
    let bpe = match cl100k_base() {
        Ok(b) => b,
        Err(_) => return estimate_chars_div_4(messages), // fallback
    };
    let mut total: u32 = 0;
    for m in messages {
        if let Some(role) = m.get("role").and_then(|v| v.as_str()) {
            total += bpe.encode_with_special_tokens(role).len() as u32;
        }
        if let Some(content) = m.get("content").and_then(|v| v.as_str()) {
            total += bpe.encode_with_special_tokens(content).len() as u32;
        }
        // 4 token overhead per message (role + delimiters), per OpenAI tokenizer doc
        total += 4;
    }
    total + 2 // priming tokens
}

pub fn budget_for_context(configured_ctx: u32, ratio: f32) -> u32 {
    ((configured_ctx as f32 * ratio) as u32).saturating_sub(1024)  // 1KB safety margin
}
```

### §4.3 Acceptance

- [ ] Unit test: 5-message chat array returns count within ±10% of Ollama's `/api/generate` echo `prompt_eval_count`
- [ ] Unit test: empty messages → 0
- [ ] `cargo test -p veronex --lib context_budget::` passes

---

## §5 Tier C — Hierarchical compression hook in `bridge::run_loop`

### §5.1 Algorithm

In `bridge.rs::run_loop` immediately AFTER the loop-state init (~line 220) and BEFORE the per-round `for round in 0..max_rounds` loop:

```text
1. resolve configured_ctx for this model (Tier A)
2. compute budget = configured_ctx * lab_settings.context_budget_ratio  (Tier B)
3. while count_messages_tokens(&messages) > budget:
     a. find oldest non-system, non-recent-K turn
     b. if its TurnRecord.compressed exists in S3 → replace its content with compressed text
     c. else if compression_model configured → call compress_input_inline(turn_content) → replace
     d. else → drop placeholder "[earlier context omitted to fit context window]"
     e. break if no replacement candidate (system + last K)
4. emit metric veronex_context_compression_runs_total{outcome=...}
```

`K` (always-preserve recent turns) defaults to **5**; configurable via `lab_settings.context_preserve_recent_turns` (new column, default 5).

### §5.2 Inside the per-round loop

After each `collect_round`, append the new assistant + tool result messages to the array. Re-run §5.1 step 3 with the freshly-grown array. This catches mid-loop overflow (which is the actual failure mode in long agentic sessions).

### §5.3 DCP-style placeholder

Per the [Opencode DCP](https://github.com/Opencode-DCP/opencode-dynamic-context-pruning) pattern: pruning replaces content with a placeholder; **the actual S3 ConversationRecord is never modified** (S16 invariant maintained). Replacement is in-memory only for this LLM call.

### §5.4 Acceptance

- [ ] Helper `prune_to_budget(messages, budget, recent_k, store)` returns the trimmed list and a `TrimReport { dropped_turns, replaced_with_compressed, budget_after }`
- [ ] Unit test: 30 mock turns × 1000 tokens each (30K total) with budget=15K → final returned array is < 15K, system + last 5 turns preserved exactly, middle turns replaced with their `compressed` text
- [ ] Unit test: when no compressed exists and `compression_model=None` → middle turns are placeholders not original content

---

## §6 Tier D — Apply across all entry points

Today only `ollama_compat_handlers.rs` calls `compress_input_inline`. Replace the disparate hooks with a single shared pre-flight that all entry points invoke.

### §6.1 Files to modify

| File | Change |
|------|--------|
| `crates/veronex/src/infrastructure/outbound/mcp/bridge.rs` | Insert pre-flight call to `prune_to_budget` at line ~220 (after init, before round loop) |
| `crates/veronex/src/infrastructure/inbound/http/openai_handlers.rs` | Native (non-MCP) path: insert pre-flight in `chat_completions` before `submit` |
| `crates/veronex/src/infrastructure/inbound/http/ollama_compat_handlers.rs` | Replace existing `compress_input_inline` block (lines 415-440) with the unified `prune_to_budget` call |
| `crates/veronex/src/application/use_cases/inference/use_case.rs` | If submit() is called from a path that didn't pre-flight (e.g., direct API key route), invoke pre-flight here too as defense-in-depth |

### §6.2 Acceptance

- [ ] `grep -rn "configured_ctx = 32_768" crates/veronex/src` returns no matches
- [ ] All four submission paths invoke the same `prune_to_budget` helper
- [ ] No path bypass possible (verified by `grep` audit)

---

## §7 Tier E — Tests

### §7.1 Unit tests (in respective modules)

| # | Test | Module |
|---|------|--------|
| 1 | `count_messages_tokens` matches reference within ±10% | `context_budget` |
| 2 | `budget_for_context` honors ratio + safety margin | `context_budget` |
| 3 | `prune_to_budget`: 30 turns × 1KB → trimmed correctly | `bridge` (or new `context_pruner` module) |
| 4 | `prune_to_budget`: system + last K always preserved | same |
| 5 | `prune_to_budget`: replaces with `compressed` when available | same |
| 6 | `prune_to_budget`: drops to placeholder when no compressed + no compression model | same |
| 7 | `resolve_model_context_size`: known model → DB value | `context_lookup` |
| 8 | `resolve_model_context_size`: unknown → 32_768 fallback | same |

### §7.2 Integration test

Mock 30+ round MCP loop fixture against `MockMessageStore` that stores compressed turns. Run through `bridge::run_loop`. Assert no message array sent to provider exceeds the budget.

### §7.3 Acceptance

- [ ] `cargo test -p veronex --lib` passes (8+ new tests)
- [ ] Integration test passes in CI

---

## §8 Live verification (dev cluster)

### §8.1 Setup

Generate a 30+ turn synthetic conversation (script: `/tmp/seed-long-conversation.sh`) using `qwen3-coder-next-200k:latest`. Each turn ~6K tokens. Total approaches 200K.

### §8.2 PASS conditions

| # | Check |
|---|-------|
| L1 | After 30 turns, `messages` array sent to Ollama is < 200K tokens (`prompt_eval_count` from response) |
| L2 | Bridge log: `veronex_context_compression_runs_total{outcome="trimmed"}` increments |
| L3 | System prompt + last 5 turns appear verbatim in the trimmed array |
| L4 | Older turns appear as their compressed summary OR placeholder string |
| L5 | Final answer is coherent (model can reference earlier facts via the compressed summaries) |
| L6 | No `context overflow` / `prompt too long` Ollama error |

---

## §9 CDD-sync (planned)

### §9.1 Existing doc — `docs/llm/inference/context-compression.md`

Doc already exists (per-turn `compress_turn` mechanism). Extend it with a new section after "Per-turn compression":

```
## Pre-flight pruning (S17 — added 2026-04-29)

- Single shared helper `prune_to_budget` invoked at every LLM submission entry point.
- Uses tiktoken-rs cl100k_base for token counting (±10% accurate for non-OpenAI models, conservative budget ratio compensates).
- Budget = `configured_ctx * lab_settings.context_budget_ratio` (default 0.75).
- Trim algorithm: drop from oldest, replace with `TurnRecord.compressed` when available; else inline-compress; else placeholder. Always preserve system + last `lab_settings.context_preserve_recent_turns` turns (default 5).
- Fails open: original messages forwarded if compression errors. Metric: `veronex_context_compression_failure_total`.
- DCP invariant: S3 ConversationRecord is NEVER mutated by pre-flight; trim is in-memory only.
```

### §9.2 Acceptance

- [ ] `grep -rn "configured_ctx = 32_768" docs/` returns no matches
- [ ] `docs/llm/inference/context-compression.md` references S17 SDD path

---

## §10 Follow-ups

None planned. If post-deploy observation shows accuracy degradation on specific model families, revisit the tokenizer choice (e.g. embed model-specific tokenizer.json from HuggingFace).

---

## §11 Resume rule recap

If `tiktoken_rs` not in `Cargo.toml`: start at Tier B. If hardcoded `configured_ctx = 32_768` still present: Tier A. If `bridge::run_loop` doesn't call `prune_to_budget`: Tier C. If §8 PASS conditions unverified: live verify pending.

---

## §12 Close-out workflow (`.add` framework)

After all implementation tiers (A/B/C/D/E) commit + cargo test green, follow `.add` to land cleanly:

| Step | What | Reference |
|------|------|-----------|
| 1 | Implementation PR(s) — each Tier in its own PR for review clarity | `.add/feature-addition.md` |
| 2 | Wait for image build + ArgoCD sync to dev | gitops |
| 3 | Run §8 live verify matrix (PASS conditions L1–L6) | this SDD §8 |
| 4 | If §8 fails → return to relevant tier (do NOT mark `[x]` in §0) | resume rule §11 |
| 5 | If §8 passes → run `.add/cdd-feedback.md` classification | `.add/cdd-feedback.md` |
| 6 | Update `docs/llm/inference/context-compression.md` per §9 (Reference type — API/contract change) | this SDD §9 |
| 7 | Run `.add/doc-sync.md` divergence audit on the touched docs | `.add/doc-sync.md` |
| 8 | Archive SDD: `git mv .specs/veronex/conversation-context-compression.md .specs/veronex/history/` | convention |
| 9 | Update `2026-Q2.md` row S17 status to `complete (#PR list, live-verify dev YYYY-MM-DD)` and adjust path to `.specs/veronex/history/` | scope hygiene |
| 10 | All §0 boxes checked; SDD cycle closed | — |

**Archive trigger**: every `[ ]` in §0 must be `[x]` AND the live-verify §8 results must be appended verbatim into a §10.5 (or equivalent) results section. Don't archive on partial verification — set §0 row to "[ ] in progress" with branch/PR pointer until conditions hold.

## Sources

- [tiktoken-rs](https://github.com/zurawiki/tiktoken-rs)
- [Markaicode — LLM Token Counting (2026)](https://markaicode.com/llm-token-counting-tiktoken-model-limits/)
- [LogRocket — LLM context strategies (2026)](https://blog.logrocket.com/llm-context-problem-strategies-2026/)
- [Morph FlashCompact — Compaction comparison](https://www.morphllm.com/flashcompact)
- [Atlan — LLM Context Window Limitations 2026](https://atlan.com/know/llm-context-window-limitations/)
- [oneuptime — Context Compression Build Guide 2026](https://oneuptime.com/blog/post/2026-01-30-context-compression/view)
- [CompLLM research (arxiv 2407.08892)](https://arxiv.org/pdf/2407.08892)
- [Opencode DCP — Dynamic Context Pruning](https://github.com/Opencode-DCP/opencode-dynamic-context-pruning)
- [Vellum — Counting tokens with tiktoken](https://vellum.ai/blog/count-openai-tokens-programmatically-with-tiktoken-and-vellum)
- [Redis — Context Window Overflow 2026](https://redis.io/blog/context-window-overflow/)
