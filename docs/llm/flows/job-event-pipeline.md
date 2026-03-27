# Job Write Pipeline — 전체 플로우

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

        subgraph REPO["PostgresJobRepository"]
            SAVE["save()\n동기 INSERT\n(메타데이터 + prompt_preview)"]
            FINALIZE["finalize()\n동기 UPDATE\n(메트릭 + has_tool_calls)"]
        end

        subgraph S3STORE["S3MessageStore"]
            PUT["put_conversation()\nzstd-3 JSON PUT\nconversations/{owner}/{date}/{id}.json.zst"]
        end

        DM["DashMap\n(in-memory)"]
        ZSET["Valkey ZSET\n(priority queue)"]
    end

    PG[("Postgres\ninference_jobs\n(메타데이터만)")]
    S3[("MinIO / S3\nConversationRecord\n~1.2 KB / job")]

    REQ --> UC
    UC --> SAVE
    UC --> DM
    UC --> ZSET

    SAVE -->|"INSERT (동기)"| PG

    DM -->|"dispatcher → run_job()"| PUT
    PUT -->|"zstd PUT (non-fatal)"| S3
    PUT --> FINALIZE
    FINALIZE -->|"UPDATE (동기)"| PG

    style SAVE fill:#e8f5e9,stroke:#43a047
    style FINALIZE fill:#e8f5e9,stroke:#43a047
    style PUT fill:#e3f2fd,stroke:#1e88e5
```

---

## ① submit() — 요청 제출

```mermaid
flowchart TD
    A(["Client\nPOST /v1/inference"]) --> B["submit(SubmitJobRequest)"]

    B --> C["JobId 생성 (UUIDv7)\nInferenceJob 생성 status=Pending\nprompt_preview 생성 (≤200자, CJK-safe)"]

    C --> D["job_repo.save()\n동기 Postgres INSERT\n(메타데이터 + prompt_preview)"]

    D --> E{이미지 있음?}
    E -->|Yes| F["tokio::spawn\nimage_store.put_base64() → S3\nupdate_image_keys() → Postgres"]
    E -->|No| G

    F --> G["DashMap에 JobEntry 삽입\n(messages 포함, in-memory)\ncancel_notify 등록\nincr_pending()"]
    G --> H["broadcast_event('pending')\nValkey pub/sub"]

    H --> I["ZSET 점수 계산\ntier_bonus 적용"]
    I --> J["valkey.zset_enqueue()"]

    J --> K{결과}
    K -->|"Ok(true)\n큐 등록 성공"| L(["JobId 반환 ✓"])
    K -->|"Ok(false)\n큐 가득 참"| M["decr_pending()\nDashMap 제거\nfail_with_reason() → Postgres"]
    M --> N(["DomainError::QueueFull ✗"])
    K -->|"Err\nValkey 장애"| O["spawn_job_direct()\n직접 실행"]
    O --> L

    style D fill:#e8f5e9,stroke:#43a047
    style M fill:#ffebee,stroke:#e53935
    style L fill:#e8f5e9,stroke:#43a047
    style N fill:#ffebee,stroke:#e53935
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
    D2 -->|Pending| D3["decr_pending()"]
    D2 -->|Running| E

    D3 --> E["job_repo.cancel_job(now)\nPostgres UPDATE (동기)"]

    E --> F["valkey.zset_cancel()\nZSET에서 제거"]

    F --> G{로컬 job?}
    G -->|No| H["valkey.publish_cancel()\n크로스 인스턴스\npub/sub 전파"]
    G -->|Yes| I

    H --> I["cancel_notifiers 제거\nschedule_cleanup(delay)"]
    I --> Z2(["Ok(()) ✓"])

    style E fill:#e8f5e9,stroke:#43a047
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

    D -->|No| F["DashMap: status=Running\nstarted_at 기록\nassigned_provider_id 세팅\n(Postgres 쓰기 없음)"]
    F --> G["decr_pending()\nincr_running()\nbroadcast_event('running')"]

    G --> H["provider.stream_tokens(&job)\nLLM 스트리밍 시작\n(messages는 DashMap에서 공급)"]

    H --> I{"tokio::select!\n스트리밍 루프"}

    I -->|"cancel_notify\n.notified()"| K["decr_running()\nreturn Ok(None)"]

    I -->|"stream.next()\n= None (스트림 종료)"| L["break → finalize_job()"]

    I -->|"stream.next()\n= Ok(token)"| M{entry.status\n= Cancelled?}
    M -->|Yes| K
    M -->|No| N["DashMap에 토큰 추가\nnotify_one() ← stream() 깨움\nTTFT / 토큰 카운트 측정\ntool_calls 수집"]
    N --> O{token_count\n> MAX?}
    O -->|Yes| P["DashMap: status=Failed\n'token_budget_exceeded'"]
    O -->|No| Q{"30s마다\nowner TTL?"}
    Q -->|Yes| R["Valkey job_owner\nTTL 갱신 (EX 300s)"]
    Q -->|No| I
    R --> I
    P --> L

    I -->|"stream.next()\n= Err(e)"| S["handle_stream_error()"]
    S --> S1["DashMap: status=Failed\njob_repo.fail_with_reason()\nPostgres UPDATE"]
    S1 --> S2["decr_running()\nemit_inference_event()\nrecord_tpm() 환불"]
    S2 --> T(["Err 반환 ✗"])

    L --> U["finalize_job()"]
    U --> V["DashMap: status=Completed\ndone=true, notify_one()"]
    V --> W["decr_running()"]
    W --> X{Valkey 소유권\n확인}
    X -->|"다른 노드 소유"| Y["ownership lost\nschedule_cleanup()\nreturn None"]
    X -->|"내 소유"| Z["S3 PUT\nConversationRecord\n(non-fatal)"]
    Z --> AA["job_repo.finalize()\nPostgres UPDATE\n(메트릭 + has_tool_calls)"]
    AA --> AB["broadcast_event('completed')\nrecord_tpm()\nemit_inference_event()\nschedule_cleanup()"]
    AB --> AC(["Ok(latency_ms) ✓"])

    style S1 fill:#e8f5e9,stroke:#43a047
    style Z fill:#e3f2fd,stroke:#1e88e5
    style AA fill:#e8f5e9,stroke:#43a047
    style T fill:#ffebee,stroke:#e53935
    style AC fill:#e8f5e9,stroke:#43a047
    style E fill:#e8f5e9,stroke:#43a047
