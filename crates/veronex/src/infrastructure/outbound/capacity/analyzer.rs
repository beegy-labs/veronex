/// Capacity analysis loop — runs every N minutes in the background.
///
/// 1. Polls Ollama /api/ps for currently loaded models + their VRAM footprint.
/// 2. Fetches /api/show architecture parameters for precise KV cache calculation.
/// 3. Aggregates throughput stats from inference_jobs (PostgreSQL).
/// 4. Optionally calls an LLM (qwen2.5:3b by default) to recommend slot count.
/// 5. Updates ConcurrencySlotMap and persists ModelCapacityEntry to DB.
///
/// This loop is completely separated from the request dispatch path — no LLM
/// calls happen during job dispatching.
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::application::ports::outbound::capacity_settings_repository::CapacitySettingsRepository;
use crate::application::ports::outbound::model_capacity_repository::{
    ModelCapacityEntry, ModelCapacityRepository, ThroughputStats,
};
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::domain::enums::ProviderType;
use crate::infrastructure::outbound::capacity::slot_map::ConcurrencySlotMap;
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

// ── Architecture profile from /api/show ──────────────────────────────────────

#[derive(Default, Debug, Clone)]
struct ModelArchProfile {
    num_layers:     u32, // block_count
    num_kv_heads:   u32, // attention.head_count_kv
    head_dim:       u32, // attention.key_length
    max_ctx:        u32, // context_length
    configured_ctx: u32, // from parameters field: "num_ctx 4096"
}

// ── KV cache estimation ───────────────────────────────────────────────────────

struct KvPerSlot {
    worst_case: i32, // kv_bytes × num_ctx → MB (upper bound)
    realistic:  i32, // kv_bytes × avg_tokens → MB (typical usage)
}

fn compute_kv_per_slot_mb(
    arch:               &ModelArchProfile,
    stats:              &ThroughputStats,
    bytes_per_element:  u64, // 1 = q8_0/q4, 2 = bf16/fp16
) -> KvPerSlot {
    if arch.num_layers == 0 {
        // No architecture info — conservative fallback
        return KvPerSlot { worst_case: 256, realistic: 128 };
    }

    // KV cache per token: 2 (K+V) × layers × kv_heads × head_dim × bytes_per_element
    let kv_bytes_per_token = 2u64
        * arch.num_layers as u64
        * arch.num_kv_heads as u64
        * arch.head_dim as u64
        * bytes_per_element;

    // Fix: clamp to the model's native maximum.
    // If OLLAMA_CONTEXT_LENGTH (e.g. 204800) exceeds the model's actual max_ctx
    // (e.g. 32768 for qwen2.5:7b), using configured_ctx would over-estimate
    // KV cache by 6× and cause the slot calculator to under-allocate.
    let effective_ctx = match (arch.configured_ctx, arch.max_ctx) {
        (c, m) if c > 0 && m > 0 => c.min(m),
        (c, _) if c > 0           => c,
        (_, m) if m > 0           => m,
        _                          => 4_096,
    };

    let avg_tokens = (stats.avg_prompt_tokens + stats.avg_output_tokens).max(128.0) as u64;

    KvPerSlot {
        worst_case: ((kv_bytes_per_token * effective_ctx as u64) / 1_048_576).max(64) as i32,
        realistic:  ((kv_bytes_per_token * avg_tokens)           / 1_048_576).max(32) as i32,
    }
}

// ── LLM analysis response ─────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct LlmCapacityAnalysis {
    recommended_slots: Option<u8>,
    concern:           Option<String>,
    reason:            Option<String>,
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
        .timeout(Duration::from_secs(10))
        .send()
        .await?
        .json()
        .await?;

    let info = resp.model_info.unwrap_or_default();

    // Fields are prefixed by model family ("llama.", "qwen2.", "gemma3.", etc.)
    // Search by suffix to be family-agnostic.
    let find = |suffix: &str| -> u32 {
        info.iter()
            .find(|(k, _)| k.ends_with(suffix))
            .and_then(|(_, v)| v.as_u64())
            .unwrap_or(0) as u32
    };

    // Parse "num_ctx 4096" from the parameters text blob
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
        num_layers:     find("block_count"),
        num_kv_heads:   find("attention.head_count_kv"),
        head_dim:       find("attention.key_length").max(128),
        max_ctx:        find("context_length"),
        configured_ctx,
    })
}

