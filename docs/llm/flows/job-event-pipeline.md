# Job Write Pipeline — Full Flow

> **Last Updated**: 2026-03-28
> Step diagrams (submit/cancel/stream/run_job): `flows/job-event-pipeline-steps.md`

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
