# Doc Sync

> ADD Execution | **Last Updated**: 2026-03-25

## Trigger

- User requests doc cleanup, CDD sync, or alignment check
- Code changed and related CDD docs are now stale
- Distinct from `cdd-feedback.md` — this fixes existing doc-code divergence; cdd-feedback adds new knowledge

## Principle

| Rule | Detail |
| ---- | ------ |
| Code is SSOT | Docs describe code, not the other way around |
| No duplication | One fact in one place, reference elsewhere |
| Token optimization | Layer 1/2 docs follow `docs/llm/policies/token-optimization.md` |
| Layer 3/4 protected | `docs/en/`, `docs/kr/` are auto-generated — never edit directly |

## Read Before Execution

| Domain | Path |
| ------ | ---- |
| CDD policy | `docs/llm/policies/cdd.md` |
| Token rules | `docs/llm/policies/token-optimization.md` |
| Terminology | `docs/llm/policies/terminology.md` |
| Doc index | `docs/llm/README.md` |

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Read current code to establish ground truth |
| 2 | Compare CDD docs against code — identify divergence |
| 3 | Fix docs that diverge from code |
| 4 | Apply token optimization (no emoji, no prose, tables over bullets) |
| 5 | Remove duplicate content across docs |
| 6 | Check line limits — split if >30 lines over limit (`token-optimization.md#split-guidelines`) |
| 7 | Update `docs/llm/README.md` index if docs added/removed |
| 8 | If control flow changed — update `docs/llm/flows/{subsystem}.md` to match code |

## Rules

| Rule | Detail |
| ---- | ------ |
| Code wins | If doc contradicts code, fix the doc |
| No code changes | Scope is docs only — never modify source code |
| Layer 1 editable | `.ai/` — pointers only, max 500 tokens (~50 lines) |
| Layer 2 editable | `docs/llm/` — SSOT, max 2,000 tokens (pages/ max 1,500) |
| Layer 3/4 read-only | `docs/en/`, `docs/kr/` — auto-generated, not directly editable |
| No orphan docs | Every doc must be indexed in `docs/llm/README.md` |
