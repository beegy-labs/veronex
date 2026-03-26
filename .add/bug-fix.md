# Bug Fix

> ADD Execution | **Last Updated**: 2026-03-25

## Trigger

Bug report, test failure, or unexpected behavior discovered.

## Read Before Execution

| Domain | Path | When |
|--------|------|------|
| Affected domain | `docs/llm/` (affected area) | Always |
| Code patterns (Rust) | `docs/llm/policies/patterns.md` | Rust bug |
| Code patterns (Frontend) | `docs/llm/policies/patterns-frontend.md` | Frontend bug |
| Auth / security | `docs/llm/auth/security.md` | Auth-related bug |
| Test files | Related `*_test.rs`, `*.spec.ts`, `scripts/e2e/` | Always |

## Execution

| Step | Action |
|------|--------|
| 1 | Reproduce the bug (local or test) |
| 2 | Identify root cause |
| 3 | Write a failing test that captures the bug |
| 4 | Fix with minimal change — no refactoring |
| 5 | Verify — see `.add/README.md` Verification Commands for the full table |
| 6 | CDD sync — if bug reveals a missing pattern, route per `.add/README.md` CDD Sync Routing |

## Rules

| Rule | Detail |
|------|--------|
| Test-first | Write failing test before fixing |
| Minimal change | Fix only the bug, nothing else |
| No refactoring | Separate refactor from fix — open a follow-up if needed |
| Regression check | Run full test suite before commit |
