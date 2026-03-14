-- ── MergeTree target tables ────────────────────────────────────────────────────

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

-- OTel logs — unified event store for inference + audit events.
CREATE TABLE IF NOT EXISTS otel_logs (
    Timestamp          DateTime64(9),
    TraceId            String,
    SpanId             String,
    SeverityText       LowCardinality(String),
    SeverityNumber     Int32,
    ServiceName        LowCardinality(String),
    Body               String,
    ResourceAttributes Map(LowCardinality(String), String),
    LogAttributes      Map(LowCardinality(String), String)
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(Timestamp)
ORDER BY (ServiceName, Timestamp)
TTL toDate(Timestamp) + INTERVAL __RETENTION_ANALYTICS_DAYS__ DAY;

-- Audit events (DEPRECATED — superseded by otel_logs)
CREATE TABLE IF NOT EXISTS audit_events (
    event_time    DateTime64(3),
    account_id    UUID,
    account_name  LowCardinality(String),
    action        LowCardinality(String),
    resource_type LowCardinality(String),
    resource_id   String,
    resource_name String,
    ip_address    String,
    details       String
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(event_time)
ORDER BY (event_time, resource_type, resource_id)
TTL toDate(event_time) + INTERVAL __RETENTION_AUDIT_DAYS__ DAY;

-- ── Derived MVs: otel_logs → specialized tables ──────────────────────────────

CREATE MATERIALIZED VIEW IF NOT EXISTS otel_inference_logs_mv
TO inference_logs AS
SELECT
    Timestamp                                                        AS event_time,
    toUUIDOrZero(LogAttributes['api_key_id'])                        AS api_key_id,
    LogAttributes['tenant_id']                                       AS tenant_id,
    toUUIDOrZero(LogAttributes['request_id'])                        AS request_id,
    LogAttributes['model_name']                                      AS model_name,
    toUInt32OrZero(LogAttributes['prompt_tokens'])                   AS prompt_tokens,
    toUInt32OrZero(LogAttributes['completion_tokens'])               AS completion_tokens,
    toUInt32OrZero(LogAttributes['latency_ms'])                      AS latency_ms,
    LogAttributes['finish_reason']                                   AS finish_reason,
    LogAttributes['status']                                          AS status
FROM otel_logs
WHERE LogAttributes['event.name'] = 'inference.completed';

CREATE MATERIALIZED VIEW IF NOT EXISTS otel_audit_events_mv
TO audit_events AS
SELECT
    Timestamp                                                        AS event_time,
    toUUIDOrZero(LogAttributes['account_id'])                        AS account_id,
    LogAttributes['account_name']                                    AS account_name,
    LogAttributes['action']                                          AS action,
    LogAttributes['resource_type']                                   AS resource_type,
    LogAttributes['resource_id']                                     AS resource_id,
    LogAttributes['resource_name']                                   AS resource_name,
    LogAttributes['ip_address']                                      AS ip_address,
    LogAttributes['details']                                         AS details
FROM otel_logs
WHERE LogAttributes['event.name'] = 'audit.action';

-- OTel metrics gauge
CREATE TABLE IF NOT EXISTS otel_metrics_gauge (
    ts           DateTime64(9),
    server_id    LowCardinality(String),
    metric_name  LowCardinality(String),
    value        Float64,
    attributes   Map(LowCardinality(String), String)
) ENGINE = MergeTree()
PARTITION BY toDate(ts)
ORDER BY (metric_name, server_id, ts)
TTL toDate(ts) + INTERVAL __RETENTION_METRICS_DAYS__ DAY;

-- OTel traces raw
CREATE TABLE IF NOT EXISTS otel_traces_raw (
    received_at DateTime64(3) DEFAULT now64(3),
    payload     String
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(received_at)
ORDER BY received_at
TTL toDate(received_at) + INTERVAL __RETENTION_METRICS_DAYS__ DAY;

-- ── Kafka Engine tables + Materialized Views ───────────────────────────────────

-- otel-logs → otel_logs
CREATE TABLE IF NOT EXISTS kafka_otel_logs (
    raw String
) ENGINE = Kafka
SETTINGS
    kafka_broker_list          = '__KAFKA_BROKER__',
    kafka_topic_list           = 'otel-logs',
    kafka_group_name           = 'clickhouse-__CLICKHOUSE_DB__-otel-logs',
    kafka_format               = 'JSONAsString',
    kafka_num_consumers        = 1,
    kafka_skip_broken_messages = 10,
    kafka_security_protocol    = '__KAFKA_SECURITY_PROTOCOL__',
    kafka_sasl_mechanism       = '__KAFKA_SASL_MECHANISM__',
    kafka_sasl_username        = '__KAFKA_USERNAME__',
    kafka_sasl_password        = '__KAFKA_PASSWORD__';

CREATE MATERIALIZED VIEW IF NOT EXISTS kafka_otel_logs_mv
TO otel_logs AS
SELECT
    fromUnixTimestamp64Nano(toInt64OrZero(JSONExtractString(lr, 'timeUnixNano'))) AS Timestamp,
    JSONExtractString(lr, 'traceId')                                               AS TraceId,
    JSONExtractString(lr, 'spanId')                                                AS SpanId,
    JSONExtractString(lr, 'severityText')                                          AS SeverityText,
    JSONExtractInt(lr, 'severityNumber')                                           AS SeverityNumber,
    ResourceAttributes['service.name']                                             AS ServiceName,
    JSONExtractString(JSONExtractRaw(lr, 'body'), 'stringValue')                   AS Body,
    ResourceAttributes,
    LogAttributes
FROM (
    SELECT
        lr,
        CAST(
            arrayMap(
                x -> (
                    JSONExtractString(x, 'key'),
                    COALESCE(
                        nullIf(JSONExtractString(JSONExtractRaw(x, 'value'), 'stringValue'), ''),
                        nullIf(JSONExtractString(JSONExtractRaw(x, 'value'), 'intValue'),    ''),
                        toString(JSONExtractFloat(JSONExtractRaw(x, 'value'), 'doubleValue'))
                    )
                ),
                JSONExtractArrayRaw(JSONExtractRaw(rm, 'resource'), 'attributes')
            ),
            'Map(LowCardinality(String), String)'
        ) AS ResourceAttributes,
        CAST(
            arrayMap(
                x -> (
                    JSONExtractString(x, 'key'),
                    COALESCE(
                        nullIf(JSONExtractString(JSONExtractRaw(x, 'value'), 'stringValue'), ''),
                        nullIf(JSONExtractString(JSONExtractRaw(x, 'value'), 'intValue'),    ''),
                        toString(JSONExtractFloat(JSONExtractRaw(x, 'value'), 'doubleValue'))
                    )
                ),
                JSONExtractArrayRaw(lr, 'attributes')
            ),
            'Map(LowCardinality(String), String)'
        ) AS LogAttributes
    FROM (
        SELECT
            arrayJoin(JSONExtractArrayRaw(raw, 'resourceLogs'))     AS rm,
            arrayJoin(JSONExtractArrayRaw(rm, 'scopeLogs'))         AS sl,
            arrayJoin(JSONExtractArrayRaw(sl, 'logRecords'))        AS lr
        FROM kafka_otel_logs
    )
);

-- otel-metrics → otel_metrics_gauge
CREATE TABLE IF NOT EXISTS kafka_otel_metrics (
    raw String
) ENGINE = Kafka
SETTINGS
    kafka_broker_list          = '__KAFKA_BROKER__',
    kafka_topic_list           = 'otel-metrics',
    kafka_group_name           = 'clickhouse-__CLICKHOUSE_DB__-otel-metrics',
    kafka_format               = 'JSONAsString',
    kafka_num_consumers        = 1,
    kafka_skip_broken_messages = 10,
    kafka_security_protocol    = '__KAFKA_SECURITY_PROTOCOL__',
    kafka_sasl_mechanism       = '__KAFKA_SASL_MECHANISM__',
    kafka_sasl_username        = '__KAFKA_USERNAME__',
    kafka_sasl_password        = '__KAFKA_PASSWORD__';

CREATE MATERIALIZED VIEW IF NOT EXISTS kafka_otel_metrics_mv
TO otel_metrics_gauge AS
SELECT
    fromUnixTimestamp64Nano(
        toInt64OrZero(JSONExtractString(dp, 'timeUnixNano'))
    ) AS ts,
    ResAttrs['server_id'] AS server_id,
    JSONExtractString(metric, 'name') AS metric_name,
    JSONExtractFloat(dp, 'asDouble') AS value,
    CAST(
        arrayMap(
            x -> (
                JSONExtractString(x, 'key'),
                JSONExtractString(JSONExtractRaw(x, 'value'), 'stringValue')
            ),
            JSONExtractArrayRaw(dp, 'attributes')
        ),
        'Map(LowCardinality(String), String)'
    ) AS attributes
FROM (
    SELECT
        rm, metric, dp,
        CAST(
            arrayMap(
                x -> (
                    JSONExtractString(x, 'key'),
                    JSONExtractString(JSONExtractRaw(x, 'value'), 'stringValue')
                ),
                JSONExtractArrayRaw(JSONExtractRaw(rm, 'resource'), 'attributes')
            ),
            'Map(LowCardinality(String), String)'
        ) AS ResAttrs
    FROM (
        SELECT
            arrayJoin(JSONExtractArrayRaw(raw, 'resourceMetrics'))              AS rm,
            arrayJoin(JSONExtractArrayRaw(rm, 'scopeMetrics'))                  AS sm,
            arrayJoin(JSONExtractArrayRaw(sm, 'metrics'))                       AS metric,
            arrayJoin(
                JSONExtractArrayRaw(JSONExtractRaw(metric, 'gauge'), 'dataPoints')
            ) AS dp
        FROM kafka_otel_metrics
        WHERE JSONHas(metric, 'gauge')
    )
);

-- otel-traces → otel_traces_raw
CREATE TABLE IF NOT EXISTS kafka_otel_traces (
    raw String
) ENGINE = Kafka
SETTINGS
    kafka_broker_list          = '__KAFKA_BROKER__',
    kafka_topic_list           = 'otel-traces',
    kafka_group_name           = 'clickhouse-__CLICKHOUSE_DB__-otel-traces',
    kafka_format               = 'JSONAsString',
    kafka_num_consumers        = 1,
    kafka_skip_broken_messages = 10,
    kafka_security_protocol    = '__KAFKA_SECURITY_PROTOCOL__',
    kafka_sasl_mechanism       = '__KAFKA_SASL_MECHANISM__',
    kafka_sasl_username        = '__KAFKA_USERNAME__',
    kafka_sasl_password        = '__KAFKA_PASSWORD__';

CREATE MATERIALIZED VIEW IF NOT EXISTS kafka_otel_traces_mv
TO otel_traces_raw AS
SELECT now64(3) AS received_at, raw AS payload
FROM kafka_otel_traces;
