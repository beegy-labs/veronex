-- Add available_on_free_tier flag to gemini_rate_limit_policies.
--
-- When false: the model is NOT available on Google free-tier projects.
-- The router will skip all free-tier backends and route directly to a paid backend.
-- RPM/RPD counters are also skipped for paid backends (no limit to enforce).
--
-- Default: true — existing policies are assumed to be free-tier available.
ALTER TABLE gemini_rate_limit_policies
    ADD COLUMN IF NOT EXISTS available_on_free_tier BOOLEAN NOT NULL DEFAULT true;
