# SDD: Conversation Context Compression (200K-context LLM history management)

> Status: planned (problem-statement only — implementation approach to be researched) | Change type: **Add** | Created: 2026-04-29 | Owner: TBD
> CDD basis: `docs/llm/inference/job-lifecycle.md` · `docs/llm/inference/mcp.md` · `docs/llm/inference/capacity.md`
> Scope reference: `.specs/veronex/history/scopes/2026-Q2.md` row S17

---

## §0 Quick-resume State

| Tier | Status | Branch | PR | Commit |
| ---- | ------ | ------ | -- | ------ |
| Research — implementation approach (token counting / compression strategy / where to hook) | [ ] | — | — | — |
| Implementation — TBD after research | [ ] | — | — | — |
| Tests | [ ] | — | — | — |
| CDD-sync | [ ] | — | — | — |
| Live verify (dev) — 30+ turn MCP loop on 200K model | [ ] | — | — | — |

**This SDD intentionally has no implementation tier yet.** Research must complete first.

---

## §1 Problem (verified 2026-04-29)

User wants: when accumulated conversation context approaches the model's native context window (e.g. 262,144 for `qwen3-coder-next-200k:latest`), the system should automatically compress earlier turns instead of hitting truncation/OOM.

Current state of the codebase has **partial infrastructure**, but with three concrete gaps that prevent it from actually working at the 200K boundary:

| # | Defect | Code site | Effect |
|---|--------|-----------|--------|
| D1 | Hardcoded `configured_ctx = 32_768u32` in inline-compression budget calc | `infrastructure/inbound/http/ollama_compat_handlers.rs:424` | Budget computed off 32K even when model is 262K. Compression triggers ~16K early or never aligns with real ceiling |
| D2 | Only the LAST user message is compressed; accumulated `messages[]` history is forwarded as-is | `compress_input_inline` → only mutates `last_user["content"]` | An MCP loop with 30+ rounds of tool-call back-and-forth grows the messages array unbounded; nothing trims it |
| D3 | Inline compression is wired only in `ollama_compat_handlers`; **MCP/OpenAI-compat routes never invoke it** | `openai_handlers.rs::mcp_ollama_chat`, `bridge::run_loop` | The route most likely to accumulate long context (agentic loops) has no compression at all |

### Existing infrastructure (preserve)

| Component | Location | Status |
|---|---|---|
| Per-turn async summary writer | `application/use_cases/inference/context_compressor.rs::compress_turn` | Implemented; produces `TurnRecord.compressed` after each round |
| Compression provider/model router | `application/use_cases/inference/compression_router.rs::decide` | Implemented |
| `lab_settings.context_compression_enabled` + `compression_model` + `compression_timeout_secs` + `context_budget_ratio` | `lab_settings` table + dashboard UI | Implemented |
| `MessageStore::CompressedTurn` schema | `application/ports/outbound/message_store.rs` | Implemented |

→ The data path that produces compressed turns is in place. The **read-path that USES them when building LLM input** is what this SDD must add.

---

## §2 Implementation approach — TO BE RESEARCHED

This section is intentionally empty. Research must produce concrete answers to:

| Open question | Why it matters |
|---|---|
| How to count tokens for a `messages[]` array reliably without per-call LLM round-trip? (tiktoken-equivalent? heuristic? `/api/show` from Ollama?) | Token-budget gate at the entry point depends on this |
| Where to hook the trim logic — `bridge::run_loop` entry, `use_case::submit`, or a shared pre-flight in handler? | Single hook avoids drift across MCP/OpenAI-compat/native paths |
| Replace strategy when `TurnRecord.compressed` exists vs not (re-summarize on the fly? skip turn? sliding-window discard?) | Defines correctness contract |
| What to always preserve (system prompt, last K turns) — pick K = ? | Quality/cost trade-off |
| How to expose budget breach to the user (header, log, surface in dashboard turn timeline?) | Operator feedback loop |
| Per-model context size lookup path — `model_vram_profiles.configured_ctx`? `max_ctx`? Cache key? | Replaces D1 hardcoded 32_768 |
| Interaction with `should_intercept` MCP routing — does compression happen before or after MCP tool injection? | Affects token budget calc accuracy |
| Failure modes — compression model unavailable, timeout, returns garbage; fail-open vs fail-closed | Reliability vs correctness |

Until these are answered, **do not implement**. Research output should be a follow-up commit on this SDD that fills §3 (Solution) with concrete file/line changes, then implementation begins.

---

## §3 Solution

(To be filled after §2 research.)

---

## §4..§N

(To be authored after §3 — Tier breakdown, acceptance criteria, tests, live verify matrix, CDD-sync.)

---

## §10 Follow-ups

None yet — section reserved for after implementation.

---

## Resume rule

If you find this SDD with §0 box "Research" unchecked: start by reading `compress_turn` and `compress_input_inline` in `crates/veronex/src/application/use_cases/inference/context_compressor.rs` and the call sites in `ollama_compat_handlers.rs:415-440`. The 8 open questions in §2 each need a concrete answer. Don't write code until §3 is filled.
