# Job Event Pipeline — 전체 플로우

> **Last Updated**: 2026-03-26

---

## 전체 아키텍처

```mermaid
flowchart TB
    subgraph CLIENT["Client"]
        REQ["POST /v1/chat/completions"]
    end

    subgraph VERONEX["veronex (API 서버)"]
        direction TB
        UC["InferenceUseCase"]

        subgraph REPO["KafkaJobRepository"]
            SAVE["save()\n동기 INSERT"]
            PRODUCE["produce()\nfire-and-forget\ntokio::spawn"]
        end

        DM["DashMap\n(in-memory)"]
        ZSET["Valkey ZSET\n(priority queue)"]
    end

    subgraph REDPANDA["Redpanda"]
        TOPIC["veronex.job.events"]
    end

    subgraph WORKER["veronex-worker"]
        CONSUMER["StreamConsumer"]
        BATCH["collect_batch()\n50ms or 256개"]
        FLUSH["flush_batch()\nbulk unnest UPDATE"]
    end

    PG[("Postgres\ninference_jobs")]

    REQ --> UC
    UC --> SAVE
    UC --> PRODUCE
    UC --> DM
    UC --> ZSET

    SAVE -->|"INSERT (동기)"| PG
    PRODUCE -->|"JobEvent JSON"| TOPIC

    TOPIC --> CONSUMER
    CONSUMER --> BATCH
    BATCH --> FLUSH
    FLUSH -->|"bulk UPDATE"| PG

    style SAVE fill:#e8f5e9,stroke:#43a047
    style PRODUCE fill:#fff3e0,stroke:#fb8c00
    style TOPIC fill:#fce4ec,stroke:#e91e63
    style FLUSH fill:#e3f2fd,stroke:#1e88e5
```

---

## ① submit() — 요청 제출

```mermaid
flowchart TD
    A(["Client\nPOST /v1/inference"]) --> B["submit(SubmitJobRequest)"]

    B --> C["JobId 생성 (UUIDv7)\nInferenceJob 생성 status=Pending"]

    C --> D{S3 설정됨?}
    D -->|Yes| E["message_store.put()\nMinIO zstd 업로드\nnon-fatal"]
    D -->|No| F
    E --> F["job_repo.save(messages=None)\n동기 Postgres INSERT"]

    F --> G{이미지 있음?}
    G -->|Yes| H["tokio::spawn\nimage_store.put_base64() → S3\nupdate_image_keys() → Redpanda"]
    G -->|No| I
    H --> I

    I --> J["DashMap에 JobEntry 삽입\ncancel_notify 등록\nincr_pending()"]
    J --> K["broadcast_event('pending')\nValkey pub/sub"]

    K --> L["ZSET 점수 계산\ntier_bonus 적용"]
    L --> M["valkey.zset_enqueue()"]

    M --> N{결과}
    N -->|"Ok(true)\n큐 등록 성공"| O(["JobId 반환 ✓"])
    N -->|"Ok(false)\n큐 가득 참"| P["decr_pending()\nDashMap 제거\nfail_with_reason() → Redpanda"]
    P --> Q(["DomainError::QueueFull ✗"])
    N -->|"Err\nValkey 장애"| R["spawn_job_direct()\n직접 실행"]
    R --> O

    style F fill:#e8f5e9,stroke:#43a047
    style H fill:#fff3e0,stroke:#fb8c00
    style P fill:#ffebee,stroke:#e53935
    style O fill:#e8f5e9,stroke:#43a047
    style Q fill:#ffebee,stroke:#e53935
```

---

## ② cancel() — 취소

```mermaid
flowchart TD
    A(["cancel(job_id)"]) --> B{DashMap에\n존재?}

    B -->|No| E
    B -->|Yes| C{현재 상태}

    C -->|"Completed\nFailed\nCancelled\n(terminal)"| Z(["No-op ✓"])

    C -->|"Pending\nor Running"| D["DashMap:\nstatus=Cancelled, done=true\nnotify_one() ← stream() 깨움\ncancel_notify_one() ← runner 중단"]

    D --> D2{이전 상태}
    D2 -->|Pending| D3["decr_pending()\n(Running이면 runner에서 처리)"]
    D2 -->|Running| E

    D3 --> E["job_repo.cancel_job(now)\nRedpanda produce\nJobEvent::Cancelled"]

    E --> F["valkey.zset_cancel()\nZSET에서 제거\n(아직 미디스패치 시)"]

    F --> G{로컬 job?}
    G -->|No| H["valkey.publish_cancel()\n크로스 인스턴스\npub/sub 전파"]
    G -->|Yes| I

    H --> I["cancel_notifiers 제거\nschedule_cleanup(delay)"]
    I --> Z2(["Ok(()) ✓"])

    style E fill:#fff3e0,stroke:#fb8c00
    style H fill:#f3e5f5,stroke:#8e24aa
    style Z fill:#e8f5e9,stroke:#43a047
    style Z2 fill:#e8f5e9,stroke:#43a047
```

