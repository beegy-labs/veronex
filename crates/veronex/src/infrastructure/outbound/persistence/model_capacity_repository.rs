use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::application::ports::outbound::model_capacity_repository::{
    ModelVramProfileEntry, ModelCapacityRepository, ThroughputStats,
};

/// Explicit column list for `model_vram_profiles` SELECT queries (SQL fragment SSOT).
const VRAM_PROFILE_COLS: &str = "\
    provider_id, model_name, weight_mb, weight_estimated, kv_per_request_mb, \
    num_layers, num_kv_heads, head_dim, configured_ctx, max_ctx, failure_count, \
    llm_concern, llm_reason, max_concurrent, baseline_tps, baseline_p95_ms, updated_at";

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
    async fn upsert(&self, e: &ModelVramProfileEntry) -> Result<()> {
        sqlx::query(
            "INSERT INTO model_vram_profiles
                 (provider_id, model_name,
                  weight_mb, weight_estimated, kv_per_request_mb,
                  num_layers, num_kv_heads, head_dim, configured_ctx, max_ctx,
                  failure_count, llm_concern, llm_reason,
                  max_concurrent, baseline_tps, baseline_p95_ms, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)
             ON CONFLICT (provider_id, model_name) DO UPDATE SET
                 weight_mb         = EXCLUDED.weight_mb,
                 weight_estimated  = EXCLUDED.weight_estimated,
                 kv_per_request_mb = EXCLUDED.kv_per_request_mb,
                 num_layers        = EXCLUDED.num_layers,
                 num_kv_heads      = EXCLUDED.num_kv_heads,
                 head_dim          = EXCLUDED.head_dim,
                 configured_ctx    = EXCLUDED.configured_ctx,
                 max_ctx           = EXCLUDED.max_ctx,
                 failure_count     = EXCLUDED.failure_count,
                 llm_concern       = EXCLUDED.llm_concern,
                 llm_reason        = EXCLUDED.llm_reason,
                 max_concurrent    = EXCLUDED.max_concurrent,
                 baseline_tps      = EXCLUDED.baseline_tps,
                 baseline_p95_ms   = EXCLUDED.baseline_p95_ms,
                 updated_at        = EXCLUDED.updated_at",
        )
        .bind(e.provider_id)
        .bind(&e.model_name)
        .bind(e.weight_mb)
        .bind(e.weight_estimated)
        .bind(e.kv_per_request_mb)
        .bind(e.num_layers)
        .bind(e.num_kv_heads)
        .bind(e.head_dim)
        .bind(e.configured_ctx)
        .bind(e.max_ctx)
        .bind(e.failure_count)
        .bind(&e.llm_concern)
        .bind(&e.llm_reason)
        .bind(e.max_concurrent)
        .bind(e.baseline_tps)
        .bind(e.baseline_p95_ms)
        .bind(e.updated_at)
        .execute(&self.pool)
        .await
        .context("failed to upsert model_vram_profiles")?;

        Ok(())
    }

    async fn get(&self, provider_id: Uuid, model: &str) -> Result<Option<ModelVramProfileEntry>> {
        let row = sqlx::query_as::<_, VramProfileRow>(
            &format!("SELECT {VRAM_PROFILE_COLS} FROM model_vram_profiles WHERE provider_id = $1 AND model_name = $2"),
        )
        .bind(provider_id)
        .bind(model)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get model_vram_profiles")?;

        Ok(row.map(Into::into))
    }

    async fn list_all(&self) -> Result<Vec<ModelVramProfileEntry>> {
        let rows = sqlx::query_as::<_, VramProfileRow>(
            &format!("SELECT {VRAM_PROFILE_COLS} FROM model_vram_profiles ORDER BY provider_id, model_name LIMIT 10000"),
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to list model_vram_profiles")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_by_provider(&self, provider_id: Uuid) -> Result<Vec<ModelVramProfileEntry>> {
        let rows = sqlx::query_as::<_, VramProfileRow>(
            &format!("SELECT {VRAM_PROFILE_COLS} FROM model_vram_profiles WHERE provider_id = $1 ORDER BY model_name LIMIT 10000"),
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await
        .context("failed to list model_vram_profiles by provider")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn list_by_providers(&self, ids: &[Uuid]) -> Result<Vec<ModelVramProfileEntry>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let rows = sqlx::query_as::<_, VramProfileRow>(
            &format!("SELECT {VRAM_PROFILE_COLS} FROM model_vram_profiles WHERE provider_id = ANY($1) ORDER BY provider_id, model_name LIMIT 10000"),
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await
        .context("failed to list model_vram_profiles by providers")?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn compute_throughput_stats(
        &self,
        provider_id: Uuid,
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
                AVG(prompt_tokens::float)     AS avg_prompt_tokens,
                AVG(completion_tokens::float) AS avg_output_tokens,
                PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms::float)           AS p95_latency_ms,
                COUNT(*)               AS sample_count
             FROM inference_jobs
             WHERE provider_id    = $1
               AND model_name    = $2
               AND status        = 'completed'
               AND created_at    > now() - ($3 * INTERVAL '1 hour')
               AND latency_ms    > 0
               AND ttft_ms       > 0
               AND prompt_tokens > 0",
        )
        .bind(provider_id)
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

    async fn min_configured_ctx_for_model(&self, model: &str) -> Result<Option<u32>> {
        // Min across providers serving this model — request-entry prune budget
        // must fit even on the smallest-ctx provider that the dispatcher
        // might select. Uses the partial-index hint
        // `WHERE configured_ctx >= 4096` to skip corrupt / unprobed rows.
        // SDD: `.specs/veronex/conversation-context-compression.md` §3.
        let min: Option<i64> = sqlx::query_scalar(
            "SELECT MIN(configured_ctx)::int8
             FROM model_vram_profiles
             WHERE model_name = $1 AND configured_ctx >= 4096",
        )
        .bind(model)
        .fetch_one(&self.pool)
        .await
        .context("min_configured_ctx_for_model")?;
        Ok(min.map(|n| n.max(0) as u32))
    }

    async fn has_unprofiled_selected_models(&self) -> Result<bool> {
        // Returns true when any (provider_id, model_name) pair in
        // `provider_selected_models` lacks a row in `model_vram_profiles`.
        // The analyzer uses this to bypass the demand-skip gate so a freshly
        // selected model still gets its first probe even before any user
        // traffic arrives.
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1
                 FROM provider_selected_models psm
                 LEFT JOIN model_vram_profiles mvp
                   ON mvp.provider_id = psm.provider_id
                  AND mvp.model_name  = psm.model_name
                 WHERE mvp.model_name IS NULL
             )",
        )
        .fetch_one(&self.pool)
        .await
        .context("failed to check for unprofiled selected models")?;
        Ok(exists)
    }
}

