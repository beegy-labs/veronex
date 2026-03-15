# AGENTS.md

> Universal LLM entry point | **Last Updated**: 2026-03-15

## Start

Read [.ai/README.md](.ai/README.md)

## Identity

| Term | Definition |
| ---- | ---------- |
| CDD | System SSOT and reconstruction baseline |
| SDD | CDD-derived change plan |
| ADD | Autonomous execution and policy selection engine |

Core loop: `CDD → SDD → ADD → CDD (feedback)`

Full definitions: [docs/llm/policies/identity.md](docs/llm/policies/identity.md)

## Frameworks

| Directory | Framework | Role |
| --------- | --------- | ---- |
| `.ai/` + `docs/llm/` | CDD | System SSOT — rules, patterns, architecture, constraints |
| `.specs/` | SDD | Change plans — specs, tasks, scope |
| `.add/` | ADD | Execution — workflow prompts, policy selection |

## Commit Rules

| Rule | Detail |
| ---- | ------ |
| No AI mention | Never reference Claude, GPT, Copilot, AI, LLM in commits, PR titles, PR bodies |
| No AI co-author | No Co-Authored-By AI trailers |
| Full spec | [.ai/git-flow.md](.ai/git-flow.md) |

## Doc Formatting

Applies to `.ai/`, `docs/llm/`, `.add/`. Full spec: [docs/llm/policies/token-optimization.md](docs/llm/policies/token-optimization.md)

| Rule | Detail |
| ---- | ------ |
| No emoji | No Unicode emoji |
| No decorative ASCII | No borders, box-drawing chars |
| No prose/filler | Tables over sentences |
| Indent | 2-space, max 2 levels |
| Headers | H1 + H2 + H3 (limited), no H4+ |
| Format priority | Tables > YAML > bullets > code > prose |

## Workflows (ADD)

| Action | Workflow |
| ------ | -------- |
| Code review | `.add/code-review.md` |
| Doc sync | `.add/doc-sync.md` |

## LLM Config

| Tool | Config |
| ---- | ------ |
| Claude Code | `CLAUDE.md` |
| OpenAI Codex | `AGENTS.md` |
| Gemini CLI | `GEMINI.md` (future) |
| Cursor | `.cursorrules` (future) |
