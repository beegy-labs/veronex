CREATE TABLE IF NOT EXISTS gpu_servers (
    id                UUID         PRIMARY KEY,
    name              VARCHAR(255) NOT NULL,
    host              TEXT         NOT NULL,
    node_exporter_url TEXT,
    total_ram_mb      BIGINT       NOT NULL DEFAULT 0,
    registered_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);
