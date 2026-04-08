-- ============================================================
-- Veronex ClickHouse schema (consolidated init)
-- Generated from migrations 000001–000005
-- Last updated: 2026-04-07
--
-- Ingest path: OTel Collector → Redpanda → Redpanda Connect
--              → ClickHouse HTTP INSERT (otel_logs / otel_metrics_gauge / otel_traces_raw)
-- ============================================================

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
ORDER BY (api_key_id, event_time)
TTL toDate(event_time) + INTERVAL __RETENTION_INFERENCE_DAYS__ DAY;

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
TTL toDate(Timestamp) + INTERVAL __RETENTION_LOGS_DAYS__ DAY;

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
TTL toDate(received_at) + INTERVAL __RETENTION_TRACES_DAYS__ DAY;

-- ── MCP tool call analytics ────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS mcp_tool_calls (
    event_time       DateTime64(3),
    request_id       UUID,
    api_key_id       UUID,
    tenant_id        LowCardinality(String),
    server_id        UUID,
    server_slug      LowCardinality(String),
    tool_name        LowCardinality(String),
    namespaced_name  LowCardinality(String),
    outcome          LowCardinality(String),
    cache_hit        UInt8,
    latency_ms       UInt32,
    result_bytes     UInt32,
    cap_charged      UInt8,
    loop_round       UInt8
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(event_time)
ORDER BY (api_key_id, event_time)
TTL toDate(event_time) + INTERVAL __RETENTION_MCP_DAYS__ DAY;

CREATE TABLE IF NOT EXISTS mcp_tool_calls_hourly (
    hour             DateTime,
    server_slug      LowCardinality(String),
    tool_name        LowCardinality(String),
    call_count       UInt64,
    success_count    UInt64,
    error_count      UInt64,
    cache_hit_count  UInt64,
    timeout_count    UInt64,
    avg_latency_ms   Float32,
    total_cap_charged UInt64
) ENGINE = AggregatingMergeTree()
PARTITION BY toYYYYMM(hour)
ORDER BY (server_slug, tool_name, hour);

CREATE MATERIALIZED VIEW IF NOT EXISTS mcp_tool_calls_hourly_mv
TO mcp_tool_calls_hourly AS
SELECT
    toStartOfHour(event_time)                   AS hour,
    server_slug,
    tool_name,
    count()                                     AS call_count,
    countIf(outcome = 'success')                AS success_count,
    countIf(outcome = 'error')                  AS error_count,
    countIf(cache_hit = 1)                      AS cache_hit_count,
    countIf(outcome = 'timeout')                AS timeout_count,
    avg(latency_ms)                             AS avg_latency_ms,
    sum(cap_charged)                            AS total_cap_charged
FROM mcp_tool_calls
GROUP BY hour, server_slug, tool_name;

-- OTel log stream → mcp_tool_calls (mirrors otel_inference_logs_mv pattern)
CREATE MATERIALIZED VIEW IF NOT EXISTS otel_mcp_tool_calls_mv
TO mcp_tool_calls AS
SELECT
    Timestamp                                   AS event_time,
    toUUIDOrZero(LogAttributes['request_id'])   AS request_id,
    toUUIDOrZero(LogAttributes['api_key_id'])   AS api_key_id,
    LogAttributes['tenant_id']                  AS tenant_id,
    toUUIDOrZero(LogAttributes['server_id'])    AS server_id,
    LogAttributes['server_slug']                AS server_slug,
    LogAttributes['tool_name']                  AS tool_name,
    LogAttributes['namespaced_name']            AS namespaced_name,
    LogAttributes['outcome']                    AS outcome,
    toUInt8OrZero(LogAttributes['cache_hit'])   AS cache_hit,
    toUInt32OrZero(LogAttributes['latency_ms']) AS latency_ms,
    toUInt32OrZero(LogAttributes['result_bytes']) AS result_bytes,
    toUInt8OrZero(LogAttributes['cap_charged']) AS cap_charged,
    toUInt8OrZero(LogAttributes['loop_round'])  AS loop_round
FROM otel_logs
WHERE LogAttributes['event.name'] = 'mcp.tool_call';
