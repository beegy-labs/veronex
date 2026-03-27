# Veronex — Logic Flow Reference

> **Last Updated**: 2026-03-26
> Code-level flowcharts for all major subsystems.
> Each file documents one subsystem end-to-end with ASCII diagrams.

---

| File | Subsystem |
|------|-----------|
| [inference.md](inference.md) | Inference request lifecycle — submit → queue → dispatch → stream |
| [auth.md](auth.md) | Authentication — API Key & JWT session flows |
| [mcp.md](mcp.md) | MCP agentic loop — ACL, tool injection, execute, loop detection |
| [scheduler.md](scheduler.md) | Provider selection — VRAM, thermal, circuit breaker, scoring |
| [thermal.md](thermal.md) | Thermal protection — state machine, drain, cooldown, ramp-up |
| [agent.md](agent.md) | veronex-agent scrape cycle — metrics, heartbeat, MCP health |
