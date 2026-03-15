# Ollama API Compatibility — Non-Streaming Response SDD

> **Status**: In Progress | **Last Updated**: 2026-03-15
> **Branch**: feat/ollama-compat-non-streaming

---

## 문제

Open-webui가 `/api/chat`, `/api/generate`를 `stream: false`로 호출할 때
Veronex가 항상 `application/x-ndjson` streaming으로 응답함.

Open-webui의 내부 task들(제목 생성, 태그 생성, follow-up 생성)은
non-streaming JSON 응답을 기대하기 때문에 파싱 실패 → 기능 오동작.

```
HTTPException: 200: Ollama: 200,
  message='Attempt to decode JSON with unexpected mimetype: application/x-ndjson',
  url='https://veronex-api.verobee.com/api/chat'
```

## 근본 원인

`OllamaChatBody`, `OllamaGenerateBody` 구조체에 `stream` 필드 없음.
→ `stream: false`가 역직렬화되지 않아 항상 streaming 경로로 진입.

## 목표

Ollama API 스펙 준수:
- `stream: true` (기본값) → 기존 `application/x-ndjson` streaming 응답
- `stream: false` → 모든 토큰 collect 후 단일 `application/json` 응답

## 적용 범위

| Endpoint       | 처리 |
|----------------|------|
| `POST /api/chat`     | stream 필드 추가 + non-streaming 경로 구현 |
| `POST /api/generate` | stream 필드 추가 + non-streaming 경로 구현 |

## Non-Streaming 응답 형식

### `/api/chat` (stream: false)
```json
{
  "model": "model-name",
  "created_at": "2026-03-15T00:00:00Z",
  "message": { "role": "assistant", "content": "<full text>" },
  "done_reason": "stop",
  "done": true,
  "total_duration": 0,
  "prompt_eval_count": 42,
  "eval_count": 128
}
```

### `/api/generate` (stream: false)
```json
{
  "model": "model-name",
  "created_at": "2026-03-15T00:00:00Z",
  "response": "<full text>",
  "done_reason": "stop",
  "done": true,
  "total_duration": 0,
  "prompt_eval_count": 42,
  "eval_count": 128
}
```

## 구현 계획

### Phase 1 — 구조체 수정
- `OllamaChatBody`에 `stream: Option<bool>` 추가 (`#[serde(default)]`)
- `OllamaGenerateBody`에 `stream: Option<bool>` 추가

### Phase 2 — chat 핸들러 분기
- `stream` 값이 `Some(false)`이면 non-streaming 경로
- token_stream을 `.collect()` → content 누적, 최종 token에서 usage 추출
- `Content-Type: application/json`으로 단일 JSON 반환

### Phase 3 — generate 핸들러 분기
- 동일 패턴으로 `/api/generate` 적용

### Phase 4 — 테스트
- `stream: false` 요청 → JSON 응답 검증
- `stream: true` (또는 미지정) → 기존 ndjson 동작 유지 검증

## Tasks

| # | Task | Status |
|---|------|--------|
| 1 | `OllamaChatBody` / `OllamaGenerateBody` stream 필드 추가 | pending |
| 2 | `/api/chat` non-streaming 경로 구현 | pending |
| 3 | `/api/generate` non-streaming 경로 구현 | pending |
| 4 | 단위 테스트 추가 | pending |
