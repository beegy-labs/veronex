# docs/llm — SSOT Index

> Tier 2 CDD documents (LLM-facing, editable) | **Last Updated**: 2026-03-03

## Policies (Cross-Cutting)

| Document | Path | Keywords |
|----------|------|---------|
| Architecture | `policies/architecture.md` | hexagonal, ports, adapters, layers, AppState, dependency rule |
| Code Patterns | `policies/patterns.md` | AppError, thiserror, sqlx query_as!, async-trait, tracing, useOptimistic, Zod, Tailwind v4 |
| Git Flow | `policies/git-flow.md` | branch, commit, squash, merge, conventional |
| Terminology | `policies/terminology.md` | provider, provider_type, naming conventions |

---

## Auth (`auth/`)

| Document | Path | Keywords |
|----------|------|---------|
| JWT & Sessions | `auth/jwt-sessions.md` | JWT HS256, accounts, sessions, RBAC, setup flow, audit trail, password reset |
| API Keys | `auth/api-keys.md` | ApiKey, BLAKE2b, UUIDv7 id, non-unique name, auth flow, RPM, TPM, rate_limiter.rs |
| Security | `auth/security.md` | CORS, circuit breaker, rate limiting, Argon2id, BLAKE2b, AES-256-GCM, security headers |

---

## Inference (`inference/`)

| Document | Path | Keywords |
|----------|------|---------|
| Job Lifecycle | `inference/job-lifecycle.md` | InferenceJob, queue, BLPOP, latency, TTFT, JobSummary, session grouping, queue depth |
| Job Analytics | `inference/job-analytics.md` | StreamToken, usageMetadata, ClickHouse, inference_logs, run_job |
| OpenAI Compat | `inference/openai-compat.md` | /v1/chat/completions, SSE, backend field, curl, Python SDK |
| Capacity | `inference/capacity.md` | ConcurrencySlotMap, ThermalThrottleMap, capacity analyzer, model_capacity |
| Model Pricing | `inference/model-pricing.md` | model_pricing table, estimated_cost_usd, LATERAL join, Ollama $0.00, provider wildcard |
| Lab Features | `inference/lab-features.md` | gemini_function_calling, LabSettingsProvider, LabSettingsRepository, feature gating |

---

## Providers (`providers/`)

| Document | Path | Keywords |
|----------|------|---------|
| Ollama | `providers/ollama.md` | LlmBackend, VRAM routing, DynamicBackendRouter, health_checker |
| Ollama Models | `providers/ollama-models.md` | ollama_models, ollama_sync_jobs, OllamaModelRepository, model-aware routing |
| Gemini | `providers/gemini.md` | GeminiRateLimitPolicy, RPM, RPD, pick_gemini_backend, tier routing |
| Gemini Models | `providers/gemini-models.md` | gemini_sync_config, gemini_models, provider_selected_models, UPSERT |
| Hardware | `providers/hardware.md` | GpuServer, node-exporter, hw_metrics, AMD APU, ClickHouse history |

---

## Infrastructure (`infra/`)

| Document | Path | Keywords |
|----------|------|---------|
| Deploy | `infra/deploy.md` | docker-compose, Helm, Kubernetes, ports, env vars, CORS, Valkey keys, DB migrations |
| OTel Pipeline | `infra/otel-pipeline.md` | OTel Collector, Redpanda, ClickHouse Kafka Engine, data retention |

---

## Frontend (`frontend/`) — Next.js Web UI

| Document | Path | Keywords |
|----------|------|---------|
| Design System | `frontend/design-system.md` | brand, tokens.css, Tailwind v4, i18n, nav sidebar, theme, LabSettingsProvider |
| Chart System | `frontend/charts.md` | chart-theme.ts SSOT, DonutChart, Recharts constants, tooltip fix |

### Pages (`frontend/pages/`)

