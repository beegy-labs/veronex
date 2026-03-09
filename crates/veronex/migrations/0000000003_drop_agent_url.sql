-- Drop deprecated agent_url column from llm_providers.
-- Hardware metrics are now resolved via server_id FK → gpu_servers.node_exporter_url.
ALTER TABLE llm_providers DROP COLUMN IF EXISTS agent_url;
