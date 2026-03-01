-- Track which API format the request arrived via.
-- The discriminator is the matched route path; no header convention is used.
-- Values: 'openai_compat' | 'ollama_native' | 'gemini_native' | 'veronex_native'
ALTER TABLE inference_jobs
    ADD COLUMN api_format TEXT NOT NULL DEFAULT 'openai_compat';