| Document | Path | Keywords |
|----------|------|---------|
| Servers | `frontend/pages/servers.md` | /servers, ServersTable, ServerMetricsCell, ServerHistoryModal |
| Providers | `frontend/pages/providers.md` | /providers, OllamaTab, GeminiTab, ConcurrencyControl, ModelSelectionModal |
| Jobs | `frontend/pages/jobs.md` | job-table, detail modal, GroupSessionsPanel, NetworkFlowTab |
| Usage | `frontend/pages/usage.md` | UsagePage, 4-tab layout, usageBreakdownQuery, per-key hourly |
| Performance | `frontend/pages/performance.md` | PerformancePage, P50/P95/P99, model latency, TPS trend |
| Keys | `frontend/pages/keys.md` | keys/page.tsx, CreateKeyModal, toggle, soft-delete, KeyUsageModal |
| Accounts | `frontend/pages/accounts.md` | /accounts, user CRUD, role assignment, AccountSessionsModal |
| Audit | `frontend/pages/audit.md` | /audit, super role, AuditTable, action filter |
| API Test | `frontend/pages/api-test.md` | api-test, SSE parsing, /docs/swagger, /docs/redoc |
| Setup | `frontend/pages/setup.md` | /setup, bootstrap flow, first-run, admin account creation |

---

## Research (`research/`)

| Document | Path | Status |
|----------|------|--------|
| Index | `research/index.md` | -- |
| CSS Animations | `research/frontend/css-animations.md` | verified |
| React Patterns | `research/frontend/react.md` | verified |
| Data Fetching | `research/frontend/data-fetching.md` | verified |
| Next.js 16 | `research/frontend/nextjs.md` | verified |
| Tailwind v4 | `research/frontend/tailwind.md` | verified |
| TanStack Query | `research/frontend/tanstack-query.md` | verified |
| Rust / Axum | `research/backend/rust-axum.md` | verified |
| API Design | `research/backend/api-design.md` | verified |
| Observability | `research/infrastructure/observability.md` | verified |
| Database | `research/infrastructure/database.md` | verified |
| Auth Sessions | `research/security/auth.md` | verified |

---

## Quick Task Reference

| Task | Read |
|------|------|
| Add new API endpoint | `policies/patterns.md` + relevant domain doc |
| Add new Port + Adapter | `policies/patterns.md` + `policies/architecture.md` |
| Error handling | `policies/patterns.md` (AppError pattern) |
| Modify job tracking | `inference/job-lifecycle.md` + `inference/job-analytics.md` |
| Model pricing | `inference/model-pricing.md` |
| Gemini rate limits | `providers/gemini.md` |
| Ollama model sync | `providers/ollama-models.md` |
| Gemini model sync | `providers/gemini-models.md` |
| Add GPU server | `providers/hardware.md` |
| Auth / JWT / session | `auth/jwt-sessions.md` + `research/security/auth.md` |
| Account RBAC | `auth/jwt-sessions.md` + `frontend/pages/accounts.md` |
| API keys / rate limiting | `auth/api-keys.md` + `frontend/pages/keys.md` |
| Security (CORS, crypto) | `auth/security.md` |
| Dynamic concurrency | `inference/capacity.md` |
| Lab feature flag | `inference/lab-features.md` |
| OTel pipeline | `infra/otel-pipeline.md` + `research/infrastructure/observability.md` |
| Kubernetes / Helm | `infra/deploy.md` |
| CORS config | `infra/deploy.md` (CORS_ALLOWED_ORIGINS) |
| Design token / theme | `frontend/design-system.md` + `policies/patterns.md` |
| Add i18n key | `frontend/design-system.md` + relevant `frontend/pages/*.md` |
| Chart / tooltip | `frontend/charts.md` |
| Modify servers UI | `frontend/pages/servers.md` |
| Modify providers UI | `frontend/pages/providers.md` |
| Modify jobs UI | `frontend/pages/jobs.md` |
| Usage page / tabs | `frontend/pages/usage.md` |
| Performance page | `frontend/pages/performance.md` |
| Setup wizard | `frontend/pages/setup.md` |
| Audit trail UI | `frontend/pages/audit.md` |
| Queue depth | `inference/job-lifecycle.md` (Queue Depth section) |
| Session grouping | `inference/job-lifecycle.md` + `frontend/pages/jobs.md` |
