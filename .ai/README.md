# Veronex

> CDD Tier 1 — Entry Point (≤50 lines) | **Last Updated**: 2026-03-03

## Project

**Veronex** (Vero+Nexus) — Queue-based LLM inference gateway (Rust/Axum) with
OpenAI-compatible API, VRAM-aware multi-GPU routing, Gemini free/paid tier management,
and a Next.js admin dashboard. Two Rust crates:
- `veronex` — main API server (`crates/veronex/`)
- `veronex-analytics` — internal analytics service (`crates/veronex-analytics/`, port 3003)

## Navigation

| Action | Read |
|--------|------|
| Core rules | `.ai/rules.md` |
| Architecture | `.ai/architecture.md` |
| Code patterns (2026) | `docs/llm/policies/patterns.md` |
| Git & commits | `.ai/git-flow.md` |
| Full docs index | `docs/llm/README.md` |

## Key Docs by Domain

| Domain | Path | Content |
|--------|------|---------|
| Auth | `docs/llm/auth/` | jwt-sessions, api-keys, security |
| Inference | `docs/llm/inference/` | job-lifecycle, job-analytics, openai-compat, capacity, model-pricing, lab-features |
| Providers | `docs/llm/providers/` | ollama, ollama-models, gemini, gemini-models, hardware |
| Infra | `docs/llm/infra/` | deploy, otel-pipeline |
| Frontend | `docs/llm/frontend/` | design-system, charts, pages/* |
| Research | `docs/llm/research/` | 2026 best practices (frontend, backend, infra, security) |