---

## ③ stream() — 토큰 스트리밍

```mermaid
flowchart TD
    A(["stream(job_id)"]) --> B{DashMap에\n존재?}

    B -->|No| C["job_repo.get(job_id)\nPostgres SELECT"]
    C --> D{상태}
    D -->|Completed| E["result_text yield\ndone yield"]
    D -->|Failed| F(["Error 반환 ✗"])
    D -->|기타| G(["'job not in memory' 에러 ✗"])
    E --> END(["스트림 종료 ✓"])

    B -->|Yes| H["idx = 0\n스트리밍 루프 시작"]
    H --> I["DashMap에서\ntokens[idx..] 읽기"]
    I --> J["새 토큰 yield\nidx += n"]
    J --> K{done = true?}
    K -->|Yes| END
    K -->|No| L["notify.notified().await\nrunner가 토큰 추가할 때까지 대기"]
    L --> I

    style C fill:#e3f2fd,stroke:#1e88e5
    style F fill:#ffebee,stroke:#e53935
    style G fill:#ffebee,stroke:#e53935
    style END fill:#e8f5e9,stroke:#43a047
```

---

## ④ run_job() — 실제 추론 실행

```mermaid
flowchart TD
    A(["dispatcher →\nrun_job(provider, job)"]) --> B{Ollama?}
    B -->|Yes| C["model_manager.ensure_loaded()\n모델 로드 확인"]
    B -->|No| D
    C --> D{이미 Cancelled?}
    D -->|Yes| E["decr_pending()\nreturn Ok(None)"]

    D -->|No| F["DashMap: status=Running\nstarted_at 기록\nassigned_provider_id 세팅"]
    F --> G["job_repo.mark_running()\nRedpanda produce\nJobEvent::Running ← 즉시 반환"]
    G --> H["decr_pending()\nincr_running()\nbroadcast_event('running')"]

    H --> I["provider.stream_tokens(&job)\nLLM 스트리밍 시작"]

    I --> J{"tokio::select!\n스트리밍 루프"}

    J -->|"cancel_notify\n.notified()"| K["decr_running()\nreturn Ok(None)"]

    J -->|"stream.next()\n= None (스트림 종료)"| L["break → finalize_job()"]

    J -->|"stream.next()\n= Ok(token)"| M{entry.status\n= Cancelled?}
    M -->|Yes| K
    M -->|No| N["DashMap에 토큰 추가\nnotify_one() ← stream() 깨움\nTTFT / 토큰 카운트 측정"]
    N --> O{token_count\n> MAX?}
    O -->|Yes| P["DashMap: status=Failed\n'token_budget_exceeded'"]
    O -->|No| Q{"30s마다\nowner TTL?"}
    Q -->|Yes| R["Valkey job_owner\nTTL 갱신 (EX 300s)"]
    Q -->|No| J
    R --> J
    P --> L

    J -->|"stream.next()\n= Err(e)"| S["handle_stream_error()"]
    S --> S1["DashMap: status=Failed\njob_repo.fail_with_reason()\nRedpanda produce\nJobEvent::Failed"]
    S1 --> S2["decr_running()\nemit_inference_event()\nrecord_tpm() 환불"]
    S2 --> T(["Err 반환 ✗"])

    L --> U["finalize_job()"]
    U --> V["DashMap: status=Completed\ndone=true, notify_one()"]
    V --> W["decr_running()"]
    W --> X{Valkey 소유권\n확인}
    X -->|"다른 노드 소유"| Y["ownership lost\nschedule_cleanup()\nreturn None"]
    X -->|"내 소유"| Z["job_repo.mark_completed()\nRedpanda produce\nJobEvent::Completed ← 즉시 반환"]
    Z --> AA["broadcast_event('completed')\nrecord_tpm()\nemit_inference_event()\nschedule_cleanup()"]
    AA --> AB(["Ok(latency_ms) ✓"])

    style G fill:#fff3e0,stroke:#fb8c00
    style S1 fill:#fff3e0,stroke:#fb8c00
    style Z fill:#fff3e0,stroke:#fb8c00
    style T fill:#ffebee,stroke:#e53935
    style AB fill:#e8f5e9,stroke:#43a047
    style E fill:#e8f5e9,stroke:#43a047
```

---

## ⑤ veronex-worker — Redpanda 컨슈머

