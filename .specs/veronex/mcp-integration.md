# MCP Integration SDD

> **Status**: In Progress — Phase 1 구현 완료 | **Last Updated**: 2026-03-22
> **Scope**: S11 — Native MCP support via McpBridgeAdapter
> **Branch**: `feat/mcp-integration`
> **Spec Target**: MCP 2025-03-26 (Streamable HTTP)

---

## 목적

사용자가 "오늘 서울 날씨 알려줘"라고 하면:
- 사람이 tool을 선택하거나 MCP를 직접 호출하지 않음
- LLM(Ollama)이 등록된 tool 목록을 보고 스스로 판단
- 필요한 tool을 병렬/순차로 호출 (get_coordinates → get_weather)
- 최종 답변만 클라이언트에 반환

**Veronex = Cursor처럼 동작하는 서버**
Cursor가 클라이언트에서 MCP 루프를 처리하듯, Veronex가 서버 내부에서 동일하게 처리.

---

## 구체적 예시

```
Client: "오늘 서울 날씨가 어때?"

[1] Veronex — MCP 필요 여부 판단
    McpToolCache에서 tool 목록 주입 → Ollama 호출
    LLM 판단: "get_coordinates, get_weather 필요"

[2] Round 1 — tool_calls 반환:
    tool_call_1: mcp_weather_get_coordinates("서울")
    → join_all() 병렬 실행 (이 경우 1개)
    → result_cache miss → McpHttpClient.call()
    ← {lat: 37.5, lng: 126.9}

[3] Round 2 — 결과 추가 후 재요청:
    tool_call_1: mcp_weather_get_weather(37.5, 126.9)
    → result_cache hit (cache_ttl_secs 설정됨) → Valkey 즉시 반환
    ← {temp: "12°C", sky: "맑음"}

[4] Round 3 — tool_calls 없음:
    LLM: "오늘 서울은 맑고 12°C입니다."

[5] Client ← "오늘 서울은 맑고 12°C입니다."
    (중간 과정 전혀 모름, 표준 OpenAI 응답)
```

---

## 설계 원칙

1. **LLM이 tool 선택** — 사람이 tool을 지정하지 않음. tool_calls가 없으면 MCP는 한 번도 호출되지 않음
2. **클라이언트 투명** — 어떤 클라이언트든 표준 OpenAI API 그대로 사용. MCP 존재를 몰라도 됨
3. **MCP 서버는 항시 실행** — HTTP 서버로 상시 대기. veronex-agent가 health 감시
4. **글로벌 tool 풀** — 등록된 모든 MCP 서버의 tool이 합쳐져서 LLM에 제공
5. **cap_points로 루프 제한** — 무한 루프 방지. 키별 최대 tool_call 라운드 횟수
6. **annotations 기반 캐싱** — `readOnlyHint: true AND idempotentHint: true`일 때만 result 캐싱

---

## 아키텍처

```
Client (Cursor, Codex CLI, 일반 앱)
  │ POST /v1/chat/completions
  │ (표준 OpenAI 포맷, MCP 설정 불필요)
  ▼
Veronex — McpBridgeAdapter
  │
  ├── [1] API key mcp_access 확인
  │         NO  → OllamaAdapter 직통 (기존 경로 그대로)
  │         YES ↓
  │
  ├── [2] MCP 필요 여부 판단 (LLM에게 위임)
  │         McpToolCache.get_all() → tools 주입 → Ollama 호출
  │         LLM 판단:
  │           tool_calls 없음 → 바로 최종 응답 → 클라이언트 스트리밍 (종료)
  │           tool_calls 반환 → [3]으로
  │
  └── [3] 병렬 실행 루프 (cap_points 소진까지)
            LLM tool_calls 수신:
              ├── 루프 감지: "tool:{sorted_args_hash}" 최근 3턴 동일 → 강제 탈출
              ├── mcp_* → buffer_unordered(8) 병렬 실행 (per-call timeout: 30s)
              │     Ollama ID 없음 → index 기반 매핑 (tool_calls[i] ↔ results[i])
              │     circuit open 서버 → 해당 tool 건너뜀
              │     result_cache hit → 즉시 반환
              │     miss → McpHttpClient.call() → cache 조건 충족 시 Valkey 저장
              ├── client tool → finish_reason: "tool_calls" → 클라이언트 반환
              └── 결과 messages 추가 → cap -= 1 (성공 있을 때만) → ClickHouse 이벤트 → LLM 재요청 → [2] 반복
```

---

## MCP 프로토콜 (2025-03-26 Streamable HTTP)

### 트랜스포트

| 구분 | 2024-11-05 (레거시) | **2025-03-26 (채택)** |
|------|-------------------|----------------------|
| 엔드포인트 | GET /sse + POST /messages (2개) | **POST+GET /mcp (1개)** |
| 세션 | 없음 | `Mcp-Session-Id` 헤더 |
| 배치 | 미지원 | JSON-RPC 배치 지원 |
| 연결 복구 | 미지원 | SSE id + Last-Event-ID |
| 스트리밍 | SSE 전용 | per-request: JSON or SSE |

**연결 흐름:**
```
Client → POST /mcp  (InitializeRequest)
         Accept: application/json, text/event-stream

Server ← 200 OK
         Mcp-Session-Id: <cryptographically_secure_uuid>
         Content-Type: application/json

Client → POST /mcp  (이후 모든 요청)
         Mcp-Session-Id: <uuid>

Client → GET /mcp   (서버 push용 SSE 선택적)
         Mcp-Session-Id: <uuid>

Client → DELETE /mcp (세션 종료)
         Mcp-Session-Id: <uuid>
```

