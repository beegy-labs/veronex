# Code Review

> ADD Execution | **Last Updated**: 2026-03-24

## Trigger

User requests code review, optimization, or review of specific files.

## Read Before Execution

Read only docs relevant to the changed domain.

| Domain | Path |
| ------ | ---- |
| Architecture | `docs/llm/policies/architecture.md` |
| Code patterns (Rust) | `docs/llm/policies/patterns.md` |
| Code patterns (Frontend) | `docs/llm/policies/patterns-frontend.md` |
| Frontend review criteria | `.add/frontend-review.md` |
| Testing | `docs/llm/policies/testing-strategy.md` |
| Security | `docs/llm/auth/security.md` |
| Capacity | `docs/llm/inference/capacity.md` |
| Thermal | `docs/llm/providers/hardware.md` |
| Job lifecycle | `docs/llm/inference/job-lifecycle.md` |
| Scheduler spec | `.specs/veronex/scheduler.md` |
| Best practices update | `.add/best-practices.md` |

> **Frontend changes**: use `.add/frontend-review.md` checklist instead of this file.

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Identify changed domain from target files |
| 2 | Read relevant CDD/SDD docs above |
| 3 | Scan target files (git diff or user-specified) |
| 4 | **Best practices check** — run the relevant audit items from `.add/best-practices.md` Part 3 against changed files: security (P1) items always; architecture/performance (P2) items if touching infra/handlers/Valkey/DashMap; quality (P3) items if touching shared utilities or tests |
| 5 | Fix violations directly in code (P1 → P2 → P3 order) |
| 6 | Verify via `cargo check`, `cargo test` |
| 7 | CDD sync — if a new pattern is established, update the specific doc: architecture change → `docs/llm/policies/architecture.md`; code pattern → `docs/llm/policies/patterns.md`; test pattern → `docs/llm/policies/testing-strategy.md` |
| 8 | Same issue repeated 2+ times → run `.add/best-practices.md` Part 1 update workflow |

## Scale Assumption

All code is written and reviewed against these targets:

| Axis | Target |
| ---- | ------ |
| Providers (Ollama servers) | **10,000** |
| Concurrent requests (TPS) | **1,000,000** |

Every review must validate that no code path introduces O(N) DB scans, sequential awaits, or unbounded memory growth at these scales. Flag any violation as P1 or higher.

## Rules

| Rule | Detail |
| ---- | ------ |
| Preserve behavior | Never alter logic outcomes, state transitions, or API contracts |
| Fix directly | No report-only output |
| Ask human | Only when fix direction is ambiguous |
| Scope | Target files only, no surrounding refactor |
