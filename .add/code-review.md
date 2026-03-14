# Code Review

> ADD Execution | Last Updated: 2026-03-14

## Trigger

User requests code review, optimization, or review of specific files.

## Read Before Execution

Read only docs relevant to the changed domain.

| Domain | Path |
| ------ | ---- |
| Architecture | `docs/llm/policies/architecture.md` |
| Code patterns | `docs/llm/policies/patterns.md` |
| Testing | `docs/llm/policies/testing-strategy.md` |
| Security | `docs/llm/auth/security.md` |
| Capacity | `docs/llm/inference/capacity.md` |
| Thermal | `docs/llm/providers/hardware.md` |
| Job lifecycle | `docs/llm/inference/job-lifecycle.md` |
| Scheduler spec | `.specs/veronex/scheduler.md` |

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Identify changed domain from target files |
| 2 | Read relevant CDD/SDD docs above |
| 3 | Scan target files (git diff or user-specified) |
| 4 | Fix violations directly in code |
| 5 | Verify via `cargo check`, `cargo test` |
| 6 | Update `docs/llm/` if patterns changed |

## Rules

| Rule | Detail |
| ---- | ------ |
| Preserve behavior | Never alter logic outcomes, state transitions, or API contracts |
| Fix directly | No report-only output |
| Ask human | Only when fix direction is ambiguous |
| Scope | Target files only, no surrounding refactor |
