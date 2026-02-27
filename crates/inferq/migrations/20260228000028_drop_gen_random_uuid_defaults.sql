-- Migrate all UUID primary keys to use PostgreSQL 18 native uuidv7().
-- Previously two tables had gen_random_uuid() (UUIDv4) as default;
-- remaining tables had no DB default (relying solely on app-side Uuid::now_v7()).
-- Now every UUID PK has DEFAULT uuidv7() as a consistent DB-level fallback.

ALTER TABLE api_keys                  ALTER COLUMN id SET DEFAULT uuidv7();
ALTER TABLE inference_jobs            ALTER COLUMN id SET DEFAULT uuidv7();
ALTER TABLE llm_backends              ALTER COLUMN id SET DEFAULT uuidv7();
ALTER TABLE gpu_servers               ALTER COLUMN id SET DEFAULT uuidv7();
ALTER TABLE gemini_rate_limit_policies ALTER COLUMN id SET DEFAULT uuidv7();
ALTER TABLE ollama_sync_jobs          ALTER COLUMN id SET DEFAULT uuidv7();
