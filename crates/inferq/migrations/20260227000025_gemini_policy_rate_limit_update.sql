-- Update Gemini free-tier rate limits to reflect actual 2026-02 Google AI Studio values.
-- Previous seed (migration 000017) used estimated/outdated values.
-- Source: Google AI Studio rate-limit page (verified 2026-02-27).
--
-- Free tier (available_on_free_tier = true):
--   gemini-2.5-flash      → RPM 5,  RPD 20   (was 10/250)
--   gemini-2.5-flash-lite → RPM 10, RPD 20   (was 15/1000)
--   gemini-3-flash-preview→ RPM 5,  RPD 20   (new entry)
--
-- Not on free tier (available_on_free_tier = false):
--   gemini-2.5-pro        → RPM 5,  RPD 25   (removed from free Dec 2025)
--   * (global default)    → available_on_free_tier = false (conservative)

INSERT INTO gemini_rate_limit_policies (model_name, rpm_limit, rpd_limit, available_on_free_tier) VALUES
    ('gemini-2.5-flash',        5,  20, true),
    ('gemini-2.5-flash-lite',  10,  20, true),
    ('gemini-3-flash-preview',  5,  20, true),
    ('gemini-2.5-pro',          5,  25, false),
    ('*',                      10, 250, false)
ON CONFLICT (model_name) DO UPDATE SET
    rpm_limit              = EXCLUDED.rpm_limit,
    rpd_limit              = EXCLUDED.rpd_limit,
    available_on_free_tier = EXCLUDED.available_on_free_tier,
    updated_at             = now();
