# Jobs — Lifecycle, Tracking & Token Observability

> SSOT | **Last Updated**: 2026-02-26

## Job Lifecycle

```
Client → POST /v1/inference  (API Key 인증)
  → InferenceUseCaseImpl::submit()
  → InferenceJob 생성 (status=Pending, api_key_id 설정)
  → Valkey RPUSH inferq:queue
  → return job_id

Client → GET /v1/inference/{id}/stream (SSE)
  → stream buffer + tokio Notify (polling 없음)

queue_dispatcher_loop (BLPOP):
  → DynamicBackendRouter::dispatch()
  → run_job() (started_at 기록)
  → OllamaAdapter | GeminiAdapter → SSE tokens
  → 완료: status=Completed, completed_at, latency_ms, ttft_ms, completion_tokens, result_text 저장
  → ObservabilityPort::record_inference() → ClickHouse inference_logs
```

## Entities

### InferenceJob (DB)

```rust
pub struct InferenceJob {
    pub id: Uuid,
    pub model_name: String,
    pub backend: BackendType,
    pub status: JobStatus,                 // Pending | Running | Completed | Failed | Cancelled
    pub prompt: String,                    // 전체 저장
    pub result_text: Option<String>,
    pub error: Option<String>,
    pub api_key_id: Option<Uuid>,          // FK → api_keys (ON DELETE SET NULL)
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>, // 큐 대기 제외, 순수 추론 시작
    pub completed_at: Option<DateTime<Utc>>,
    pub latency_ms: Option<i32>,           // started_at → completed_at (ms)
    pub ttft_ms: Option<i32>,              // started_at → 첫 토큰 수신 (ms)
    pub completion_tokens: Option<i32>,    // 생성된 완성 토큰 수
}
```

> `latency_ms` = 순수 추론 시간 (큐 대기 제외)
> `created_at - started_at` = 큐 대기 시간
> `ttft_ms` = Time To First Token (TTFT); 프리필 + 네트워크 포함
> `tps` = `completion_tokens / (latency_ms - ttft_ms) * 1000` (API 응답에서 계산, DB 미저장)

## DB Schema

```sql
CREATE TABLE inference_jobs (
    id                UUID         PRIMARY KEY,
    model_name        VARCHAR(255) NOT NULL,
    backend           VARCHAR(50)  NOT NULL,
    status            VARCHAR(20)  NOT NULL DEFAULT 'pending',
    prompt            TEXT         NOT NULL DEFAULT '',
    result_text       TEXT,
    error             TEXT,
    api_key_id        UUID         REFERENCES api_keys(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT now(),
    started_at        TIMESTAMPTZ,
    completed_at      TIMESTAMPTZ,
    latency_ms        INTEGER,
    ttft_ms           INTEGER,       -- migration 000020
    completion_tokens INTEGER        -- migration 000020
);
-- migrations: 000004 result_text, 000014 api_key_id, 000015 latency_ms, 000020 ttft_ms+completion_tokens
```

## JobRepository 패턴

- `save()` = UPSERT (ON CONFLICT id DO UPDATE — status, result_text, error, timestamps, latency_ms, ttft_ms, completion_tokens)
- `get_status()` = 인메모리 맵 우선, 없으면 DB fallback
- `stream()` = 토큰 버퍼 인덱스 + tokio Notify (polling 없음, broadcast channel 없음)

---

## Token Observability

### StreamToken (value_objects.rs)

```rust
pub struct StreamToken {
    pub value: String,
    pub is_final: bool,
    pub prompt_tokens: Option<u32>,      // 마지막 토큰에만 설정
    pub completion_tokens: Option<u32>,  // None → 백엔드 미제공
}
```

### Gemini usageMetadata 추출

Gemini SSE 마지막 청크(`finishReason: "STOP"`)에만 `usageMetadata` 포함:

```json
{
  "usageMetadata": {
    "promptTokenCount": 12,
    "candidatesTokenCount": 85,
    "totalTokenCount": 97
  }
}
```

`gemini/adapter.rs` `extract_usage()`:

```rust
fn extract_usage(resp: &GenerateResponse) -> (Option<u32>, Option<u32>) {
    let u = resp.usage_metadata.as_ref();
    (u.and_then(|u| u.prompt_token_count), u.and_then(|u| u.candidates_token_count))
}
// done=true 청크에서 → StreamToken { prompt_tokens, completion_tokens }
```

### run_job() 토큰 처리

```
ttft_ms_value: Option<i32> = None

while token_stream.next():
    if token.prompt_tokens.is_some():
        actual_prompt_tokens = token.prompt_tokens
        actual_completion_tokens = token.completion_tokens

    // TTFT: 첫 번째 비어있지 않은 non-final 토큰 수신 시점 기록
    if ttft_ms_value.is_none() && !token.is_final && !token.value.is_empty():
        ttft_ms_value = now() - started_at

완료 저장:
    latency_ms        = completed_at - started_at
    ttft_ms           = ttft_ms_value
    completion_tokens = actual_completion_tokens ?? token_count

emit_inference_event(
    prompt_tokens     = actual_prompt_tokens.unwrap_or(0),
    completion_tokens = actual_completion_tokens.unwrap_or(token_count),
)
```

- **Gemini**: `usageMetadata`에서 실제 수치 사용
- **Ollama**: SSE 이벤트 수 폴백 (스트림에서 실제 토큰 수 미제공)

### ClickHouse inference_logs

