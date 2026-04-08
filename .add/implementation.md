# Implementation

> ADD Execution | **Last Updated**: 2026-03-15

## Trigger

SDD task moves to active status.

## Read Before Execution

| Domain | Path |
| ------ | ---- |
| Task spec | `.specs/veronex/<task>.md` |
| Domain docs | `docs/llm/` (relevant domain) |
| Patterns | `docs/llm/policies/patterns.md` |
| Architecture | `docs/llm/policies/architecture.md` |

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Read SDD spec, confirm requirements |
| 2 | Read CDD constraints for target domain |
| 3 | Plan approach (mental model, no doc) |
| 4 | Implement — hexagonal for `veronex`; flat module for `veronex-mcp` (see `infra/crate-structure.md`) |
| 5 | Write tests (unit + integration) |
| 6 | Run full test suite |
| 7 | Update CDD docs via `.add/cdd-feedback.md` |

## Scale Assumption

All implementation targets:

| Axis | Target |
| ---- | ------ |
| Providers (Ollama servers) | **10,000** |
| Concurrent requests (TPS) | **1,000,000** |

Design every data path, query, and async task with these numbers in mind. No O(N) sequential DB queries over providers, no unbounded in-memory collections, no blocking hot paths.

## Rules

| Rule | Detail |
| ---- | ------ |
| Spec-first | Validate SDD spec before coding |
| veronex: hexagonal | domain -> application -> infrastructure |
| veronex-mcp: flat module | Tool trait + tools/{name}.rs — no hexagonal layers |
| CDD compliance | Check constraints before and after |
| CDD feedback | Always update docs after completion |
