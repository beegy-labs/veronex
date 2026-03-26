# Job Event Pipeline (Redpanda)

> CDD Layer 2 | **Last Updated**: 2026-03-26

## 개요

`inference_jobs` 상태 전환 쓰기를 Postgres critical path에서 완전히 제거하기 위한 아키텍처.
`save()` 단 하나만 동기 INSERT로 남기고, 이후 모든 상태 전환은 Redpanda 이벤트로 교체.

---

## 설계 원칙

| 원칙 | 내용 |
|------|------|
| **단일 쓰기점** | `save()` = 유일한 INSERT. Kafka CDC 앵커. row가 존재해야 이후 UPDATE가 타겟을 찾을 수 있음 |
| **상태전환 비동기화** | `mark_running`, `mark_completed`, `fail_with_reason`, `cancel_job`, `update_image_keys` → fire-and-forget Redpanda produce |
| **At-least-once** | veronex-worker는 bulk UPDATE 성공 후에만 offset commit. 재배달 시 UPDATE 중복 실행되지만 idempotent |
| **Fallback** | Redpanda 접속 실패 시 Postgres 직접 쓰기로 자동 fallback (서비스 중단 없음) |

---

## Topic 목록

| Topic | Producer | Consumer | 내용 |
|-------|----------|----------|------|
| `veronex.job.events` | `KafkaJobRepository` | `veronex-worker` db_writer | 모든 상태 전환 이벤트 |

---

## JobEvent 스키마

`domain/events.rs`에 정의. serde tagged union (`"type"` 필드로 구분).

```json
// Running
{"type":"running","job_id":"...","started_at":"...","provider_id":"...","queue_time_ms":12}

// Completed
{"type":"completed","job_id":"...","completed_at":"...","result_text":"...","latency_ms":320,...}

// Failed
{"type":"failed","job_id":"...","reason":"provider_error","error_msg":"..."}

// Cancelled
{"type":"cancelled","job_id":"...","cancelled_at":"..."}

// ImageKeysUpdated
{"type":"image_keys_updated","job_id":"...","image_keys":["images/..."]}
```

---

## KafkaJobRepository

`infrastructure/outbound/kafka/job_repository.rs`

```
save()         → inner.save() (Postgres INSERT, 동기)
mark_running() → produce(JobEvent::Running)    ─┐
mark_completed()→ produce(JobEvent::Completed) ─┤ fire-and-forget
fail_with_reason()→ produce(JobEvent::Failed)  ─┤ (tokio::spawn)
cancel_job()   → produce(JobEvent::Cancelled)  ─┤
update_image_keys()→ produce(JobEvent::ImageKeysUpdated)─┘

get()          → inner.get()          (동기, read)
list_pending() → inner.list_pending() (동기, 시작 시 복구용)
update_status()→ inner.update_status()(동기, 시작 시 복구용)
```

### Producer 설정

| 설정 | 값 | 이유 |
|------|----|------|
| `message.timeout.ms` | 5000 | 미배달 메시지 5초 후 포기 |
| `queue.buffering.max.messages` | 1,000,000 | 로컬 링 버퍼 (1M 메시지) |
| `queue.buffering.max.ms` | 5 | 5ms 배치 리거 (네트워크 왕복 분산) |
| `compression.type` | snappy | 네트워크 오버헤드 감소 |

---

## veronex-worker DB Writer

`crates/veronex-worker/src/db_writer.rs`

### 배치 전략

1. `StreamConsumer`로 `veronex.job.events` 구독
2. **50ms 또는 256개** 단위로 메시지 수집 (먼저 도달하는 조건 기준)
3. 이벤트 타입별 분류 → `tokio::try_join!`으로 bulk UPDATE 동시 실행
4. 전부 성공 시 offset commit (at-least-once)

### Bulk UPDATE 방식

상태별로 `unnest()` 배열 UPDATE 사용 (individual UPDATE × N 대비 ~10× 효율):

```sql
-- mark_running 예시
UPDATE inference_jobs AS j
SET status = 'running',
    started_at = v.started_at,
    provider_id = v.provider_id,
    queue_time_ms = v.queue_time_ms
FROM (
    SELECT unnest($1::uuid[])        AS id,
           unnest($2::timestamptz[]) AS started_at,
           unnest($3::uuid[])        AS provider_id,
           unnest($4::int4[])        AS queue_time_ms
) AS v
WHERE j.id = v.id
```

`image_keys`만 row-by-row UPDATE (text[][] unnest는 복잡하고 볼륨이 낮음).

---

## 환경 변수

| 변수 | 기본값 | 대상 |
|------|--------|------|
| `KAFKA_BROKER` | `redpanda:9092` | veronex (producer) |
| `DATABASE_URL` | 필수 | veronex-worker |
| `KAFKA_BROKER` | `redpanda:9092` | veronex-worker |
| `KAFKA_CONSUMER_GROUP` | `veronex-worker-db-writer` | veronex-worker |

---

## 키 파일

| 파일 | 역할 |
|------|------|
| `crates/veronex/src/domain/events.rs` | `JobEvent` enum — Redpanda 메시지 스키마 SSOT |
| `crates/veronex/src/infrastructure/outbound/kafka/job_repository.rs` | `KafkaJobRepository` — 이벤트 produce |
| `crates/veronex/src/bootstrap/repositories.rs` | `KAFKA_BROKER`로 wiring, 실패 시 Postgres fallback |
| `crates/veronex-worker/src/db_writer.rs` | 컨슈머 — 배치 수집 + bulk unnest UPDATE |
| `crates/veronex-worker/src/main.rs` | 워커 바이너리 진입점 |

---

## 태스크 가이드

| 태스크 | 파일 |
|--------|------|
| 새 상태 전환 추가 | `domain/events.rs` 새 variant + `kafka/job_repository.rs` + `db_writer.rs` 파티셔닝 + bulk UPDATE |
| 배치 크기 조정 | `db_writer.rs` `BATCH_SIZE` / `BATCH_TIMEOUT` 상수 |
| 프로듀서 튜닝 | `kafka/job_repository.rs` `ClientConfig` 블록 |
| 컨슈머 그룹 추가 | 새 `KAFKA_CONSUMER_GROUP` + 별도 소비 로직 |
| Job lifecycle 전체 흐름 | `docs/llm/flows/job-event-pipeline.md` 참조 |
