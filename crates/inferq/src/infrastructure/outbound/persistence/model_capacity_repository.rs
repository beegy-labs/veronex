use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::model_capacity_repository::{
    ModelCapacityEntry, ModelCapacityRepository, ThroughputStats,
};

pub struct PostgresModelCapacityRepository {
    pool: PgPool,
}

impl PostgresModelCapacityRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ModelCapacityRepository for PostgresModelCapacityRepository {
    async fn upsert(&self, e: &ModelCapacityEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO model_capacity
                 (backend_id, model_name,
                  vram_model_mb, vram_total_mb,
                  arch_num_layers, arch_num_kv_heads, arch_head_dim, arch_configured_ctx,
                  vram_kv_per_slot_mb, vram_kv_worst_case_mb,
                  recommended_slots,
                  avg_tokens_per_sec, avg_prefill_tps, avg_prompt_tokens, avg_output_tokens,
                  p95_latency_ms, sample_count,
                  llm_concern, llm_reason, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)
             ON CONFLICT (backend_id, model_name) DO UPDATE SET
                 vram_model_mb         = EXCLUDED.vram_model_mb,
                 vram_total_mb         = EXCLUDED.vram_total_mb,
                 arch_num_layers       = EXCLUDED.arch_num_layers,
                 arch_num_kv_heads     = EXCLUDED.arch_num_kv_heads,
                 arch_head_dim         = EXCLUDED.arch_head_dim,
                 arch_configured_ctx   = EXCLUDED.arch_configured_ctx,
                 vram_kv_per_slot_mb   = EXCLUDED.vram_kv_per_slot_mb,
                 vram_kv_worst_case_mb = EXCLUDED.vram_kv_worst_case_mb,
                 recommended_slots     = EXCLUDED.recommended_slots,
                 avg_tokens_per_sec    = EXCLUDED.avg_tokens_per_sec,
                 avg_prefill_tps       = EXCLUDED.avg_prefill_tps,
                 avg_prompt_tokens     = EXCLUDED.avg_prompt_tokens,
                 avg_output_tokens     = EXCLUDED.avg_output_tokens,
                 p95_latency_ms        = EXCLUDED.p95_latency_ms,
                 sample_count          = EXCLUDED.sample_count,
                 llm_concern           = EXCLUDED.llm_concern,
                 llm_reason            = EXCLUDED.llm_reason,
                 updated_at            = EXCLUDED.updated_at",
        )
        .bind(e.backend_id)
        .bind(&e.model_name)
        .bind(e.vram_model_mb)
        .bind(e.vram_total_mb)
        .bind(e.arch_num_layers)
        .bind(e.arch_num_kv_heads)
        .bind(e.arch_head_dim)
        .bind(e.arch_configured_ctx)
        .bind(e.vram_kv_per_slot_mb)
        .bind(e.vram_kv_worst_case_mb)
        .bind(e.recommended_slots)
        .bind(e.avg_tokens_per_sec)
        .bind(e.avg_prefill_tps)
        .bind(e.avg_prompt_tokens)
        .bind(e.avg_output_tokens)
        .bind(e.p95_latency_ms)
        .bind(e.sample_count)
        .bind(&e.llm_concern)
        .bind(&e.llm_reason)
        .bind(e.updated_at)
        .execute(&self.pool)
        .await
        .context("failed to upsert model_capacity")?;

