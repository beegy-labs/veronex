/// Unified sync loop — runs in the background, combines:
/// 1. Health check (GET /api/version)
/// 2. Model sync (GET /api/tags → DB + Valkey cache)
/// 3. VRAM probing (GET /api/ps + POST /api/show → VramPool update)
/// 4. LLM analysis (qwen2.5:3b) for concern/reason
///
/// Replaces the old separate health_checker (Ollama portion), model sync,
/// and capacity analysis loops.
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::application::ports::outbound::capacity_settings_repository::CapacitySettingsRepository;
use crate::application::ports::outbound::concurrency_port::{ModelVramProfile, VramPoolPort};
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::application::ports::outbound::model_capacity_repository::{
    ModelVramProfileEntry, ModelCapacityRepository, ThroughputStats,
};
use crate::application::ports::outbound::ollama_model_repository::OllamaModelRepository;
use crate::application::ports::outbound::provider_model_selection::ProviderModelSelectionRepository;
use crate::domain::constants::{
    LLM_ANALYSIS_TIMEOUT, LLM_BATCH_ANALYSIS_TIMEOUT, OLLAMA_HEALTH_CHECK_TIMEOUT,
    OLLAMA_METADATA_TIMEOUT,
};
use crate::domain::enums::{LlmProviderStatus, ProviderType};
use crate::infrastructure::outbound::hw_metrics::load_hw_metrics;

// ── Ollama API response types ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct OllamaProcessStatus {
    models: Vec<OllamaRunningModel>,
}

#[derive(Deserialize)]
struct OllamaRunningModel {
    name:      String,
    size_vram: u64,
}

#[derive(Deserialize, Default)]
struct ShowResponse {
    model_info: Option<serde_json::Map<String, serde_json::Value>>,
    parameters: Option<String>,
}

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<TagModel>,
}

#[derive(Deserialize)]
struct TagModel {
    name: String,
}

// ── Architecture profile from /api/show ──────────────────────────────────────

#[derive(Default, Debug, Clone)]
struct ModelArchProfile {
    num_layers:     u32,
    num_kv_heads:   u32,
    head_dim:       u32,
    max_ctx:        u32,
    configured_ctx: u32,
}

// ── KV cache estimation ───────────────────────────────────────────────────────

