# inferq

> CDD Tier 1 — Entry Point (≤50 lines) | **Last Updated**: 2026-02-25

## Project

Queue-based LLM inference server (Rust/Axum) with real-time SSE streaming,
VRAM-aware multi-GPU routing, and hardware metrics dashboard.

## Navigation

| Action          | Read                                |
| --------------- | ----------------------------------- |
| Core rules      | `.ai/rules.md`                      |
| Architecture    | `.ai/architecture.md`               |
| Git & commits   | `.ai/git-flow.md`                   |
| All docs        | `docs/llm/README.md`                |
| Full policies   | `docs/llm/policies/`                |
| Dev protocol    | `vendor/agentic-dev-protocol/` (submodule: https://github.com/beegy-labs/agentic-dev-protocol) |

## Docs Index (Tier 2 — topic-based)

| Topic | Path |
|-------|------|
| Backends (Ollama/Gemini, routing, rate limit rolling) | `docs/llm/backends.md` |
| Hardware (GPU Server, node-exporter, metrics pipeline) | `docs/llm/hardware.md` |
| Jobs (lifecycle, token observability, ClickHouse) | `docs/llm/jobs.md` |
| Infrastructure (docker-compose, OTel, Helm, ports) | `docs/llm/infrastructure.md` |
| Web (brand, design system, pages) | `docs/llm/web.md` |