        Ok(())
    }

    async fn get(&self, backend_id: Uuid, model: &str) -> Result<Option<ModelCapacityEntry>> {
        let row = sqlx::query_as::<_, ModelCapacityRow>(
            "SELECT * FROM model_capacity WHERE backend_id = $1 AND model_name = $2",
        )
        .bind(backend_id)
        .bind(model)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get model_capacity")?;

        Ok(row.map(Into::into))
    }

    async fn list_all(&self) -> Result<Vec<ModelCapacityEntry>> {
        let rows = sqlx::query_as::<_, ModelCapacityRow>(
            "SELECT * FROM model_capacity ORDER BY backend_id, model_name",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list model_capacity")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn compute_throughput_stats(
        &self,
        backend_id: Uuid,
        model: &str,
        window_hours: u32,
    ) -> Result<Option<ThroughputStats>> {
        #[allow(dead_code)]
        #[derive(sqlx::FromRow)]
        struct StatsRow {
            avg_tokens_per_sec: Option<f64>,
            avg_prefill_tps:    Option<f64>,
            avg_prompt_tokens:  Option<f64>,
            avg_output_tokens:  Option<f64>,
            p95_latency_ms:     Option<f64>,
            sample_count:       Option<i64>,
        }

        let row = sqlx::query_as::<_, StatsRow>(
            "SELECT
                AVG(completion_tokens::float / NULLIF(latency_ms - ttft_ms, 0) * 1000.0) AS avg_tokens_per_sec,
                AVG(prompt_tokens::float     / NULLIF(ttft_ms, 0) * 1000.0)               AS avg_prefill_tps,
                AVG(prompt_tokens)     AS avg_prompt_tokens,
                AVG(completion_tokens) AS avg_output_tokens,
                PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms)                  AS p95_latency_ms,
                COUNT(*)               AS sample_count
             FROM inference_jobs
             WHERE backend_id    = $1
               AND model_name    = $2
               AND status        = 'completed'
               AND created_at    > now() - ($3 * INTERVAL '1 hour')
               AND latency_ms    > 0
               AND ttft_ms       > 0
               AND prompt_tokens > 0",
        )
        .bind(backend_id)
        .bind(model)
        .bind(window_hours as i32)
        .fetch_optional(&self.pool)
        .await
        .context("failed to compute throughput stats")?;

        Ok(row.and_then(|r| {
            let count = r.sample_count.unwrap_or(0);
            if count == 0 {
                None
            } else {
                Some(ThroughputStats {
                    avg_tokens_per_sec: r.avg_tokens_per_sec.unwrap_or(0.0),
                    avg_prefill_tps:    r.avg_prefill_tps.unwrap_or(0.0),
                    avg_prompt_tokens:  r.avg_prompt_tokens.unwrap_or(0.0),
                    avg_output_tokens:  r.avg_output_tokens.unwrap_or(0.0),
                    p95_latency_ms:     r.p95_latency_ms.unwrap_or(0.0),
                    sample_count:       count,
                })
            }
        }))
    }
}

// ── Internal row type for sqlx ────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct ModelCapacityRow {
    backend_id:            Uuid,
    model_name:            String,
    vram_model_mb:         i32,
    vram_total_mb:         i32,
    arch_num_layers:       i32,
    arch_num_kv_heads:     i32,
    arch_head_dim:         i32,
    arch_configured_ctx:   i32,
    vram_kv_per_slot_mb:   i32,
    vram_kv_worst_case_mb: i32,
    recommended_slots:     i16,
    avg_tokens_per_sec:    f64,
    avg_prefill_tps:       f64,
    avg_prompt_tokens:     f64,
    avg_output_tokens:     f64,
    p95_latency_ms:        f64,
    sample_count:          i32,
    llm_concern:           Option<String>,
    llm_reason:            Option<String>,
    updated_at:            chrono::DateTime<Utc>,
}

impl From<ModelCapacityRow> for ModelCapacityEntry {
    fn from(r: ModelCapacityRow) -> Self {
        Self {
            backend_id:            r.backend_id,
            model_name:            r.model_name,
            vram_model_mb:         r.vram_model_mb,
            vram_total_mb:         r.vram_total_mb,
            arch_num_layers:       r.arch_num_layers,
            arch_num_kv_heads:     r.arch_num_kv_heads,
            arch_head_dim:         r.arch_head_dim,
            arch_configured_ctx:   r.arch_configured_ctx,
            vram_kv_per_slot_mb:   r.vram_kv_per_slot_mb,
            vram_kv_worst_case_mb: r.vram_kv_worst_case_mb,
            recommended_slots:     r.recommended_slots,
            avg_tokens_per_sec:    r.avg_tokens_per_sec,
            avg_prefill_tps:       r.avg_prefill_tps,
            avg_prompt_tokens:     r.avg_prompt_tokens,
            avg_output_tokens:     r.avg_output_tokens,
            p95_latency_ms:        r.p95_latency_ms,
            sample_count:          r.sample_count,
            llm_concern:           r.llm_concern,
            llm_reason:            r.llm_reason,
            updated_at:            r.updated_at,
        }
    }
}