**에러 코드:**
```
400 — 세션 ID 없음 (비-초기화 요청)
404 — 세션 만료 → Mcp-Session-Id 헤더 제거 후 새 InitializeRequest POST (헤더 포함 시 무한 404)
-32602 — 지원하지 않는 프로토콜 버전 또는 알 수 없는 tool
```

**세션 만료 재초기화 순서:**
```
404 수신
  → session_manager.invalidate(server_id)
  → Mcp-Session-Id 헤더 없이 새 POST /mcp (InitializeRequest)
  → 새 Mcp-Session-Id 획득
  → 원래 요청 재시도
```

### Initialize 핸드셰이크

McpClient가 연결 시 반드시 수행. `initialize` 이전에는 `ping`만 허용.

```json
// 1. Client → Server
{
  "jsonrpc": "2.0", "id": 1, "method": "initialize",
  "params": {
    "protocolVersion": "2025-03-26",
    "capabilities": {
      "roots": { "listChanged": false }
      // sampling 미선언 — MCP 서버가 LLM 역호출 불가 (v1 의도적 제외)
      // resources, prompts 미선언 — v2 이후
    },
    "clientInfo": { "name": "veronex", "version": "0.11.0" }
  }
}

// 2. Server → Client
{
  "jsonrpc": "2.0", "id": 1,
  "result": {
    "protocolVersion": "2025-03-26",
    "capabilities": { "tools": { "listChanged": true } },
    "serverInfo": { "name": "weather-mcp", "version": "1.0.0" }
  }
}

// 3. Client → Server (응답 없음, notification)
{ "jsonrpc": "2.0", "method": "notifications/initialized" }
```

**Sampling capability 의도적 제외:**
MCP 서버가 `sampling/createMessage`로 Veronex에 역으로 LLM 추론을 요청하는 reverse channel.
`sampling` capability를 선언하지 않으면 MCP 서버가 요청하지 않음.
선언 후 미구현 시 → MCP 서버 hang. v1에서 반드시 제외.

### tools/list

```json
// Request
{ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }

// Response
{
  "jsonrpc": "2.0", "id": 2,
  "result": {
    "tools": [{
      "name": "get_weather",
      "description": "Get current weather for a location",
      "inputSchema": {
        "type": "object",
        "properties": { "lat": { "type": "number" }, "lng": { "type": "number" } },
        "required": ["lat", "lng"]
      },
      "annotations": {
        "readOnlyHint": true,      // 부작용 없음
        "idempotentHint": true,    // 동일 인자 → 동일 결과
        "destructiveHint": false,
        "openWorldHint": true
      }
    }]
  }
}
```

**캐싱 결정:** `readOnlyHint: true AND idempotentHint: true` → result 캐싱 가능. 둘 중 하나라도 false → 항상 직접 호출.

### tools/call 및 에러 구분

**두 가지 에러 채널 (반드시 구분):**
```json
// [A] tool 실행 실패 (isError: true) — LLM이 결과를 보고 판단
{
  "jsonrpc": "2.0", "id": 3,
  "result": {
    "content": [{ "type": "text", "text": "API rate limit exceeded" }],
    "isError": true
  }
}

// [B] 프로토콜 오류 — 세션 파괴, 재연결 필요
{
  "jsonrpc": "2.0", "id": 3,
  "error": { "code": -32602, "message": "Unknown tool: invalid_tool" }
}
```

**처리 원칙:**
- `[A] isError: true` → `tool` role 메시지로 LLM에 전달, LLM이 판단. **cap 미차감** (성공 없는 라운드)
- `[B] JSON-RPC error` → 해당 MCP 서버 circuit open, 세션 재초기화, 해당 서버 tool 제외

### JSON-RPC ping (liveness)

```json
// Request
{ "jsonrpc": "2.0", "id": 99, "method": "ping" }

// Response
{ "jsonrpc": "2.0", "id": 99, "result": {} }
```

TCP 연결이 살아있어도 JSON-RPC 스택이 응답하지 않을 수 있으므로 ping 사용.

---

## DB 스키마

> 구현 파일: `migrations/postgres/000011_mcp_capabilities.up.sql`

### mcp_servers

```sql
-- 실제 구현 스키마 (migration 000011)
CREATE TABLE mcp_servers (
    id           UUID        PRIMARY KEY DEFAULT uuidv7(),
    name         VARCHAR(128) NOT NULL UNIQUE,
    slug         VARCHAR(64)  NOT NULL UNIQUE CHECK (slug ~ '^[a-z0-9_]+$'),
    url          TEXT         NOT NULL,
    is_enabled   BOOLEAN      NOT NULL DEFAULT true,
    timeout_secs SMALLINT     NOT NULL DEFAULT 30 CHECK (timeout_secs BETWEEN 1 AND 300),
    metadata     JSONB        NOT NULL DEFAULT '{}',
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);
```

> slug는 tool 네임스페이스에 사용: `mcp_{slug}_{tool_name}` (예: `mcp_weather_get_weather`)

### mcp_server_tools

```sql
CREATE TABLE mcp_server_tools (
    server_id       UUID NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name       TEXT NOT NULL,
    namespaced_name TEXT NOT NULL,   -- "mcp_{slug}_{tool_name}"
    description     TEXT,
    input_schema    JSONB NOT NULL DEFAULT '{}',
    annotations     JSONB NOT NULL DEFAULT '{}',
    discovered_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (server_id, tool_name)
);
```

### mcp_key_access

