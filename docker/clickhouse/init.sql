-- ── Application tables (written directly by the Rust service) ───────────────
-- NOTE: Kafka Engine tables are in 02_kafka.sql and require Redpanda to be
-- healthy first. This file only contains tables safe to create at boot time.

CREATE TABLE IF NOT EXISTS inference_logs (
    event_time        DateTime64(3),
    api_key_id        UUID,
    tenant_id         LowCardinality(String),
    request_id        UUID,
    model_name        LowCardinality(String),
    prompt_tokens     UInt32,
    completion_tokens UInt32,
    latency_ms        UInt32,
    finish_reason     LowCardinality(String),
    status            LowCardinality(String)
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(event_time)
ORDER BY (api_key_id, event_time);

CREATE TABLE IF NOT EXISTS api_key_usage_hourly (
    hour              DateTime,
    api_key_id        UUID,
    tenant_id         LowCardinality(String),
    request_count     UInt64,
    success_count     UInt64,
    cancelled_count   UInt64,
    error_count       UInt64,
    prompt_tokens     UInt64,
    completion_tokens UInt64,
    total_tokens      UInt64
) ENGINE = AggregatingMergeTree()
PARTITION BY toYYYYMM(hour)
ORDER BY (api_key_id, hour);

CREATE MATERIALIZED VIEW IF NOT EXISTS api_key_usage_hourly_mv
TO api_key_usage_hourly AS
SELECT
    hour, api_key_id, tenant_id,
    request_count, success_count, cancelled_count, error_count,
    prompt_tokens, completion_tokens,
    prompt_tokens + completion_tokens AS total_tokens
FROM (
    SELECT
        toStartOfHour(event_time)             AS hour,
        api_key_id,
        tenant_id,
        count()                               AS request_count,
        countIf(finish_reason = 'stop')       AS success_count,
        countIf(finish_reason = 'cancelled')  AS cancelled_count,
        countIf(finish_reason = 'error')      AS error_count,
        sum(prompt_tokens)                    AS prompt_tokens,
        sum(completion_tokens)                AS completion_tokens
    FROM inference_logs
    GROUP BY hour, api_key_id, tenant_id
);

-- ── OTel metrics gauge (consumed from Redpanda via Kafka Engine in 02_kafka.sql) ──
-- Created here explicitly because OTel Collector no longer auto-creates tables.

CREATE TABLE IF NOT EXISTS otel_metrics_gauge (
    ResourceAttributes      Map(LowCardinality(String), String),
    ResourceSchemaUrl       String,
    ScopeName               LowCardinality(String),
    ScopeVersion            String,
    ScopeAttributes         Map(LowCardinality(String), String),
    ScopeDroppedAttrCount   UInt32,
    ScopeSchemaUrl          String,
    ServiceName             LowCardinality(String),
    MetricName              LowCardinality(String),
    MetricDescription       String,
    MetricUnit              String,
    Attributes              Map(LowCardinality(String), String),
    StartTimeUnix           DateTime64(9),
    TimeUnix                DateTime64(9),
    Value                   Float64,
    Flags                   UInt32,
    Exemplars Nested (
        FilteredAttributes  Map(LowCardinality(String), String),
        TimeUnix            DateTime64(9),
        Value               Float64,
        SpanId              String,
        TraceId             String
    )
) ENGINE = MergeTree()
PARTITION BY toDate(TimeUnix)
ORDER BY (MetricName, TimeUnix)
TTL toDate(TimeUnix) + INTERVAL 30 DAY;

-- ── OTel traces raw (consumed from Redpanda via Kafka Engine in 02_kafka.sql) ──

CREATE TABLE IF NOT EXISTS otel_traces_raw (
    received_at DateTime64(3) DEFAULT now64(3),
    payload     String
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(received_at)
ORDER BY received_at
TTL toDate(received_at) + INTERVAL 30 DAY;

-- ── node-exporter curated metrics (dashboard queries) ────────────────────────

CREATE TABLE IF NOT EXISTS node_metrics (
    ts              DateTime64(3),
    instance        LowCardinality(String),
    gpu_index       UInt8,
    gpu_vram_used_bytes   UInt64,
    gpu_vram_total_bytes  UInt64,
    gpu_util_ratio        Float32,
    gpu_temp_celsius      Float32,
    gpu_power_watts       Float32,
    mem_total_bytes       UInt64,
    mem_available_bytes   UInt64
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(ts)
ORDER BY (instance, gpu_index, ts)
TTL toDate(ts) + INTERVAL 30 DAY;
