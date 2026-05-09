# Roadmap

> L1: Direction | Load on planning only | **Last Updated**: 2026-05-09

## 2026 — AI BaaS 전환

Veronex를 "Ollama 게이트웨이"에서 "AI BaaS (AI Backend as a Service)"로 전환.
핵심 문제: Ollama cold start (163s+) → llama-server 상시 로드로 근본 해결.
모델 관리 직접 소유: HuggingFace + S3 캐시.

| Phase | 우선순위 | 변경 | 타입 | 상태 |
| ----- | -------- | ---- | ---- | ---- |
| Phase 1 | P0 | LlamaServer 어댑터 + ProviderType 추가 | Migrate | → scopes/2026-Phase1.md |
| Phase 2 | P0 | ModelStore (HuggingFace + S3 캐시) | Add | → scopes/2026-Phase2.md |
| Phase 3 | P1 | ProcessManager (llama-server 설치·운영) | Add | → scopes/2026-Phase3.md |
| Phase 4 | P1 | AIMD slots_idle 기반 재구현 | Change | → scopes/2026-Phase4.md |

## 제거 대상 (Ollama 의존성)

| 제거 항목 | 교체 |
| --------- | ---- |
| `OllamaAdapter` (probe·stall·coalescing) | `LlamaServerAdapter` (health check) |
| `lifecycle.rs` 600줄 | `process.rs` ~50줄 |
| `preloader.rs` | 불필요 (프로세스 시작 = 모델 로드) |
| `/api/ps`, `/api/tags`, `/api/show` | GET `/health` (slots_idle) |
| Ollama num_ctx SSOT 정렬 복잡성 | `--ctx-size` 고정 (프로세스 기동 시) |
| Ollama 모델 레지스트리 의존 | HuggingFace API + S3 캐시 |

## 유지 대상

Queue 시스템, Circuit Breaker, Thermal 보호, MCP/ReAct 루프,
Context 압축, OTel 파이프라인, API Key, Rate Limiting, Job 생명주기,
Frontend, DB 스키마(대부분), 기존 테스트.

## Dependencies

Phase 1 → Phase 2 → Phase 3 → Phase 4 (순차)
Phase 1은 독립 배포 가능 (Ollama 레거시 병렬 운영)