```mermaid
flowchart TD
    A(["veronex-worker 시작\nDATABASE_URL + KAFKA_BROKER"]) --> B["StreamConsumer\nsubscribe(veronex.job.events)"]

    B --> C{"consumer.recv()\n50ms timeout\nor 256개"}

    C -->|"Ok(msg)"| D["serde_json::from_slice\n::<JobEvent>()"]
    D -->|Ok| E["batch.push(event)"]
    D -->|Err| F["warn 로그\n(skip)"]
    E --> G{배치 조건\n달성?}
    F --> G
    G -->|"No\n(계속 수집)"| C
    G -->|"Yes\n(timeout or 256개)"| H

    C -->|"Err\n(kafka recv error)"| I["warn 로그"] --> C

    H["flush_batch(pool, batch)"] --> J["이벤트 타입별 분류"]

    J --> K["Running[]"]
    J --> L["Completed[]"]
    J --> M["Failed[]"]
    J --> N["Cancelled[]"]
    J --> O["ImageKeysUpdated[]"]

    K --> P["bulk_mark_running()\nUPDATE ... FROM\nunnest(uuid[], timestamptz[],\nuuid[], int4[])"]
    L --> Q["bulk_mark_completed()\nUPDATE ... FROM\nunnest (9개 배열)"]
    M --> R["bulk_fail_with_reason()\nUPDATE ... FROM\nunnest (3개 배열)"]
    N --> S["bulk_cancel_job()\nUPDATE ... FROM\nunnest (2개 배열)"]
    O --> T["bulk_update_image_keys()\nrow-by-row UPDATE"]

    P & Q & R & S & T --> U{"tokio::try_join!\n결과"}

    U -->|"모두 Ok"| V["consumer.commit(Async)\noffset 커밋"]
    V --> B

    U -->|"하나라도 Err"| W["error 로그\ncommit 없음\n← 재배달 보장"]
    W --> B

    style P fill:#e3f2fd,stroke:#1e88e5
    style Q fill:#e3f2fd,stroke:#1e88e5
    style R fill:#e3f2fd,stroke:#1e88e5
    style S fill:#e3f2fd,stroke:#1e88e5
    style T fill:#e3f2fd,stroke:#1e88e5
    style V fill:#e8f5e9,stroke:#43a047
    style W fill:#ffebee,stroke:#e53935
```

---

## ⑥ 상태 전이

```mermaid
stateDiagram-v2
    [*] --> Pending : submit()\nsave() → INSERT

    Pending --> Running : dispatcher picks up\nmark_running() → Redpanda
    Pending --> Cancelled : cancel()\ncancel_job() → Redpanda
    Pending --> Failed : queue full\nfail_with_reason() → Redpanda

    Running --> Completed : stream 정상 완료\nmark_completed() → Redpanda
    Running --> Failed : stream 에러\nfail_with_reason() → Redpanda
    Running --> Cancelled : cancel_notify 수신\ncancel_job() → Redpanda

    Completed --> [*]
    Failed --> [*]
    Cancelled --> [*]

    note right of Pending
        DashMap + Valkey ZSET
    end note
    note right of Running
        DashMap + job_owner TTL
        (Valkey EX 300s)
    end note
```

---

## ⑦ JobRepository 호출 매핑

```mermaid
flowchart LR
    subgraph CALLS["호출 위치"]
        A1["submit()"]
        A2["submit() 큐 가득"]
        A3["submit() 이미지"]
        A4["cancel()"]
        A5["run_job() 시작"]
        A6["run_job() 에러"]
        A7["finalize_job()"]
        A8["recover_pending_jobs()"]
        A9["get_status() miss"]
    end

    subgraph SYNC["동기 (Postgres 직접)"]
        B1["save()\nINSERT"]
        B2["list_pending()\nSELECT"]
        B3["update_status()\nUPDATE"]
        B4["get()\nSELECT"]
    end

    subgraph ASYNC["비동기 (Redpanda → worker)"]
        C1["fail_with_reason()\nJobEvent::Failed"]
        C2["update_image_keys()\nJobEvent::ImageKeysUpdated"]
        C3["cancel_job()\nJobEvent::Cancelled"]
        C4["mark_running()\nJobEvent::Running"]
        C5["mark_completed()\nJobEvent::Completed"]
    end

    A1 --> B1
    A2 --> C1
    A3 --> C2
    A4 --> C3
    A5 --> C4
    A6 --> C1
    A7 --> C5
    A8 --> B2
    A8 --> B3
    A9 --> B4

    style B1 fill:#e8f5e9,stroke:#43a047
    style B2 fill:#e8f5e9,stroke:#43a047
    style B3 fill:#e8f5e9,stroke:#43a047
    style B4 fill:#e8f5e9,stroke:#43a047
    style C1 fill:#fff3e0,stroke:#fb8c00
    style C2 fill:#fff3e0,stroke:#fb8c00
    style C3 fill:#fff3e0,stroke:#fb8c00
    style C4 fill:#fff3e0,stroke:#fb8c00
    style C5 fill:#fff3e0,stroke:#fb8c00
```
