-- Extract MCP tool call events from the OTel log stream into mcp_tool_calls.
-- Mirrors the pattern used by otel_inference_logs_mv for inference events.
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
FROM veronex.otel_logs
WHERE LogAttributes['event.name'] = 'mcp.tool_call';
