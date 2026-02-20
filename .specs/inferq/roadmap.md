# inferq Roadmap

> L1: Master direction | Load on planning only

## Vision

Open-source, queue-based LLM inference gateway with web dashboard.
Multi-backend (local GPU + cloud API), API key auth, real-time SSE streaming.

## Phases

| Phase | Scope | Status |
| ----- | ----- | ------ |
| Q1 2026 | PoC: end-to-end inference + multi-backend + API key + web dashboard | → scopes/2026-Q1.md |
| Q2 2026 | Production hardening: rate limiting, advanced observability, alerts | Pending |
| Q3 2026 | Additional backends: OpenAI, Anthropic, llama.cpp | Pending |
| Q4 2026 | Multi-tenant, priority queue, billing, SLO dashboard | Pending |

## Q1 PoC Scope

Full working system from prompt to dashboard:

1. Hexagonal project structure (FastAPI + ARQ + Valkey)
2. Domain model (InferenceJob, LlmBackend, ApiKey, BackendType)
3. Ollama adapter + Gemini adapter (IInferenceBackendPort)
4. Model manager (greedy allocation + LRU eviction, multi-server)
5. InferenceRouter (model-affinity for Ollama, least-conn for cloud)
6. Queue worker (ARQ, max_jobs=1, serial GPU)
7. SSE streaming endpoint (sse-starlette + disconnect handling)
8. PostgreSQL state (jobs, backends, api_keys — SQLAlchemy 2.0 async)
9. ClickHouse analytics (inference_logs, api_key_usage_hourly MV)
10. Observability (stdout / OTel / ClickHouse — env var 전환)
11. API Key (UUIDv7 + base62 + BLAKE2b, rate limit 인프라)
12. Web Dashboard (Next.js 15 + shadcn/ui):
    - Overview, Usage, Performance, Backend Mgmt, API Keys, Error Logs
13. docker-compose (base + monitoring/analytics profiles)
14. Backend 등록 API (`POST /v1/backends`) — 배포 환경 무관

## Q2 Production Hardening

1. Rate limiting 실 적용 (RPM/TPM per API key, Valkey sliding window)
2. Prometheus `/metrics` 엔드포인트
3. Grafana dashboard 템플릿
4. 알림 (Slack/webhook): 에러율 급증, 큐 깊이 임계치
5. 백엔드 자동 failover

## Q3 Additional Backends

1. OpenAIAdapter (OPENAI + OPENAI_COMPATIBLE)
2. AnthropicAdapter
3. llama.cpp adapter (로컬 대안)
4. 어댑터 추가: factory case 1줄 원칙 검증

## Q4 Scale

1. 우선순위 큐 (priority job queue)
2. 멀티테넌트 격리
3. 사용량 기반 과금 (ClickHouse → billing)
4. SLO 대시보드 (TTFT P99 vs 목표선)

## References

- Best Practices: `references/best-practices.md`
- Architecture: `docs/llm/policies/architecture.md`
- Q1 Scope: `scopes/2026-Q1.md`
