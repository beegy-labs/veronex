# MCP Server Management Page

> SSOT | **Last Updated**: 2026-03-25 | Branch: feat/mcp-integration

Route: `/mcp` | Nav group: standalone link (footer of Providers group) | Auth: required

---

## Architecture

```
web/app/mcp/page.tsx                     — route entry, heading only
web/app/providers/components/mcp-tab.tsx — all logic: list, register, toggle, delete, stats
web/lib/queries/mcp.ts                   — queryOptions factory for mcp-servers list
web/lib/api.ts                           — mcpServers, registerMcpServer, patchMcpServer, deleteMcpServer, mcpStats
web/lib/types.ts                         — McpServer, McpServerStat, RegisterMcpServerRequest
```

`McpTab` is also embedded in `web/app/providers/page.tsx` as the `mcp` tab — same component, two routes.

---

## Components

### `McpTab` (exported)

Main orchestrator. Responsibilities:
- Fetches `mcpServersQuery()` (list of registered servers)
- 404 handling: if the endpoint doesn't exist (feature flag / old backend), calls `hideSection('mcp')` via `useNav404` to remove the MCP nav item dynamically
- Renders `OrchestratorModelSelector`, register button, server table, `McpStatsCard`
- Delete: uses native `confirm()` currently — **known violation** (should use `ConfirmDialog`)

### `RegisterMcpModal`

Registration modal. Fields: `name`, `slug` (auto-derived from name), `url`, `timeout_secs`.
- Slug auto-derive: `name.toLowerCase().replace(/[^a-z0-9]/g, '_').replace(/_+/g, '_').replace(/^_|_$/g, '')`
- No 2-Step Verify Flow — MCP servers are not verified at registration (unlike GPU servers/Ollama)
- `onSettled` used correctly for cache invalidation
- `canSubmit` gates the register button

### `OrchestratorModelSelector`

Card that selects the Ollama model used for MCP orchestration (stored in `lab_settings`).
- Queries `['lab-settings']` and `['capacity-settings']` (for available Ollama models)
- Uses `onSuccess` for invalidation (known violation — should be `onSettled`)
- Select value: current `mcp_orchestrator_model` or `NONE_VALUE` sentinel (`'__none__'`)

### `McpStatsCard`

Per-server call statistics table with time-window selector (1h / 6h / 24h / 7d / 30d).
- Query key: `['mcp-stats', hours]` — re-fetches when `hours` changes
- **Known violations**: local `fmt_pct`/`fmt_ms` formatters — should use `fmtPct`/`fmtMs` from `chart-theme.ts`; `staleTime: 30_000` hardcoded — should use `STALE_TIME_FAST`

---

## Query: `mcpServersQuery`

File: `web/lib/queries/mcp.ts`

```typescript
export const mcpServersQuery = () => queryOptions({
  queryKey: ['mcp-servers'] as const,
  queryFn: () => api.mcpServers(),
  staleTime: STALE_TIME_FAST,
  refetchInterval: () => withJitter(REFETCH_INTERVAL_FAST),
  refetchIntervalInBackground: false,
})
```

Factory function (not plain object) because `refetchInterval` uses the callback form required by `withJitter()`.

---

## Types

| Type | Fields |
|------|--------|
| `McpServer` | `id`, `name`, `slug`, `url`, `timeout_secs`, `is_enabled`, `online`, `tool_count` |
| `McpServerStat` | `server_slug`, `server_name`, `total_calls`, `success_rate`, `cache_hit_count`, `avg_latency_ms` |
| `RegisterMcpServerRequest` | `name`, `slug`, `url`, `timeout_secs` |

---

## Nav

| Property | Value |
|----------|-------|
| Nav entry | `navItems` in `nav.tsx` under Providers group (or standalone) |
| i18n key | `nav.mcp` |
| Route | `/mcp` |
| Auth | Required (no `PUBLIC_PATHS` entry) |
| Nav hide | `hideSection('mcp')` called on 404 to remove item when backend doesn't support MCP |

---

## i18n Keys (`mcp.*`)

| Key | Usage |
|-----|-------|
| `mcp.title` | Page heading + empty state title |
| `mcp.description` | Page subtitle + empty state body |
| `mcp.register` | Register button label + modal title |
| `mcp.name` | Name field label + table column |
| `mcp.slug` | Slug field label + table column |
| `mcp.slugHint` | Slug format hint below input |
| `mcp.url` | URL field label + table column |
| `mcp.timeout` | Timeout field label |
| `mcp.enabled` | Enabled column header |
| `mcp.tools` | Tools count column header |
| `mcp.online` / `mcp.offline` | Status cell text |
| `mcp.deleteConfirm` | Native confirm message (interpolates `{{name}}`) |
| `mcp.orchestratorModel` | OrchestratorModelSelector card title |
| `mcp.orchestratorModelDesc` | Selector description |
| `mcp.orchestratorModelNone` | None option label |
| `mcp.orchestratorModelSaved` | Save confirmation inline text |
| `mcp.stats` | McpStatsCard title |
| `mcp.statsDesc` | Stats card description |
| `mcp.statsTotalCalls` | Table column |
| `mcp.statsSuccessRate` | Table column |
| `mcp.statsCacheHit` | Table column |
| `mcp.statsAvgLatency` | Table column |
| `mcp.statsNoData` | Empty state text |
| `mcp.statsLoadError` | Error text |

---

## Known Violations (pre-existing, to fix in next review pass)

| Severity | Location | Issue |
|----------|----------|-------|
| P0 | `McpStatsCard` | `fmt_pct`/`fmt_ms` local formatters — use `fmtPct`/`fmtMs` from `chart-theme.ts` |
| P0 | Server table | `Status` column header hardcoded English — add `mcp.status` i18n key |
| P1 | `McpStatsCard` | `staleTime: 30_000` hardcoded — use `STALE_TIME_FAST` |
| P1 | `OrchestratorModelSelector` | `onSuccess` for invalidation — change to `onSettled` |
| P1 | `McpTab` toggle `Switch` | Missing `useOptimistic` |
| P2 | `McpTab` delete | `confirm()` native dialog — replace with `ConfirmDialog` component |
