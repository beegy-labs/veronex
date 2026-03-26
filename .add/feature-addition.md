# Feature Addition & Implementation

> ADD Execution | **Last Updated**: 2026-03-25

## Trigger

- User requests a new feature (no SDD yet), **or**
- SDD spec moves to active status (SDD already exists)

## Read Before Execution

Read only docs relevant to the target domain:

| Domain | Path | When |
|--------|------|------|
| Feature spec | `.specs/veronex/<feature>.md` | Always |
| Architecture | `docs/llm/policies/architecture.md` | Always |
| Code patterns (Rust) | `docs/llm/policies/patterns.md` | Rust changes |
| Code patterns (Frontend) | `docs/llm/policies/patterns-frontend.md` | Frontend changes |
| Frontend review | `.add/frontend-review.md` | Frontend changes |
| Domain docs | `docs/llm/` (affected domain) | Domain-specific |
| Scale targets | `.add/README.md` | Always |

## Execution

| Step | Action |
|------|--------|
| 1 | **SDD check** — verify spec exists in `.specs/veronex/`; if not, create it before writing any code |
| 2 | Read CDD constraints for the target domain |
| 3 | Plan approach — identify layers to touch (domain / application / infrastructure); flag scale risks early |
| 4 | Implement following hexagonal architecture (`domain → application (ports) → infrastructure (adapters)`) |
| 5 | Write tests — unit for pure logic, integration for API contracts, E2E for user flows |
| 6 | Verify — see [Verification Commands](.add/README.md) for the full table |
| 7 | CDD sync — update the relevant doc per [CDD Sync Routing](.add/README.md) |

## Rules

| Rule | Detail |
|------|--------|
| Spec-first | No code without SDD spec |
| Hexagonal | `domain → application → infrastructure` — never skip layers |
| Scale-aware | All data paths must hold at 10,000 providers / 1,000,000 TPS — see `.add/README.md` |
| Test before commit | Every new code path needs a test |
| CDD feedback | Always update docs after completion |