fn compute_kv_per_request_mb(
    arch:              &ModelArchProfile,
    stats:             &ThroughputStats,
    bytes_per_element: u64,
) -> u32 {
    if arch.num_layers == 0 {
        return 128; // conservative fallback
    }

    let kv_bytes_per_token = 2u64
        * arch.num_layers as u64
        * arch.num_kv_heads as u64
        * arch.head_dim as u64
        * bytes_per_element;

    let effective_ctx = match (arch.configured_ctx, arch.max_ctx) {
        (c, m) if c > 0 && m > 0 => c.min(m),
        (c, _) if c > 0          => c,
        (_, m) if m > 0          => m,
        _                         => 4_096,
    };

    let avg_tokens = (stats.avg_prompt_tokens + stats.avg_output_tokens).max(128.0) as u64;
    // Use average token count but clamp to effective context
    let tokens = avg_tokens.min(effective_ctx as u64);
    

    ((kv_bytes_per_token * tokens) / 1_048_576).max(32) as u32
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::outbound::model_capacity_repository::ThroughputStats;
    use proptest::prelude::*;

    fn make_arch(layers: u32, kv_heads: u32, head_dim: u32, max_ctx: u32, cfg_ctx: u32) -> ModelArchProfile {
        ModelArchProfile { num_layers: layers, num_kv_heads: kv_heads, head_dim: head_dim, max_ctx: max_ctx, configured_ctx: cfg_ctx }
    }

    fn make_stats(prompt: f64, output: f64) -> ThroughputStats {
        ThroughputStats {
            avg_prompt_tokens: prompt,
            avg_output_tokens: output,
            ..Default::default()
        }
    }

    // ── Unit tests ───────────────────────────────────────────────────────

    #[test]
    fn zero_layers_returns_fallback() {
        let arch = make_arch(0, 8, 128, 4096, 4096);
        let stats = make_stats(100.0, 50.0);
        assert_eq!(compute_kv_per_request_mb(&arch, &stats, 1), 128);
    }

    #[test]
    fn minimum_32mb_enforced() {
        // Tiny model: 1 layer, 1 head, 1 dim, 128 tokens → near-zero KV
        let arch = make_arch(1, 1, 1, 4096, 4096);
        let stats = make_stats(64.0, 64.0);
        assert_eq!(compute_kv_per_request_mb(&arch, &stats, 1), 32);
    }

    #[test]
    fn token_clamped_to_128_minimum() {
        // avg_prompt + avg_output = 10 (below 128 minimum) → uses 128
        let arch = make_arch(32, 8, 128, 4096, 4096);
        let stats_low = make_stats(5.0, 5.0);
        let stats_exact = make_stats(64.0, 64.0); // exactly 128
        assert_eq!(
            compute_kv_per_request_mb(&arch, &stats_low, 1),
            compute_kv_per_request_mb(&arch, &stats_exact, 1),
        );
    }

    #[test]
    fn token_clamped_to_effective_ctx() {
        // avg tokens = 10000, but effective_ctx = 4096 → uses 4096
        let arch = make_arch(32, 8, 128, 4096, 4096);
        let stats = make_stats(8000.0, 2000.0);
        let stats_at_ctx = make_stats(2048.0, 2048.0);
        assert_eq!(
            compute_kv_per_request_mb(&arch, &stats, 1),
            compute_kv_per_request_mb(&arch, &stats_at_ctx, 1),
        );
    }

    #[test]
    fn effective_ctx_uses_min_of_configured_and_max() {
        let stats = make_stats(500.0, 500.0); // 1000 tokens, within both ctx
        let arch_cfg_smaller = make_arch(32, 8, 128, 8192, 2048); // min(2048,8192) = 2048
        let arch_max_smaller = make_arch(32, 8, 128, 2048, 8192); // min(8192,2048) = 2048
        assert_eq!(
            compute_kv_per_request_mb(&arch_cfg_smaller, &stats, 1),
            compute_kv_per_request_mb(&arch_max_smaller, &stats, 1),
        );
    }

    #[test]
    fn effective_ctx_fallback_4096() {
        // Both configured_ctx and max_ctx are 0 → fallback 4096
        let arch = make_arch(32, 8, 128, 0, 0);
        let stats = make_stats(2000.0, 2048.0); // 4048, clamped to 4096
        let arch_explicit = make_arch(32, 8, 128, 4096, 4096);
        assert_eq!(
            compute_kv_per_request_mb(&arch, &stats, 1),
            compute_kv_per_request_mb(&arch_explicit, &stats, 1),
        );
    }

    #[test]
    fn bytes_per_element_scales_approximately() {
        let arch = make_arch(32, 8, 128, 4096, 4096);
        let stats = make_stats(500.0, 500.0);
        let result_1 = compute_kv_per_request_mb(&arch, &stats, 1);
        let result_2 = compute_kv_per_request_mb(&arch, &stats, 2);
        // Integer division may cause ±1 difference from exact 2x
        let expected = result_1 * 2;
        assert!(result_2 >= expected - 1 && result_2 <= expected + 1,
            "result_2={result_2} should be ~2x result_1={result_1}");
    }

    #[test]
    fn realistic_qwen3_8b() {
        // qwen3:8b approximate: 32 layers, 8 kv_heads, 128 head_dim, q8_0 kv
        let arch = make_arch(32, 8, 128, 32768, 4096);
        let stats = make_stats(300.0, 200.0); // 500 tokens
        let result = compute_kv_per_request_mb(&arch, &stats, 1); // bytes_per_element=1 for q8_0
        // 2 * 32 * 8 * 128 * 1 * 500 / 1_048_576 = 31.25 → max(31, 32) = 32
        assert_eq!(result, 32);
    }

    #[test]
    fn realistic_llama3_70b() {
        // llama3:70b approximate: 80 layers, 8 kv_heads, 128 head_dim, q8_0 kv
        let arch = make_arch(80, 8, 128, 8192, 4096);
        let stats = make_stats(600.0, 400.0); // 1000 tokens
        let result = compute_kv_per_request_mb(&arch, &stats, 1);
        // 2 * 80 * 8 * 128 * 1 * 1000 / 1_048_576 = 156.25 → 156
        assert!(result >= 100 && result <= 200, "70B result={result}");
    }

    // ── Property-based tests ─────────────────────────────────────────────

    proptest! {
        /// Result is always >= 32 (minimum floor) unless zero layers fallback.
        #[test]
        fn result_at_least_32_or_128_fallback(
            layers in 0u32..200,
            kv_heads in 1u32..64,
            head_dim in 1u32..256,
            prompt in 0.0f64..10000.0,
            output in 0.0f64..10000.0,
            bpe in 1u64..4,
        ) {
            let arch = make_arch(layers, kv_heads, head_dim, 4096, 4096);
            let stats = make_stats(prompt, output);
            let result = compute_kv_per_request_mb(&arch, &stats, bpe);
            if layers == 0 {
                prop_assert_eq!(result, 128);
            } else {
                prop_assert!(result >= 32, "result={result} < 32");
            }
        }

        /// More layers → same or higher KV (monotonic).
        #[test]
        fn more_layers_more_kv(
            layers_a in 1u32..100,
            layers_b in 1u32..100,
            kv_heads in 1u32..32,
            head_dim in 32u32..256,
            prompt in 100.0f64..2000.0,
            output in 100.0f64..2000.0,
        ) {
            let stats = make_stats(prompt, output);
            let a = compute_kv_per_request_mb(&make_arch(layers_a, kv_heads, head_dim, 4096, 4096), &stats, 1);
            let b = compute_kv_per_request_mb(&make_arch(layers_b, kv_heads, head_dim, 4096, 4096), &stats, 1);
            if layers_a <= layers_b {
                prop_assert!(a <= b, "layers {layers_a}→{a} > layers {layers_b}→{b}");
            }
        }
    }
}

