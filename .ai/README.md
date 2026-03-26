# Veronex

> CDD Layer 1 — Entry Point (≤50 lines) | **Last Updated**: 2026-03-15

## Project

**Veronex** (Vero+Nexus) — Autonomous intelligence scheduler/gateway for N Ollama
servers. Integrates request routing + capacity learning + thermal protection to
maximize cluster-wide throughput. OpenAI-compatible API + Next.js admin dashboard.

Scale target: 10,000+ providers, tens of thousands req/s.
All architecture decisions must consider this scale (O(1) counters, no DB polling hot paths).

Three Rust crates:
- `veronex` — main API server + scheduler (`crates/veronex/`)
- `veronex-analytics` — internal analytics service (`crates/veronex-analytics/`, port 3003)
- `veronex-agent` — metrics collector, scrapes node-exporter + Ollama, OTLP push (`crates/veronex-agent/`)

## Navigation

| Action | Read |
|--------|------|
| Core rules | `.ai/rules.md` |
| Architecture | `.ai/architecture.md` |
| Security | `.ai/security.md` |
| Code patterns (2026) | `docs/llm/policies/patterns.md` |
| Testing strategy | `docs/llm/policies/testing-strategy.md` |
| Git & commits | `.ai/git-flow.md` |
| Full docs index | `docs/llm/README.md` |

## Key Docs by Domain

| Domain | Path | Content |
|--------|------|---------|
| Auth | `docs/llm/auth/` | jwt-sessions (+impl), api-keys, security |
| Inference | `docs/llm/inference/` | job-lifecycle, job-api, session-grouping, job-analytics, openai-compat, capacity, model-pricing, lab-features, mcp |
| Providers | `docs/llm/providers/` | ollama (+impl), ollama-models, gemini, gemini-models, hardware |
| Infra | `docs/llm/infra/` | deploy, otel-pipeline (+ops), build-optimization |
| Frontend | `docs/llm/frontend/` | design-system (core, i18n, components), charts, pages/* |
| **Logic Flows** | `docs/llm/flows/` | **algorithm reference — read before implementing any subsystem logic** |
| Research | `docs/llm/research/` | 2026 best practices (frontend, backend, infra, security) |
