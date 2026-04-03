-- Context compression, multi-turn gate, vision model, and session handoff settings.
ALTER TABLE lab_settings
  -- compression
  ADD COLUMN IF NOT EXISTS context_compression_enabled  BOOLEAN  NOT NULL DEFAULT false,
  ADD COLUMN IF NOT EXISTS compression_model            TEXT,
  ADD COLUMN IF NOT EXISTS context_budget_ratio         REAL     NOT NULL DEFAULT 0.60,
  ADD COLUMN IF NOT EXISTS compression_trigger_turns    INT      NOT NULL DEFAULT 1,
  ADD COLUMN IF NOT EXISTS recent_verbatim_window       INT      NOT NULL DEFAULT 1,
  ADD COLUMN IF NOT EXISTS compression_timeout_secs     INT      NOT NULL DEFAULT 10,
  -- multi-turn gate
  ADD COLUMN IF NOT EXISTS multiturn_min_params         INT      NOT NULL DEFAULT 7,
  ADD COLUMN IF NOT EXISTS multiturn_min_ctx            INT      NOT NULL DEFAULT 16384,
  ADD COLUMN IF NOT EXISTS multiturn_allowed_models     TEXT[]   NOT NULL DEFAULT '{}',
  -- vision
  ADD COLUMN IF NOT EXISTS vision_model                 TEXT,
  -- handoff
  ADD COLUMN IF NOT EXISTS handoff_enabled              BOOLEAN  NOT NULL DEFAULT true;
