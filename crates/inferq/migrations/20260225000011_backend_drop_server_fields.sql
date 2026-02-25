ALTER TABLE llm_backends
    DROP COLUMN IF EXISTS node_exporter_url,
    DROP COLUMN IF EXISTS total_ram_mb;
