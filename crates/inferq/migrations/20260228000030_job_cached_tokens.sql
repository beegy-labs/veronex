-- Cached tokens: prompt tokens served from KV/context cache.
-- Gemini: cachedContentTokenCount (billed at ~25% of normal input rate).
-- Ollama: always NULL (KV cache is internal, not exposed via API).
ALTER TABLE inference_jobs
    ADD COLUMN IF NOT EXISTS cached_tokens INTEGER;
