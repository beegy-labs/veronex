-- G16: Add failure_reason column for machine-readable failure cause tracking.
-- Values: queue_full, no_eligible_provider, thermal_hard_gate, drain_forced,
--         queue_wait_exceeded, provider_error, token_budget_exceeded
ALTER TABLE inference_jobs ADD COLUMN IF NOT EXISTS failure_reason TEXT;
