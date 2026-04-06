# Health Page

> SSOT | **Last Updated**: 2026-04-06
> Route: `/health` (Monitor group sub-item)

## Purpose

Infrastructure service health + pod status + Kafka pipeline monitoring. Shows real-time status of core services (PostgreSQL, Valkey, ClickHouse, S3, Vespa), API/Agent pod instances, and per-topic pipeline stats.

## Data Sources

| Query | Endpoint | Refresh |
|-------|----------|---------|
| `serviceHealthQuery` | `GET /v1/dashboard/services` | 30s (FAST) |
| `pipelineHealthQuery` | `GET /v1/dashboard/pipeline` | 30s (FAST) |

## Response Shape

```typescript
interface ServiceHealthResponse {
  infrastructure: ServiceStatus[]   // PG, Valkey, ClickHouse, S3, Vespa
  api_pods: PodItem[]               // per-instance from veronex:instances SET
  agent_pods: PodItem[]             // from agent heartbeat keys
}

interface PipelineHealthResponse {
  available: boolean                // false when ClickHouse unavailable
  topics: TopicPipelineStats[]
}

interface TopicPipelineStats {
  topic: string
  lag: number
  consumer_count: number
  tpm_1m: number
  tpm_5m: number
  last_poll_secs: number | null
  last_error: string | null
  consumer_offset: number
  log_end_offset: number
  is_active: boolean
}
```

## Layout (4 Sections)

### Section 1: Infrastructure Services (flat table)
Columns: status dot | icon | service name | status text | latency (ms) | time ago + stale warning.
Services: PostgreSQL, Valkey, ClickHouse, S3/MinIO, Vespa. Icons: Database, Server, Activity, HardDrive, Search.

### Section 2: Pods (combined, collapsible)
Single section with two collapsible rows — API Pods and Agent Pods.
Each row: expand button showing online/total count → expands to per-pod table.
Pod table columns: status dot | container icon + truncated pod ID (font-mono) | heartbeat age.

### Section 3: Pipeline (Kafka topic table)
Columns: status dot | topic (font-mono) + offset range | consumers | lag | tpm_1m | tpm_5m | last poll.
Lag coloring: `lagColor(lag, is_active, last_poll_secs, hasError)` → error/warning/ok.
Status dot: error (last_error set) → warning (inactive or stale poll) → ok.
Error icon: `AlertTriangle` beside topic name when `last_error` is set (tooltip = error text).
Hidden when `!pipeline.available || topics.length === 0` → shows "pipelineUnavailable" message.

## Staleness Detection

- `checked_at` > 60s ago: amber `⚠` icon on service row
- Any stale service + not loading: "Monitor stale" badge in page header

## Key Files

| File | Purpose |
|------|---------|
| `web/app/health/page.tsx` | Page component |
| `web/lib/queries/dashboard.ts` | `serviceHealthQuery`, `pipelineHealthQuery` |
| `web/lib/types.ts` | `ServiceHealthResponse`, `PipelineHealthResponse`, `TopicPipelineStats`, `PodItem` |
| `web/lib/constants.ts` | `SERVICE_STATUS_DOT`, `PROVIDER_STATUS_DOT` |
| `infrastructure/outbound/health_checker.rs` | `check_and_store_services(vespa_url)` probe loop |
| `infrastructure/inbound/http/dashboard_handlers.rs` | `GET /v1/dashboard/services`, `GET /v1/dashboard/pipeline` |

## i18n Keys

`health.*`: title, description, stale, infrastructure, pods, apiPods, agentPods, noPods, podsOnline, lastSeen, pipeline, pipelineUnavailable, topic, consumers, lag, tpm1m, tpm5m, lastPoll, noData, ok, error
