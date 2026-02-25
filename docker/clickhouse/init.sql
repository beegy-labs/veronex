-- ── Application tables (written directly by the Rust service) ───────────────

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

-- ── node-exporter curated metrics (dashboard queries) ────────────────────────
-- OTel ClickHouse exporter also auto-creates otel_metrics_* tables with the
-- full OTLP schema. This table is a curated roll-up for backend card display.
-- Populated via the kafka_node_metrics_mv below.

CREATE TABLE IF NOT EXISTS node_metrics (
    ts              DateTime64(3),
    instance        LowCardinality(String),  -- node-exporter instance label
    gpu_index       UInt8,                   -- GPU index (0-based), 255 = N/A
    -- GPU (from --collector.drm + --collector.hwmon)
    gpu_vram_used_bytes   UInt64,
    gpu_vram_total_bytes  UInt64,
    gpu_util_ratio        Float32,           -- 0.0–1.0
    gpu_temp_celsius      Float32,
    gpu_power_watts       Float32,
    -- System RAM (from --collector.meminfo)
    mem_total_bytes       UInt64,
    mem_available_bytes   UInt64
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(ts)
ORDER BY (instance, gpu_index, ts)
TTL toDate(ts) + INTERVAL 30 DAY;

-- ── Redpanda / Kafka consumer tables ─────────────────────────────────────────
-- OTel Collector fans out to both ClickHouse (direct, otel_metrics_* auto-schema)
-- and Redpanda (kafka exporter, otlp_json encoding).
-- These Kafka Engine tables consume from Redpanda and store raw OTLP payloads,
-- enabling stream processing and future migration to a Kafka cluster
-- (swap broker address only — zero code changes).

-- metrics topic ───────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS kafka_otel_metrics (
    raw String
) ENGINE = Kafka
SETTINGS
    kafka_broker_list       = 'redpanda:9092',
    kafka_topic_list        = 'otel-metrics',
    kafka_group_name        = 'clickhouse-otel-metrics',
    kafka_format            = 'JSONAsString',
    kafka_num_consumers     = 1,
    kafka_skip_broken_messages = 10;

CREATE TABLE IF NOT EXISTS otel_metrics_raw (
    received_at DateTime64(3) DEFAULT now64(3),
    payload     String
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(received_at)
ORDER BY received_at
TTL toDate(received_at) + INTERVAL 30 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS kafka_otel_metrics_mv
TO otel_metrics_raw AS
SELECT now64(3) AS received_at, raw AS payload
FROM kafka_otel_metrics;

-- traces topic ────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS kafka_otel_traces (
    raw String
) ENGINE = Kafka
SETTINGS
    kafka_broker_list       = 'redpanda:9092',
    kafka_topic_list        = 'otel-traces',
    kafka_group_name        = 'clickhouse-otel-traces',
    kafka_format            = 'JSONAsString',
    kafka_num_consumers     = 1,
    kafka_skip_broken_messages = 10;

CREATE TABLE IF NOT EXISTS otel_traces_raw (
    received_at DateTime64(3) DEFAULT now64(3),
    payload     String
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(received_at)
ORDER BY received_at
TTL toDate(received_at) + INTERVAL 30 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS kafka_otel_traces_mv
TO otel_traces_raw AS
SELECT now64(3) AS received_at, raw AS payload
FROM kafka_otel_traces;
