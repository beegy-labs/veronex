# docs/llm — SSOT Index

> CDD Layer 2 — SSOT (LLM-facing, editable) | **Last Updated**: 2026-03-28

## Policies (Cross-Cutting)

| Document | Path | Keywords |
|----------|------|---------|
| Identity Anchor | `policies/identity.md` | CDD, SDD, ADD fixed definitions, core loop |
| CDD Policy | `policies/cdd.md` | layers, reconstructability, constitutional, operational |
| SDD Policy | `policies/sdd.md` | change plan, scope, impact, completion criteria |
| ADD Policy | `policies/add.md` | work type, policy selection, autonomous execution |
| Architecture | `policies/architecture.md` | hexagonal, ports, adapters, layers, AppState, dependency rule |
| Code Patterns (Rust) | `policies/patterns.md` | AppError, thiserror, sqlx query_as!, async-trait, tracing, DashMap, Valkey Lua |
| ID & API Key Policy | `policies/id-policy.md` | base62 public IDs, prefix registry, API key hash flow, vnx_live/test |
| Code Patterns (Frontend) | `policies/patterns-frontend.md` | TanStack Query v5, useOptimistic, Zod, Tailwind v4 |
| Git Flow | `policies/git-flow.md` | branch, commit, squash, merge, conventional |
| Testing Strategy | `policies/testing-strategy.md` | testing trophy, purity, proptest, cargo-mutants, OpenAPI schema, layer responsibility |
| Terminology | `policies/terminology.md` | provider, provider_type, naming conventions |
| Token Optimization | `policies/token-optimization.md` | format rules, no emoji, tables over prose |
| Development Methodology | `policies/development-methodology.md` | AI-native, methodology overview |
| Monorepo | `policies/monorepo.md` | project structure, backend/frontend layout |
| Agents Customization | `policies/agents-customization.md` | AGENTS.md guide, LLM config |
| Agents Customization Sync | `policies/agents-customization-sync.md` | sync behavior, migration guide, best practices, validation |

---

## Auth (`auth/`)

| Document | Path | Keywords |
|----------|------|---------|
| JWT & Sessions | `auth/jwt-sessions.md` | JWT HS256, accounts, sessions, RBAC, setup flow, password reset |
| JWT Impl | `auth/jwt-sessions-impl.md` | test runs, account endpoints, audit trail, web frontend token storage |
| JWT Endpoints | `auth/jwt-sessions-endpoints.md` | auth endpoints, test run endpoints, account endpoints, env vars |
| API Keys | `auth/api-keys.md` | ApiKey, BLAKE2b, UUIDv7 id, non-unique name, RPM, TPM |
| API Keys Impl | `auth/api-keys-impl.md` | auth flow, rate limiting, soft-delete, audit trail, provider access, web UI |
| Security | `auth/security.md` | CORS, circuit breaker, rate limiting, Argon2id, BLAKE2b, security headers, SSRF |

---

## Inference (`inference/`)

| Document | Path | Keywords |
|----------|------|---------|
| Job Lifecycle | `inference/job-lifecycle.md` | InferenceJob, queue, BLPOP, latency, TTFT, cancellation, DashMap, JobEntry |
| Job API | `inference/job-api.md` | JobSummary, JobDetail, dashboard endpoints, queue depth, SSE stream, JobStatusEvent |
| Session Grouping | `inference/session-grouping.md` | conversation_id, messages_hash, training data, batch auto-inference, Blake2b |
| Job Analytics | `inference/job-analytics.md` | StreamToken, usageMetadata, ClickHouse, inference_logs, run_job |
| OpenAI Compat | `inference/openai-compat.md` | /v1/chat/completions, SSE, provider_type field, curl, Python SDK |
| OpenAI Compat Native | `inference/openai-compat-native.md` | native endpoints, API doc endpoints, shared constants, SSE parsing, client examples |
| Capacity | `inference/capacity.md` | VramPool, AIMD+p95, LLM Batch±2, thermal auto-detect, model stickiness, gate chain |
| Model Pricing | `inference/model-pricing.md` | model_pricing table, estimated_cost_usd, LATERAL join, Ollama $0.00, provider wildcard |
| Lab Features | `inference/lab-features.md` | gemini_function_calling, mcp_orchestrator_model, LabSettingsProvider, LabSettingsRepository, feature gating |
| MCP | `inference/mcp.md` | McpBridgeAdapter, run_loop, tool intercept, orchestrator model, mcp_servers, tool naming, concurrency |

---

## Providers (`providers/`)

| Document | Path | Keywords |
|----------|------|---------|
| Ollama | `providers/ollama.md` | LlmProvider, VRAM routing, DynamicProviderRouter, health_checker |
| Ollama Allocation | `providers/ollama-allocation.md` | end-to-end automatic allocation flow, scheduling logic |
| Ollama Implementation | `providers/ollama-impl.md` | OllamaAdapter, streaming protocol, num_ctx, format conversion |
| Ollama Models | `providers/ollama-models.md` | ollama_models, ollama_sync_jobs, OllamaModelRepository, model-aware routing |
| Gemini | `providers/gemini.md` | GeminiRateLimitPolicy, RPM, RPD, pick_gemini_provider, tier routing |
| Gemini Models | `providers/gemini-models.md` | gemini_sync_config, gemini_models, provider_selected_models, UPSERT |
| Hardware | `providers/hardware.md` | GpuServer, node-exporter, hw_metrics, AMD APU, gpu_vendor detection, thermal thresholds, service health monitoring |
| Hardware Metrics | `providers/hardware-metrics.md` | history buckets, ClickHouse history query, thermal state machine, web UI |

