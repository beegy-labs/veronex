# Migration

> ADD Execution | **Last Updated**: 2026-03-25

## Trigger

Schema change, data migration, or new table needed.

## Read Before Execution

| Domain | Path |
|--------|------|
| Current schema | `migrations/postgres/`, `migrations/clickhouse/` |
| Affected domain docs | `docs/llm/` (affected domain) |
| DB patterns | `docs/llm/policies/patterns.md` |

## Execution

| Step | Action |
|------|--------|
| 1 | Write `up.sql` + `down.sql` migration pair |
| 2 | Test on clean DB: `docker compose down -v && docker compose up -d` |
| 3 | Verify rollback: apply `down.sql`, then re-apply `up.sql` |
| 4 | Run `cargo nextest run --workspace` — no regressions |
| 5 | CDD sync — update affected domain docs per `.add/README.md` CDD Sync Routing |

## Rules

| Rule | Detail |
|------|--------|
| Always provide down | Every `up.sql` needs a matching `down.sql` |
| Test on clean DB | Never assume existing state |
| Sequential numbering | `000NNN_<name>.{up,down}.sql` |
| No ALTER in prod | Without backup and rollback plan |
