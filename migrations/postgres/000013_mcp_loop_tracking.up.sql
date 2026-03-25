-- Track the full agentic loop as a single unit.
-- mcp_loop_id groups all inference_jobs in one run_loop() execution,
-- and mcp_loop_tool_calls stores each MCP tool execution with its result.

ALTER TABLE inference_jobs
    ADD COLUMN mcp_loop_id UUID;

CREATE INDEX idx_inference_jobs_mcp_loop_id
    ON inference_jobs(mcp_loop_id)
    WHERE mcp_loop_id IS NOT NULL;

CREATE TABLE mcp_loop_tool_calls (
    id               UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    mcp_loop_id      UUID        NOT NULL,
    job_id           UUID        NOT NULL REFERENCES inference_jobs(id) ON DELETE CASCADE,
    loop_round       SMALLINT    NOT NULL,
    server_id        UUID        NOT NULL,
    tool_name        TEXT        NOT NULL,  -- raw name on MCP server (e.g. get_weather)
    namespaced_name  TEXT        NOT NULL,  -- full name injected to LLM (e.g. mcp_weather_mcp_get_weather)
    args_json        JSONB       NOT NULL,
    result_text      TEXT,                  -- NULL on error/timeout
    outcome          TEXT        NOT NULL,  -- success|error|timeout|circuit_open|cache_hit
    cache_hit        BOOLEAN     NOT NULL DEFAULT false,
    latency_ms       INT,
    result_bytes     INT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_mcp_loop_tool_calls_loop   ON mcp_loop_tool_calls(mcp_loop_id);
CREATE INDEX idx_mcp_loop_tool_calls_job    ON mcp_loop_tool_calls(job_id);
CREATE INDEX idx_mcp_loop_tool_calls_server ON mcp_loop_tool_calls(server_id, created_at DESC);
