-- Maximum RAM allocation for this backend in MiB.
-- Phase 1: manually entered (informational, used for dashboard display).
-- Phase 2: will be populated from K8s resource limits / cgroup max.
ALTER TABLE llm_backends ADD COLUMN IF NOT EXISTS total_ram_mb BIGINT NOT NULL DEFAULT 0;
