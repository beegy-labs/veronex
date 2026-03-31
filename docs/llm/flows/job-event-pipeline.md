# Job Write Pipeline — Full Flow

> **Last Updated**: 2026-03-28

---

## Overall Architecture

```mermaid
flowchart TB
    subgraph CLIENT["Client"]
        REQ["POST /v1/chat/completions"]
    end

    subgraph VERONEX["veronex (API Server)"]
        direction TB
        UC["InferenceUseCase"]

        subgraph REPO["PostgresJobRepository"]
            SAVE["save()\nsync INSERT\n(metadata + prompt_preview)"]
            FINALIZE["finalize()\nsync UPDATE\n(metrics + has_tool_calls)"]
        end

        subgraph S3STORE["S3MessageStore"]
            PUT["put_conversation()\nzstd-3 JSON PUT\nconversations/{owner}/{date}/{id}.json.zst"]
        end

        DM["DashMap\n(in-memory)"]
        ZSET["Valkey ZSET\n(priority queue)"]
    end

    PG[("Postgres\ninference_jobs\n(metadata only)")]
    S3[("MinIO / S3\nConversationRecord\n~1.2 KB / job")]

    REQ --> UC
    UC --> SAVE
    UC --> DM
    UC --> ZSET

    SAVE -->|"INSERT (sync)"| PG

    DM -->|"dispatcher → run_job()"| PUT
    PUT -->|"zstd PUT (non-fatal)"| S3
    PUT --> FINALIZE
    FINALIZE -->|"UPDATE (sync)"| PG

    style SAVE fill:#e8f5e9,stroke:#43a047
    style FINALIZE fill:#e8f5e9,stroke:#43a047
    style PUT fill:#e3f2fd,stroke:#1e88e5
```

---

## ① submit() — Request Submission

```mermaid
flowchart TD
    A(["Client\nPOST /v1/inference"]) --> B["submit(SubmitJobRequest)"]

    B --> C["Generate JobId (UUIDv7)\nCreate InferenceJob status=Pending\nGenerate prompt_preview (≤200 chars, CJK-safe)"]

    C --> D["job_repo.save()\nsync Postgres INSERT\n(metadata + prompt_preview)"]

    D --> E{Has images?}
    E -->|Yes| F["tokio::spawn\nimage_store.put_base64() → S3\nupdate_image_keys() → Postgres"]
    E -->|No| G

    F --> G["Insert JobEntry into DashMap\n(includes messages, in-memory)\nRegister cancel_notify\nincr_pending()"]
    G --> H["broadcast_event('pending')\nValkey pub/sub"]

    H --> I["Compute ZSET score\nApply tier_bonus"]
    I --> J["valkey.zset_enqueue()"]

    J --> K{Result}
    K -->|"Ok(true)\nenqueue success"| L(["Return JobId ✓"])
    K -->|"Ok(false)\nqueue full"| M["decr_pending()\nRemove from DashMap\nfail_with_reason() → Postgres"]
    M --> N(["DomainError::QueueFull ✗"])
    K -->|"Err\nValkey failure"| O["spawn_job_direct()\ndirect execution"]
    O --> L

    style D fill:#e8f5e9,stroke:#43a047
    style M fill:#ffebee,stroke:#e53935
    style L fill:#e8f5e9,stroke:#43a047
    style N fill:#ffebee,stroke:#e53935
```

---

## ② cancel() — Cancellation

```mermaid
flowchart TD
    A(["cancel(job_id)"]) --> B{Exists in\nDashMap?}

    B -->|No| E
    B -->|Yes| C{Current status}

    C -->|"Completed\nFailed\nCancelled\n(terminal)"| Z(["No-op ✓"])

    C -->|"Pending\nor Running"| D["DashMap:\nstatus=Cancelled, done=true\nnotify_one() ← wake stream()\ncancel_notify_one() ← abort runner"]

    D --> D2{Previous status}
    D2 -->|Pending| D3["decr_pending()"]
    D2 -->|Running| E

    D3 --> E["job_repo.cancel_job(now)\nPostgres UPDATE (sync)"]

    E --> F["valkey.zset_cancel()\nRemove from ZSET"]

    F --> G{Local job?}
    G -->|No| H["valkey.publish_cancel()\ncross-instance\npub/sub propagation"]
    G -->|Yes| I

    H --> I["Remove cancel_notifiers\nschedule_cleanup(delay)"]
    I --> Z2(["Ok(()) ✓"])

    style E fill:#e8f5e9,stroke:#43a047
    style H fill:#f3e5f5,stroke:#8e24aa
    style Z fill:#e8f5e9,stroke:#43a047
    style Z2 fill:#e8f5e9,stroke:#43a047
```