```sql
-- API key → MCP server 접근 제어 (기본: deny)
CREATE TABLE mcp_key_access (
    api_key_id  UUID    NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    server_id   UUID    NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    is_allowed  BOOLEAN NOT NULL DEFAULT true,
    granted_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (api_key_id, server_id)
);
```

**cap_points 예시:**
| 등급 | cap_points | 처리 가능 체인 |
|------|-----------|--------------|
| 고급 키 | 5 | 주식 조사 (3단계 이상) |
| 기본 키 | 2 | 날씨 (get_coords → get_weather) |
| 비활성 테스트 | 0 | mcp_access 있어도 MCP 미호출 |

### mcp_settings (글로벌 설정)

```sql
CREATE TABLE mcp_settings (
    id                        INT  PRIMARY KEY DEFAULT 1,  -- 단일 행
    routing_cache_ttl_secs    INT  NOT NULL DEFAULT 3600,
    tool_schema_refresh_secs  INT  NOT NULL DEFAULT 30,
    embedding_model           TEXT NOT NULL DEFAULT 'nomic-embed-text',
    max_tools_per_request     INT  NOT NULL DEFAULT 32,   -- context window 보호
    max_routing_cache_entries INT  NOT NULL DEFAULT 200,  -- cosine 계산 대상 상한
    CHECK (id = 1)
);
```

### 마이그레이션 파일

`migrations/postgres/000011_mcp_capabilities.up.sql` — 위 3개 테이블 + GIN index on mcp_servers.name

---

## 구성 요소 상세

### McpSessionManager

```
infrastructure/outbound/mcp/session_manager.rs

역할: MCP 서버별 세션 수명 관리
  - initialize 핸드셰이크
  - Mcp-Session-Id 저장 및 헤더 자동 주입
  - 세션 만료(404) 시 자동 재초기화
  - 레플리카별 독립 세션 (공유 불가)

구조:
  DashMap<McpServerId, McpSession>
  McpSession { session_id: String, client: reqwest::Client, initialized_at: Instant }
```

### McpHttpClient

```
infrastructure/outbound/mcp/client.rs

  initialize(url) → McpSession
  ping(session)   → Result<()>
  list_tools(session) → Vec<McpTool>
  call_tool(session, name, args) → McpToolResult
    - isError: true → Ok(McpToolResult { is_error: true, content })
    - JSON-RPC error → Err(McpProtocolError)
```

### McpToolCache (L1 DashMap + L2 Valkey)

```
infrastructure/outbound/mcp/tool_cache.rs

구조:
  L1: DashMap<McpServerId, CachedTools>  (로컬, TTL 30s)
  L2: Valkey  key: veronex:mcp:tools:{server_id}  TTL: 35s

읽기:
  DashMap hit → 즉시 반환 (O(1))
  DashMap miss → Valkey → DashMap 저장

갱신 (두 가지 트리거):
  [A] 30s 폴링:
      SET NX veronex:mcp:tools:lock:{server_id} → 성공한 레플리카만 갱신
      list_tools() 호출 → 변환 → Valkey SET → DashMap 갱신

  [B] notifications/tools/list_changed 수신 (push 기반):
      SSE GET /mcp 스트림에서 수신
      → 즉시 해당 서버 DashMap 무효화 → 다음 요청 시 Valkey/MCP 재조회
      → SET NX 경쟁 없이 즉시 갱신 (push 신호 = 이미 변경됨)

변환 (MCP → OpenAI function 포맷):
  tool.name         → "mcp_{server_name}_{tool_name}"
  tool.description  → function.description
  tool.inputSchema  → function.parameters
  tool.annotations  → 내부 메타 (캐싱 여부 결정용, LLM에는 미노출)

server_id 역매핑 (tool name → server_id):
  DashMap<String, McpServerId>  key: "mcp_{server_name}_{tool_name}"
  tool_calls 수신 시 O(1) server_id 조회 (DB 쿼리 없음)
  갱신: tool schema 갱신 시 함께 업데이트

tool 주입 상한 (context window 보호):
  mcp_settings.max_tools_per_request (기본 32)
  routing_cache hit → 해당 tool만 (보통 2~5개)
  miss → 전체에서 우선순위 32개 (active server 우선, 이름 알파벳 순)

SSE 수신 (notifications/tools/list_changed):
  부팅 시 각 MCP 서버에 GET /mcp 스트림 유지 (McpSseListenerTask)
  수신 시 → tool_cache.invalidate(server_id) 즉시
  연결 끊김 → Last-Event-ID로 재연결, 실패 시 30s polling으로 fallback
```

### McpResultCache (Valkey)

```
infrastructure/outbound/mcp/result_cache.rs

캐싱 조건:
  mcp_servers.cache_ttl_secs IS NOT NULL
  AND tool.annotations.readOnlyHint = true
  AND tool.annotations.idempotentHint = true

캐시 키:
  veronex:mcp:result:{tool_name}:{args_hash}
  args_hash = SHA256(sort_keys(JSON(arguments)))[:16]  -- 키 정규화 필수

TTL:
  mcp_servers.cache_ttl_secs (초 단위)

흐름:
  hit  → 즉시 반환, MCP 서버 미호출
  miss → McpHttpClient.call_tool() → Valkey SETEX → 반환

가이드:
  좌표/지명 (정적)         → cache_ttl_secs: 86400  (24h)
  날씨/환율 (준실시간)      → cache_ttl_secs: 600   (10분)
  뉴스/문서               → cache_ttl_secs: 3600  (1h)
  실시간 주가/잔고/주문     → cache_ttl_secs: NULL  (캐싱 불가)
```

### McpRoutingCache (Valkey + embedding)

