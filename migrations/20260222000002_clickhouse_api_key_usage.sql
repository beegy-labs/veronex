CREATE TABLE api_key_usage_hourly (
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

CREATE MATERIALIZED VIEW api_key_usage_hourly_mv
TO api_key_usage_hourly AS
SELECT
    toStartOfHour(event_time)           AS hour,
    api_key_id,
    tenant_id,
    count()                              AS request_count,
    countIf(finish_reason = 'stop')      AS success_count,
    countIf(finish_reason = 'cancelled') AS cancelled_count,
    countIf(finish_reason = 'error')     AS error_count,
    sum(prompt_tokens)                   AS prompt_tokens,
    sum(completion_tokens)               AS completion_tokens,
    sum(prompt_tokens + completion_tokens) AS total_tokens
FROM inference_logs
GROUP BY hour, api_key_id, tenant_id;
