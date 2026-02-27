-- ── Redpanda / Kafka consumer tables ─────────────────────────────────────────
-- Requires Redpanda to be healthy. Mounted as 02_kafka.sql so it runs after
-- 01_init.sql. If Redpanda is not yet available, this file may fail on first
-- boot — the core application tables in 01_init.sql are unaffected.

-- metrics topic ───────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS kafka_otel_metrics (
    raw String
) ENGINE = Kafka
SETTINGS
    kafka_broker_list          = 'redpanda:9092',
    kafka_topic_list           = 'otel-metrics',
    kafka_group_name           = 'clickhouse-otel-metrics',
    kafka_format               = 'JSONAsString',
    kafka_num_consumers        = 1,
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
    kafka_broker_list          = 'redpanda:9092',
    kafka_topic_list           = 'otel-traces',
    kafka_group_name           = 'clickhouse-otel-traces',
    kafka_format               = 'JSONAsString',
    kafka_num_consumers        = 1,
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
