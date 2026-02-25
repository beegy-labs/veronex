-- Add GPU index for manual correlation with node-exporter / OTel metrics.
-- When a node has multiple GPUs, each Ollama backend pod targets a specific GPU.
ALTER TABLE llm_backends ADD COLUMN IF NOT EXISTS gpu_index SMALLINT;
