# Roadmap

> L1: Direction | Load on planning only | **Last Updated**: 2026-05-09 (Modelfile + CAS blob store)

## 2026 — AI BaaS 전환

Veronex를 "llama-server 게이트웨이"에서 "AI BaaS (AI Backend as a Service)"로 전환.
핵심 문제: llama-server cold start (163s+) →
  llama-server **lazy 라이프사이클**(요청 시 시작 → 유휴 1분 후 종료, 어드민에서 TTL 조정)로 근본 해결.
  CAS blob store (sha256 dedup) + 노드별 Local PV 캐시로 cold start 단축, 명시적 unload API 도 동일 경로로 처리.
모델 관리 직접 소유: **Modelfile registry** — 압축버전(quantization)별 row + family default,
  4종 source (HuggingFace / Direct upload / URL / S3 pointer), HF LFS OID 사전 dedup.
설치 1회 실패 시 자동 재시도 X, admin manual retry only. install_attempts 영구 보관.
Storage: k8s 내부 Garage S3 (CAS) + 노드별 Local PV (LRU 90% threshold).
LLM 노드 지원 매트릭스 (좁힘, 2026-05-09):
  - **Mac M-chip 베어메탈** (darwin-aarch64 + Apple Metal, brew tap + launchd)
  - **AMD AI 395+ Strix Halo k8s** (linux-x86_64 + Vulkan/RADV, DaemonSet + /dev/dri 마운트)
  - 제외: NVIDIA CUDA, AMD ROCm, 일반 dGPU, Intel Mac (필요 시 별도 Phase 에서 확장)

| Phase | 우선순위 | 변경 | 타입 | 상태 |
| ----- | -------- | ---- | ---- | ---- |
| Phase 1 | P0 | LlamaServer 어댑터 + ProviderType 추가 | Migrate | → scopes/2026-Phase1.md |
| Phase 2 | P0 | Modelfile registry + CAS blob store (HF/Upload/URL/S3 pointer, Garage + Local PV) | Add | → scopes/2026-Phase2.md |
| Phase 3 | P1 | ProcessManager (lazy 라이프사이클 + 다중 노드) | Add | → scopes/2026-Phase3.md |
| Phase 4 | P1 | AIMD composite + Snapshot/Lease/Forecast/TieredKV/Criticality 본격 구현 | Change | → scopes/2026-Phase4.md |

## 제거 대상 (llama-server 의존성)

| 제거 항목 | 교체 |
| --------- | ---- |
| `LlamaServerAdapter` (probe·stall·coalescing) | `LlamaServerAdapter` (health check) |
| `lifecycle.rs` 600줄 | `process.rs` ~50줄 |
| `preloader.rs` | 불필요 (프로세스 시작 = 모델 로드) |
| `/api/ps`, `/api/tags`, `/api/show` | GET `/health` (slots_idle) |
| llama-server num_ctx SSOT 정렬 복잡성 | `--ctx-size` 고정 (프로세스 기동 시) |
| llama-server 모델 레지스트리 의존 | HuggingFace API + S3 캐시 |

## 유지 대상

Queue 시스템, Circuit Breaker, Thermal 보호, MCP/ReAct 루프,
Context 압축, OTel 파이프라인, API Key, Rate Limiting, Job 생명주기,
Frontend, DB 스키마(대부분), 기존 테스트.

## Dependencies

Phase 1 → Phase 2 → Phase 3 → Phase 4 (순차)
Phase 1은 독립 배포 가능 (llama-server 레거시 병렬 운영)