// ── LLM analysis response ─────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct LlmCapacityAnalysis {
    concern: Option<String>,
    reason:  Option<String>,
}

/// LLM batch recommendation: per-model concurrency + overall reasoning.
#[derive(Deserialize, Default, Debug)]
struct LlmBatchRecommendation {
    #[serde(default)]
    models: Vec<LlmModelRecommendation>,
    #[serde(default)]
    reasoning: Option<String>,
}

#[derive(Deserialize, Debug)]
struct LlmModelRecommendation {
    model: String,
    recommended_max_concurrent: u32,
}

// ── Architecture fetch ────────────────────────────────────────────────────────

async fn fetch_model_arch_profile(
    client:     &reqwest::Client,
    ollama_url: &str,
    model_name: &str,
) -> Result<ModelArchProfile> {
    #[derive(Serialize)]
    struct ShowReq<'a> { name: &'a str }

    let resp: ShowResponse = client
        .post(format!("{ollama_url}/api/show"))
        .json(&ShowReq { name: model_name })
        .timeout(OLLAMA_METADATA_TIMEOUT)
        .send()
        .await?
        .json()
        .await?;

    let info = resp.model_info.unwrap_or_default();

    let find = |suffix: &str| -> u32 {
        info.iter()
            .find(|(k, _)| k.ends_with(suffix))
            .and_then(|(_, v)| v.as_u64())
            .unwrap_or(0) as u32
    };

    let block_count = find("block_count");

    // Hybrid Mamba+Attention models (e.g. qwen3next) have `full_attention_interval`
    // meaning only every Nth layer uses attention (rest are SSM/Mamba).
    let attn_interval = find("full_attention_interval");
    let attn_layers = if attn_interval > 1 {
        // Only count attention layers for KV computation.
        block_count.div_ceil(attn_interval)
    } else {
        block_count
    };

    // head_count_kv can be null (JSON) for hybrid models → fall back to head_count.
    let kv_heads = find("attention.head_count_kv");
    let kv_heads = if kv_heads > 0 { kv_heads } else { find("attention.head_count") };

    let configured_ctx = resp
        .parameters
        .as_deref()
        .and_then(|p| {
            p.lines()
                .find(|l| l.starts_with("num_ctx"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0u32);

    Ok(ModelArchProfile {
        num_layers:     attn_layers,
        num_kv_heads:   kv_heads,
        head_dim:       find("attention.key_length").max(128),
        max_ctx:        find("context_length"),
        configured_ctx,
    })
}

// ── LLM analysis (background only) ─────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn call_llm_analysis(
    client:         &reqwest::Client,
    ollama_url:     &str,
    analyzer_model: &str,
    provider_name:  &str,
    model_name:     &str,
    weight_mb:      i32,
    vram_total_mb:  u32,
    temp_c:         Option<f32>,
    arch:           &ModelArchProfile,
    kv_per_req_mb:  u32,
    stats:          &ThroughputStats,
) -> Result<LlmCapacityAnalysis> {
    let prompt = format!(
        r#"GPU VRAM analysis. Respond with JSON only.

Provider: {provider_name}, Model: {model_name}
Architecture: {layers} layers, {kv_heads} KV heads, {head_dim} head_dim
VRAM: {vram_total}MB total, {weight}MB model weight, KV/request={kv_req}MB
Temperature: {temp}
Stats ({samples} jobs/1h): {tps:.1} tok/s, p95={p95:.0}ms

Is there a concern? Respond ONLY with valid JSON:
{{"concern":<null or "string">,"reason":"<brief>"}}"#,
        layers    = arch.num_layers,
        kv_heads  = arch.num_kv_heads,
        head_dim  = arch.head_dim,
        vram_total = vram_total_mb,
        weight    = weight_mb,
        kv_req    = kv_per_req_mb,
        temp      = temp_c.map_or("?".to_string(), |t| format!("{t:.1}")),
        samples   = stats.sample_count,
        tps       = stats.avg_tokens_per_sec,
        p95       = stats.p95_latency_ms,
    );

    #[derive(Serialize)]
    struct Req<'a> {
        model:   &'a str,
        prompt:  &'a str,
        stream:  bool,
        options: serde_json::Value,
    }
    #[derive(Deserialize)]
    struct Resp { response: String }

    let resp: Resp = client
        .post(format!("{ollama_url}/api/generate"))
        .json(&Req {
            model: analyzer_model,
            prompt: &prompt,
            stream: false,
            options: serde_json::json!({ "num_ctx": 512, "temperature": 0.0 }),
        })
        .timeout(LLM_ANALYSIS_TIMEOUT)
        .send()
        .await?
        .json()
        .await?;

    let raw = resp
        .response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    Ok(serde_json::from_str(raw).unwrap_or_default())
}

