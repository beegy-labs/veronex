-- Lab (experimental) feature flags.
-- Singleton row (id = 1 enforced by CHECK).
-- New experimental features default to false — must be explicitly enabled.

CREATE TABLE lab_settings (
    id                      INT         PRIMARY KEY DEFAULT 1 CHECK (id = 1),

    -- Gemini function-calling (tool use) support.
    -- Still in development: disabled by default.
    gemini_function_calling BOOLEAN     NOT NULL DEFAULT false,

    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO lab_settings DEFAULT VALUES;
