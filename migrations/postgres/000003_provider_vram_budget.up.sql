-- ── Provider VRAM Budget ──────────────────────────────────────────────────────
-- Persistent VRAM management state per Ollama provider.
-- Complements llm_providers (which holds num_parallel and total_vram_mb).
-- Survives server restart so AIMD safety margins and source attribution are preserved.

CREATE TABLE provider_vram_budget (
    provider_id       UUID        PRIMARY KEY REFERENCES llm_providers(id) ON DELETE CASCADE,
    safety_permil     INT         NOT NULL DEFAULT 100,      -- safety margin ÷1000; 100=10%, max 500
    vram_total_source TEXT        NOT NULL DEFAULT 'probe',  -- 'probe' | 'node_exporter' | 'manual'
    kv_cache_type     TEXT        NOT NULL DEFAULT 'q8_0',   -- 'f16' | 'q8_0' | 'q4_0'
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