---

## Infrastructure (`infra/`)

| Document | Path | Keywords |
|----------|------|---------|
| Deploy | `infra/deploy.md` | docker-compose, services, env vars, CORS, Valkey keys, DB migrations, UUID policy |
| Deploy Helm | `infra/deploy-helm.md` | Helm, Kubernetes, KEDA, HPA, autoscaling, AppState wiring |
| OTel Pipeline | `infra/otel-pipeline.md` | OTel Collector, Redpanda, ClickHouse Kafka Engine, pipeline overview, agent policy |
| OTel Pipeline Chains | `infra/otel-pipeline-chains.md` | collector config yaml, Chain 1 otel-logs MV, derived MVs, PG fallback |
| OTel Pipeline Ops | `infra/otel-pipeline-ops.md` | Chains 2-3, gotchas, verification, data retention, Rust adapters, Redpanda, GPU server |
| Distributed Coordination | `infra/distributed.md` | Instance ID, VRAM leases, reliable queue, ZSET, model filter, stickiness |
| Distributed Ops | `infra/distributed-ops.md` | cross-instance pub/sub, TPM accounting, crash recovery, Valkey key registry, wiring |
| Build Optimization | `infra/build-optimization.md` | mold, cargo-chef, hakari, nextest, Docker cache mounts, cargo profiles |
| Crate Structure | `infra/crate-structure.md` | workspace members, dependency rules, veronex, veronex-agent, veronex-analytics, veronex-mcp, veronex-embed |
| Hot-Path Caching | `infra/hot-path-caching.md` | TtlCache wrappers, Valkey ACL cache, inference hot-path SQL audit, long-term roadmap |
| Job Event Pipeline | `infra/job-event-pipeline.md` | KafkaJobRepository, JobEvent, veronex.job.events, bulk unnest UPDATE, at-least-once |

---

## Frontend (`frontend/`) — Next.js Web UI

| Document | Path | Keywords |
|----------|------|---------|
| Design System | `frontend/design-system.md` | brand, tokens.css, Tailwind v4, nav sidebar, theme, DataTable, state management |
| Design System i18n | `frontend/design-system-i18n.md` | i18n, locale config, timezone provider, date formatting, translation workflow |
| Design System Components | `frontend/design-system-components.md` | login page, auth guard, API client, status colors, auth-cookie session |
| Design System Component Patterns | `frontend/design-system-components-patterns.md` | provider taxonomy, network flow viz, accounts page, dialogs, hooks, 2-step registration |
| Chart System | `frontend/charts.md` | chart-theme.ts SSOT, DonutChart, Recharts constants, tooltip fix |

### Pages (`frontend/pages/`)

| Document | Path | Keywords |
|----------|------|---------|
| Overview | `frontend/pages/overview.md` | /overview, dashboard KPIs, thermal alert, power, latency, top models, recent jobs |
| Servers | `frontend/pages/servers.md` | /servers, ServersTable, ServerMetricsCell, ServerHistoryModal |
| Providers | `frontend/pages/providers.md` | /providers, OllamaTab, GeminiTab, routing, lab gating |
| Providers Impl | `frontend/pages/providers-impl.md` | OllamaServerMetrics, sync section, capacity settings |
| Providers Gemini | `frontend/pages/providers-gemini.md` | Gemini sync, rate limit table, EditPolicyModal, SetSyncKeyModal |
| Jobs | `frontend/pages/jobs.md` | job-table, GroupSessionsPanel, NetworkFlowTab, i18n |
| Jobs Impl | `frontend/pages/jobs-impl.md` | handleRetry, NetworkFlow SVG, detail modal, result branching |
| Jobs Types | `frontend/pages/jobs-types.md` | ToolCall, Job, ChatMessage, JobDetail, messages_json, S3, cost |
| Usage | `frontend/pages/usage.md` | UsagePage, 4-tab layout, usageBreakdownQuery, per-key hourly |
| Performance | `frontend/pages/performance.md` | PerformancePage, P50/P95/P99, model latency, TPS trend |
| Keys | `frontend/pages/keys.md` | keys/page.tsx, CreateKeyModal, toggle, soft-delete, KeyUsageModal |
| Accounts | `frontend/pages/accounts.md` | /accounts, user CRUD, role assignment, AccountSessionsModal |
| Audit | `frontend/pages/audit.md` | /audit, super role, AuditTable, action filter |
| API Test | `frontend/pages/api-test.md` | api-test, SSE parsing, /docs/swagger, /docs/redoc |
| Login | `frontend/pages/login.md` | /login, auth form, token storage, redirect |
| Flow | `frontend/pages/flow.md` | /flow, network flow visualization, real-time inference traffic |
| Health | `frontend/pages/health.md` | /health, service health, pod status, HPA, KEDA, staleness |
| Setup | `frontend/pages/setup.md` | /setup, bootstrap flow, first-run, admin account creation |