// ── LLM slot recommendation (background only) ─────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn call_llm_capacity_analysis(
    client:         &reqwest::Client,
    ollama_url:     &str,
    model:          &str,
    backend_name:   &str,
    model_name:     &str,
    loaded_vram_mb: i32,
    vram_total_mb:  i32,
    temp_c:         Option<f32>,
    arch:           &ModelArchProfile,
    kv:             &KvPerSlot,
    stats:          &ThroughputStats,
    math_slots:     i32,
) -> Result<LlmCapacityAnalysis> {
    let prompt = format!(
        r#"GPU capacity analysis. Respond with JSON only.

Backend: {backend_name}, Model: {model_name}
Architecture: {layers} layers, {kv_heads} KV heads, {head_dim} head_dim (GQA={gqa})
VRAM: {vram_total}MB total, {loaded}MB model loaded, available={avail}MB
KV cache: {kv_worst}MB/slot (worst, ctx={ctx}), {kv_real}MB/slot (avg {avg_tok:.0} tokens)
Temperature: {temp}
Stats ({samples} jobs/1h): {tps:.1} tok/s, p95={p95:.0}ms
Math estimate: {math} slots

Respond ONLY with valid JSON (no markdown):
{{"recommended_slots":<1-8>,"concern":<null or "string">,"reason":"<brief>"}}"#,
        layers    = arch.num_layers,
        kv_heads  = arch.num_kv_heads,
        head_dim  = arch.head_dim,
        gqa       = arch.num_kv_heads < 32,
        vram_total = vram_total_mb,
        loaded    = loaded_vram_mb,
        avail     = vram_total_mb - loaded_vram_mb - 512,
        ctx       = if arch.configured_ctx > 0 { arch.configured_ctx } else { arch.max_ctx.min(4096) },
        kv_worst  = kv.worst_case,
        kv_real   = kv.realistic,
        avg_tok   = stats.avg_prompt_tokens + stats.avg_output_tokens,
        temp      = temp_c.map_or("?".to_string(), |t| format!("{t:.1}")),
        samples   = stats.sample_count,
        tps       = stats.avg_tokens_per_sec,
        p95       = stats.p95_latency_ms,
        math      = math_slots,
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
            model,
            prompt: &prompt,
            stream: false,
            options: serde_json::json!({ "num_ctx": 512, "temperature": 0.0 }),
        })
        .timeout(Duration::from_secs(30))
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

// ── Per-backend analysis ──────────────────────────────────────────────────────

async fn analyze_backend(
    client:              &reqwest::Client,
    backend_id:          Uuid,
    backend_name:        &str,
    ollama_url:          &str,
    analyzer_url:        &str,
    analyzer_model:      &str,
    capacity_repo:       &dyn ModelCapacityRepository,
    slot_map:            &ConcurrencySlotMap,
    valkey_pool:         Option<&fred::clients::Pool>,
    ollama_num_parallel: u32,
) -> Result<()> {
    // 1. Ollama /api/ps — currently loaded models + their VRAM
    let ps: OllamaProcessStatus = client
        .get(format!("{ollama_url}/api/ps"))
        .timeout(Duration::from_secs(10))
        .send()
        .await?
        .json()
        .await?;

    if ps.models.is_empty() {
        return Ok(());
    }

    // 2. hw_metrics from Valkey → total GPU VRAM
    let hw = if let Some(pool) = valkey_pool {
        load_hw_metrics(pool, backend_id).await
    } else {
        None
    };
    let vram_total_mb = hw.as_ref().map(|h| h.vram_total_mb as i32).unwrap_or(0);
    let temp_c = hw.as_ref().map(|h| h.temp_c);

    for model in &ps.models {
        let loaded_vram_mb = (model.size_vram / 1_048_576) as i32;

        // 3. Architecture params from /api/show
        let arch = fetch_model_arch_profile(client, ollama_url, &model.name)
            .await
            .unwrap_or_default();

        // 4. Throughput stats (last 1 hour)
        let stats = capacity_repo
            .compute_throughput_stats(backend_id, &model.name, 1)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();

        // 5. KV cache per slot (accurate formula).
        //    OLLAMA_KV_CACHE_TYPE=q8_0 → 1 byte per element (not BF16's 2).
        //    Ollama uses q8_0 by default on this deployment, so we match it.
        let kv = compute_kv_per_slot_mb(&arch, &stats, 1 /* q8_0 = 1 byte */);

        // 6. Math-based slot estimate.
        //    Upper bound is OLLAMA_NUM_PARALLEL — Ollama won't process more
        //    requests concurrently than that regardless of VRAM headroom.
        let vram_buffer  = 512i32;
        let available_mb = vram_total_mb - loaded_vram_mb - vram_buffer;
        let math_slots = if available_mb > 0 && kv.realistic > 0 {
            let by_realistic = available_mb / kv.realistic;
            let by_worst     = available_mb / kv.worst_case.max(1);
            // Use realistic estimate but cap at 2× worst-case for safety,
            // and never exceed OLLAMA_NUM_PARALLEL (Ollama's hard ceiling).
            (1 + by_realistic.min(by_worst * 2)).clamp(1, ollama_num_parallel as i32)
        } else {
            1
        };

        // 7. LLM interpretation (qwen2.5:3b) — fallback to math_slots on error
        let llm = call_llm_capacity_analysis(
            client,
            analyzer_url,
            analyzer_model,
            backend_name,
            &model.name,
            loaded_vram_mb,
            vram_total_mb,
            temp_c,
            &arch,
            &kv,
            &stats,
            math_slots,
        )
        .await
        .unwrap_or_default();

        let recommended = llm
            .recommended_slots
            .map(|s| s as i16)
            .unwrap_or(math_slots as i16)
            .clamp(1, 8);

        // 8. Persist + update slot map
        capacity_repo
            .upsert(&ModelCapacityEntry {
                provider_id: backend_id,
                model_name:          model.name.clone(),
                vram_model_mb:       loaded_vram_mb,
                vram_total_mb,
                arch_num_layers:     arch.num_layers as i32,
                arch_num_kv_heads:   arch.num_kv_heads as i32,
                arch_head_dim:       arch.head_dim as i32,
                arch_configured_ctx: arch.configured_ctx as i32,
                vram_kv_per_slot_mb:   kv.realistic,
                vram_kv_worst_case_mb: kv.worst_case,
                recommended_slots:   recommended,
                avg_tokens_per_sec:  stats.avg_tokens_per_sec,
                avg_prefill_tps:     stats.avg_prefill_tps,
                avg_prompt_tokens:   stats.avg_prompt_tokens,
                avg_output_tokens:   stats.avg_output_tokens,
                p95_latency_ms:      stats.p95_latency_ms,
                sample_count:        stats.sample_count as i32,
                llm_concern:         llm.concern,
                llm_reason:          llm.reason,
                updated_at:          Utc::now(),
            })
            .await?;

        slot_map.update_capacity(backend_id, &model.name, recommended as u32);

        tracing::info!(
            backend = %backend_name,
            model   = %model.name,
            slots   = recommended,
            kv_realistic_mb  = kv.realistic,
            kv_worst_mb      = kv.worst_case,
            "capacity updated"
        );
    }

    Ok(())
}

