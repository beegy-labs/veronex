# Web — Servers Page (/servers)

> SSOT | **Last Updated**: 2026-02-28

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add action button to server row | `web/app/servers/page.tsx` `ServersTable` row actions | Add button + handler + modal if needed |
| Add field to RegisterServerModal | `web/app/servers/page.tsx` modal form + `web/lib/api.ts` `registerServer()` | Add field → pass to `api.registerServer(body)` |
| Change live metrics refresh interval | `web/app/servers/page.tsx` `ServerMetricsCell` `refetchInterval` | Default: 30 000 ms |
| Change history hour options | `web/app/servers/page.tsx` `HIST_HOUR_OPTIONS` | Tuple of numbers |
| Add live metric field to server cell | `web/app/servers/page.tsx` `ServerMetricsCell` render | Add field from `NodeMetrics.gpus[n]` |
| Add pill badge to server header | `web/app/servers/page.tsx` `ServersTable` pill section | Follow SSOT pill pattern |
| Change empty state text | `web/app/servers/page.tsx` + `web/messages/en.json` `backends.servers.noServers` | Update i18n key in all 3 locales |
| Change table page size | `web/app/servers/page.tsx` `PAGE_SIZE` constant | Default: 10 |

## Key Files

| File | Purpose |
|------|---------|
| `web/app/servers/page.tsx` | All components + modals for /servers |
| `web/lib/api.ts` | `api.servers()`, `api.registerServer()`, `api.updateServer()`, `api.deleteServer()`, `api.serverMetrics()`, `api.serverMetricsHistory()` |
| `web/lib/types.ts` | `GpuServer`, `NodeMetrics`, `ServerMetricsPoint`, `RegisterGpuServerRequest`, `UpdateGpuServerRequest` |
| `web/messages/en.json` | i18n keys under `backends.servers.*` |

---

## Routing

URL: `/servers` — no `?s=` query param, single page.

Nav entry: top-level `NavLink` with `HardDrive` icon, `labelKey: 'nav.servers'`.

---

## Page Layout

```
GPU Servers
Physical GPU servers — one node-exporter per server.

[N registered]  [● N with metrics]  [N no exporter]    [+ Register Server]

Name         node-exporter endpoint       Live Metrics (30s auto-refresh)    Registered  Actions
──────────────────────────────────────────────────────────────────────────────────────────────
gpu-node-1   http://192.168.1.10:9100     MEM 28.5/64.0 GB  32%              Feb 26      [📊][✏️][🗑]
                                          CPU 32 cores
                                          GPU card0 · 32°C · 10W
gpu-node-2   not configured               —                                   Feb 26      [📊][✏️][🗑]

                 [← 1 / 2 →]   ← pagination controls (hidden when ≤ PAGE_SIZE rows)
```

### Status Pill Badges (SSOT pattern)

| Pill | Condition | Style |
|------|-----------|-------|
| `N registered` | always shown | `bg-muted/60 border-border text-muted-foreground` + `HardDrive` icon |
| `● N with metrics` | `configuredCount > 0` | `bg-status-success/10 border-status-success/30 text-status-success-fg` |
| `N no exporter` | `servers.length - configuredCount > 0` | `bg-muted/40 border-border/60 text-muted-foreground/70` |

### Pagination

`PAGE_SIZE = 10`. Local `page` state inside `ServersTable`. Same pattern as providers page tables:

```typescript
const totalPages = Math.max(1, Math.ceil(servers.length / PAGE_SIZE))
const safePage = Math.min(page, totalPages)
const pageItems = servers.slice((safePage - 1) * PAGE_SIZE, safePage * PAGE_SIZE)
```

Controls (border-t inside Card): range label + ChevronLeft / ChevronRight. Hidden when `totalPages <= 1`.

### Live Metrics (ServerMetricsCell)

`useQuery<NodeMetrics>({ queryKey: ['server-metrics', serverId], refetchInterval: 30_000, retry: false })`

Displays:
- `MEM N.N/N.N GB  N%` — colored: red ≥90%, amber ≥75%
- `CPU N cores`
- `GPU card0 · N°C · NW` — temp colored: red ≥85°C; power in amber `Zap` icon

Error → `unreachable` badge (`bg-status-error/10`) + retry button.

### Actions (per row)

| Button | Icon | Action |
|--------|------|--------|
| History | `BarChart2` | Opens `ServerHistoryModal` |
| Edit | `Pencil` | Opens `EditServerModal` |
| Delete | `Trash2` | `confirm()` → `DELETE /v1/servers/{id}` |

---

## ServerHistoryModal

- Range tabs: 1h / 3h / 6h / 24h
- Charts: Memory Used %, GPU Temperature (°C), GPU Power (W) — Recharts `LineChart`
- Data: `GET /v1/servers/{id}/metrics/history?hours=N`
- Sync button refreshes chart data

---

## RegisterServerModal

Fields:
- **Name** `*` — required, `t('backends.servers.name')`
- **node-exporter URL** — optional, `t('backends.servers.nodeExporterUrl')`, placeholder `http://192.168.1.10:9100`

Calls: `POST /v1/servers` → invalidates `['servers']`

---

## EditServerModal

Pre-filled with current `name` + `node_exporter_url`. Uses TanStack `useMutation`.

Calls: `PATCH /v1/servers/{id}` → invalidates `['servers']`

---

## i18n Keys (messages/en.json → `backends.servers.*`)

```json
"title", "name", "nodeExporterUrl", "registeredAt", "liveMetrics", "history",
"noServers", "noServersHint", "registerServer", "registerTitle", "editTitle",
"nodeExporterUrlPlaceholder", "serverMeta", "loadingServers",
"description", "registered", "withMetrics", "noExporter",
"notConfigured", "unreachable", "nodeExporterHint", "nodeExporterOptional"
```
