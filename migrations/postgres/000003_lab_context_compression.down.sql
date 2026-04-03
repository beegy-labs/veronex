ALTER TABLE lab_settings
  DROP COLUMN IF EXISTS context_compression_enabled,
  DROP COLUMN IF EXISTS compression_model,
  DROP COLUMN IF EXISTS context_budget_ratio,
  DROP COLUMN IF EXISTS compression_trigger_turns,
  DROP COLUMN IF EXISTS recent_verbatim_window,
  DROP COLUMN IF EXISTS compression_timeout_secs,
  DROP COLUMN IF EXISTS multiturn_min_params,
  DROP COLUMN IF EXISTS multiturn_min_ctx,
  DROP COLUMN IF EXISTS multiturn_allowed_models,
  DROP COLUMN IF EXISTS vision_model,
  DROP COLUMN IF EXISTS handoff_enabled;
