# Web — Flow Page (/flow)

> SSOT | Tier 2 | **Last Updated**: 2026-03-04

## Task Guide

| Task | Files |
|------|-------|
| Add flow visualization element | `web/app/flow/page.tsx`, `web/app/overview/components/provider-flow-panel.tsx` |
| Modify network topology | `web/app/overview/components/provider-flow-panel.tsx` |
| Change live feed display | `web/app/overview/components/live-feed.tsx` |
| Adjust queue depth poll interval | `web/lib/queries/dashboard.ts` `queueDepthQuery` |

## Overview

Standalone page for the network flow visualization. Displays real-time inference job routing across providers using an ArgoCD-style topology diagram and a live event feed. Reuses `NetworkFlowTab` from the overview page.

## Key Files

| File | Purpose |
|------|---------|
| `web/app/flow/page.tsx` | Page component — fetches providers, renders `NetworkFlowTab` |
| `web/app/overview/components/network-flow-tab.tsx` | Flow tab: wires SSE stream + queue depth into sub-components |
| `web/app/overview/components/provider-flow-panel.tsx` | ArgoCD-style topology (API → Queue → Providers) with animated bees |
| `web/app/overview/components/live-feed.tsx` | Scrollable event log of recent inference jobs |
| `web/hooks/use-inference-stream.ts` | SSE hook: `GET /v1/dashboard/jobs/stream` → `FlowEvent[]` |
| `web/lib/queries/providers.ts` | `providersQuery` (30s poll) |
| `web/lib/queries/dashboard.ts` | `queueDepthQuery` (3s poll) |

## API Dependencies

| Method | Path | Auth | Purpose |
|--------|------|------|---------|
| `GET` | `/v1/providers` | JWT | Provider list for topology nodes |
| `GET` | `/v1/dashboard/jobs/stream` | JWT (SSE) | Real-time job status events |
| `GET` | `/v1/dashboard/queue/depth` | JWT | Live queue depth counter |

## Flow Phases

The topology visualizes 3-phase bidirectional job flow:

| Phase | Direction | Trigger |
|-------|-----------|---------|
| `enqueue` | API → Queue | Job placed in Valkey queue |
| `dispatch` | Queue → Provider | Job dequeued, sent to provider |
| `response` | Provider → API | Inference complete (bypasses queue) |

## Notes

- Reuses `NetworkFlowTab` from overview page — changes there affect both pages
- SSE-based real-time updates via `useInferenceStream` hook
- Provider flow panel uses CSS `offset-path` + `@keyframes` for GPU-composited animations
- Panel scales via `ResizeObserver` with `transform:scale`; max-width 680px cap
- i18n: `nav.flow` (page title), `overview.networkFlowDesc` (subtitle)
