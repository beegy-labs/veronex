-- Add message hash columns for server-side session grouping.
--
-- messages_hash:        Blake2b-256 of the full messages array (serialized JSON).
--                       Used as the "identity" of a job's context snapshot.
--
-- messages_prefix_hash: Blake2b-256 of messages[0..-1] (all turns except the last).
--                       When job B's prefix_hash == job A's messages_hash,
--                       they belong to the same conversation chain.
--                       Empty string = first turn (no prior context).
--
-- Both are NULL for jobs with no messages (single-prompt / legacy jobs).

ALTER TABLE inference_jobs
    ADD COLUMN messages_hash        TEXT,
    ADD COLUMN messages_prefix_hash TEXT;

-- Fast lookup: given a prefix_hash, find the parent job in the same key's history.
CREATE INDEX idx_inference_jobs_messages_hash
    ON inference_jobs (api_key_id, messages_hash)
    WHERE messages_hash IS NOT NULL;

-- Fast scan: find ungrouped jobs that have a linkable prefix.
CREATE INDEX idx_inference_jobs_session_ungrouped
    ON inference_jobs (api_key_id, messages_prefix_hash, created_at)
    WHERE conversation_id IS NULL
      AND messages_prefix_hash IS NOT NULL
      AND messages_prefix_hash != '';
