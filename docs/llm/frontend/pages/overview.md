# Overview Page

> SSOT | **Last Updated**: 2026-03-08
> Route: `/overview` (default landing page after login)

## Purpose

Main dashboard — single-screen system health view. Aggregates KPIs, infrastructure status, workload metrics, and recent activity from multiple API endpoints.

## Data Sources

| Query | Endpoint | Refresh |
|-------|----------|---------|
| `dashboardStatsQuery` | `GET /v1/dashboard/stats` | default staleTime |
| `providersQuery` | `GET /v1/providers` | default staleTime |
| `serversQuery` | `GET /v1/servers` | default staleTime |
| `serverMetricsQuery(id)` | `GET /v1/servers/{id}/metrics` | per server |
| `serverMetricsHistoryQuery(id, 1440)` | `GET /v1/servers/{id}/metrics/history` | 24h window |
| `performanceQuery(24/168/720)` | `GET /v1/dashboard/performance` | 24h, 7d, 30d |
| `usageAggregateQuery(24)` | `GET /v1/usage/aggregate` | 24h |
| `usageBreakdownQuery(24)` | `GET /v1/usage/breakdown` | 24h |
| `recentJobsQuery` | `GET /v1/dashboard/jobs` | recent jobs |

## Layout (8 Sections)

### Section 1: System KPIs (3 cards)
- **Provider Status**: `online/total` with color coding (all green / partial yellow / all red)
- **Waiting**: pending job count (0=green, <10=yellow, >=10=red)
- **Running**: active job count (>0 = blue info)

### Section 2: Thermal Alert Banner
- Conditional — only renders when any server >= 80C
- Warning (yellow) for >= 80C, Critical (red) for >= 90C
- Per-server temperature badges with link to `/servers`

### Section 3: Infrastructure (Server Health + Power)
- **Server Health card**: per-server connection status + thermal level (normal/warning/critical)
  - Summary counts: connected, unreachable, thermal states
  - Row-level color: `THERMAL_ROW_CLS` Record for border highlighting
- **Power cards** (3): Daily / Weekly / Monthly kWh
  - Delta comparison: today vs same weekday last week, this week vs last, this month vs last
  - Uses `serverHistoryQueries` to sum `gpu_power_w` across all servers

### Section 4: Workload + Latency Monitor (2 cards)
- **Workload table**: Requests + Success Rate across 24h / 7d / 30d
  - Success rate color: >= 99% green, >= 95% yellow, < 95% red
- **Latency Monitor**: P50/P95/P99 across 24h / 7d / 30d
  - Color thresholds per percentile (P50: 1s/3s, P95: 2s/5s, P99: 5s/10s)
  - Mini sparkline: hourly avg latency (24h AreaChart)

### Section 5: Provider Status + API Keys (2 cards)
- **Provider Status**: Ollama (local) + Gemini (API, gated by `gemini_function_calling` lab flag)
  - `ProviderRow` component: online/degraded/offline dot counts
- **API Keys**: active_keys count + total_keys subtitle

### Section 6: Request Trend (24h AreaChart)
- Total vs Success requests per hour
- Gradient fill, theme-aware colors

### Section 7: Top Models (horizontal BarChart)
- Top 8 models by request count (24h)
- Color-coded: Ollama = primary, Gemini = info
- Gemini models filtered out when lab flag disabled

### Section 8: Token Summary + Recent Jobs
- **Token Summary**: total_tokens (24h) with prompt/completion breakdown
- **Recent Jobs table**: model, provider_type, status badge, latency, created_at

## Key Patterns

- **Lab gating**: Gemini providers/models hidden when `gemini_function_calling` disabled via `useLabSettings()`
- **Skeleton loading**: `StatSkeleton` component during stats fetch
- **Error state**: Full-page error card when `dashboardStatsQuery` fails
- **Constants**: `PROVIDER_OLLAMA`, `PROVIDER_GEMINI`, `STATUS_STYLES` from `lib/constants.ts`
- **Chart theme**: All chart styling from `lib/chart-theme.ts` (TOOLTIP_STYLE, AXIS_TICK, etc.)
- **Timezone**: `useTimezone()` for hour labels via `fmtHourLabel()`

## Files

| File | Role |
|------|------|
| `web/app/overview/page.tsx` | Page component — data fetching + error boundary |
| `web/app/overview/components/dashboard-tab.tsx` | Main dashboard layout (sections 1-5) |
| `web/app/overview/components/dashboard-helpers.tsx` | Shared helpers: `ThermalBadge`, `ConnectionDot`, `ProviderRow`, color utils |
| `web/app/overview/components/dashboard-lower-sections.tsx` | `RequestTrendSection`, `TopModelsSection`, `RecentJobsSection`, `TokenSummarySection` |
| `web/app/overview/components/network-flow-tab.tsx` | Network flow visualization (used from /flow) |
| `web/app/overview/components/provider-flow-panel.tsx` | Provider flow SVG panel |
| `web/app/overview/components/live-feed.tsx` | Real-time SSE event feed |
