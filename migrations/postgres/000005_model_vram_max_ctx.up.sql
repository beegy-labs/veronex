-- Add max_ctx to model_vram_profiles.
-- Populated by the capacity analyzer from Ollama /api/show (model_info.llama.context_length).
-- Exposed in GET /v1/ollama/models for frontend model-selector context-window warnings.
ALTER TABLE model_vram_profiles
    ADD COLUMN IF NOT EXISTS max_ctx INT NOT NULL DEFAULT 0;
