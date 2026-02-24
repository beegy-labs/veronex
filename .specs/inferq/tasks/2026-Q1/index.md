# Tasks: 2026-Q1

> L3: LLM autonomous | Based on scopes/2026-Q1.md

## Completed

- [x] 01: Project structure (Rust hexagonal arch — domain / application / infrastructure)
- [x] 02: Domain model (InferenceJob, LlmBackend, ApiKey, StreamToken, enums, value objects)
- [x] 04: Inference backends — OllamaAdapter + GeminiAdapter + BackendRouter (InferenceBackendPort)
- [x] 06: SSE streaming endpoint (`GET /v1/inference/{id}/stream`, Notify-based token buffering)
- [x] 07: PostgreSQL job state (inference_jobs table, UPSERT, status transitions, DB fallback on get_status)
- [x] 09: docker-compose (postgres:5433, valkey:6380, clickhouse:8123/9000, inferq:3001)
- [x] 10: API Key (UUIDv7+base62+BLAKE2b, auth middleware, RPM/TPM rate limiting via Valkey, usage query)
- [x] 08: Observability (OTel OTLP tracing via TraceLayer + ClickHouse inference_logs INSERT on job completion)
- [x] 11: Web Dashboard (Next.js 15 in web/ + dashboard API endpoints GET /v1/dashboard/stats + jobs)
- [x] 03: Persistent queue worker (Valkey RPUSH/BLPOP, max_jobs=1 serial, startup recovery)
- [x] 05: Model manager (OllamaModelManager — LRU eviction, max_loaded=1, ensure_loaded/record_used in run_job)

## Pending

- [x] 14: Backend 등록 API (`POST /v1/backends` + `GET /v1/backends` + `DELETE /v1/backends/{id}` + `POST /v1/backends/{id}/healthcheck`, 30s 주기 자동 헬스체크)

## Blocked

(none)

## Notes

- 스택 변경: Python/FastAPI → **Rust/Axum** (기존 spec은 Python 기준이나 구현은 Rust)
- 03 (queue worker): 현재 `submit()` 시 tokio::spawn으로 즉시 실행. 재시작 내성은 PostgreSQL job 상태 저장으로 부분 보완됨. Valkey 기반 영속 큐는 미구현
- 10 (rate limiting): RPM sliding window + TPM per-minute counter 완성. ClickHouse usage 기록은 schema만 존재, 실제 INSERT 미구현
- 08 (observability): InferenceEvent → ClickHouseObservabilityAdapter → INSERT inference_logs. OTel OTLP via OTEL_EXPORTER_OTLP_ENDPOINT env, HTTP TraceLayer 적용
- 11 (dashboard): Rust 백엔드 GET /v1/dashboard/stats + /v1/dashboard/jobs. Next.js 15 web/ (overview/jobs/keys/api-test), Docker web 서비스 port 3002
- 03 (queue worker): RPUSH on submit + BLPOP worker loop (serial, max_jobs=1). list_pending() on startup → reset running→pending → re-enqueue. Valkey 없으면 tokio::spawn fallback
- 05 (model manager): OllamaModelManager — VecDeque LRU, ensure_loaded (sync /api/ps + evict + register), record_used, max_loaded=1 (single GPU greedy). run_job에서 Ollama 백엔드 한정 호출. Gemini는 skip
- 14 (backend API): llm_backends 테이블 마이그레이션. PostgresBackendRegistry. CRUD 핸들러 4개. Ollama=/api/version, Gemini=models 엔드포인트 헬스체크. 30s 백그라운드 체커 → ONLINE/OFFLINE 자동 갱신

## Dependencies

```
01 → 02 → 04
     02 → 06, 07
     06, 07 → 08
     08 → 09 ✓, 10 ✓, 11
     03, 04 → 05
     05 → 14
     10 → 11
```
