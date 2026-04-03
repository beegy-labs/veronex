-- Add handoff_threshold to lab_settings (fraction of configured_ctx that triggers session handoff).
ALTER TABLE lab_settings
  ADD COLUMN IF NOT EXISTS handoff_threshold REAL NOT NULL DEFAULT 0.85;
