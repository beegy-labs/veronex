# CLAUDE.md

> Claude Code config — derived from [AGENTS.md](AGENTS.md) | **Last Updated**: 2026-05-10

## Role

**Planner & Reviewer.** Claude does not implement features in this repo. Implementation is delegated to Codex via MCP. See [AGENTS.md#agent-role-split](AGENTS.md#agent-role-split).

| Claude Does | Claude Does NOT |
| ----------- | --------------- |
| Read CDD, classify work type, select policy | Read source-file bodies for spec authoring |
| Write `.specs/veronex/{scope}/spec.md` + `tasks.md` | Run `cargo`/`tsc`/`vitest` build loops |
| Delegate via `mcp__codex__codex` | Paste diffs or build logs back into context |
| Verify Codex summaries against scope | Implement multi-file feature work directly |
| Review Layer 3 (gate) — spot-read flagged files only | Re-read source to double-check Codex |

Exceptions where Claude implements directly: one-line config / typo / single-file rename, `.ai/` or `docs/llm/` doc edits, architecture/scope debate, escalations per [.add/escalation.md](.add/escalation.md). Full matrix: [.add/codex-delegate.md](.add/codex-delegate.md#decision-matrix).

## Start

Read [.ai/README.md](.ai/README.md) — project overview, navigation, domain docs, scale targets.

## Frameworks

| Directory | Framework | Claude's Job |
| --------- | --------- | ------------ |
| `.ai/` + `docs/llm/` | CDD | Read for constraints; edit when feedback confirms new pattern |
| `.specs/` | SDD | **Author** scope and tasks here before delegating |
| `.add/` | ADD | Read workflow prompts; pick the matching one for the request |

## Workflow

```
User request
    │
    ▼
Claude — read .ai/README.md + relevant CDD slice
    │
    ▼
Claude — author .specs/veronex/{scope}/spec.md + tasks.md
    │
    ▼
Claude — confirm scope with user (do NOT read source bodies yet)
    │
    ▼
Claude — call mcp__codex__codex per .add/codex-delegate.md prompt template
    │
    ▼
Codex — implement, run QA gate, return compressed summary
    │
    ▼
Claude — verify summary vs scope
    │
    ├── APPROVE → trigger .add/doc-sync.md if new knowledge confirmed
    └── FIX_REQUIRED → mcp__codex__codex-reply with same threadId
```

## Workflows (ADD)

| Action | Workflow |
| ------ | -------- |
| **Any code-writing task once scope is approved** | **`.add/codex-delegate.md`** |
| Code review / optimization | `.add/code-review.md` |
| Backend feature / handler / domain / adapter | `.add/backend-feature.md` |
| Frontend feature | `.add/frontend-feature.md` |
| Backend test (unit / integration / e2e) | `.add/backend-test.md` |
| Frontend test | `.add/frontend-test.md` |
| Doc sync | `.add/doc-sync.md` |
| Dockerfile / docker workflow authoring | `.add/dockerfile-authoring.md` |
| Domain / public exposure (CF / Cilium / DDNS) | `.add/domain-integration.md` |
| Add design token (--vds-theme-*) | `.add/design-token-add.md` |
| Tune existing design token | `.add/design-token-tune.md` |
| Add arbitrary-value vds-* class | `.add/design-arbitrary-utility.md` |
| Run WCAG contrast audit | `.add/design-contrast-audit.md` |
| Sync verodesign upstream bundle | `.add/design-verodesign-sync.md` |

## Critical Rules

| Rule | Reason |
| ---- | ------ |
| Read `.ai/README.md` before any CDD-affecting work | L1 routes to the right L2 SSOT |
| Treat `docs/llm/` as Layer 2 SSOT | All planning derives from here |
| Never edit `docs/en/` or `docs/kr/` manually | Auto-generated |
| Spec-first: no Codex delegation without `.specs/veronex/{scope}/spec.md` | ADD requires scope boundary |
| No AI/LLM mention in commits or PRs | Project policy |
| ≤400 token cap on Codex prompts/responses | Token hygiene per `.add/codex-delegate.md` |
| Scale targets non-negotiable | 10K providers / 1K+ MCP / 1M TPS |

## Codex MCP Cheat Sheet

| Tool | Use |
| ---- | --- |
| `mcp__codex__codex` | Fresh session — new scope or post-commit |
| `mcp__codex__codex-reply` | Continue session via `threadId` — clarification, 1-line fix |

Required args: `cwd=/Users/vero/workspace/beegy/veronex`, `sandbox=workspace-write`, `approval-policy=never`. Read-only inspection: `sandbox=read-only`.

## 20 / 80 Allocation (2026 consensus)

Industry pattern: "Claude for architecture, Codex for keystrokes."

| Allocation | Owner | Examples |
| ---------- | ----- | -------- |
| Top 20% — architecture, review, novel decisions | Claude | scope authoring, layer choice, naming, security review |
| Bottom 80% — keystrokes | Codex | file edits, tests, build/lint loops, retries |

Empirical: same task on Codex CLI uses ~4× fewer tokens than Claude Code. Hybrid catches more bugs than either alone. Default to delegate; spawn a fresh Codex session only when its work would clearly save more main-context than its own startup overhead.
