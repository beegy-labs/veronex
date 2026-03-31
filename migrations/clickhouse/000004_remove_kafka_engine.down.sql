-- Rollback 000004: Restore ClickHouse Kafka Engine tables
--
-- Reverts to the internal Kafka Engine consumer pattern.
-- Run only if rolling back to the previous architecture.

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
    fromUnixTimestamp64Nano(toInt64OrZero(JSONExtractString(dp, 'timeUnixNano'))) AS ts,
    ResAttrs['server_id'] AS server_id,
    JSONExtractString(metric, 'name') AS metric_name,
    JSONExtractFloat(dp, 'asDouble') AS value,
    CAST(
        arrayMap(
            x -> (JSONExtractString(x, 'key'), JSONExtractString(JSONExtractRaw(x, 'value'), 'stringValue')),
            JSONExtractArrayRaw(dp, 'attributes')
        ),
        'Map(LowCardinality(String), String)'
    ) AS attributes
FROM (
    SELECT rm, metric, dp,
        CAST(
            arrayMap(
                x -> (JSONExtractString(x, 'key'), JSONExtractString(JSONExtractRaw(x, 'value'), 'stringValue')),
                JSONExtractArrayRaw(JSONExtractRaw(rm, 'resource'), 'attributes')
            ),
            'Map(LowCardinality(String), String)'
        ) AS ResAttrs
    FROM (
        SELECT
            arrayJoin(JSONExtractArrayRaw(raw, 'resourceMetrics'))   AS rm,
            arrayJoin(JSONExtractArrayRaw(rm, 'scopeMetrics'))       AS sm,
            arrayJoin(JSONExtractArrayRaw(sm, 'metrics'))            AS metric,
            arrayJoin(if(JSONHas(metric, 'gauge'),
                JSONExtractArrayRaw(JSONExtractRaw(metric, 'gauge'), 'dataPoints'),
                JSONExtractArrayRaw(JSONExtractRaw(metric, 'sum'), 'dataPoints')
            )) AS dp
        FROM kafka_otel_metrics
        WHERE JSONHas(metric, 'gauge') OR JSONHas(metric, 'sum')
    )
);

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
