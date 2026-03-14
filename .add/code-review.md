# Code Review

> ADD Execution | Last Updated: 2026-03-14

## Trigger

User requests code review, optimization, or review of specific files.

## Read Before Execution

| Domain | CDD / SDD Path |
|--------|----------------|
| Architecture | `docs/llm/policies/architecture.md` |
| Code patterns | `docs/llm/policies/patterns.md` |
| Testing | `docs/llm/policies/testing-strategy.md` |
| Security | `docs/llm/auth/security.md` |
| Capacity | `docs/llm/inference/capacity.md` |
| Thermal | `docs/llm/providers/hardware.md` |
| Job lifecycle | `docs/llm/inference/job-lifecycle.md` |
| Scheduler spec | `.specs/veronex/scheduler.md` |

Read only the docs relevant to the changed domain.

## Execution

```
1. Identify    changed domain from target files
2. Read        relevant CDD/SDD docs above
3. Scan        target files (git diff or user-specified)
4. Fix         violations directly in code
5. Verify      cargo check, cargo test
6. Update      docs/llm/ if patterns changed
```

## Rules

- Optimize code structure and performance. Never alter logic outcomes, state transitions, or API contracts.
- Fix directly. Do not produce report-only output.
- Ask human only when the fix direction is ambiguous.
- Scope only the target files. Do not refactor surroundings.
