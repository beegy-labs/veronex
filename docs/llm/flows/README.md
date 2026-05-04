# Veronex — Logic Flow Reference

> **Last Updated**: 2026-04-28
> Code-level flowcharts for all major subsystems.
> Each file documents one subsystem end-to-end with ASCII diagrams.

---

| File | Subsystem |
|------|-----------|
| [inference.md](inference.md) | Inference request lifecycle — submit → queue → dispatch → stream |
| [model-lifecycle.md](model-lifecycle.md) | Phase 1 (`ensure_ready`) ↔ Phase 2 (`stream_tokens`) SoD — `MCP_LIFECYCLE_PHASE` flag, `LoadInFlight` coalescing, state machine |
| [job-event-pipeline.md](job-event-pipeline.md) | Job event pipeline — overall architecture, state transitions, repo call mapping |
| [job-event-pipeline-steps.md](job-event-pipeline-steps.md) | Job event pipeline steps — submit, cancel, stream, run_job step diagrams |
| [auth.md](auth.md) | Authentication — API Key & JWT session flows |
| [mcp.md](mcp.md) | MCP agentic loop — ACL, tool injection, execute, loop detection |
| [scheduler.md](scheduler.md) | Provider selection — VRAM, thermal, circuit breaker, scoring |
| [thermal.md](thermal.md) | Thermal protection — state machine, drain, cooldown, ramp-up |
| [agent.md](agent.md) | veronex-agent scrape cycle — metrics, heartbeat, MCP health |
| [streaming.md](streaming.md) | Job event & stats streaming — SSE ring buffer, FlowStats ticker |
| [pubsub-relay.md](pubsub-relay.md) | Multi-instance pub/sub relay — Valkey pub/sub + Streams |
| [reaper.md](reaper.md) | Crash recovery & job reaping — heartbeat, Lua CAS, VRAM lease cleanup |
| [queue-maintenance.md](queue-maintenance.md) | Queue maintenance loops — promote, resync, wait-cancel |
| [placement-planner.md](placement-planner.md) | Model auto-scaling — placement planner, scale-out/in, preload/evict |
| [Context Compression](context-compression.md) | Multi-turn compression — eligibility gate, assembly, handoff, failure modes |
| — | **Service health** — infra probes + pod status → documented in `providers/hardware-impl.md` § Service Health Monitoring |
