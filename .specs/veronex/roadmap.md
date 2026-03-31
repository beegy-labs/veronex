# Roadmap — Veronex

> L1: Master direction | Load on planning only | **Last Updated**: 2026-03-22

## 2026

| Q | Priority | Feature | Change Type | CDD Reference | Status |
|---|----------|---------|-------------|---------------|--------|
| Q1 | P0 | Intelligence Scheduler | Add | inference/capacity.md | Done |
| Q2 | P0 | 10K Scale-Out (pagination, VRAM, cluster) | Improve | inference/capacity.md | Done |
| Q2 | P1 | Global Model Settings + Key Access Control | Add | auth/api-keys.md | Done |
| Q2 | P1 | MCP Integration (native McpBridgeAdapter) | Add | inference/mcp.md | Planned |
| Q2 | P2 | NVIDIA GPU support | Add | providers/hardware.md | Pending |
| Q3 | P1 | Multi-server Scale-Out (distributed) | Improve | infra/distributed.md | Pending |
| Q3 | P2 | Agent OTLP push enhancements | Improve | infra/otel-pipeline.md | Pending |

## Dependencies

| From | To | Reason |
|------|----|--------|
| Intelligence Scheduler | 10K Scale-Out | Expand after single-server validation |
| Intelligence Scheduler | NVIDIA support | Thermal profile extension |
| API key capabilities | MCP Integration | Cap system gates MCP access |
| MCP Integration | Multi-server Scale-Out | MCP tool cache must survive replica scale |
