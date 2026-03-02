-- Model pricing table for estimating token costs per job.
-- Ollama (self-hosted) has no rows — cost is always $0.00.
-- Gemini rows use exact model name matching; '*' is the default fallback.
CREATE TABLE model_pricing (
    provider      TEXT    NOT NULL,
    model_name    TEXT    NOT NULL,   -- exact name or '*' for default fallback
    input_per_1m  FLOAT8  NOT NULL DEFAULT 0,
    output_per_1m FLOAT8  NOT NULL DEFAULT 0,
    currency      TEXT    NOT NULL DEFAULT 'USD',
    notes         TEXT,
    PRIMARY KEY (provider, model_name)
);

-- Gemini pricing (Google AI Studio, 2026-03)
INSERT INTO model_pricing (provider, model_name, input_per_1m, output_per_1m, notes) VALUES
    ('gemini', 'gemini-2.0-flash',                   0.10,   0.40,  'Gemini 2.0 Flash'),
    ('gemini', 'gemini-2.0-flash-lite',              0.075,  0.30,  'Gemini 2.0 Flash Lite'),
    ('gemini', 'gemini-2.0-flash-thinking-exp',      0.10,   0.40,  'Gemini 2.0 Flash Thinking'),
    ('gemini', 'gemini-2.0-flash-thinking-exp-01-21',0.10,   0.40,  'Gemini 2.0 Flash Thinking 0121'),
    ('gemini', 'gemini-2.0-pro-exp',                 1.25,  10.00,  'Gemini 2.0 Pro Exp'),
    ('gemini', 'gemini-2.5-pro-preview-03-25',       1.25,  10.00,  'Gemini 2.5 Pro Preview'),
    ('gemini', 'gemini-1.5-flash',                   0.075,  0.30,  'Gemini 1.5 Flash'),
    ('gemini', 'gemini-1.5-flash-8b',                0.0375, 0.15,  'Gemini 1.5 Flash 8B'),
    ('gemini', 'gemini-1.5-pro',                     1.25,   5.00,  'Gemini 1.5 Pro'),
    ('gemini', 'gemini-1.0-pro',                     0.50,   1.50,  'Gemini 1.0 Pro'),
    ('gemini', '*',                                  0.10,   0.40,  'Gemini (default fallback)');
