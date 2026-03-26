# Bug Fix

> ADD Execution | **Last Updated**: 2026-03-15

## Trigger

Bug report, test failure, or unexpected behavior discovered.

## Read Before Execution

| Domain | Path |
| ------ | ---- |
| Related domain | `docs/llm/` (affected area) |
| Test files | Related `*_test.rs`, `*.test.ts`, `scripts/e2e/` |
| Patterns | `docs/llm/policies/patterns.md` |

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Reproduce the bug (local or test) |
| 2 | Identify root cause |
| 3 | Write failing test that captures the bug |
| 4 | Fix with minimal change |
| 5 | Verify: `cargo check --workspace` + `cargo nextest run` (or `npx tsc --noEmit` + `vitest` for frontend) |
| 6 | CDD feedback — run `.add/cdd-feedback.md` only if a new pattern or constraint was confirmed |

## Rules

| Rule | Detail |
| ---- | ------ |
| Test-first | Write failing test before fixing |
| Minimal change | Fix only the bug, nothing else |
| No refactoring | Separate refactor from fix |
| Regression check | Run full test suite before commit |
| CDD optional | Only update CDD if new stable knowledge found — not for every fix |
