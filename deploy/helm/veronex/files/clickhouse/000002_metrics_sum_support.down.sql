-- Revert to gauge-only MV
DROP VIEW IF EXISTS kafka_otel_metrics_mv;

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