---

## ③ stream() — Token Streaming

```mermaid
flowchart TD
    A(["stream(job_id)"]) --> B{Exists in\nDashMap?}

    B -->|No| C["job_repo.get(job_id)\nPostgres SELECT"]
    C --> D{Status}
    D -->|Completed| E["result_text yield\ndone yield"]
    D -->|Failed| F(["Return Error ✗"])
    D -->|Other| G(["'job not in memory' error ✗"])
    E --> END(["Stream end ✓"])

    B -->|Yes| H["idx = 0\nStart streaming loop"]
    H --> I["Read tokens[idx..]\nfrom DashMap"]
    I --> J["Yield new tokens\nidx += n"]
    J --> K{done = true?}
    K -->|Yes| END
    K -->|No| L["notify.notified().await\nWait until runner appends tokens"]
    L --> I

    style C fill:#e3f2fd,stroke:#1e88e5
    style F fill:#ffebee,stroke:#e53935
    style G fill:#ffebee,stroke:#e53935
    style END fill:#e8f5e9,stroke:#43a047
```

---

## ④ run_job() — Inference Execution

```mermaid
flowchart TD
    A(["dispatcher →\nrun_job(provider, job)"]) --> B{Ollama?}
    B -->|Yes| C["model_manager.ensure_loaded()\nVerify model loaded"]
    B -->|No| D
    C --> D{Already Cancelled?}
    D -->|Yes| E["decr_pending()\nreturn Ok(None)"]

    D -->|No| F["DashMap: status=Running\nRecord started_at\nSet assigned_provider_id\n(no Postgres write)"]
    F --> G["decr_pending()\nincr_running()\nbroadcast_event('running')"]

    G --> H["provider.stream_tokens(&job)\nStart LLM streaming\n(messages supplied from DashMap)"]

    H --> I{"tokio::select!\nStreaming loop"}

    I -->|"cancel_notify\n.notified()"| K["decr_running()\nreturn Ok(None)"]

    I -->|"stream.next()\n= None (stream end)"| L["break → finalize_job()"]

    I -->|"stream.next()\n= Ok(token)"| M{entry.status\n= Cancelled?}
    M -->|Yes| K
    M -->|No| N["Append token to DashMap\nnotify_one() ← wake stream()\nMeasure TTFT / token count\nCollect tool_calls"]
    N --> O{token_count\n> MAX?}
    O -->|Yes| P["DashMap: status=Failed\n'token_budget_exceeded'"]
    O -->|No| Q{"Every 30s\nowner TTL?"}
    Q -->|Yes| R["Valkey job_owner\nTTL refresh (EX 300s)"]
    Q -->|No| I
    R --> I
    P --> L

    I -->|"stream.next()\n= Err(e)"| S["handle_stream_error()"]
    S --> S1["DashMap: status=Failed\njob_repo.fail_with_reason()\nPostgres UPDATE"]
    S1 --> S2["decr_running()\nemit_inference_event()\nrecord_tpm() refund"]
    S2 --> T(["Return Err ✗"])

    L --> U["finalize_job()"]
    U --> V["DashMap: status=Completed\ndone=true, notify_one()"]
    V --> W["decr_running()"]
    W --> X{Valkey ownership\ncheck}
    X -->|"Other node owns"| Y["ownership lost\nschedule_cleanup()\nreturn None"]
    X -->|"Owned by self"| Z["S3 PUT\nConversationRecord\n(non-fatal)"]
    Z --> AA["job_repo.finalize()\nPostgres UPDATE\n(metrics + has_tool_calls)"]
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

## ⑤ State Transitions

```mermaid
stateDiagram-v2
    [*] --> Pending : submit()\nsave() → INSERT

    Pending --> Running : dispatcher picks up\n(no Postgres write)
    Pending --> Cancelled : cancel()\ncancel_job() → UPDATE
    Pending --> Failed : queue full\nfail_with_reason() → UPDATE

    Running --> Completed : stream success\nS3 PUT + finalize() → UPDATE
    Running --> Failed : stream error\nfail_with_reason() → UPDATE
    Running --> Cancelled : cancel_notify received\n(cancel_job handled by caller)

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
        metadata only in Postgres
    end note
```

---

## ⑥ JobRepository Call Mapping

```mermaid
flowchart LR
    subgraph CALLS["Call Sites"]
        A1["submit()"]
        A2["submit() queue full"]
        A3["submit() images"]
        A4["cancel()"]
        A5["run_job() error"]
        A6["finalize_job()"]
        A7["recover_pending_jobs()"]
        A8["get_status() miss"]
    end

    subgraph POSTGRES["Postgres (direct)"]
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
