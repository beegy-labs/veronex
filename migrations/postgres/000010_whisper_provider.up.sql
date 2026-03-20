-- Extend llm_providers.provider_type to allow 'whisper' (STT provider).
ALTER TABLE llm_providers
    DROP CONSTRAINT IF EXISTS llm_providers_provider_type_check;

ALTER TABLE llm_providers
    ADD CONSTRAINT llm_providers_provider_type_check
    CHECK (provider_type IN ('ollama', 'gemini', 'whisper'));
