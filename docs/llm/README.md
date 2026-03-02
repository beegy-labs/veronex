# docs/llm — SSOT Index

> Tier 2 CDD documents (LLM-facing, editable) | **Last Updated**: 2026-03-02 (rev: dep upgrades — fred 10, reqwest 0.13, Next.js 16, recharts 3, Valkey 9, Redpanda v25.3, OTel 0.146; migrations 41-45)

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
| API Keys (backend) | `backend/api_keys.md` | ApiKey, BLAKE2b, UUIDv7 id, non-unique name, auth flow, RPM, TPM, rate_limiter.rs |
| Jobs (lifecycle) | `backend/jobs.md` | InferenceJob, queue, BLPOP, latency, TTFT, JobSummary |
| Jobs (analytics) | `backend/jobs-analytics.md` | StreamToken, usageMetadata, ClickHouse, inference_logs, run_job |
| Ollama backends | `backend/backends-ollama.md` | LlmBackend, VRAM routing, DynamicBackendRouter, health_checker |
| Ollama model sync | `backend/backends-ollama-models.md` | ollama_models, ollama_sync_jobs, OllamaModelRepository, model-aware routing |
| Gemini rate limits | `backend/backends-gemini.md` | GeminiRateLimitPolicy, RPM, RPD, pick_gemini_backend, tier routing |
| Gemini model sync | `backend/backends-gemini-models.md` | gemini_sync_config, gemini_models, backend_selected_models, UPSERT |
| Hardware | `backend/hardware.md` | GpuServer, node-exporter, hw_metrics, AMD APU, ClickHouse history |
| Infrastructure | `backend/infrastructure.md` | docker-compose, ports, env vars, Valkey keys, DB migrations |
| OTel pipeline | `backend/infrastructure-otel.md` | OTel Collector, Redpanda, ClickHouse Kafka Engine, Helm |
| Authentication | `backend/auth.md` | JWT HS256, accounts, sessions, RBAC, setup flow, audit trail, password reset |
| Capacity Control | `backend/capacity.md` | ConcurrencySlotMap, ThermalThrottleMap, capacity analyzer, model_capacity, qwen2.5:3b |

---

## Frontend Docs (`frontend/`) — Next.js Web UI

| Document | Path | Keywords |
|----------|------|---------|
| Design system | `frontend/web.md` | brand, tokens.css, Tailwind v4, i18n, nav sidebar, theme |
| Servers page | `frontend/web-servers.md` | /servers, ServersTable, ServerMetricsCell, ServerHistoryModal, RegisterServerModal, EditServerModal |
| Providers page | `frontend/web-providers.md` | /providers, OllamaTab, OllamaSyncSection, OllamaCapacitySection, GeminiTab, GeminiStatusSyncSection, GeminiSyncSection, ModelSelectionModal, ConcurrencyControl |
| Jobs/Usage/Perf | `frontend/web-jobs.md` | job-table, detail modal, formatDuration, usage charts, performance P50/P99 |
| API Keys page | `frontend/web-keys.md` | keys/page.tsx, CreateKeyModal, toggle, soft-delete |
| Test + API Docs | `frontend/web-test.md` | api-test, SSE parsing, /docs/swagger, /docs/redoc |
| Chart System    | `frontend/web-charts.md` | chart-theme.ts SSOT, DonutChart, Recharts constants, tooltip fix |

---

## Research — 2026 Best Practices (`research/`)

> Web-searched + implementation-verified findings. Status: ✅ verified | 🔬 research-only | 📋 to-research

| Document | Path | Topics | Status |
|----------|------|--------|--------|
| Index | `research/index.md` | Master index, quick reference | — |
| CSS Animations | `research/frontend/css-animations.md` | CSS Motion Path, offset-path vs SMIL, particle systems | ✅ |
| React Patterns | `research/frontend/react.md` | useReducer, ResizeObserver, onAnimationEnd, useMemo rules | ✅ |
| Data Fetching | `research/frontend/data-fetching.md` | TanStack Query v5, polling, background refetch | ✅ |
| Next.js 16 | `research/frontend/nextjs.md` | App Router, 'use client' rationale, Server Actions, PPR, Suspense | ✅ |
| Tailwind v4 | `research/frontend/tailwind.md` | CSS-first config, 4-layer tokens, @utility, container queries | ✅ |
| TanStack Query | `research/frontend/tanstack-query.md` | queryOptions factory, lib/queries/ SSOT, invalidation, optimistic updates | ✅ |
| Rust / Axum | `research/backend/rust-axum.md` | Axum 0.8 breaking changes, AppState, SSE, sqlx | ✅ |
| API Design | `research/backend/api-design.md` | URL conventions, OpenAPI 3.1, rate limit headers, pagination strategy | ✅ |
| Observability | `research/infrastructure/observability.md` | OTel pipeline, Redpanda, ClickHouse Kafka Engine | ✅ |
| Database | `research/infrastructure/database.md` | PG18 uuidv7, sqlx, ClickHouse query patterns | ✅ |
| Auth & Sessions | `research/security/auth.md` | JWT jti, rolling refresh, BLAKE2b, Valkey revocation | ✅ |

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
| Add / modify a chart | `frontend/web-charts.md` (SSOT constants + DonutChart props) |
| Fix chart tooltip text color | `frontend/web-charts.md` (labelStyle / itemStyle requirement) |
| Implement animation / particles | `research/frontend/css-animations.md` |
| Choose polling strategy | `research/frontend/data-fetching.md` |
| TanStack Query queryOptions / invalidation | `research/frontend/tanstack-query.md` |
| Tailwind token / custom utility | `research/frontend/tailwind.md` |
| Next.js page architecture decision | `research/frontend/nextjs.md` |
| API endpoint design / OpenAPI 3.1 | `research/backend/api-design.md` |
| Complex React state (reducers) | `research/frontend/react.md` |
| Background task / graceful shutdown | `research/backend/rust-axum.md` (Background Tasks section) + `policies/patterns.md` |
| Axum route / middleware | `research/backend/rust-axum.md` |
| OTel pipeline change | `research/infrastructure/observability.md` |
| Auth / JWT / session | `research/security/auth.md` + `backend/auth.md` |
| Account management (RBAC) | `backend/auth.md` (account + session endpoints) |
| Submit test inference (JWT, no API key) | `backend/auth.md` (Test Run Endpoints section) |
| Add new test route format | `test_handlers.rs` + `router.rs` `build_test_router()` |
| Dynamic concurrency / thermal throttle | `backend/capacity.md` |
| Capacity analysis / slot recommendation | `backend/capacity.md` |
| Modify capacity control web UI | `frontend/web-providers.md` (OllamaCapacitySection) + `backend/capacity.md` |
| Network flow SSE stream | `backend/jobs.md` (Real-Time Job Status Stream section) + `frontend/web.md` (useInferenceStream) |
| Network flow visualization | `frontend/web.md` (Network Flow Page — Detail; ArgoCD-style ProviderFlowPanel topology) |
| Queue depth (waiting jobs) | `backend/jobs.md` (Queue Depth section) — `GET /v1/dashboard/queue/depth`; 3-key LLEN; `queueDepthQuery` polls 3 s |