---

## Logic Flows (`flows/`)

> Algorithm reference — read before implementing logic in any subsystem.
> Flowcharts are the source of truth for control flow; code must match.

| Document | Path | Keywords |
|----------|------|---------|
| Index | `flows/README.md` | all subsystems overview |
| Inference Lifecycle | `flows/inference.md` | submit, queue, dispatch, VRAM reserve, stream, cleanup |
| Job Event Pipeline | `flows/job-event-pipeline.md` | direct Postgres writes, S3 conversation store, targeted DB updates, state transitions |
| Authentication | `flows/auth.md` | API key BLAKE2b, JWT HS256, InferCaller dual-auth, rate limit, MCP ACL, provider ACL |
| MCP Agentic Loop | `flows/mcp.md` | run_loop, execute_one, ACL, circuit breaker, result cache, loop detect |
| Provider Scheduler | `flows/scheduler.md` | select_provider, VRAM pool, placement planner, scale-out/in, circuit breaker |
| Thermal Protection | `flows/thermal.md` | Normal→Soft→Hard→Cooldown→RampUp, forced drain, 60s/90s watchdog |
| Agent Scrape Cycle | `flows/agent.md` | scrape_cycle, MCP heartbeat, node-exporter, ollama, OTLP, dynamic sharding, KEDA |

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
| Rust / Axum Shutdown | `research/backend/rust-axum-shutdown.md` | verified |
| API Design | `research/backend/api-design.md` | verified |
| Rust Performance | `research/backend/rust-perf-2026.md` | verified |
| LLM Scheduling | `research/backend/llm-scheduling-2026.md` | research |
| LLM Scheduling Demand | `research/backend/llm-scheduling-demand-2026.md` | research |
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
| Ollama streaming / context | `providers/ollama-impl.md` |
| Ollama model sync | `providers/ollama-models.md` |
| Gemini model sync | `providers/gemini-models.md` |
| Add GPU server | `providers/hardware.md` |
| Auth / JWT / session | `auth/jwt-sessions.md` + `research/security/auth.md` |
| Account RBAC | `auth/jwt-sessions.md` + `frontend/pages/accounts.md` |
| API keys / rate limiting | `auth/api-keys.md` + `frontend/pages/keys.md` |
| Security (CORS, crypto) | `auth/security.md` |
| VRAM pool / AIMD / thermal | `inference/capacity.md` + `flows/scheduler.md` + `flows/thermal.md` |
| Lab feature flag | `inference/lab-features.md` |
| MCP integration / orchestrator model | `inference/mcp.md` + `flows/mcp.md` |
| Any subsystem logic / control flow | `flows/{subsystem}.md` |
| veronex-mcp server / add a tool | `infra/crate-structure.md` (veronex-mcp Layout) |
| OTel pipeline | `infra/otel-pipeline.md` + `infra/otel-pipeline-ops.md` + `research/infrastructure/observability.md` |
| Kubernetes / Helm / KEDA | `infra/deploy-helm.md` + `flows/agent.md` |
| Service health monitoring | `providers/hardware.md` (§ Service Health Monitoring) + `frontend/pages/health.md` |
| CORS config | `infra/deploy.md` (CORS_ALLOWED_ORIGINS) |
| Design token / theme | `frontend/design-system.md` + `policies/patterns-frontend.md` |
| Add i18n key | `frontend/design-system-i18n.md` + relevant `frontend/pages/*.md` |
| Chart / tooltip | `frontend/charts.md` |
| Modify overview/dashboard | `frontend/pages/overview.md` |
| Modify servers UI | `frontend/pages/servers.md` |
| Modify providers UI | `frontend/pages/providers.md` |
| Modify jobs UI | `frontend/pages/jobs.md` |
| Usage page / tabs | `frontend/pages/usage.md` |
| Performance page | `frontend/pages/performance.md` |
| Setup wizard | `frontend/pages/setup.md` |
| Audit trail UI | `frontend/pages/audit.md` |
| Queue depth | `inference/job-api.md` (Queue Depth section) |
| Session grouping | `inference/session-grouping.md` + `frontend/pages/jobs.md` |
| Job dashboard API | `inference/job-api.md` |
| Rust performance / allocator | `research/backend/rust-perf-2026.md` |
| Hot-path DB optimization / caching strategy | `infra/hot-path-caching.md` |
| Job state-transition writes / Redpanda pipeline | `infra/job-event-pipeline.md` + `flows/job-event-pipeline.md` |
| Add application constant | `policies/architecture.md` — Domain constants live in `domain/constants.rs` |
| Testing strategy / purity | `policies/testing-strategy.md` — layer responsibility, decision checklist |
| Build / compile speed | `infra/build-optimization.md` — mold, hakari, cargo-chef, profiles |