```sql
CREATE TABLE inference_logs (
    job_id           String,
    api_key_id       Nullable(String),
    model_name       String,
    backend          String,
    status           String,
    prompt_tokens    UInt32,
    completion_tokens UInt32,
    latency_ms       Int32,
    created_at       DateTime
) ENGINE = MergeTree() ORDER BY created_at;
```

---

## API Endpoints

```
GET /v1/dashboard/stats
    → { total_keys, active_keys, total_jobs, jobs_last_24h, jobs_by_status }

GET /v1/dashboard/jobs?limit=&offset=&status=&q=
    q      → prompt ILIKE '%{q}%'
    status → all | pending | running | completed | failed | cancelled
    → { total: i64, jobs: Vec<JobSummary> }

GET /v1/dashboard/jobs/{id}
    → JobDetail

GET /v1/usage?hours=
    → UsageAggregate (전체 키 합산, ClickHouse)

GET /v1/usage/{key_id}?hours=
    → Vec<HourlyUsage> (키별 시간대 breakdown)

GET /v1/dashboard/performance?hours=
    → PerformanceStats { avg/p50/p95/p99 latency, success_rate, total_tokens, hourly }
```

### JobSummary (list)

```rust
pub struct JobSummary {
    pub id: String,
    pub model_name: String,
    pub backend: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub latency_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub tps: Option<f64>,               // completion_tokens / gen_ms * 1000 (소수점 1자리)
    pub api_key_name: Option<String>,   // LEFT JOIN api_keys
}
```

> `tps` = `completion_tokens / (latency_ms - ttft_ms) * 1000` (TTFT/프리필 제외 생성 속도)

### JobDetail (단건)

```rust
pub struct JobDetail {
    pub id: String,
    pub model_name: String,
    pub backend: String,
    pub status: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub latency_ms: Option<i64>,
    pub ttft_ms: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub tps: Option<f64>,
    pub api_key_name: Option<String>,
    pub prompt: String,
    pub result_text: Option<String>,
    pub error: Option<String>,
}
```

---

## Web UI (admin `/jobs`)

### 테이블 컬럼

```
[Search prompt…]  [Status: All ▼]

┌──────────────────────────────────────────────────────────────────────┐
│ ID      Model    Backend  API Key   Status     Created   TTFT  Latency│
│ 3a9f…  llama3   gpu-1    dev-key   ✓complete  Feb 25   142ms  1.2s  │
│ 8b2c…  gemini   cloud    prod-key  ✓complete  Feb 25   380ms  2m 3s │
└──────────────────────────────────────────────────────────────────────┘
```

**Latency / TTFT 포맷**: `formatDuration(ms)`
- `< 1000ms` → `"842ms"`
- `1000ms ~ 60s` → `"2.3s"`
- `≥ 60s` → `"2m 5s"`

### Job Detail 모달

```
┌─ 3a9fbcd… · ✓completed ─────────────────────────────────────────────┐
│ llama3 · gpu-ollama-1                                                 │
│ Created: Feb 25, 14:32  Started: 14:32  Completed: 14:32             │
│ Latency: 1.2s  TTFT: 142ms  TPS: 44.3 tok/s                         │
│ Tokens: 53  API Key: dev-key                                          │
├───────────────────────────────────────────────────────────────────────┤
│ PROMPT                                                                │
│ 한국어로 간단한 인사말을 작성해줘                                         │
├───────────────────────────────────────────────────────────────────────┤
│ RESULT                                                                │
│ 안녕하세요! 반갑습니다.                                                  │
└───────────────────────────────────────────────────────────────────────┘
```

- TanStack Query `queryKey: ['job-detail', jobId]`, `enabled: !!jobId && open`
- Prompt/Result: `pre` + monospace + `whitespace-pre-wrap` + `max-h-52 overflow-y-auto`

### Inference Test (/api-test)

- 모델 목록 staleTime: **10분** (매 진입마다 API 호출 방지)
- SSE 파싱: `data:` 뒤 공백 1개만 제거 (`trimStart()` 제거로 토큰 내 공백 보존)
- 출력: `whitespace-pre-wrap` — 줄바꿈/띄어쓰기 원문 그대로 렌더링

---

## DB 장기 관리

```sql
-- PostgreSQL: pg_cron으로 90일 TTL
SELECT cron.schedule('0 3 * * *',
  'DELETE FROM inference_jobs WHERE created_at < now() - interval ''90 days''');

-- ClickHouse: 학습 데이터 보관 (선택)
CREATE TABLE training_samples (
    id            UUID,
    model_name    String,
    prompt        String   CODEC(ZSTD(3)),
    result_text   String   CODEC(ZSTD(3)),
    quality_score Nullable(Float32),
    created_at    DateTime
) ENGINE = MergeTree()
  PARTITION BY toYYYYMM(created_at)
  ORDER BY created_at;
```

---

## Gemini 스트리밍 버그 수정 이력

**증상**: `result_text = null` (결과가 저장되지 않음)

**원인**: 마지막 SSE 이벤트(`finishReason: "STOP"`)가 trailing `\n` 없이 HTTP body가 끝나
`buf`에 미처리 잔류

**수정 (`gemini/adapter.rs`)**:
1. 바이트 스트림 종료 후 `buf` 잔여 내용 플러시
2. JSON 파싱 실패 시 에러 대신 `warn` 로그 후 skip
3. 최종 센티널 `StreamToken { value: "", is_final: true }` 항상 보장
