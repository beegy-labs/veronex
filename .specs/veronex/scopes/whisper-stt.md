# Scope: Whisper STT Provider

> L2: Active scope | **Last Updated**: 2026-03-19

## Objective

Whisper ASR를 `ProviderType::Whisper`로 통합.
`/v1/audio/transcriptions` (OpenAI 호환)을 실제 동작하도록 구현.
기존 Ollama/Gemini Provider 패턴을 그대로 따르되, STT 전용 Port 신규 정의.

## Change Summary

| ID | Type | Target | CDD Reference | Status |
|----|------|--------|--------------|--------|
| S1 | Add | ProviderType::Whisper | providers/whisper-stt.md | pending |
| S2 | Add | SttProviderPort trait | policies/patterns.md | pending |
| S3 | Add | WhisperAdapter | providers/whisper-stt.md | pending |
| S4 | Improve | health_checker.rs — Whisper 브랜치 | providers/ollama.md (참조) | pending |
| S5 | Improve | provider_handlers.rs — Whisper 등록 분기 | providers/whisper-stt.md | pending |
| S6 | Add | /v1/audio/transcriptions 구현 | inference/openai-compat.md | pending |
| S7 | Add | DB migration | — | pending |
| S8 | Add | CDD doc (providers/whisper-stt.md) | — | pending |

## Architecture Decision

### Provider로 통합하는 이유
- 여러 Whisper 인스턴스 등록/관리 필요 (DB + Admin UI 재사용)
- Health checker 자동 감시 (30s)
- 기존 provider CRUD API 재사용 (`POST /v1/providers`)
- 미래: 부하 분산 확장 가능

### InferenceProviderPort 미사용
- Whisper는 텍스트 생성이 아닌 STT → 별도 `SttProviderPort` 신규 정의
- VRAM / Thermal / AIMD: 불필요 (skip)
- 모델 sync: 불필요 (fixed model, large-v3-turbo)

### 기존 llm_providers 테이블 재사용
- `provider_type = 'whisper'` 추가
- `url`: Whisper ASR base URL (e.g. `https://whisper-asr-1.kr1.girok.dev`)
- `total_vram_mb`, `gpu_index`, `server_id`: 무시 (Whisper에서 사용 안 함)
- `is_active`, `status`: 동일하게 사용

## Completion Criteria

- `POST /v1/providers {provider_type: "whisper", url: "..."}` 등록 동작
- `POST /v1/audio/transcriptions` (multipart: file + model + language) → 200 `{"text": "..."}`
- Whisper provider offline 시 → 503 ServiceUnavailable
- Health checker 30s 주기로 Whisper `GET /` 체크
- 기존 Ollama/Gemini provider 동작 영향 없음
- 테스트: unit (WhisperAdapter mock), integration (health check 분기)

## Out of Scope

- Whisper provider 부하 분산 (복수 인스턴스 라운드로빈) — Multi-server Scale-Out에서
- 화자 분리(diarization) 결과 structured output — 추후 확장
- TTS (`/v1/audio/speech`) — 별도 스펙
- Admin UI Whisper 전용 탭 — 기존 Provider UI 재사용
