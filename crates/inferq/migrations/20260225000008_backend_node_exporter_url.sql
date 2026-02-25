-- node-exporter endpoint for this backend's host.
-- e.g. "http://192.168.1.10:9100"
-- Used by inferq to serve Prometheus HTTP SD targets for the OTel Collector.
ALTER TABLE llm_backends ADD COLUMN IF NOT EXISTS node_exporter_url TEXT;
