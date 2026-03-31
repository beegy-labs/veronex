# Health Page

> SSOT | **Last Updated**: 2026-03-29
> Route: `/health` (Monitor group sub-item)

## Purpose

Infrastructure service health + HPA pod status monitoring. Shows real-time status of core services (PostgreSQL, Valkey, ClickHouse, S3) and all API/Agent pod instances with staleness detection.

## Data Sources

| Query | Endpoint | Refresh |
|-------|----------|---------|
| `serviceHealthQuery` | `GET /v1/dashboard/services` | 30s (FAST) |

## Response Shape

```typescript
interface ServiceHealthResponse {
  infrastructure: ServiceStatus[]  // PG, Valkey, ClickHouse, S3
  api_pods: PodStatus[]            // per-instance from veronex:instances SET
  agent_pods: PodStatus[]          // derived from provider heartbeat sharding
}
```

## Layout (3 Sections)

### Section 1: Infrastructure Services (2x2 grid)
Each card: status dot + icon + service name + latency (ms) + staleness ("Ns ago").
Status merges all pod perspectives: any "ok" = ok, mixed = degraded, all error = unavailable.

### Section 2: API Pods (horizontal grid)
Per-pod card: status dot + truncated instance_id (font-mono) + heartbeat age.
Online/offline derived from `veronex:heartbeat:{id}` TTL.

### Section 3: Agent Pods (horizontal grid)
Per-shard card: `veronex-agent-{ordinal}` + online/offline.
Derived from provider heartbeat keys grouped by `hash(provider_id) % AGENT_REPLICAS`.

## Staleness Detection

- `checked_at` > 60s ago: amber warning icon on service card
- Any stale service: "Monitor stale" badge in page header

## HPA Considerations

- Each API pod writes to its own Valkey HASH (`veronex:svc:health:{instance_id}`) - no write conflicts
- Dead pod's HASH expires via 60s TTL - auto cleanup
- Agent pods are stateless (StatefulSet ordinals) - health inferred from provider heartbeat grouping
- `AGENT_REPLICAS` env controls expected agent pod count (default 1)

## Key Files

| File | Purpose |
|------|---------|
| `web/app/health/page.tsx` | Page component |
| `web/lib/queries/dashboard.ts` | `serviceHealthQuery` |
| `web/lib/types.ts` | `ServiceHealthResponse`, `ServiceStatus`, `PodStatus` |
| `web/lib/constants.ts` | `SERVICE_STATUS_DOT`, `SERVICE_STATUS_TEXT` |
| `web/components/nav.tsx` | Nav entry (Monitor group) |
