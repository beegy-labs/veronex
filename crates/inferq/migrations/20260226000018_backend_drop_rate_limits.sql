-- rpm_limit / rpd_limit are now managed in gemini_rate_limit_policies (per model, shared)
ALTER TABLE llm_backends
    DROP COLUMN IF EXISTS rpm_limit,
    DROP COLUMN IF EXISTS rpd_limit;
