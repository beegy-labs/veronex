# Web — Audit Page (/audit)

> **SSOT** | **Tier 2** | Last Updated: 2026-03-02

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add new action filter option | `web/app/audit/page.tsx` action `<Select>` + `ACTION_COLORS` map | Add `<SelectItem>` + color variant entry |
| Add new resource type filter | `web/app/audit/page.tsx` resource type `<Select>` | Add `<SelectItem>` value matching backend enum |
| Add pagination | `web/app/audit/page.tsx` + `web/lib/queries/audit.ts` | Add `offset` state; pass to `auditQuery`; add pagination footer to `DataTable` |
| Change result limit | `web/lib/queries/audit.ts` `auditQuery` `limit: 200` | Adjust numeric value |
| Add new column | `web/app/audit/page.tsx` table + `web/lib/types.ts` `AuditEvent` | Add `TableHead` + `TableCell` + extend type |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/audit/page.tsx` | Audit log page |
| `web/lib/api.ts` | `api.auditEvents()` — builds query string, calls `GET /v1/audit` |
| `web/lib/queries/audit.ts` | `auditQuery` TanStack Query definition |
| `web/lib/types.ts` | `AuditEvent` interface |
| `web/messages/en.json` | i18n keys under `audit.*` |

---

## Page Purpose

Read-only audit trail of all administrative actions taken in the system. Displays who did what, on which resource, from which IP address. Intended for `super` role operators to review activity.

Access control is enforced by the backend (`GET /v1/audit` — JWT-protected). The frontend applies no additional role guard beyond requiring a valid session.

---

## Page Layout

```
Title: "Audit Log"  Subtitle: description                    [Refresh]

[Action filter ▼]  [Resource Type filter ▼]

┌──────────────────────────────────────────────────────────────────────────────────┐
│ Time              Account   Action          Resource Type   Resource Name   IP    │
│ Mar 2 09:14:22   alice     [create]        api_key         prod-key        …     │
│ Mar 2 08:05:11   bob       [login]         account         bob             …     │
│ Mar 1 22:30:00   alice     [delete]        ollama_backend  local-gpu       …     │
└──────────────────────────────────────────────────────────────────────────────────┘
```

- `DataTable minWidth="800px"` — SSOT wrapper.
- **Refresh** button calls `refetch()` on the active query.
- Results capped at **200** most recent events per fetch (no client-side pagination yet).

---

## Filters

Both filters are client-controlled `useState` values that re-key the TanStack Query (triggers a new fetch on change).

### Action filter

| UI value | API value passed |
|----------|-----------------|
| All Actions | *(omitted)* |
| `create` | `create` |
| `update` | `update` |
| `delete` | `delete` |
| `login` | `login` |
| `logout` | `logout` |
| `reset_password` | `reset_password` |

### Resource Type filter

| UI value | API value passed |
|----------|-----------------|
| All Resources | *(omitted)* |
| `account` | `account` |
| `api_key` | `api_key` |
| `ollama_backend` | `ollama_backend` |
| `gemini_backend` | `gemini_backend` |
| `gpu_server` | `gpu_server` |

Filter value `'all'` is mapped to `undefined` in `auditQuery` and omitted from the query string.

---

## Action Badge Colors

```ts
const ACTION_COLORS: Record<string, 'default' | 'secondary' | 'destructive' | 'outline'> = {
  create:         'default',      // filled primary
  update:         'secondary',    // muted
  delete:         'destructive',  // red
  login:          'outline',      // ghost
  logout:         'outline',      // ghost
  reset_password: 'secondary',    // muted
}
// Unmapped actions fall back to 'outline'
```

---

## API Endpoint

| Method | Path | Auth | Query params |
|--------|------|------|-------------|
| `GET` | `/v1/audit` | JWT | `limit`, `offset`, `action`, `resource_type` |

Response: `AuditEvent[]`

The current page always requests `limit=200` and `offset=0`. `action` and `resource_type` are included only when not `'all'`.

```ts
// api.ts
auditEvents(params?: { limit?: number; offset?: number; action?: string; resource_type?: string })
  → GET /v1/audit?limit=200[&action=...][&resource_type=...]
```

---

## Data Type

```ts
interface AuditEvent {
  event_time: string      // ISO 8601 UTC
  account_id: string
  account_name: string    // display name; falls back to resource_id when resource_name is empty
  action: string          // 'create' | 'update' | 'delete' | 'login' | 'logout' | 'reset_password' | …
  resource_type: string   // 'account' | 'api_key' | 'ollama_backend' | 'gemini_backend' | 'gpu_server'
  resource_id: string
  resource_name: string   // shown in table; falls back to resource_id when empty
  ip_address: string      // shown as '—' when empty
  details: string
}
```

---

## TanStack Query Configuration

```ts
// staleTime: 30 000 ms; retry: false; re-fetches on filter change (queryKey includes action + resourceType)
auditQuery(action, resourceType) = queryOptions({
  queryKey: ['audit', action, resourceType],
  queryFn: () => api.auditEvents({ limit: 200, action: ..., resource_type: ... }),
  staleTime: 30_000,
  retry: false,
})
```

---

## Table Columns

| Column | Source field | Notes |
|--------|-------------|-------|
| Time | `event_time` | `fmtDatetime(e.event_time, tz)` — user timezone |
| Account | `account_name` | Monospace, `text-xs` |
| Action | `action` | `<Badge>` with color variant from `ACTION_COLORS` |
| Resource Type | `resource_type` | Muted text |
| Resource Name | `resource_name \|\| resource_id` | Falls back to ID when name absent |
| IP | `ip_address` | Muted text; `'—'` when empty |

Rows use index `i` as key (no stable unique ID available per event).

---

## Date Formatting

All timestamps use `fmtDatetime(value, tz)` from `web/lib/date.ts` with `useTimezone()`.

---

## i18n Keys (`audit.*`)

```
audit.title
audit.description
audit.filterAction
audit.allActions
audit.filterResource
audit.allResources
audit.noEvents
audit.time
audit.account
audit.action
audit.resourceType
audit.resourceName
audit.ip
```

Shared keys also used: `common.loading`, `common.error`, `common.refresh`.

---

## Related Docs

- Analytics / observability pipeline (where audit events originate): `../backend/observability.md`
- Auth and session model: `../backend/auth.md`
- DataTable SSOT: `web/components/data-table.tsx`
