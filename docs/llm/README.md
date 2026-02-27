# docs/llm — SSOT Index

> Tier 2 CDD documents (LLM-facing, editable) | **Last Updated**: 2026-02-28 (rev: split backends → providers + servers; pg18; uuidv7)

## Policies

| Document | Path | Keywords |
|----------|------|---------|
| Architecture | `policies/architecture.md` | hexagonal, ports, adapters, layers, AppState, dependency rule |
| Code Patterns | `policies/patterns.md` | AppError, thiserror, sqlx query_as!, async-trait, tracing, useOptimistic, Zod, Tailwind v4 |
| Git Flow | `policies/git-flow.md` | branch, commit, squash, merge, conventional |
| CDD | `policies/cdd.md` | doc structure, line limits, RAG, splits |

---

## Backend Docs (`backend/`) — Rust/Axum API

| Document | Path | Keywords |
|----------|------|---------|
| OpenAI API | `backend/openai.md` | /v1/chat/completions, SSE, backend field, curl, Python SDK |
| API Keys (backend) | `backend/api_keys.md` | ApiKey, BLAKE2b, auth flow, RPM, TPM, rate_limiter.rs |
| Jobs (lifecycle) | `backend/jobs.md` | InferenceJob, queue, BLPOP, latency, TTFT, JobSummary |
| Jobs (analytics) | `backend/jobs-analytics.md` | StreamToken, usageMetadata, ClickHouse, inference_logs, run_job |
| Ollama backends | `backend/backends-ollama.md` | LlmBackend, VRAM routing, DynamicBackendRouter, health_checker |
| Ollama model sync | `backend/backends-ollama-models.md` | ollama_models, ollama_sync_jobs, OllamaModelRepository, model-aware routing |
| Gemini rate limits | `backend/backends-gemini.md` | GeminiRateLimitPolicy, RPM, RPD, pick_gemini_backend, tier routing |
| Gemini model sync | `backend/backends-gemini-models.md` | gemini_sync_config, gemini_models, backend_selected_models, UPSERT |
| Hardware | `backend/hardware.md` | GpuServer, node-exporter, hw_metrics, AMD APU, ClickHouse history |
| Infrastructure | `backend/infrastructure.md` | docker-compose, ports, env vars, Valkey keys, DB migrations |
| OTel pipeline | `backend/infrastructure-otel.md` | OTel Collector, Redpanda, ClickHouse Kafka Engine, Helm |

---

## Frontend Docs (`frontend/`) — Next.js Web UI

| Document | Path | Keywords |
|----------|------|---------|
| Design system | `frontend/web.md` | brand, tokens.css, Tailwind v4, i18n, nav sidebar, theme |
| Servers page | `frontend/web-servers.md` | /servers, ServersTable, ServerMetricsCell, ServerHistoryModal, RegisterServerModal, EditServerModal |
| Providers page | `frontend/web-providers.md` | /providers, OllamaTab, OllamaSyncSection, GeminiTab, GeminiStatusSyncSection, GeminiSyncSection, ModelSelectionModal |
| Jobs/Usage/Perf | `frontend/web-jobs.md` | job-table, detail modal, formatDuration, usage charts, performance P50/P99 |
| API Keys page | `frontend/web-keys.md` | keys/page.tsx, CreateKeyModal, toggle, soft-delete |
| Test + API Docs | `frontend/web-test.md` | api-test, SSE parsing, /docs/swagger, /docs/redoc |

---

## Design System Spec

| Document | Path |
|----------|------|
| Design CI/BI | `specs/20-design-ci-bi.md` |

---

## Quick Task Reference

| Task | Read |
|------|------|
| Add new API endpoint | `policies/patterns.md` (handler template) + relevant `backend/*.md` |
| Add new Port + Adapter | `policies/patterns.md` (Port+Adapter order) + `policies/architecture.md` |
| Error handling | `policies/patterns.md` (AppError pattern) |
| Modify job tracking | `backend/jobs.md` + `backend/jobs-analytics.md` |
| Gemini rate limits | `backend/backends-gemini.md` |
| Ollama model sync / routing | `backend/backends-ollama-models.md` |
| Gemini model sync | `backend/backends-gemini-models.md` |
| Add GPU server | `backend/hardware.md` |
| Modify servers UI | `frontend/web-servers.md` + `policies/patterns.md` (React 19 / TQ v5) |
| Modify providers UI | `frontend/web-providers.md` + `policies/patterns.md` (React 19 / TQ v5) |
| Add i18n key | `frontend/web.md` (procedure) + relevant `frontend/web-*.md` |
| Change design token | `frontend/web.md` (token flow) + `policies/patterns.md` (Tailwind v4 rules) |
| Add Zod schema | `policies/patterns.md` (TypeScript + Zod section) |
