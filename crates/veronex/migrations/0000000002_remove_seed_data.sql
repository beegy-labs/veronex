-- Remove seed data from init migration.
-- Tables should start empty; settings are created on first use via the dashboard.

DELETE FROM capacity_settings;
DELETE FROM lab_settings;
DELETE FROM model_pricing;
DELETE FROM gemini_rate_limit_policies;
