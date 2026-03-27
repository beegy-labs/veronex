# Job Write Pipeline

> CDD Layer 2 | **Last Updated**: 2026-03-26

## 개요

`inference_jobs` 쓰기를 최소화한 심플 아키텍처.
Postgres에는 메타데이터만, 대용량 콘텐츠(prompt, messages, tool_calls, result)는 S3에 저장.

---

## 설계 원칙

| 원칙 | 내용 |
|------|------|
| **2회 쓰기** | `save()` = 초기 INSERT, `finalize()` = 단일 terminal UPDATE. 완료된 job당 Postgres 쓰기 2회 |
| **S3 콘텐츠** | ConversationRecord (prompt + messages + tool_calls + result) → zstd-3 압축 JSON, S3 1회 PUT |
| **조기 종료** | `cancel_job()` / `fail_with_reason()` 는 early-exit 전용 (큐 가득 참, 스트림 오류) |
| **단일 진실** | Postgres = 메타데이터 SSOT, S3 = 콘텐츠 SSOT |

---

## JobRepository 호출 매핑

| 호출 위치 | 메서드 | 대상 |
|-----------|--------|------|
| `submit()` | `save()` | Postgres INSERT (동기) |
| `submit()` 큐 가득 참 | `fail_with_reason()` | Postgres UPDATE (동기) |
| `finalize_job()` | S3 PUT + `finalize()` | S3 (non-fatal) + Postgres UPDATE |
| `cancel()` | `cancel_job()` | Postgres UPDATE (동기) |
| `handle_stream_error()` | `fail_with_reason()` | Postgres UPDATE (동기) |
| 재시작 복구 | `list_pending()` | Postgres SELECT |
| 상태 조회 miss | `get()` | Postgres SELECT |

---

## S3 ConversationRecord

키 패턴: `conversations/{owner_id}/{YYYY-MM-DD}/{job_id}.json.zst`

```rust
pub struct ConversationRecord {
    pub prompt: String,
    pub messages: Option<serde_json::Value>,   // 전체 대화 컨텍스트
    pub tool_calls: Option<serde_json::Value>, // MCP + OpenAI function calls 모두 포함
    pub result: Option<String>,                // 최종 텍스트 출력
}
```

- `owner_id = account_id ?? api_key_id ?? job_id`
- zstd-3 압축 (~1.2 KB / record, 원본 대비 ~8–11x)
- `finalize_job()` 시점에 1회 PUT (non-fatal — S3 실패 시 경고 후 계속)
- 어드민 상세 뷰에서 클릭 시 1회 GET (~20–50ms)

---

## `finalize()` 파라미터

```rust
async fn finalize(
    job_id, started_at, completed_at,
    provider_id, queue_time_ms,
    latency_ms, ttft_ms,
    prompt_tokens, completion_tokens, cached_tokens,
    has_tool_calls: bool,   // S3에 tool_calls 있으면 true → 목록 뷰 표시용
) -> Result<()>
```

---

## Postgres 컬럼 (inference_jobs)

메타데이터만 유지 — 대용량 컬럼 제거:

| 제거 | 대체 |
|------|------|
| `prompt` (전체) | `prompt_preview VARCHAR(200)` (목록/검색용) |
| `result_text` | S3 `ConversationRecord.result` |
| `messages_json` | S3 `ConversationRecord.messages` |
| `tool_calls_json` | S3 `ConversationRecord.tool_calls` + `has_tool_calls BOOLEAN` |

마이그레이션: `crates/veronex/migrations/0000000002_s3_conversation_store.sql`

---

## 환경 변수

| 변수 | 기본값 | 용도 |
|------|--------|------|
| `S3_ENDPOINT` | 필수 | MinIO/S3 엔드포인트 |
| `S3_BUCKET` | `veronex` | 버킷 이름 |
| `S3_ACCESS_KEY` | 필수 | 자격 증명 |
| `S3_SECRET_KEY` | 필수 | 자격 증명 |

---

## 키 파일

| 파일 | 역할 |
|------|------|
| `crates/veronex/src/application/ports/outbound/job_repository.rs` | `JobRepository` 트레이트 — `save()`, `finalize()` 정의 |
| `crates/veronex/src/application/ports/outbound/message_store.rs` | `MessageStore` 트레이트 — `ConversationRecord`, S3 read/write |
| `crates/veronex/src/infrastructure/outbound/persistence/job_repository.rs` | `PostgresJobRepository` 구현 |
| `crates/veronex/src/infrastructure/outbound/s3/message_store.rs` | `S3MessageStore` 구현 (zstd-3) |
| `crates/veronex/src/application/use_cases/inference/runner.rs` | `finalize_job()` — S3 PUT + DB finalize 호출 위치 |
| `crates/veronex/migrations/0000000002_s3_conversation_store.sql` | Postgres 마이그레이션 |

---

## 태스크 가이드

| 태스크 | 파일 |
|--------|------|
| ConversationRecord 필드 추가 | `message_store.rs` struct + S3 impl |
| finalize 메트릭 추가 | `job_repository.rs` 포트 + 인프라 + `runner.rs` 호출부 |
| 검색 범위 변경 | `dashboard_queries.rs` `fetch_jobs` — `prompt_preview ILIKE` |
| S3 키 패턴 변경 | `s3/message_store.rs` `key()` 함수 |
| 전체 플로우 | `docs/llm/flows/job-event-pipeline.md` 참조 |
