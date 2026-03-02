-- Persist the full LLM input context for training data collection.
--
-- `messages_json` stores the complete messages array that was sent to the model:
--   - system prompt
--   - prior conversation turns (user + assistant + tool responses)
--   - current user message
--
-- This is the ground truth input for fine-tuning:
--   input  = messages_json  (24k+ tokens for agentic sessions with file contents)
--   output = result_text + tool_calls_json
--
-- Column is NULLABLE: legacy rows and single-prompt /api/generate jobs
-- that have no messages array keep NULL here.
--
-- Storage note: large agentic sessions can reach 100–500 KB per row.
-- A GIN index is NOT added here (no containment queries planned);
-- use conversation_id index to group related turns.

ALTER TABLE inference_jobs
    ADD COLUMN IF NOT EXISTS messages_json JSONB;
