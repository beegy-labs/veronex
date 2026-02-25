-- Add free-tier flag and per-backend rate limit configuration to llm_backends.
-- is_free_tier: true = Google free project (rate limits enforced by inferq)
-- rpm_limit:    requests per minute limit; 0 = no local enforcement
-- rpd_limit:    requests per day limit;    0 = no local enforcement
ALTER TABLE llm_backends
    ADD COLUMN IF NOT EXISTS is_free_tier BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS rpm_limit    INTEGER  NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS rpd_limit    INTEGER  NOT NULL DEFAULT 0;
