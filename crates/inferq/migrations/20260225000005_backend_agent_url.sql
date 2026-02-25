-- Optional URL of an inferq-agent (or node-exporter) running on the Ollama server.
-- When set, the health checker polls this endpoint every 30 s to collect
-- real-time GPU temperature, VRAM usage, and system RAM.
ALTER TABLE llm_backends
    ADD COLUMN IF NOT EXISTS agent_url TEXT;
