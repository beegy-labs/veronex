DROP INDEX IF EXISTS uq_llm_providers_ollama_url;
ALTER TABLE llm_providers DROP COLUMN IF EXISTS num_parallel;
