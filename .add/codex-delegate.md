# Codex Delegate

> ADD Reference | Delegate implementation to Codex MCP to preserve Claude tokens

## Purpose

Claude plans and reviews. Codex implements. Goal: keep Claude context clean, route file-body reads, build loops, and retry cycles to Codex.

## MCP Tools

| Tool | Use |
|------|-----|
| `mcp__codex__codex` | Start new Codex session — `prompt`, `cwd`, `sandbox`, `approval-policy`, `model` |
| `mcp__codex__codex-reply` | Continue same session via `threadId` |

## Required Args

| Arg | Default | Notes |
|-----|---------|-------|
| `cwd` | `/Users/vero/workspace/beegy/veronex` | Always absolute |
| `sandbox` | `workspace-write` | `read-only` for scouting |
| `approval-policy` | `never` | Codex retries without round-tripping Claude |
| `prompt` | — | Reference SDD spec + result format |

## Decision Matrix

| Task | Delegate | Stay in Claude |
|------|----------|----------------|
| Backend handler/domain/adapter with `.specs/veronex/` spec | ✓ | ✗ |
| New backend test per `backend-test.md` | ✓ | ✗ |
| Dockerfile authoring per `dockerfile-authoring.md` | ✓ | ✗ |
| Migration with explicit SQL | ✓ | ✗ |
| Provider scale-out / MCP fan-out work | ✓ | ✗ |
| Architecture decision affecting 10K-provider / 1M-TPS targets | ✗ | ✓ |
| Bug with unknown root cause | ✗ (until cause identified) | ✓ |
| `docs/llm/` doc edits | ✗ (token-optimization rules) | ✓ |
| One-line config / typo | ✗ | ✓ |

## Handoff Pattern

| Step | Owner | Action |
|------|-------|--------|
| 1 | Claude | Open or write `.specs/veronex/{scope}/spec.md` + `tasks.md` |
| 2 | Claude | Confirm scope; do NOT read full source files |
| 3 | Claude | Call `mcp__codex__codex` with template below |
| 4 | Codex | Implement, run cargo clippy + cargo test (+ frontend checks if touched), retry on failure |
| 5 | Codex | Return compressed summary |
| 6 | Claude | Verify against scope; trigger `doc-sync.md` / `best-practices.md` Part 1 if needed |

## Prompt Template

```
Read .specs/veronex/{scope}/spec.md and tasks.md.
Implement only what tasks.md lists. Do not expand scope.

Follow .add/best-practices.md, .add/backend-feature.md (if backend) or
.add/frontend-feature.md (if frontend). Respect scale targets:
10K providers, 1K+ MCP servers, 1M TPS.

Run cargo clippy + cargo test for backend; tsc + vitest for frontend.
Retry up to 3 times on failure.

Return ONLY:
- changed files (path — one-sentence intent)
- test result: pass/fail with failing test names
- scope deviations
DO NOT paste code, diffs, or build logs. ≤400 tokens.
```

## Review Pipeline

Three layers. Layers 1–2 cost zero Claude tokens.

| Layer | Runner | Scope | Claude tokens |
|-------|--------|-------|---------------|
| 1. Mechanical | Codex (impl session) | clippy, test, wiring, scope items closed | 0 |
| 2. Fresh-eyes | Codex (new session, no impl context) | scope deviation, layer breaks, missed edges | 0 |
| 3. Gate | Claude | reads Layer 2 report; spot-reads flagged files only | ≤500 |

### Layer 2 Prompt

Start a fresh `mcp__codex__codex` session:

```
Review the diff at HEAD against .specs/veronex/{scope}/spec.md and tasks.md.
Apply .add/code-review.md (or backend-review.md / frontend-review.md as
appropriate) and .add/best-practices.md.

You did NOT write this code — review with fresh eyes.

Report ONLY:
- scope violations
- architectural drift (Rust crate boundaries, frontend FSD layers)
- security / OWASP issues (see best-practices.md Part 4)
- scale-target violations (10K provider / 1M TPS / 1K MCP)
- verdict: APPROVE / FIX_REQUIRED
DO NOT paste code. ≤300 tokens.
```

### When Claude Reads Source Directly

| Trigger | Reason |
|---------|--------|
| Layer 2 `FIX_REQUIRED` with ambiguous cause | Judgment |
| Auth / provider trust / payment paths | Blast radius |
| First introduction of new pattern | SSOT decision |
| User explicitly requests Claude review | Intent |

## Response Compression Rules

| Rule | Reason |
|------|--------|
| No code in response | Pasting back into Claude defeats savings |
| No build logs | Errors stay in Codex |
| File list with one-line intent | Claude can grep later |
| Mark deviations explicitly | Forces flag rather than silent expansion |
| ≤400 tokens default | Adjust for multi-crate change |

## Sandbox Selection

| Goal | Sandbox |
|------|---------|
| Module summary, structure scout | `read-only` |
| Implement spec, run tests | `workspace-write` |
| Touch system / docker / provider infra | Stay in Claude — escalate per `escalation.md` |

## Continuation

Reuse `codex-reply` with `threadId` when:

| Case | Why |
|------|-----|
| Layer 2 flagged a fixable item | Avoid re-priming |
| Test failure with 1-line fix | Cheaper than fresh session |
| Follow-up task in same scope | Keep context warm |

Start fresh session on scope change or after commit.

## Token Hygiene

| Anti-pattern | Fix |
|--------------|-----|
| Claude reads source to write spec | Ask Codex (read-only) for module summary |
| Codex returns full diff | Enforce 400-token cap |
| Claude re-reads to verify | Trust summary; spot-check flagged lines only |
| Multiple fresh sessions per scope | Use `codex-reply` |

## Escalation

If Codex flags scope deviations, fails 3 retries, or returns "scope unclear" — return to Claude. Update `.specs/veronex/{scope}/spec.md`, then redelegate. Do not let Claude solve by reading source unless `escalation.md` criteria met.

## Empirical Targets (2026 data)

| Source | Reduction |
| ------ | --------- |
| GitHub Agentic Workflows post-fix | 62% sustained over 109 runs |
| Focused-task case studies | 40–70% |
| Anthropic prompt caching (proper static-first layout) | up to 90% on cached prefix tokens |
| This repo's hybrid Claude+Codex target | 25–30% conservative, 40–50% achievable |

Risk: subagent-heavy / multi-session workflows can use **4–7× more** tokens than single-thread sessions if used for small tasks. Skip delegation for one-line edits, typo fixes, single-file renames.

Cache hygiene (Anthropic 5-min TTL since 2026 Q1):

| Rule | Reason |
| ---- | ------ |
| Static prefix before per-request material | Tools, system, `.ai/`, this file — keeps prefix cache valid |
| No dynamic timestamps in static sections | One shifting byte invalidates the whole prefix |
| Reuse `mcp__codex__codex-reply` over fresh sessions | Forks reuse parent prompt cache |

## Sources (2026)

- [Anthropic Prompt Caching 2026](https://aicheckerhub.com/anthropic-prompt-caching-2026-cost-latency-guide)
- [GitHub Agentic Workflows token efficiency](https://github.blog/ai-and-ml/github-copilot/improving-token-efficiency-in-github-agentic-workflows/)
- [OpenAI Codex Best Practices](https://developers.openai.com/codex/learn/best-practices)
- [Claude Code Subagents 2026 Guide](https://nimbalyst.com/blog/claude-code-subagents-guide/)
- [Codex CLI vs Claude Code 2026](https://particula.tech/blog/codex-vs-claude-code-cli-agent-comparison)
