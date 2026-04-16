# Web — Audit Page (/audit)

> CDD Layer 2 | **Last Updated**: 2026-03-04

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add action filter option | `web/app/audit/page.tsx` action `<Select>` + `ACTION_COLORS` | Add `<SelectItem>` + color entry |
| Add resource type filter | `web/app/audit/page.tsx` resource type `<Select>` | Add `<SelectItem>` matching server enum |
| Add pagination | `page.tsx` + `web/lib/queries/audit.ts` | Add `offset` state; pass to query; add pagination footer |
| Change result limit | `web/lib/queries/audit.ts` `auditQuery` | Adjust `limit: 200` value |
| Add new column | `page.tsx` table + `web/lib/types.ts` `AuditEvent` | Add `TableHead` + `TableCell` + extend type |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/audit/page.tsx` | Audit log page |
| `web/lib/api.ts` | `api.auditEvents()` — `GET /v1/audit` |
| `web/lib/queries/audit.ts` | `auditQuery` TanStack Query definition |
| `web/lib/types.ts` | `AuditEvent` interface |
| `web/messages/en.json` | i18n keys under `audit.*` |

## Page Purpose

Read-only audit trail of admin actions. Shows who did what, on which resource, from which IP. For `super` role operators. Access controlled by backend JWT on `GET /v1/audit`.

## Page Layout

```
Title: "Audit Log"  Subtitle: description                    [Refresh]
[Action filter v]  [Resource Type filter v]
| Time | Account | Action | Resource Type | Resource Name | IP |
```

- `DataTable minWidth="800px"` wrapper
- Refresh calls `refetch()`; results capped at 200 most recent (no pagination yet)

## Filters

Both are `useState` values that re-key the TanStack Query (new fetch on change). Value `'all'` maps to `undefined` and is omitted from the query string.

### Action filter

| Values | `create`, `update`, `delete`, `login`, `logout`, `reset_password` |
|--------|-------------------------------------------------------------------|

### Resource Type filter

| Values | `account`, `api_key`, `ollama_provider`, `gemini_provider`, `gpu_server` |
|--------|-------------------------------------------------------------------------|

## Action Badge Colors

| Action | Badge variant |
|--------|---------------|
| `create` | `default` (filled primary) |
| `update`, `reset_password` | `secondary` (muted) |
| `delete` | `destructive` (red) |
| `login`, `logout` | `outline` (ghost) |
| *(unmapped)* | `outline` fallback |

## API Endpoint

| Method | Path | Auth | Params |
|--------|------|------|--------|
| `GET` | `/v1/audit` | JWT | `limit`, `offset`, `action`, `resource_type` |

Response: `AuditEvent[]`. Current page requests `limit=200`, `offset=0`. Filters included only when not `'all'`.

## AuditEvent Type

| Field | Type | Notes |
|-------|------|-------|
| `event_time` | `string` | ISO 8601 UTC |
| `account_id` | `string` | |
| `account_name` | `string` | Display name |
| `action` | `string` | create/update/delete/login/logout/reset_password |
| `resource_type` | `string` | account/api_key/ollama_provider/gemini_provider/gpu_server |
| `resource_id` | `string` | |
| `resource_name` | `string` | Falls back to `resource_id` when empty |
| `ip_address` | `string` | Shown as `'--'` when empty |
| `details` | `string` | For update events, includes old→new deltas: e.g. `"name: 'old' → 'new', url: 'http://a' → 'http://b'"`. Delete events include the removed resource's URL. |

## TanStack Query Config

`queryKey: ['audit', action, resourceType]` | `staleTime: 30s` | `retry: false`
Re-fetches on filter change via queryKey dependency.

## Table Columns

| Column | Source | Notes |
|--------|--------|-------|
| Time | `event_time` | `fmtDatetime(e.event_time, tz)` via `useTimezone()` |
| Account | `account_name` | Monospace, `text-xs` |
| Action | `action` | `<Badge>` with `ACTION_COLORS` variant |
| Resource Type | `resource_type` | Muted text |
| Resource Name | `resource_name \|\| resource_id` | Falls back to ID |
| IP | `ip_address` | Muted; `'--'` when empty |

Rows keyed by index `i` (no stable unique ID per event).

## i18n Keys

`audit.*`: title, description, filterAction, allActions, filterResource, allResources, noEvents, time, account, action, resourceType, resourceName, ip

Shared: `common.loading`, `common.error`, `common.refresh`

## Related Docs

- Audit event pipeline: `../../infra/otel-pipeline.md`
- Auth/session model: `../../auth/jwt-sessions.md`
- DataTable SSOT: `web/components/data-table.tsx`