// ── LLM batch analysis (all models on a provider) ───────────────────────────

/// Collected per-model data for batch LLM analysis.
struct ModelSnapshot {
    name:          String,
    weight_mb:     u32,
    kv_per_req_mb: u32,
    tps:           f64,
    p95_ms:        f64,
    samples:       i64,
    max_concurrent: u32,
}


async fn call_llm_batch_analysis(
    client:         &reqwest::Client,
    analyzer_url:   &str,
    analyzer_model: &str,
    provider_name:  &str,
    vram_total_mb:  u32,
    temp_c:         Option<f32>,
    models:         &[ModelSnapshot],
) -> Result<LlmBatchRecommendation> {
    if models.is_empty() {
        return Ok(LlmBatchRecommendation::default());
    }

    // Build model summary table
    let mut model_lines = String::new();
    let mut total_weight = 0u32;
    for m in models {
        total_weight += m.weight_mb;
        model_lines.push_str(&format!(
            "- {name}: weight={w}MB, kv/req={kv}MB, tps={tps:.1}, p95={p95:.0}ms, samples={s}, current_limit={lim}\n",
            name = m.name,
            w    = m.weight_mb,
            kv   = m.kv_per_req_mb,
            tps  = m.tps,
            p95  = m.p95_ms,
            s    = m.samples,
            lim  = m.max_concurrent,
        ));
    }

    let prompt = format!(
        r#"You are an Ollama GPU capacity optimizer. Analyze all loaded models on this provider and recommend optimal max_concurrent for each model.

Provider: {provider_name}
VRAM: {vram_total}MB total, {used}MB used by loaded models ({count} models)
Temperature: {temp}

Loaded models:
{model_lines}
Rules:
1. Larger models need fewer concurrent requests (VRAM + compute contention)
2. If a model has very few samples (<5), recommend conservative limit (1-2)
3. If throughput is high with current limit, recommend keeping or increasing slightly
4. Consider the COMBINED VRAM of all loaded models — total weight must fit in VRAM
5. Consider model combinations: running many small models together is fine, but 1 large + 1 medium may compete
6. Weight-based heuristic: <5GB→8, 5-20GB→4, 20-50GB→2, >50GB→1 — use as upper bound reference

Respond ONLY with valid JSON:
{{"models":[{{"model":"<name>","recommended_max_concurrent":<int>}}],"reasoning":"<brief explanation>"}}"#,
        vram_total = vram_total_mb,
        used = total_weight,
        count = models.len(),
        temp = temp_c.map_or("unknown".to_string(), |t| format!("{t:.1}°C")),
    );

    #[derive(Serialize)]
    struct Req<'a> { model: &'a str, prompt: &'a str, stream: bool, options: serde_json::Value }
    #[derive(Deserialize)]
    struct Resp { response: String }

    let resp: Resp = client
        .post(format!("{analyzer_url}/api/generate"))
        .json(&Req {
            model: analyzer_model,
            prompt: &prompt,
            stream: false,
            options: serde_json::json!({ "num_ctx": 1024, "temperature": 0.0 }),
        })
        .timeout(LLM_BATCH_ANALYSIS_TIMEOUT)
        .send()
        .await?
        .json()
        .await?;

    let raw = resp
        .response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    Ok(serde_json::from_str(raw).unwrap_or_default())
}