// ── Internal row type for sqlx ────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct VramProfileRow {
    provider_id:       Uuid,
    model_name:        String,
    weight_mb:         i32,
    weight_estimated:  bool,
    kv_per_request_mb: i32,
    num_layers:        i16,
    num_kv_heads:      i16,
    head_dim:          i16,
    configured_ctx:    i32,
    max_ctx:           i32,
    failure_count:     i16,
    llm_concern:       Option<String>,
    llm_reason:        Option<String>,
    max_concurrent:    i32,
    baseline_tps:      i32,
    baseline_p95_ms:   i32,
    updated_at:        chrono::DateTime<Utc>,
}

impl From<VramProfileRow> for ModelVramProfileEntry {
    fn from(r: VramProfileRow) -> Self {
        Self {
            provider_id:       r.provider_id,
            model_name:        r.model_name,
            weight_mb:         r.weight_mb,
            weight_estimated:  r.weight_estimated,
            kv_per_request_mb: r.kv_per_request_mb,
            num_layers:        r.num_layers,
            num_kv_heads:      r.num_kv_heads,
            head_dim:          r.head_dim,
            configured_ctx:    r.configured_ctx,
            max_ctx:           r.max_ctx,
            failure_count:     r.failure_count,
            llm_concern:       r.llm_concern,
            llm_reason:        r.llm_reason,
            max_concurrent:    r.max_concurrent,
            baseline_tps:      r.baseline_tps,
            baseline_p95_ms:   r.baseline_p95_ms,
            updated_at:        r.updated_at,
        }
    }
}
