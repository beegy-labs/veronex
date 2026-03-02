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

## Key Docs by Area

**Backend** (Rust) → `docs/llm/backend/`
- Inference + queue: `jobs.md`, `openai.md`, `backends-ollama.md`, `backends-gemini.md`
- Auth + security: `auth.md`, `api_keys.md`, `security.md`
- Infra + deploy: `infrastructure.md` (env, ports, migrations, Helm), `infrastructure-otel.md`
- Capacity + routing: `capacity.md`, `backends-ollama-models.md`, `backends-gemini-models.md`
- Data + pricing: `jobs-analytics.md`, `model-pricing.md`, `lab_features.md`, `hardware.md`

**Frontend** (Next.js) → `docs/llm/frontend/`
- Design system + brand: `web.md` (tokens, i18n, nav, theme)
- Pages: `web-servers.md`, `web-providers.md`, `web-jobs.md`, `web-usage.md`,
  `web-performance.md`, `web-keys.md`, `web-test.md`, `web-charts.md`,
  `web-accounts.md`, `web-audit.md`, `web-setup.md`

**Research** (2026 best practices) → `docs/llm/research/`