```

---

## ⑤ 상태 전이

```mermaid
stateDiagram-v2
    [*] --> Pending : submit()\nsave() → INSERT

    Pending --> Running : dispatcher picks up\n(Postgres 쓰기 없음)
    Pending --> Cancelled : cancel()\ncancel_job() → UPDATE
    Pending --> Failed : queue full\nfail_with_reason() → UPDATE

    Running --> Completed : stream 정상 완료\nS3 PUT + finalize() → UPDATE
    Running --> Failed : stream 에러\nfail_with_reason() → UPDATE
    Running --> Cancelled : cancel_notify 수신\n(cancel_job은 호출자가 처리)

    Completed --> [*]
    Failed --> [*]
    Cancelled --> [*]

    note right of Pending
        DashMap + Valkey ZSET
    end note
    note right of Running
        DashMap + job_owner TTL
        (Valkey EX 300s)
        messages in DashMap (in-memory)
    end note
    note right of Completed
        ConversationRecord in S3
        메타데이터만 Postgres
    end note
```

---

## ⑥ JobRepository 호출 매핑

```mermaid
flowchart LR
    subgraph CALLS["호출 위치"]
        A1["submit()"]
        A2["submit() 큐 가득"]
        A3["submit() 이미지"]
        A4["cancel()"]
        A5["run_job() 에러"]
        A6["finalize_job()"]
        A7["recover_pending_jobs()"]
        A8["get_status() miss"]
    end

    subgraph POSTGRES["Postgres (직접)"]
        B1["save()\nINSERT (metadata + prompt_preview)"]
        B2["list_pending()\nSELECT"]
        B3["update_status()\nUPDATE"]
        B4["get()\nSELECT"]
        B5["fail_with_reason()\nUPDATE"]
        B6["cancel_job()\nUPDATE"]
        B7["finalize()\nUPDATE (metrics + has_tool_calls)"]
        B8["update_image_keys()\nUPDATE"]
    end

    subgraph S3WRITE["S3 (non-fatal)"]
        C1["put_conversation()\nConversationRecord zstd PUT"]
    end

    A1 --> B1
    A2 --> B5
    A3 --> B8
    A4 --> B6
    A5 --> B5
    A6 --> C1
    A6 --> B7
    A7 --> B2
    A7 --> B3
    A8 --> B4

    style B1 fill:#e8f5e9,stroke:#43a047
    style B7 fill:#e8f5e9,stroke:#43a047
    style C1 fill:#e3f2fd,stroke:#1e88e5
```
