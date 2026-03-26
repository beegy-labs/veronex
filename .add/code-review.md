# Code Review

> ADD Execution | **Last Updated**: 2026-03-25

## Trigger

User requests code review, optimization, or review of specific files.

> **Frontend-only changes**: use [`frontend-review.md`](frontend-review.md) instead.

## Read Before Execution

Read only docs relevant to the changed domain:

| Domain | Path | When |
|--------|------|------|
| Architecture | `docs/llm/policies/architecture.md` | Always |
| Code patterns (Rust) | `docs/llm/policies/patterns.md` | Rust changes |
| Code patterns (Frontend) | `docs/llm/policies/patterns-frontend.md` | Frontend changes |
| Frontend review criteria | `.add/frontend-review.md` | Frontend changes |
| Testing | `docs/llm/policies/testing-strategy.md` | Test changes |
| Security | `docs/llm/auth/security.md` | Auth / token / session changes |
| Capacity | `docs/llm/inference/capacity.md` | Scheduler / inference changes |
| Thermal | `docs/llm/providers/hardware.md` | GPU / hardware-aware changes |
| Job lifecycle | `docs/llm/inference/job-lifecycle.md` | Job routing / dispatch changes |
| Scheduler spec | `.specs/veronex/scheduler.md` | Scheduler changes |
| Scale targets | `.add/README.md` | Always |

## Execution

| Step | Action |
|------|--------|
| 1 | Identify changed domain from target files |
| 2 | Read relevant CDD/SDD docs above |
| 3 | Scan target files (`git diff` or user-specified) |
| 4 | **Best practices check** — run audit greps from `.add/best-practices.md` Part 3 against changed files: P1 always; P2 if touching infra/handlers/Valkey/DashMap; P3 if touching shared utilities or tests |
| 5 | Fix violations directly in code (P1 → P2 → P3 order) |
| 6 | Verify — see `.add/README.md` Verification Commands |
| 7 | **CDD sync** — if a new pattern is established, route to the correct doc per `.add/README.md` CDD Sync Routing |
| 8 | Same issue repeated 2+ times → run `.add/best-practices.md` Part 1 update workflow |

## Rules

| Rule | Detail |
|------|--------|
| Preserve behavior | Never alter logic outcomes, state transitions, or API contracts |
| Fix directly | No report-only output |
| Ask human | Only when fix direction is ambiguous |
| Scope | Target files only — no surrounding refactor |
| Scale | Every review must validate no O(N) scan / sequential await / unbounded memory at 10,000 providers / 1M TPS — see `.add/README.md` |
