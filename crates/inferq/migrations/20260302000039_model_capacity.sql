-- Model capacity tracking: per-(backend, model) VRAM + concurrency analysis.

CREATE TABLE model_capacity (
    backend_id              UUID        NOT NULL REFERENCES llm_backends(id) ON DELETE CASCADE,
    model_name              TEXT        NOT NULL,

    -- VRAM measurement from /api/ps
    vram_model_mb           INT         NOT NULL DEFAULT 0,
    vram_total_mb           INT         NOT NULL DEFAULT 0,

    -- Architecture parameters from /api/show model_info
    arch_num_layers         INT         NOT NULL DEFAULT 0,   -- block_count
    arch_num_kv_heads       INT         NOT NULL DEFAULT 0,   -- attention.head_count_kv
    arch_head_dim           INT         NOT NULL DEFAULT 0,   -- attention.key_length
    arch_configured_ctx     INT         NOT NULL DEFAULT 0,   -- actual num_ctx setting

    -- KV cache calculation (exact formula)
    vram_kv_per_slot_mb     INT         NOT NULL DEFAULT 0,   -- realistic (avg_tokens basis)
    vram_kv_worst_case_mb   INT         NOT NULL DEFAULT 0,   -- worst case (num_ctx basis)

    -- Recommended concurrency
    recommended_slots       SMALLINT    NOT NULL DEFAULT 1,

    -- Throughput stats (last 1h inference_jobs)
    avg_tokens_per_sec      FLOAT8      NOT NULL DEFAULT 0,
    avg_prefill_tps         FLOAT8      NOT NULL DEFAULT 0,
    avg_prompt_tokens       FLOAT8      NOT NULL DEFAULT 0,
    avg_output_tokens       FLOAT8      NOT NULL DEFAULT 0,
    p95_latency_ms          FLOAT8      NOT NULL DEFAULT 0,
    sample_count            INT         NOT NULL DEFAULT 0,

    -- LLM analysis result (qwen2.5:3b, nullable)
    llm_concern             TEXT,
    llm_reason              TEXT,

    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (backend_id, model_name)
);

-- Track which backend processed each job (for throughput analysis)
ALTER TABLE inference_jobs
    ADD COLUMN backend_id UUID REFERENCES llm_backends(id);

-- Index for efficient per-(backend, model) throughput aggregation
CREATE INDEX idx_inference_jobs_backend_capacity
    ON inference_jobs(backend_id, model_name, created_at DESC)
    WHERE status = 'completed';

-- Capacity analysis settings (singleton row, id=1)
CREATE TABLE capacity_settings (
    id                   INT         PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    analyzer_model       TEXT        NOT NULL DEFAULT 'qwen2.5:3b',
    batch_enabled        BOOLEAN     NOT NULL DEFAULT true,
    batch_interval_secs  INT         NOT NULL DEFAULT 300,
    last_run_at          TIMESTAMPTZ,
    last_run_status      TEXT,
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO capacity_settings DEFAULT VALUES;
