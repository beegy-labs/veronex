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

---

## 목표

Ollama API 스펙 준수:
- `stream: true` (기본값) → 기존 `application/x-ndjson` streaming 응답
- `stream: false` → 모든 토큰 collect 후 단일 `application/json` 응답

---

## 적용 범위

| Endpoint | 처리 |
|----------|------|
| `POST /api/chat` | stream 필드 추가 + non-streaming 경로 구현 |
| `POST /api/generate` | stream 필드 추가 + non-streaming 경로 구현 |

---

## Non-Streaming 응답 형식 (Ollama 공식 스펙)

### `/api/chat` (stream: false) — 일반 텍스트 응답

```json
{
  "model": "llama3.2",
  "created_at": "2026-03-15T00:00:00Z",
  "message": { "role": "assistant", "content": "<full text>" },
  "done_reason": "stop",
  "done": true,
  "total_duration": 0,
  "load_duration": 0,
  "prompt_eval_count": 42,
  "prompt_eval_duration": 0,
  "eval_count": 128,
  "eval_duration": 0
}
```

### `/api/chat` (stream: false) — tool call 응답

tool call 토큰이 하나라도 있으면 `done_reason: "tool_calls"`, `message.content: ""`.

```json
{
  "model": "llama3.2",
  "created_at": "2026-03-15T00:00:00Z",
  "message": {
    "role": "assistant",
    "content": "",
    "tool_calls": [
      {
        "function": {
          "name": "get_weather",
          "arguments": { "location": "Seoul" }
        }
      }
    ]
  },
  "done_reason": "tool_calls",
  "done": true,
  "total_duration": 0,
  "load_duration": 0,
  "prompt_eval_count": 42,
  "prompt_eval_duration": 0,
  "eval_count": 128,
  "eval_duration": 0
}
```

### `/api/generate` (stream: false)

```json
{
  "model": "llama3.2",
  "created_at": "2026-03-15T00:00:00Z",
  "response": "<full text>",
  "done_reason": "stop",
  "done": true,
  "total_duration": 0,
  "load_duration": 0,
  "prompt_eval_count": 42,
  "prompt_eval_duration": 0,
  "eval_count": 128,
  "eval_duration": 0
}
```

> timing 필드(`total_duration`, `load_duration`, `prompt_eval_duration`, `eval_duration`)는
> veronex가 측정하지 않으므로 `0`으로 고정. Ollama 스펙상 허용됨.

---

## StreamToken 구조 (구현 참고)

`OllamaAdapter`가 emit하는 `StreamToken`:

```rust
StreamToken {
    value: String,                          // 토큰 텍스트
    is_final: bool,                         // true → 마지막 토큰
    prompt_tokens: Option<u32>,             // is_final=true 일 때만 set
    completion_tokens: Option<u32>,         // is_final=true 일 때만 set
    tool_calls: Option<serde_json::Value>,  // /api/chat tool call 시 set
}
```

non-streaming 경로에서 토큰 처리:
- `tool_calls.is_some()` → `tool_calls_acc`에 누적 (보통 1개 토큰에 전부 옴)
- `is_final` → `prompt_tokens`, `completion_tokens` 추출
- 나머지 → `content`에 `push_str`

---

## 구현 계획

### Phase 1 — 구조체 수정 (`ollama_compat_handlers.rs`)

```rust
// OllamaChatBody
#[serde(default)]
stream: Option<bool>,

// OllamaGenerateBody
#[serde(default)]
stream: Option<bool>,
```

### Phase 2 — `/api/chat` non-streaming 경로

`stream == Some(false)` 분기:

```rust
if req.stream == Some(false) {
    let mut content = String::new();
    let mut tool_calls: Option<serde_json::Value> = None;
    let mut prompt_tokens = 0u32;
    let mut eval_tokens = 0u32;

    let mut token_stream = state.use_case.stream(&job_id);
    while let Some(result) = token_stream.next().await {
        match result {
            Ok(t) if t.tool_calls.is_some() => tool_calls = t.tool_calls,
            Ok(t) if t.is_final => {
                prompt_tokens = t.prompt_tokens.unwrap_or(0);
                eval_tokens   = t.completion_tokens.unwrap_or(0);
            }
            Ok(t) => content.push_str(&t.value),
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": sanitize_sse_error(&e)}))).into_response(),
        }
    }

    let (done_reason, message) = if let Some(tc) = tool_calls {
        ("tool_calls", serde_json::json!({"role":"assistant","content":"","tool_calls": tc}))
    } else {
        ("stop", serde_json::json!({"role":"assistant","content": content}))
    };

    return Json(serde_json::json!({
        "model": model,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "message": message,
        "done_reason": done_reason,
        "done": true,
        "total_duration": 0, "load_duration": 0,
        "prompt_eval_count": prompt_tokens, "prompt_eval_duration": 0,
        "eval_count": eval_tokens, "eval_duration": 0,
    })).into_response();
}
// stream: true → 기존 ndjson 경로
```

### Phase 3 — `/api/generate` non-streaming 경로

동일 패턴, `message` 대신 `response` 필드:

```rust
Json(serde_json::json!({
    "model": model,
    "created_at": chrono::Utc::now().to_rfc3339(),
    "response": content,      // ← "message" 아닌 "response"
    "done_reason": "stop",
    "done": true,
    "total_duration": 0, "load_duration": 0,
    "prompt_eval_count": prompt_tokens, "prompt_eval_duration": 0,
    "eval_count": eval_tokens, "eval_duration": 0,
}))
```

> `/api/generate`에 tool call은 없으므로 tool_calls 분기 불필요.

### Phase 4 — 테스트

| 케이스 | 검증 내용 |
|--------|----------|
| `stream: false` (chat) | `Content-Type: application/json`, `done: true`, `message.content` 누적 |
| `stream: false` (generate) | `response` 필드 누적 |
| `stream: false` + tool call | `done_reason: "tool_calls"`, `message.tool_calls` 포함 |
| `stream: true` / 미지정 | 기존 `application/x-ndjson` 동작 유지 |
| `stream: false` (Ollama 프로바이더 없음) | 503 반환 |

---

## Tasks

| # | Task | 파일 | Status |
|---|------|------|--------|
| 1 | `OllamaChatBody` / `OllamaGenerateBody`에 `stream: Option<bool>` 추가 | `ollama_compat_handlers.rs` | **done** |
| 2 | `/api/chat` non-streaming 경로 구현 (tool_calls 포함) | `ollama_compat_handlers.rs` | **done** |
| 3 | `/api/generate` non-streaming 경로 구현 | `ollama_compat_handlers.rs` | **done** |
| 4 | 테스트 추가 (7개 케이스 + proptest) | `ollama_compat_handlers.rs` | **done** |
