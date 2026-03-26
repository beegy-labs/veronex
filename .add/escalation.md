# Escalation

> ADD Execution | **Last Updated**: 2026-03-25

## Trigger

Uncertainty during any execution workflow.

## Decision Table

| Situation | Action | Reason |
| --------- | ------ | ------ |
| System identity change | Escalate | Constitutional — requires approval |
| CDD ambiguity | Escalate | May affect policy interpretation |
| Policy conflict | Escalate | Cannot resolve without authority |
| Implementation choice | Proceed (pick simpler) | Operational decision |
| Test failure | Proceed (fix it) | Standard bug-fix workflow |
| Performance tradeoff | Proceed (document decision) | Log rationale in CDD |
| Missing SDD spec | Escalate | Spec-first rule |
| Unclear requirements | Escalate | Avoid building wrong thing |

## Rules

| Rule | Detail |
| ---- | ------ |
| Default: ask | When in doubt, escalate |
| Document decisions | Even when proceeding, log rationale |
| No silent assumptions | Make all assumptions explicit |