```
infrastructure/outbound/mcp/routing_cache.rs

역할: 쿼리 패턴 → 사용할 tool 목록 캐싱 (LLM tool 선택 라운드 생략)

구현 방식 (Valkey HNSW 미지원 → 경량 cosine 계산):
  Valkey HNSW는 Redis 8.0+ 전용. Valkey(Redis 포크)는 별도 모듈 없이 미지원.
  → 대안: Valkey에 embedding 벡터 ZSET으로 저장 + Rust에서 cosine 계산

  흐름:
  1. 쿼리 embedding: Ollama /api/embed (mcp_settings.embedding_model)
     → Vec<f32> 생성
  2. Valkey HGETALL veronex:mcp:routes → 최근 N개 (FIFO, 기본 200개) 로드
  3. Rust에서 cosine_similarity 벡터 계산
     max cosine >= 0.92 → hit: 해당 tool 패턴 반환
     < 0.92             → miss: 전체 tool 주입, LLM 선택

캐시 저장 (miss 후):
  ValkeyPort에 HSET/HGETALL 미존재 → kv_set/kv_get + JSON 직렬화로 구현
  key: veronex:mcp:route:{sha256(embedding)[:16]}
  value: JSON { embedding: Vec<f32>, tools: Vec<String>, ts: i64 }
  TTL: mcp_settings.routing_cache_ttl_secs (kv_set의 ttl_secs 파라미터)

  전체 목록 관리:
  veronex:mcp:route:index → JSON Vec<String> (키 목록, kv_set으로 업데이트)
  200개 초과 시 가장 오래된 키 kv_del + index 업데이트

효과:
  "서울 날씨" / "부산 날씨" / "날씨 알려줘" → cosine >= 0.92 → 동일 tool 패턴 hit
  → LLM tool 선택 라운드 생략, cap 절약
  200개 제한 + Rust 계산 → DB 쿼리 없이 O(200) 계산 (수 ms)
```

### McpBridgeAdapter

```
infrastructure/outbound/mcp/adapter.rs

  주입 위치: openai_handlers.rs 내부
    현재 핸들러는 use_case.stream_tokens() 결과를 클라이언트에 즉시 스트리밍.
    McpBridgeAdapter는 이 스트림을 intercept하여 tool_call 루프를 핸들러 안에서 처리.
    tool_calls가 모두 mcp_* 이면 클라이언트에 미노출, 직접 실행 후 재요청.

  Ollama tool_calls 원시 포맷 (serde_json::Value):
    [{"type":"function","function":{"index":0,"name":"get_weather","arguments":{...}}}]
    name 필드로 mcp_* 여부 판단, arguments 필드로 args 추출

  dispatch(request, api_key, ollama_adapter, state) -> Stream<ChatToken>:

    // [1] mcp_access 확인
    cap = api_key_capabilities.get(api_key, "mcp_access") ?? return ollama.stream_chat()
    if cap.cap_points == 0: return ollama.stream_chat()

    // [2] routing_cache 확인
    tools = routing_cache.get(query)
              .unwrap_or_else(|| tool_cache.get_all())

    // [3] tool_call 루프
    remaining = cap.cap_points
    action_history: Vec<String> = vec![]  // 루프 감지용

    loop:
      response = ollama.stream_chat(messages + tools)

      if response.tool_calls.is_empty():
        stream_to_client(response.content)
        break

      // 루프 감지: 동일 signature 3회 반복 시 탈출
      signatures = response.tool_calls.map(|tc| format!("{}:{}", tc.name, sorted_args_hash(tc.args)))
      if action_history.last_n(3).contains_all(&signatures):
        messages.push(system: "동일 tool을 반복 호출하지 마라. 지금까지의 결과로 최종 답변하라")
        stream_to_client(ollama.stream_chat(messages))
        break
      action_history.extend(signatures)

      // 병렬 실행 (join_all 사용 — 원래 index 순서 보존 보장)
      // buffer_unordered는 완료 순서로 반환 → Ollama ID 없는 환경에서 순서 깨짐
      // join_all은 입력 순서 그대로 Vec으로 반환
      // Ollama tool_call ID 없음 → index 기반 매핑 (tool_calls[i] ↔ results[i])
      futs = response.tool_calls.iter().map(|tc| async move {
        if is_mcp_tool(tc.name):
          if circuit_breaker.is_open(server_of(tc.name)):
            return ToolResult::skipped()
          timeout(30s, async {
            result_cache.get(tc)
              .or_else(|| mcp_client.call_tool(tc) → cache_if_eligible)
          }).await.unwrap_or(ToolResult::timeout())
        else:
          return_to_client_as_tool_call(tc)
      })
      results = join_all(futs).await  // 순서 보존 보장

      // circuit breaker 업데이트 (join_all → index 순서 동일)
      results.iter().zip(response.tool_calls.iter())
        .for_each(|(r, tc)| circuit_breaker.record(server_of(tc.name), r))

      // tool 결과를 index 순서대로 messages에 추가
      // join_all이 순서 보존 → tool_calls[i] ↔ results[i] 보장
      messages.append(assistant_tool_calls)
      for (tc, result) in response.tool_calls.iter().zip(results.iter()):
        messages.push(tool_result(name: tc.name, content: result))

      // cap_points 차감: 성공한 tool_call이 1개 이상인 라운드에만 차감
      // 실패(isError:true) / timeout / circuit_open / cache_hit 전용 라운드는 차감 안 함
      round_has_success = results.iter().any(|(_, r)| r.is_success())
      if round_has_success:
        remaining -= 1

      // analytics_repo: 라운드 내 각 tool_call 이벤트 (fire-and-forget, None이면 skip)
      if let Some(repo) = &state.analytics_repo:
        for (tc, result) in response.tool_calls.iter().zip(results.iter()):
          repo.emit("mcp.tool_call", now(), [
            ("mcp.api_key_id",   api_key.id.to_string()),
            ("mcp.server_id",    server_of(tc.name)),
            ("mcp.tool_name",    tc.name),
            ("mcp.args_hash",    sorted_args_hash(tc.arguments)),
            ("mcp.cache_hit",    result.is_cache_hit()),
            ("mcp.success",      result.is_success()),
            ("mcp.is_error",     result.is_mcp_error()),
            ("mcp.timed_out",    result.is_timeout()),
            ("mcp.circuit_open", result.is_skipped()),
            ("mcp.latency_ms",   result.latency_ms),
            ("mcp.cap_consumed", if round_has_success { 1 } else { 0 }),
            ("mcp.cap_remaining", remaining),
          ])

      if remaining == 0:
        messages.push(system: "더 이상 tool을 호출하지 말고 지금까지의 결과로 답변하라")
        stream_to_client(ollama.stream_chat(messages))
        break

    // routing_cache miss였으면 저장
    routing_cache.save(query, used_tools)
```

