-- Gemini rate limit policies (shared per model, editable from admin UI)
CREATE TABLE IF NOT EXISTS gemini_rate_limit_policies (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    model_name  TEXT        NOT NULL UNIQUE,   -- "*" = global default
    rpm_limit   INTEGER     NOT NULL DEFAULT 0,
    rpd_limit   INTEGER     NOT NULL DEFAULT 0,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Seed known 2026 free-tier limits
INSERT INTO gemini_rate_limit_policies (model_name, rpm_limit, rpd_limit) VALUES
    ('gemini-2.5-pro',        5,   100),
    ('gemini-2.5-flash',     10,   250),
    ('gemini-2.5-flash-lite', 15, 1000),
    ('*',                    10,   250)
ON CONFLICT (model_name) DO NOTHING;
