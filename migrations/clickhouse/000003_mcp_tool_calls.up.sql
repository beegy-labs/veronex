-- MCP tool call analytics.
-- Populated via OTel spans (tracing → opentelemetry-otlp → Redpanda → ClickHouse).
--
-- Retention: controlled by __RETENTION_MCP_DAYS__ placeholder (default: 90 days).

CREATE TABLE IF NOT EXISTS mcp_tool_calls (
    -- When the tool call completed
    event_time       DateTime64(3),
    -- Veronex request context
    request_id       UUID,
    api_key_id       UUID,
    tenant_id        LowCardinality(String),
    -- MCP server
    server_id        UUID,
    server_slug      LowCardinality(String),
    -- Tool invocation
    tool_name        LowCardinality(String),
    namespaced_name  LowCardinality(String),
    -- Outcome: 'success' | 'error' | 'timeout' | 'circuit_open' | 'cache_hit' | 'skipped'
    outcome          LowCardinality(String),
    -- Whether result was served from cache (no network call)
    cache_hit        UInt8,
    -- Latency in milliseconds (0 for cache_hit / circuit_open)
    latency_ms       UInt32,
    -- Content size of the tool result (bytes of JSON)
    result_bytes     UInt32,
    -- Number of cap points charged (0 for failure / cache_hit)
    cap_charged      UInt8,
    -- Agent loop round number (1-based)
    loop_round       UInt8
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(event_time)
ORDER BY (api_key_id, event_time)
TTL toDate(event_time) + INTERVAL 90 DAY;

-- Per-server hourly aggregation for dashboard
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
