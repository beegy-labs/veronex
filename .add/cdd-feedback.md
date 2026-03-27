# CDD Feedback

> ADD Execution | **Last Updated**: 2026-03-25

## Trigger

After any task completion (feature, fix, refactor, migration).
Distinct from `doc-sync.md` — this adds **new confirmed knowledge**; doc-sync fixes existing doc-code divergence.

## Read Before Execution

| Domain | Path |
| ------ | ---- |
| Completed diff | `git diff` of completed work |
| Related CDD docs | `docs/llm/` (affected domain) |
| CDD policy | `docs/llm/policies/cdd.md` |

## Classification Decision

| Classification | Condition | Action |
| -------------- | --------- | ------ |
| Constitutional | System identity, boundary, or external contract changed | Update + flag for human approval |
| Operational | New pattern confirmed through use (convention, architecture decision) | Update freely |
| Reference | Feature, API, or schema catalog changed | Update index/catalog |
| None | No new stable knowledge gained | Archive in SDD history only — do NOT update CDD |

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Read completed diff — identify what actually changed |
| 2 | Ask: is there new stable knowledge not yet in CDD? |
| 3 | If no → stop. Archive to SDD history only |
| 4 | Classify each piece of new knowledge (table above) |
| 5 | Update appropriate CDD doc in `docs/llm/` |
| 6 | If Constitutional → flag for human approval before merging |
| 7 | Apply token optimization (`docs/llm/policies/token-optimization.md`) |

## Rules

| Rule | Detail |
| ---- | ------ |
| Confirmed only | Only document what was actually built and validated |
| No speculation | Do not add patterns that were considered but not used |
| No duplication | One fact in one place — reference elsewhere |
| Constitutional = approval | Never merge Constitutional changes without human sign-off |
| Operational = accumulate | Add freely; reflects implementation experience |
| Reference = refresh | Update catalogs/indexes after completion |
| doc-sync is separate | This workflow adds knowledge; doc-sync fixes divergence |