### 병렬 Tool 실행 상세

**기본 병렬 (v1 — 프롬프트 가공 불필요):**

LLM이 `parallel_tool_calls`로 여러 tool_call을 한 번에 반환 → `buffer_unordered(N)` 동시 실행.

```rust
// 핵심 패턴
let results: Vec<_> = stream::iter(tool_calls)
    .map(|tc| async move { self.execute_mcp_tool(tc).await })
    .buffer_unordered(MAX_PARALLEL_TOOLS)  // 동시 실행 상한
    .collect()
    .await;
```

**의존성 있는 체인 (자동 처리):**

```
예: 마이크론 주식 분석

Round 1 — LLM이 병렬 반환:
  search_company("마이크론")    ─┐
  get_stock_price("MU")         ─┤ → join 병렬 실행
  search_recent_news("마이크론") ─┘

Round 2 — Round 1 결과 보고 추가 호출:
  search_related_stocks(round1_keywords) → 단일 실행

Round 3 — tool_calls 없음 → 최종 답변
```

**Plan-then-Execute (v2 이후 — 프롬프트 가공 필요):**

복잡한 다단계에서 LLM이 먼저 의존성 그래프를 JSON으로 출력, Veronex가 파싱 후 토폴로지 정렬.

```
시스템 프롬프트: "먼저 실행 계획을 JSON으로 출력. 독립 tool은 같은 round에 묶어라."

LLM 출력:
{
  "rounds": [
    ["search_company", "get_stock_price", "search_news"],  // 병렬
    ["summarize_results"]                                   // 의존
  ]
}
```

> v1은 기본 병렬(자동)로 구현. Plan-then-Execute는 v2에서 opt-in.

---

## Cap 차감 정책

| 상황 | cap 차감 | 이유 |
|------|---------|------|
| 성공 tool_call 포함 라운드 | **-1** | 실제 MCP 소비 |
| 전부 `isError: true` 라운드 | **0** | 서버 실패, 사용자 책임 아님 |
| 전부 timeout 라운드 | **0** | 네트워크 문제, 사용자 책임 아님 |
| 전부 circuit_open 라운드 | **0** | 서버 다운, 사용자 책임 아님 |
| 전부 cache_hit 라운드 | **0** | Veronex 내부 처리, MCP 미호출 |
| 루프 감지 탈출 | **0** | 라운드 미실행 |

> 라운드 내 성공 1개 이상이면 전체 라운드 cap -= 1 (개별 tool 단위 아님)

---

## ClickHouse 사용량 추적

### mcp_tool_calls 테이블

```sql
CREATE TABLE mcp_tool_calls (
    timestamp        DateTime64(3),
    api_key_id       UUID,
    mcp_server_id    UUID,
    tool_name        String,         -- "mcp_{server}_{tool}"
    args_hash        String,         -- SHA256[:16], PII 제외
    cache_hit        Bool,
    success          Bool,
    is_error         Bool,           -- MCP isError: true
    timed_out        Bool,
    circuit_open     Bool,           -- 서버 차단 상태
    latency_ms       UInt32,         -- 0 if cache_hit
    cap_consumed     UInt8,          -- 0 or 1 (라운드 기준)
    cap_remaining    UInt8
)
ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (api_key_id, timestamp)
```

### 활용 쿼리 예시

```sql
-- 키별 일일 cap 사용량
SELECT api_key_id, sum(cap_consumed) as used
FROM mcp_tool_calls
WHERE timestamp >= today()
GROUP BY api_key_id

-- tool별 캐시 적중률
SELECT tool_name,
       countIf(cache_hit) / count() as hit_rate,
       avg(latency_ms) as avg_latency
FROM mcp_tool_calls
WHERE success = true
GROUP BY tool_name
ORDER BY hit_rate DESC

-- 서버별 에러율 (circuit breaker 판단 보조)
SELECT mcp_server_id,
       countIf(is_error or timed_out) / count() as error_rate
FROM mcp_tool_calls
WHERE timestamp >= now() - INTERVAL 5 MINUTE
GROUP BY mcp_server_id
HAVING error_rate > 0.5
```

### 전송 방식

**기존 `analytics_repo` 파이프라인 사용** (별도 ClickHouse 연결 불필요)