// ── Analysis loop ─────────────────────────────────────────────────────────────

/// Spawns a background loop that periodically re-evaluates capacity for all
/// active Ollama backends.
///
/// The loop checks the DB settings on every tick (30 s) to pick up dynamic
/// changes to `batch_interval_secs` and `batch_enabled`.  A `manual_trigger`
/// Notify bypasses the interval check for immediate on-demand analysis.
pub async fn run_capacity_analysis_loop(
    registry:            Arc<dyn LlmProviderRegistry>,
    capacity_repo:       Arc<dyn ModelCapacityRepository>,
    settings_repo:       Arc<dyn CapacitySettingsRepository>,
    slot_map:            Arc<ConcurrencySlotMap>,
    valkey_pool:         Option<fred::clients::Pool>,
    analyzer_url:        String,
    manual_trigger:      Arc<Notify>,
    analysis_lock:       Arc<tokio::sync::Semaphore>,
    base_tick:           Duration,
    shutdown:            CancellationToken,
    ollama_num_parallel: u32,
) {
    let client = reqwest::Client::new();
    let mut ticker = tokio::time::interval(base_tick);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    tracing::info!("capacity analysis loop started (tick={}s)", base_tick.as_secs());

    loop {
        let is_manual = tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = ticker.tick() => false,
            _ = manual_trigger.notified() => true,
        };

        let settings = settings_repo.get().await.unwrap_or_default();

        if !is_manual {
            if !settings.batch_enabled {
                continue;
            }
            // Check whether enough time has passed since last run
            let elapsed_secs = settings
                .last_run_at
                .map(|t| Utc::now().signed_duration_since(t).num_seconds())
                .unwrap_or(i64::MAX);
            if elapsed_secs < settings.batch_interval_secs as i64 {
                continue;
            }
        }

        // Acquire the analysis lock — prevents concurrent runs (e.g. rapid POST /sync spam).
        // `acquire_owned` never fails on a non-closed semaphore, so unwrap is safe here.
        let _permit = analysis_lock.clone().acquire_owned().await.unwrap();

        // Run analysis for all active Ollama backends
        let backends = registry.list_all().await.unwrap_or_default();
        let ollama_backends: Vec<_> = backends
            .into_iter()
            .filter(|b| b.is_active && b.provider_type == ProviderType::Ollama)
            .collect();

        let mut any_error = false;
        for backend in ollama_backends {
            if let Err(e) = analyze_backend(
                &client,
                backend.id,
                &backend.name,
                &backend.url,
                &analyzer_url,
                &settings.analyzer_model,
                &*capacity_repo,
                &slot_map,
                valkey_pool.as_ref(),
                ollama_num_parallel,
            )
            .await
            {
                tracing::warn!(
                    backend = %backend.name,
                    "capacity analysis failed (non-fatal): {e}"
                );
                any_error = true;
            }
        }

        let status = if any_error { "partial" } else { "ok" };
        settings_repo.record_run(status).await.ok();
        // `_permit` dropped here — releases the lock
    }

    tracing::info!("capacity analysis loop stopped");
}
