# Veronex

> CDD Tier 1 — Entry Point (≤50 lines) | **Last Updated**: 2026-03-02

## Project

**Veronex** (Vero+Nexus) — Queue-based LLM inference gateway (Rust/Axum) with
OpenAI-compatible API, VRAM-aware multi-GPU routing, Gemini free/paid tier management,
and a Next.js admin dashboard. Two Rust crates:
- `veronex` — main API server (`crates/inferq/`)
- `veronex-analytics` — internal analytics service (`crates/veronex-analytics/`, port 3003)

## Navigation

| Action | Read |
|--------|------|
| Core rules | `.ai/rules.md` |
| Architecture | `.ai/architecture.md` |
| Code patterns (2026) | `docs/llm/policies/patterns.md` |
| Git & commits | `.ai/git-flow.md` |
| Full docs index | `docs/llm/README.md` |

## Quick Docs Reference

### Backend (Rust/Axum — `crates/inferq/`)

| Topic | Path |
|-------|------|
| OpenAI `/v1/chat/completions` | `docs/llm/backend/openai.md` |
| API Keys + rate limiting | `docs/llm/backend/api_keys.md` |
| Job lifecycle + queue + API | `docs/llm/backend/jobs.md` |
| Token observability + analytics | `docs/llm/backend/jobs-analytics.md` |
| Ollama backends + VRAM routing | `docs/llm/backend/backends-ollama.md` |
| Ollama global model sync + routing | `docs/llm/backend/backends-ollama-models.md` |
| Gemini rate limits + tier routing | `docs/llm/backend/backends-gemini.md` |
| Gemini model sync + selection | `docs/llm/backend/backends-gemini-models.md` |
| GPU servers + node-exporter | `docs/llm/backend/hardware.md` |
| RBAC + JWT + Audit trail | `docs/llm/backend/auth.md` |
| Dynamic concurrency + thermal throttle | `docs/llm/backend/capacity.md` |
| Lab feature flags | `docs/llm/backend/lab_features.md` |
| Services + env + ports + DB migrations | `docs/llm/backend/infrastructure.md` |
| OTel Logs pipeline + veronex-analytics | `docs/llm/backend/infrastructure-otel.md` |

### Frontend (Next.js — `web/`)

| Topic | Path |
|-------|------|
| Brand + design tokens + nav + i18n | `docs/llm/frontend/web.md` |
| Dashboard (/overview) + Network Flow (/flow) | `docs/llm/frontend/web.md` |
| Servers page (/servers) | `docs/llm/frontend/web-servers.md` |
| Providers page (/providers — Ollama + Gemini) | `docs/llm/frontend/web-providers.md` |
| Jobs page (/jobs) | `docs/llm/frontend/web-jobs.md` |
| Usage page (/usage) | `docs/llm/frontend/web-usage.md` |
| Performance page (/performance) | `docs/llm/frontend/web-performance.md` |
| API Keys page (/keys) | `docs/llm/frontend/web-keys.md` |
| API Test + API Docs pages | `docs/llm/frontend/web-test.md` |
| Accounts + Audit + Setup pages | `docs/llm/frontend/web-accounts.md`, `web-audit.md`, `web-setup.md` |