```
McpBridgeAdapter
  → state.analytics_repo.emit("mcp.tool_call", attrs)  (fire-and-forget)
  → OTel LogRecord → OTel Collector → Redpanda [otel-logs] → ClickHouse

analytics_repo: Option<Arc<dyn AnalyticsRepository>> — AppState에 이미 존재
None이면 skip (analytics 비활성 환경에서도 추론 흐름 영향 없음)
```

**이벤트 속성 매핑:**
```rust
// OTel attribute keys
"mcp.api_key_id"     → api_key.id
"mcp.server_id"      → mcp_server_id
"mcp.tool_name"      → tool_name
"mcp.args_hash"      → args_hash
"mcp.cache_hit"      → cache_hit (bool)
"mcp.success"        → success (bool)
"mcp.is_error"       → is_mcp_error (bool)
"mcp.timed_out"      → timed_out (bool)
"mcp.circuit_open"   → circuit_open (bool)
"mcp.latency_ms"     → latency_ms (u32)
"mcp.cap_consumed"   → 0 or 1
"mcp.cap_remaining"  → remaining (u8)
```

---

## 역할 분리 (N 레플리카 환경)

| 역할 | 담당 | 근거 |
|------|------|------|
| MCP 서버 health check | **veronex-agent** | 샤딩으로 N agent → M 서버 분산, 중복 없음 |
| API 요청 시 MCP 직접 호출 | **Veronex** | 추론 실시간 흐름, agent 경유 시 latency 증가 |
| result cache / routing cache | **Veronex** (Valkey 공유) | N 레플리카 자동 공유 |
| tool schema 갱신 | **Veronex** (SET NX) | 단일 레플리카만 실제 갱신, 나머지는 Valkey 읽기 |
| MCP 세션 | **Veronex** 레플리카별 독립 | Mcp-Session-Id는 공유 불가 |

### veronex-agent MCP Health Check

```
crates/veronex-agent/src/scraper.rs

scrape_mcp_health(server: McpServer):
  // HTTP health endpoint
  GET {server.url}/health  timeout: 5s
    → 200: continue

  // JSON-RPC ping (application-level liveness)
  POST {server.url}
    { "jsonrpc": "2.0", "id": 99, "method": "ping" }
    → result: {}  → online
    → 5xx / timeout → offline

  결과:
    online  → SETEX veronex:mcp:heartbeat:{mcp_server_id} 180 "1"
    offline → TTL 자연 만료 → mcp_servers.status = 'offline'
              Valkey DEL veronex:mcp:tools:{mcp_server_id}
              McpToolCache DashMap.remove(mcp_server_id)

  McpBridgeAdapter 조회:
    tool_cache.get_all() 시 heartbeat key 존재 여부로 online 판단
    EXISTS veronex:mcp:heartbeat:{id} → 0이면 해당 서버 tool 제외

샤딩:
  shard_key() 에 "mcp" 분기 추가
  → N agent가 M MCP 서버를 균등 분산 (기존 Ollama 서버 샤딩 동일 패턴)
```

---

## API

### MCP 서버 관리 (`mcp_handlers.rs`)

```
POST   /v1/mcp/servers                → 등록
GET    /v1/mcp/servers                → 목록 (paginated, ListPageParams)
PATCH  /v1/mcp/servers/{id}          → 수정
DELETE /v1/mcp/servers/{id}          → 비활성화
GET    /v1/mcp/servers/{id}/tools    → 캐시된 tool schema 조회 (annotations 포함)
POST   /v1/mcp/servers/sync          → tool schema 강제 갱신
```

**등록 요청:**
```json
// 캐싱 가능 (날씨, 좌표)
{
  "name": "weather-mcp",
  "url": "http://weather-mcp:3000/mcp",
  "transport": "streamable_http",
  "cache_ttl_secs": 600
}

// 캐싱 불가 (실시간 주가)
{
  "name": "stock-mcp",
  "url": "http://stock-mcp:3000/mcp",
  "transport": "streamable_http",
  "cache_ttl_secs": null
}
```

> `cache_ttl_secs` 설정 시에도 `readOnlyHint AND idempotentHint` 조건 미충족 tool은 캐싱 안 함.

### API Key Capabilities (`key_capability_handlers.rs`)

```
GET    /v1/keys/{id}/capabilities
PUT    /v1/keys/{id}/capabilities/mcp_access   → { "cap_points": 3 }
DELETE /v1/keys/{id}/capabilities/mcp_access
```

### 글로벌 MCP 설정 (`/v1/mcp/settings`)

```
GET   /v1/mcp/settings
PATCH /v1/mcp/settings
```

```json
{
  "routing_cache_ttl_secs": 3600,
  "tool_schema_refresh_secs": 30,
  "embedding_model": "nomic-embed-text",
  "max_tools_per_request": 32,
  "max_routing_cache_entries": 200
}
```

- `routing_cache_ttl_secs`: DB 초 단위, UI 시·분·초 입력
- `tool_schema_refresh_secs`: DB 초 단위, UI 시·분·초 입력
- `embedding_model`: Ollama에서 사용 가능한 embedding 모델
- `max_tools_per_request`: LLM에 주입할 최대 tool 수 (context window 보호)
- `max_routing_cache_entries`: cosine 계산 대상 상한 (메모리/성능 제어)

### Metrics Target Discovery 확장

```
GET /v1/metrics/targets
  → { targets: ["{mcp_url}/health"], labels: { type: "mcp", id: "{id}" } }
```

---

## TTL 설정 전체 정리

| 설정 | 위치 | DB 타입 | UI 입력 | 관리자 설정 |
|------|------|---------|---------|-----------|
| result cache TTL | `mcp_servers.cache_ttl_secs` | `INT` (초) | 시·분·초 또는 "설정 안 함" | 서버별 |
| routing cache TTL | `mcp_settings.routing_cache_ttl_secs` | `INT` (초) | 시·분·초 | 글로벌 |
| tool schema 갱신 주기 | `mcp_settings.tool_schema_refresh_secs` | `INT` (초) | 시·분·초 | 글로벌 |