// ── Per-provider unified sync ────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub async fn sync_provider(
    client:          &reqwest::Client,
    provider_id:     Uuid,
    provider_name:   &str,
    ollama_url:      &str,
    provider_total_vram_mb: i64,
    num_parallel:    u32,
    analyzer_model:  &str,
    capacity_repo:   &dyn ModelCapacityRepository,
    vram_pool:       &dyn VramPoolPort,
    valkey_pool:     Option<&fred::clients::Pool>,
    registry:        &dyn LlmProviderRegistry,
    ollama_model_repo: &dyn OllamaModelRepository,
    model_selection_repo: &dyn ProviderModelSelectionRepository,
) -> Result<()> {
    // 1. Health check: GET /api/version
    let health_ok = client
        .get(format!("{ollama_url}/api/version"))
        .timeout(OLLAMA_HEALTH_CHECK_TIMEOUT)
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    let new_status = if health_ok {
        LlmProviderStatus::Online
    } else {
        LlmProviderStatus::Offline
    };
    registry.update_status(provider_id, new_status).await.ok();

    if !health_ok {
        return Ok(()); // Skip further sync if offline
    }

    // 2. Model sync: GET /api/tags
    let tags: TagsResponse = client
        .get(format!("{ollama_url}/api/tags"))
        .timeout(OLLAMA_METADATA_TIMEOUT)
        .send()
        .await?
        .json()
        .await?;
    let model_names: Vec<String> = tags.models.into_iter().map(|m| m.name).collect();

    // Persist to DB
    ollama_model_repo.sync_provider_models(provider_id, &model_names).await.ok();
    model_selection_repo.upsert_models(provider_id, &model_names).await.ok();

    // Update Valkey cache
    if let Some(pool) = valkey_pool {
        use fred::prelude::*;
        let cache_key = format!("veronex:models:{provider_id}");
        let json = serde_json::to_string(&model_names).unwrap_or_default();
        let ttl = crate::infrastructure::inbound::http::constants::MODELS_CACHE_TTL;
        let _: Result<(), _> = pool.set(&cache_key, &json, Some(Expiration::EX(ttl)), None, false).await;
    }

    // 3. VRAM probing: GET /api/ps
    let ps: OllamaProcessStatus = client
        .get(format!("{ollama_url}/api/ps"))
        .timeout(OLLAMA_METADATA_TIMEOUT)
        .send()
        .await?
        .json()
        .await?;

    // hw_metrics from Valkey → total GPU VRAM, fallback to provider DB field
    let hw = if let Some(pool) = valkey_pool {
        load_hw_metrics(pool, provider_id).await
    } else {
        None
    };
    let drm_vram_mb = hw.as_ref().map(|h| h.vram_total_mb)
        .filter(|&v| v > 0)
        .unwrap_or({
            if provider_total_vram_mb > 0 { provider_total_vram_mb as u32 } else { 0 }
        });

    // APU / unified-memory detection: AMD Ryzen AI (iGPU) and similar APUs report only
    // the dedicated BIOS-allocated VRAM via DRM (e.g. 1024 MiB), while Ollama transparently
    // uses shared system RAM. Use mem_available_mb from node-exporter as the real capacity.
    let mem_available_mb = hw.as_ref().map(|h| h.mem_available_mb).unwrap_or(0);
    let is_apu = hw.as_ref().is_some_and(|h| {
        h.gpu_vendor == "amd" && drm_vram_mb > 0 && mem_available_mb > drm_vram_mb * 2
    });
    let vram_total_mb = if is_apu {
        // APU unified memory: use node-exporter mem_available_mb as total VRAM.
        // safety_permil in VramPool.compute_available() handles the buffer.
        mem_available_mb
    } else {
        drm_vram_mb
    };

    let temp_c = hw.as_ref().map(|h| h.max_temp_c());

    // Set total VRAM on the pool
    if vram_total_mb > 0 {
        vram_pool.set_total_vram(provider_id, vram_total_mb);
    }

    // Mark loaded models + unload models no longer in /api/ps
    let ps_names: std::collections::HashSet<&str> =
        ps.models.iter().map(|m| m.name.as_str()).collect();
    for name in vram_pool.loaded_model_names(provider_id) {
        if !ps_names.contains(name.as_str()) {
            vram_pool.mark_model_unloaded(provider_id, &name);
            tracing::info!(provider = %provider_name, model = %name, "model unloaded (no longer in /api/ps)");
        }
    }
    for model in &ps.models {
        let weight_mb = (model.size_vram / 1_048_576) as u32;
        vram_pool.mark_model_loaded(provider_id, &model.name, weight_mb);
    }

    // 4. Architecture analysis + KV cache for each loaded model
    let mut model_snapshots: Vec<ModelSnapshot> = Vec::new();

    for model in &ps.models {
        let weight_mb = (model.size_vram / 1_048_576) as i32;

        let arch = fetch_model_arch_profile(client, ollama_url, &model.name)
            .await
            .unwrap_or_default();

        let stats = match capacity_repo
            .compute_throughput_stats(provider_id, &model.name, 1)
            .await
        {
            Ok(Some(s)) => s,
            Ok(None) => ThroughputStats::default(),
            Err(e) => {
                tracing::warn!(model = %model.name, "throughput stats query failed: {e:#}");
                ThroughputStats::default()
            }
        };

        // q8_0 = 1 byte per element
        let kv_per_req = compute_kv_per_request_mb(&arch, &stats, 1);

        // 5. LLM analysis
        let llm = call_llm_analysis(
            client,
            ollama_url,  // Use provider's Ollama URL for LLM analysis
            analyzer_model,
            provider_name,
            &model.name,
            weight_mb,
            vram_total_mb,
            temp_c,
            &arch,
            kv_per_req,
            &stats,
        )
        .await
        .unwrap_or_default();

        // 6. Update VramPool profile
        vram_pool.set_model_profile(
            provider_id,
            &model.name,
            ModelVramProfile {
                weight_mb: weight_mb as u32,
                weight_estimated: false,
                kv_per_request_mb: kv_per_req,
                num_layers: arch.num_layers as u16,
                num_kv_heads: arch.num_kv_heads as u16,
                head_dim: arch.head_dim as u16,
                configured_ctx: arch.configured_ctx,
                failure_count: 0,
                llm_concern: llm.concern.clone(),
                llm_reason: llm.reason.clone(),
            },
        );

        // 7. Adaptive concurrency (AIMD)
        let current_tps_x100 = (stats.avg_tokens_per_sec * 100.0) as u32;
        let baseline = vram_pool.baseline_tps(provider_id, &model.name);
        let current_limit = vram_pool.max_concurrent(provider_id, &model.name);

        if baseline == 0 {
            // First data point → set baseline + initial limit from num_parallel (Phase 8)
            if stats.sample_count > 0 {
                vram_pool.set_baseline_tps(provider_id, &model.name, current_tps_x100);
                let initial = num_parallel.max(1);
                vram_pool.set_max_concurrent(provider_id, &model.name, initial);
                if stats.p95_latency_ms > 0.0 {
                    vram_pool.set_baseline_p95_ms(provider_id, &model.name, stats.p95_latency_ms as u32);
                }
            }
        } else if stats.sample_count >= 3 {
            let ratio = current_tps_x100 as f64 / baseline as f64;
            let baseline_p95 = vram_pool.baseline_p95_ms(provider_id, &model.name);
            let current_p95 = stats.p95_latency_ms as u32;
            // p95 spike: tail latency doubled from baseline → force decrease
            let p95_spike = baseline_p95 > 0 && current_p95 > baseline_p95 * 2;

            let new_limit = if ratio < 0.7 || p95_spike {
                // Throughput 30%+ drop or p95 doubled → multiplicative decrease
                (current_limit * 3 / 4).max(1)
            } else if ratio >= 0.9 {
                // Throughput maintained + p95 stable → additive increase
                vram_pool.set_baseline_tps(
                    provider_id,
                    &model.name,
                    baseline.max(current_tps_x100),
                );
                if baseline_p95 > 0 {
                    vram_pool.set_baseline_p95_ms(
                        provider_id,
                        &model.name,
                        baseline_p95.min(current_p95),
                    );
                }
                current_limit.saturating_add(1).min(num_parallel)
            } else {
                current_limit
            };
            if new_limit != current_limit {
                vram_pool.set_max_concurrent(provider_id, &model.name, new_limit);
                tracing::info!(
                    provider = %provider_name,
                    model = %model.name,
                    old = current_limit,
                    new = new_limit,
                    tps = stats.avg_tokens_per_sec,
                    baseline_tps = baseline as f64 / 100.0,
                    p95_ms = stats.p95_latency_ms,
                    ?p95_spike,
                    "adaptive concurrency limit updated"
                );
            }
        }

        // Persist to DB
        capacity_repo
            .upsert(&ModelVramProfileEntry {
                provider_id,
                model_name:        model.name.clone(),
                weight_mb,
                weight_estimated:  false,
                kv_per_request_mb: kv_per_req as i32,
                num_layers:        arch.num_layers as i16,
                num_kv_heads:      arch.num_kv_heads as i16,
                head_dim:          arch.head_dim as i16,
                configured_ctx:    arch.configured_ctx as i32,
                failure_count:     0,
                llm_concern:       llm.concern,
                llm_reason:        llm.reason,
                max_concurrent:    vram_pool.max_concurrent(provider_id, &model.name) as i32,
                baseline_tps:      vram_pool.baseline_tps(provider_id, &model.name) as i32,
                baseline_p95_ms:   vram_pool.baseline_p95_ms(provider_id, &model.name) as i32,
                updated_at:        Utc::now(),
            })
            .await?;

        // Collect snapshot for batch LLM analysis
        model_snapshots.push(ModelSnapshot {
            name:           model.name.clone(),
            weight_mb:      weight_mb as u32,
            kv_per_req_mb:  kv_per_req,
            tps:            stats.avg_tokens_per_sec,
            p95_ms:         stats.p95_latency_ms,
            samples:        stats.sample_count,
            max_concurrent: vram_pool.max_concurrent(provider_id, &model.name),
        });

        tracing::info!(
            provider = %provider_name,
            model   = %model.name,
            weight_mb,
            kv_per_req,
            "vram profile updated"
        );
    }

    // 8. LLM batch analysis — recommend optimal max_concurrent per model
    //    Only runs when there's enough data (total samples >= 10 across all models).
    let total_samples: i64 = model_snapshots.iter().map(|m| m.samples).sum();
    tracing::info!(
        provider = %provider_name,
        total_samples,
        model_count = model_snapshots.len(),
        "batch analysis check"
    );
    if total_samples >= 10 && !model_snapshots.is_empty() {
        match call_llm_batch_analysis(
            client,
            ollama_url,  // Use provider's Ollama URL for LLM analysis
            analyzer_model,
            provider_name,
            vram_total_mb,
            temp_c,
            &model_snapshots,
        )
        .await
        {
            Ok(rec) => {
                for mr in &rec.models {
                    if mr.recommended_max_concurrent == 0 {
                        continue; // LLM returned 0 = no opinion
                    }
                    // Clamp to num_parallel upper bound and ±2 from current for stability (Phase 8)
                    let upper = num_parallel * 2;
                    let current = vram_pool.max_concurrent(provider_id, &mr.model);
                    let change_floor = current.saturating_sub(2).max(1);
                    let change_ceil = current.saturating_add(2);
                    let recommended = mr.recommended_max_concurrent
                        .min(upper)
                        .clamp(change_floor, change_ceil)
                        .max(1);

                    if recommended != current {
                        vram_pool.set_max_concurrent(provider_id, &mr.model, recommended);
                        // Re-persist with LLM-recommended limit
                        if let Ok(Some(mut entry)) = capacity_repo.get(provider_id, &mr.model).await {
                            entry.max_concurrent = recommended as i32;
                            capacity_repo.upsert(&entry).await.ok();
                        }
                        tracing::info!(
                            provider = %provider_name,
                            model = %mr.model,
                            old = current,
                            new = recommended,
                            "LLM batch analysis updated max_concurrent"
                        );
                    }
                }
                if let Some(reasoning) = &rec.reasoning {
                    tracing::info!(provider = %provider_name, reasoning, "LLM batch analysis reasoning");
                }
            }
            Err(e) => {
                tracing::warn!(provider = %provider_name, "LLM batch analysis failed: {e:#}");
            }
        }
    }

    Ok(())
}

