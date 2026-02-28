USE veronex;

-- ── Redpanda / Kafka consumer tables ─────────────────────────────────────────
-- Requires Redpanda to be healthy. Mounted as 02_kafka.sql so it runs after
-- 01_init.sql. If Redpanda is not yet available, this file may fail on first
-- boot — the core application tables in 01_init.sql are unaffected.
--
-- Pipeline:
--   Rust → Redpanda [inference]   → kafka_inference   → inference_logs   (MV)
--   OTel → Redpanda [otel-metrics] → kafka_otel_metrics → otel_metrics_gauge (MV)
--   OTel → Redpanda [otel-traces]  → kafka_otel_traces  → otel_traces_raw   (MV)

-- ── inference topic → inference_logs ─────────────────────────────────────────

CREATE TABLE IF NOT EXISTS kafka_inference (
    raw String
) ENGINE = Kafka
SETTINGS
    kafka_broker_list          = 'redpanda:9092',
    kafka_topic_list           = 'inference',
    kafka_group_name           = 'clickhouse-inference',
    kafka_format               = 'JSONAsString',
    kafka_num_consumers        = 1,
    kafka_skip_broken_messages = 10;

CREATE MATERIALIZED VIEW IF NOT EXISTS kafka_inference_mv
TO inference_logs AS
SELECT
    fromUnixTimestamp64Milli(JSONExtractInt(raw, 'event_time_ms'))  AS event_time,
    toUUID(JSONExtractString(raw, 'api_key_id'))                    AS api_key_id,
    JSONExtractString(raw, 'tenant_id')                             AS tenant_id,
    toUUID(JSONExtractString(raw, 'request_id'))                    AS request_id,
    JSONExtractString(raw, 'model_name')                            AS model_name,
    JSONExtractUInt(raw, 'prompt_tokens')                           AS prompt_tokens,
    JSONExtractUInt(raw, 'completion_tokens')                       AS completion_tokens,
    JSONExtractUInt(raw, 'latency_ms')                              AS latency_ms,
    JSONExtractString(raw, 'finish_reason')                         AS finish_reason,
    JSONExtractString(raw, 'status')                                AS status
FROM kafka_inference;

-- ── otel-metrics topic → otel_metrics_gauge ──────────────────────────────────
-- OTLP JSON format (otlp_json encoding):
--   { "resourceMetrics": [ { "resource": { "attributes": [...] },
--       "scopeMetrics": [ { "metrics": [ { "name": "...", "gauge": {
--         "dataPoints": [ { "attributes": [...], "timeUnixNano": "...",
--                           "startTimeUnixNano": "...", "asDouble": 0.0 } ] } } ] } ] } ] }
-- Keys are camelCase (protobuf JSON serialisation).

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

CREATE MATERIALIZED VIEW IF NOT EXISTS kafka_otel_metrics_mv
TO otel_metrics_gauge AS
SELECT
    -- Resource attributes: array of {key, value{stringValue}} → Map
    CAST(
        arrayMap(
            x -> (
                JSONExtractString(x, 'key'),
                JSONExtractString(JSONExtractRaw(x, 'value'), 'stringValue')
            ),
            JSONExtractArrayRaw(rm, 'resource.attributes')
        ),
        'Map(LowCardinality(String), String)'
    ) AS ResourceAttributes,
    '' AS ResourceSchemaUrl,
    '' AS ScopeName,
    '' AS ScopeVersion,
    CAST(map(), 'Map(LowCardinality(String), String)') AS ScopeAttributes,
    0  AS ScopeDroppedAttrCount,
    '' AS ScopeSchemaUrl,
    '' AS ServiceName,
    JSONExtractString(metric, 'name')        AS MetricName,
    JSONExtractString(metric, 'description') AS MetricDescription,
    JSONExtractString(metric, 'unit')        AS MetricUnit,
    -- Data-point attributes
    CAST(
        arrayMap(
            x -> (
                JSONExtractString(x, 'key'),
                JSONExtractString(JSONExtractRaw(x, 'value'), 'stringValue')
            ),
            JSONExtractArrayRaw(dp, 'attributes')
        ),
        'Map(LowCardinality(String), String)'
    ) AS Attributes,
    fromUnixTimestamp64Nano(
        toInt64OrZero(JSONExtractString(dp, 'startTimeUnixNano'))
    ) AS StartTimeUnix,
    fromUnixTimestamp64Nano(
        toInt64OrZero(JSONExtractString(dp, 'timeUnixNano'))
    ) AS TimeUnix,
    JSONExtractFloat(dp, 'asDouble') AS Value,
    0  AS Flags,
    CAST([], 'Array(Map(LowCardinality(String), String))') AS `Exemplars.FilteredAttributes`,
    CAST([], 'Array(DateTime64(9))')                       AS `Exemplars.TimeUnix`,
    CAST([], 'Array(Float64)')                             AS `Exemplars.Value`,
    CAST([], 'Array(String)')                              AS `Exemplars.SpanId`,
    CAST([], 'Array(String)')                              AS `Exemplars.TraceId`
FROM (
    SELECT
        arrayJoin(JSONExtractArrayRaw(raw, 'resourceMetrics'))                       AS rm,
        arrayJoin(JSONExtractArrayRaw(rm, 'scopeMetrics'))                           AS sm,
        arrayJoin(JSONExtractArrayRaw(sm, 'metrics'))                                AS metric,
        arrayJoin(
            JSONExtractArrayRaw(JSONExtractRaw(metric, 'gauge'), 'dataPoints')
        ) AS dp
    FROM kafka_otel_metrics
    WHERE JSONHas(metric, 'gauge')
);

-- ── otel-traces topic → otel_traces_raw ──────────────────────────────────────

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

CREATE MATERIALIZED VIEW IF NOT EXISTS kafka_otel_traces_mv
TO otel_traces_raw AS
SELECT now64(3) AS received_at, raw AS payload
FROM kafka_otel_traces;