**UI 입력 → 초 변환:**
```
1시간 30분  → 5400
10분        → 600
24시간      → 86400
설정 안 함  → NULL (캐싱 비활성)
```

---

## 에러 처리

| 상황 | 처리 |
|------|------|
| `isError: true` (tool 실행 실패) | tool_result로 LLM에 전달, LLM이 판단. **cap 미차감**. 동일 에러 3회 → 탈출 |
| JSON-RPC protocol error | 해당 MCP 서버 circuit open, 세션 재초기화 |
| MCP 세션 만료 (404) | **Mcp-Session-Id 헤더 제거** 후 새 InitializeRequest POST → 재시도 |
| MCP 서버 offline (ping 실패) | 해당 서버 tool 제외, 나머지로 계속 |
| Circuit breaker open | 연속 5회 실패 → open → health check 통과 전까지 해당 서버 tool 제외 |
| Per-call timeout (30s) | tool_result에 timeout 메시지 → LLM 판단 |
| 루프 감지 (동일 signature 3회) | 시스템 프롬프트로 최종 답변 강제 탈출 |
| cap_points 소진 | 시스템 프롬프트로 최종 답변 강제 후 반환 |
| 클라이언트 연결 끊김 | upstream MCP 호출 **자동 취소 안 함** (명시적 CancelledNotification 시만) |
| embedding 실패 | routing cache 건너뜀, 전체 tool 주입 fallback |
| tool name 충돌 | namespace 강제 (`mcp_{server}_{tool}`) |
| notifications/tools/list_changed | 해당 서버 DashMap 즉시 무효화, 다음 요청 시 재조회 |

### Circuit Breaker

```
infrastructure/outbound/mcp/circuit_breaker.rs

  주의: AppState에 기존 circuit_breaker: Arc<CircuitBreakerMap> 존재 (Ollama용)
  MCP용은 별도 타입 McpCircuitBreaker, AppState 필드명: mcp_circuit_breaker

  DashMap<McpServerId, McpCircuitState>

  States: Closed → Open → HalfOpen → Closed
    Closed:   정상 운영
    Open:     연속 5회 실패 → tool 건너뜀, health check만 허용
    HalfOpen: 60s 후 probe 1회 → 성공 시 Closed, 실패 시 Open 유지
```

---

## UI

```
/providers?s=mcp  (lab gate: mcp_integration)
  ├── MCP 서버 목록
  │     컬럼: 이름 / URL / status / tool 수 / result cache TTL
  │
  ├── 서버 등록/수정 폼
  │     이름, URL (streamable_http 엔드포인트)
  │     Result Cache TTL:
  │       [ ] 설정 안 함  (NULL → 항상 MCP 직접 호출)
  │       [●] 직접 설정  → [시간 __] [분 __] [초 __]
  │
  ├── tool 목록 보기 (캐시된 schema + annotations)
  │     readOnly / idempotent 배지 표시
  ├── 수동 sync 버튼
  │
  └── 글로벌 MCP 설정
        routing cache TTL: [시간 __] [분 __] [초 __]
        tool schema 갱신:  [시간 __] [분 __] [초 __]
        embedding 모델:    [select]

/keys → 키 상세 → Capabilities 섹션
  ├── mcp_access 토글
  └── cap_points 입력 (0~10, 기본 1)
        0: 비활성 (테스트용)
        1: 단순 단일 tool
        3: 날씨/검색 체인
        5: 주식 분석 등 복잡 체인
```

---

## 클라이언트 설정 예시

```bash
# Cursor / Codex CLI / 일반 앱
OPENAI_BASE_URL=http://veronex/v1
OPENAI_API_KEY=vnx_xxxx   # mcp_access capability + cap_points >= 2

# MCP 관련 추가 설정 불필요
# Veronex가 내부에서 전부 처리
```

---

## 파일 목록

| 파일 | 역할 |
|------|------|
**Domain / Application**

| 파일 | 역할 |
|------|------|
| `domain/entities/mcp_server.rs` | McpServer 엔티티 (annotations 포함) |
| `domain/entities/api_key_capability.rs` | ApiKeyCapability 엔티티 (cap_points) |
| `domain/enums.rs` | KeyCapability enum 추가 |
| `application/ports/outbound/mcp_server_repository.rs` | McpServerRepository trait |
| `application/ports/outbound/mcp_settings_repository.rs` | McpSettingsRepository trait |
| `application/ports/outbound/api_key_capability_repository.rs` | ApiKeyCapabilityRepository trait |

**Infrastructure — Persistence**

| 파일 | 역할 |
|------|------|
| `infrastructure/outbound/persistence/mcp_server_repository.rs` | PostgresMcpServerRepository impl |
| `infrastructure/outbound/persistence/mcp_settings_repository.rs` | PostgresMcpSettingsRepository impl |
| `infrastructure/outbound/persistence/api_key_capability_repository.rs` | PostgresApiKeyCapabilityRepository impl |

**veronex-mcp 크레이트** (`crates/veronex-mcp/src/`) ← **Phase 1 구현 완료**

