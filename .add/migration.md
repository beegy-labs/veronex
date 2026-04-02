# Migration

> ADD Execution | **Last Updated**: 2026-03-15

## Trigger

Schema change, data migration, or new table needed.

## Read Before Execution

| Domain | Path |
| ------ | ---- |
| Current schema | `migrations/postgres/`, `migrations/clickhouse/` |
| Domain docs | `docs/llm/` (affected domain) |
| DB patterns | `docs/llm/policies/patterns.md` |

## Execution

| Step | Action |
| ---- | ------ |
| 1 | Write `up.sql` + `down.sql` migration pair |
| 2 | Test on clean DB (`docker compose down -v && up`) |
| 3 | Verify rollback works (apply down, re-apply up) |
| 4 | CDD feedback — run `.add/cdd-feedback.md` (schema changes are Reference classification) |

## Rules

| Rule | Detail |
| ---- | ------ |
| Always provide down | Every up.sql needs a matching down.sql |
| Test on clean DB | Never assume existing state |
| Sequential numbering | `000NNN_<name>.{up,down}.sql` |
| No ALTER in prod | Without backup and rollback plan |
