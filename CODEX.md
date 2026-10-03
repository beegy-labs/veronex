# CODEX.md

> Codex entry point — derived from [AGENTS.md](AGENTS.md) | **Last Updated**: 2026-05-10

## Role

**Implementer.** Codex executes the change plan that Claude has already approved and written. Codex does not author scopes, debate architecture, or update `.ai/` / `docs/llm/`. See [AGENTS.md#agent-role-split](AGENTS.md#agent-role-split).

| Codex Does | Codex Does NOT |
| ---------- | -------------- |
| Read `.specs/veronex/{scope}/spec.md` + `tasks.md` | Expand scope beyond what `tasks.md` lists |
| Read source code, implement only listed tasks | Author or rewrite `.specs/veronex/*` |
| Run `cargo clippy` + `cargo nextest` + `tsc --noEmit` + `vitest` | Edit `.ai/` or `docs/llm/` (token-optimization rules apply — Claude only) |
| Retry up to 3× on test/build failure | Make architecture decisions on its own |
| Return compressed summary (≤400 tokens, no diffs/logs) | Paste full diffs, code blocks, or build logs |
| Flag scope deviations explicitly | Silently expand into adjacent files |

## Start

Always read in this order before touching code:

1. `.specs/veronex/{scope}/spec.md` — change boundary (approved by human)
2. `.specs/veronex/{scope}/tasks.md` — exact task list to execute
3. `.add/best-practices.md` — multi-tenancy, security, scale-target patterns
4. `.add/backend-feature.md` or `.add/frontend-feature.md` — implementation conventions
5. `.ai/README.md` — QA gate commands and project overview
6. Domain docs under `docs/llm/` — only the slice the scope actually touches

If `.specs/veronex/{scope}/spec.md` does not exist, **stop and return** `scope unclear` — do not improvise. Claude must author the scope first.

## Frameworks

| Directory | Framework | Codex's Job |
| --------- | --------- | ----------- |
| `.ai/` + `docs/llm/` | CDD | **Read-only**. Treat as constraints. Never edit. |
| `.specs/` | SDD | Read scope + tasks; mark task progress only |
| `.add/` | ADD | Read the workflow Claude pointed to |

## Veronex Context

| Layer | Tech |
| ----- | ---- |
| Backend | Rust + Axum (hexagonal), modular monolith |
| Frontend | Next.js 16 + React 19 |
| Cache / Queue | Valkey ZSET priority queue (`veronex:queue:zset`) |
| RDBMS / Analytics | PostgreSQL 18, ClickHouse |
| Streaming / Observability | Redpanda (Kafka API), OpenTelemetry Collector |
| Capacity | AIMD + p95 fast adapt; thermal auto-detect (`gpu_vendor`) |

| Scale Target | Value |
| ------------ | ----- |
| Providers (llama-server nodes) | 10,000 |
| MCP servers | 1,000+ |
| Concurrent requests (TPS) | 1,000,000 |

Full overview: `.ai/README.md`.

## Critical Rules

| Rule | Detail |
| ---- | ------ |
| Spec-first | No code changes without an active `.specs/veronex/{scope}/spec.md` |
| Stay in scope | Implement only `tasks.md` items; flag deviations, do not absorb |
| CDD is read-only | Never modify `.ai/`, `docs/llm/`, `docs/en/`, `docs/kr/` |
| Scale targets | 10K providers / 1K MCP / 1M TPS — non-negotiable |
| Queue dispatch | ZSET priority queue; tier-scored |
| Lab settings | Gate features via `useLabSettings()` / `LabSettingsRepository` |
| No AI/LLM mention in commits or PR text | Project policy |
| Hexagonal layer | domain → application → infrastructure (never reverse) |

## Documentation Layers (read-only context)

| Layer | Path | Editable | Purpose |
| ----- | ---- | -------- | ------- |
| L1 | `.ai/` | **No (Codex)** | Entry pointers — Claude edits |
| L2 | `docs/llm/` | **No (Codex)** | Machine SSOT — Claude edits via `.add/doc-sync.md` |
| L3 | `docs/en/` | No | Generated human docs |
| L4 | `docs/kr/` | No | Generated translated docs |

## QA Gate (run before returning summary)

```bash
# Backend
cargo clippy --workspace -- -D warnings   # 0 warnings
cargo check --workspace                   # 0 errors
cargo nextest run --workspace             # all green

# Frontend (only if web/ changed)
cd web && npx tsc --noEmit && npx vitest run

# OWASP / dependency
cargo deny check                          # only if deps changed
```

Retry up to 3 cycles on failure; if still red, return `FIX_REQUIRED` with failing test names.

## Response Format (back to Claude)

Hard cap **≤400 tokens**. Return only:

- `changed files`: one line each → `path — one-sentence intent`
- `test result`: `pass` or `fail` with failing test names
- `scope deviations`: any `tasks.md` item not implementable as written
- `verdict` (if you ran a fresh-eyes review): `APPROVE` or `FIX_REQUIRED`

Forbidden: code blocks, diffs, build logs, file contents, full error stacks.

## Self-Review Checklist (before returning)

| Check | Required |
| ----- | -------- |
| All `tasks.md` items closed or flagged | ✓ |
| `cargo clippy` zero warnings | ✓ |
| Hexagonal layer dependencies clean (no reverse imports) | ✓ |
| Scale-target patterns upheld (queue dispatch, capacity, thermal) | ✓ |
| FSD layers intact on frontend (entities ← features ← widgets ← pages ← app) | ✓ |
| OWASP basics if input/auth touched | ✓ |
| No AI/LLM mention in commit message | ✓ |

## Escalation

Return to Claude (do not improvise) when:

- `.specs/veronex/{scope}/spec.md` is missing or contradicts `tasks.md`
- A task touches auth, provider trust, payment, or migration in a way the scope does not cover
- 3 retries still red on the same test
- A new pattern would need to be introduced (Constitutional CDD change)
- Scale-target conflict (a fix would violate 10K/1M/1K targets)

Per [.add/escalation.md](.add/escalation.md) and [.add/codex-delegate.md#escalation](.add/codex-delegate.md#escalation).

## Compaction & Internal Subagents (2026)

| Tactic | When |
| ------ | ---- |
| `/compact` | Long thread before a major step — preserves intent, drops verbose history |
| Internal Codex subagent | Bounded exploration, running test suites, triage — keeps verbose output out of the main thread |
| Plan mode (`/plan`) | Complex tasks — produces an execution plan you can review without modifying code |
| One thread per coherent unit | Fork only when work truly branches; do not multiplex |

Subagents are NOT automatically cheaper — heavy multi-agent flows can use 4–7× more tokens than a single thread. Only spawn one when isolating verbose output beats the spawn overhead.

## Reference

Full delegation contract: [.add/codex-delegate.md](.add/codex-delegate.md).