| 파일 | 역할 |
|------|------|
| `session.rs` | McpSessionManager (Mcp-Session-Id, 404 재초기화, call_tool 편의 메서드) |
| `client.rs` | McpHttpClient (Streamable HTTP 2025-03-26, initialize/ping/list_tools/call_tool) |
| `tool_cache.rs` | McpToolCache (DashMap L1 + Valkey L2, server_id 역매핑, 상한 32, get_tool_raw/all_namespaced_names) |
| `result_cache.rs` | McpResultCache (SHA256 canonical JSON 키, annotations 체크) |
| `circuit_breaker.rs` | McpCircuitBreaker (5회 → open → 60s HalfOpen, sync API) |
| `types.rs` | McpTool, McpToolCall, McpToolResult, McpContent |
| `bin/weather.rs` | weather-mcp 예제 서버 (open-meteo.com, get_coordinates/get_weather) |

> routing_cache (Rust cosine): Phase 2에서 추가 예정

**Infrastructure — MCP Outbound** (`crates/veronex/src/infrastructure/outbound/mcp/`) ← **Phase 1 구현 완료**

| 파일 | 역할 |
|------|------|
| `bridge.rs` | McpBridgeAdapter (agentic loop max 5라운드, join_all 순서 보존, 루프 감지, stream=true 지원) |

**AppState 추가 필드 (Phase 1):**
```rust
// state.rs
pub mcp_bridge: Option<Arc<McpBridgeAdapter>>,
// None = MCP 비활성화 (기본값). 서버 등록 시 Some으로 교체.
```

**Infrastructure — HTTP Inbound**

| 파일 | 역할 | 상태 |
|------|------|------|
| `openai_handlers.rs` | chat_completions에서 mcp_ollama_chat 분기 추가 | ✅ Phase 1 완료 |
| `mcp_handlers.rs` | MCP 서버 CRUD API | Phase 2 |

**Bootstrap / Background Tasks**

| 파일 | 역할 | 상태 |
|------|------|------|
| `crates/veronex-agent/src/scraper.rs` | ping_mcp_server(), set_mcp_heartbeat() | ✅ Phase 1 완료 |
| `crates/veronex-agent/src/main.rs` | MCP_SERVERS env var, scrape_cycle 통합 | ✅ Phase 1 완료 |
| `bootstrap/background.rs` | McpSseListenerTask (list_changed 수신 → L1 무효화) | Phase 2 |

**Background Tasks 상세:**
```
McpSseListenerTask: 각 MCP 서버에 GET /mcp SSE 스트림 유지
  → notifications/tools/list_changed 수신 → tool_cache.invalidate()
  → 연결 끊김: Last-Event-ID 재연결 → 실패 시 30s polling fallback

McpAnalyticsTask: 불필요 — state.analytics_repo 직접 사용 (fire-and-forget)
```

**Migrations**

| 파일 | 역할 |
|------|------|
| `migrations/postgres/000011_mcp_capabilities.up.sql` | mcp_servers, mcp_server_tools, mcp_key_access 테이블 + GIN index |
| `migrations/postgres/000011_mcp_capabilities.down.sql` | 위 테이블 DROP |
| `migrations/clickhouse/000003_mcp_tool_calls.up.sql` | mcp_tool_calls MergeTree + mcp_tool_calls_hourly AggregatingMergeTree + Materialized View |
| `migrations/clickhouse/000003_mcp_tool_calls.down.sql` | 위 테이블/뷰 DROP |

---

## 확장성 (Scale-Out)

### 1M+ TPS 설계 적합성

| 컴포넌트 | 특성 | 판단 |
|---------|------|------|
| McpToolCache DashMap | O(1) read, 레플리카 독립 | ✅ |
| McpResultCache Valkey | 클러스터 호환 키 설계 | ✅ 클러스터 전환 시 무중단 |
| join_all 병렬 실행 | MCP 서버 응답 대기만 | ✅ Veronex CPU 미점유 |
| McpRoutingCache cosine | 200개 × f32 = μs | ✅ |
| McpAnalyticsTask | mpsc 채널 버퍼링 | ✅ 채널 full 시 drop |
| Circuit Breaker | DashMap O(1) | ✅ |
| **Ollama 추론** | GPU VRAM 제한 | ⚠️ 기존 capacity scheduler 영역 |
| **MCP 서버** | 외부 HTTP 서버 | ⚠️ Veronex 제어 불가 |

**결론**: Veronex MCP 레이어 자체는 레플리카 확장으로 선형 스케일. 실제 병목은 Ollama GPU와 MCP 서버.

### Valkey 클러스터 키 설계

```
# result cache: server_id 기준 슬롯 → 동일 서버 결과 같은 슬롯
veronex:mcp:result:{server_id}:{tool_name}:{args_hash}

# heartbeat: server_id 기준
veronex:mcp:heartbeat:{mcp_server_id}

# tool schema: server_id 기준
veronex:mcp:tools:{server_id}
veronex:mcp:tools:lock:{server_id}   TTL: tool_schema_refresh_secs + 5s

# routing cache: 글로벌 hash
veronex:mcp:routes   (HSET)
```

---

## Out of Scope (v1)

- stdio MCP 서버 (로컬 프로세스 스폰) — Streamable HTTP only
- 2024-11-05 레거시 SSE 트랜스포트 — 신규 서버는 2025-03-26만
- MCP 서버 인증 (OAuth 2.1) — plaintext URL only
- Gemini 프로바이더 MCP 지원 — Ollama only
- Plan-then-Execute 시스템 프롬프트 — v2
- MCP tool 의존성 그래프 명시 — v2
- Sampling capability (MCP → Veronex LLM 역호출) — v2, initialize에서 의도적 미선언
- Resources / Prompts / Completions primitives — v2
- CancelledNotification upstream 전달 — v2 (v1: 클라이언트 연결 끊겨도 upstream 유지)
