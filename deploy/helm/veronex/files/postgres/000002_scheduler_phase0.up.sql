ALTER TABLE llm_providers ADD COLUMN num_parallel SMALLINT NOT NULL DEFAULT 4;
CREATE UNIQUE INDEX uq_llm_providers_ollama_url ON llm_providers(url) WHERE provider_type = 'ollama';