// Phase 8: initial_max_concurrent / weight_based_max_concurrent removed.
// Cold start initial = num_parallel (passed to sync_provider).

// ── Sync loop ─────────────────────────────────────────────────────────────────

/// Background loop that periodically syncs all active Ollama providers.
#[allow(clippy::too_many_arguments)]
pub async fn run_sync_loop(
    registry:              Arc<dyn LlmProviderRegistry>,
    capacity_repo:         Arc<dyn ModelCapacityRepository>,
    settings_repo:         Arc<dyn CapacitySettingsRepository>,
    vram_pool:             Arc<dyn VramPoolPort>,
    valkey_pool:           Option<fred::clients::Pool>,
    manual_trigger:        Arc<Notify>,
    sync_lock:             Arc<tokio::sync::Semaphore>,
    base_tick:             Duration,
    shutdown:              CancellationToken,
    client:                reqwest::Client,
    ollama_model_repo:     Arc<dyn OllamaModelRepository>,
    model_selection_repo:  Arc<dyn ProviderModelSelectionRepository>,
) {
    let mut ticker = tokio::time::interval(base_tick);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    tracing::info!("sync loop started (tick={}s)", base_tick.as_secs());

    loop {
        let is_manual = tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = ticker.tick() => false,
            _ = manual_trigger.notified() => true,
        };

        let settings = settings_repo.get().await.unwrap_or_default();

        // Push probe config to VramPool
        vram_pool.set_probe_config(settings.probe_permits, settings.probe_rate);

        if !is_manual {
            if !settings.sync_enabled {
                continue;
            }
            let elapsed_secs = settings
                .last_run_at
                .map(|t| Utc::now().signed_duration_since(t).num_seconds())
                .unwrap_or(i64::MAX);
            if elapsed_secs < settings.sync_interval_secs as i64 {
                continue;
            }
        }

        let Ok(_permit) = sync_lock.clone().acquire_owned().await else {
            tracing::error!("sync semaphore closed unexpectedly");
            break;
        };

        let all_providers = registry.list_all().await.unwrap_or_default();
        let ollama_providers: Vec<_> = all_providers
            .into_iter()
            .filter(|p| p.is_active && p.provider_type == ProviderType::Ollama)
            .collect();

        let mut any_error = false;
        for provider in ollama_providers {
            if let Err(e) = sync_provider(
                &client,
                provider.id,
                &provider.name,
                &provider.url,
                provider.total_vram_mb,
                provider.num_parallel.max(1) as u32,
                &settings.analyzer_model,
                &*capacity_repo,
                &*vram_pool,
                valkey_pool.as_ref(),
                &*registry,
                &*ollama_model_repo,
                &*model_selection_repo,
            )
            .await
            {
                tracing::warn!(
                    provider = %provider.name,
                    "sync failed (non-fatal): {e}"
                );
                any_error = true;
            }
        }

        let status = if any_error { "partial" } else { "ok" };
        settings_repo.record_run(status).await.ok();
    }

    tracing::info!("sync loop stopped");
}
