-- Conversation threading + structured tool call storage
--
-- conversation_id: groups all LLM turns in one agent session.
--   Set from the X-Conversation-ID request header (client-supplied).
--   NULL for single-turn requests or clients that don't send the header.
--
-- tool_calls_json: JSONB array of tool calls returned by the model.
--   Stored separately from result_text so they are queryable and exportable
--   as training data without string parsing.
--   Format: Ollama tool_calls array  (normalized from Gemini functionCall on ingest).
--   NULL when the model produced only text output.

ALTER TABLE inference_jobs
    ADD COLUMN IF NOT EXISTS conversation_id  TEXT,
    ADD COLUMN IF NOT EXISTS tool_calls_json  JSONB;

CREATE INDEX IF NOT EXISTS idx_inference_jobs_conversation_id
    ON inference_jobs(conversation_id)
    WHERE conversation_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_inference_jobs_tool_calls
    ON inference_jobs USING GIN (tool_calls_json)
    WHERE tool_calls_json IS NOT NULL;
