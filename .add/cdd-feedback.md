# CDD Feedback

> ADD Execution | **Last Updated**: 2026-03-15

## Trigger

After any task completion (feature, fix, refactor, migration).

## Read Before Execution

| Domain | Path |
| ------ | ---- |
| Completed diff | `git diff` of completed work |
| Related CDD docs | `docs/llm/` (affected domain) |
| CDD policy | `docs/llm/policies/cdd.md` |

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Identify new patterns or constraints from completed work |
| 2 | Classify: Constitutional / Operational / Reference |
| 3 | Update appropriate CDD doc in `docs/llm/` |
| 4 | If Constitutional change, flag for human approval |

## Rules

| Rule | Detail |
| ---- | ------ |
| Operational | Accumulate freely (patterns, conventions) |
| Constitutional | Require human approval (identity, policy) |
| Reference | Update after completion (API docs, schema) |
| No speculation | Only document what was actually built |
