//! Placement Planner — 5s loop for automated model placement decisions (Phase 5).
//!
//! 2-pass structure:
//!   Pass 0 (read-only): compute scale_out_candidates and scale_out_needed
//!   Steps ④①②③⑤: STANDBY recovery, Scale-Out, Preload, Evict, Scale-In

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::application::ports::outbound::concurrency_port::VramPoolPort;
use crate::application::ports::outbound::circuit_breaker_port::CircuitBreakerPort;
use crate::application::ports::outbound::llm_provider_registry::LlmProviderRegistry;
use crate::application::ports::outbound::thermal_drain_port::ThermalDrainPort;
use crate::application::ports::outbound::thermal_port::ThermalPort;
use crate::application::ports::outbound::valkey_port::ValkeyPort;
use crate::domain::constants::demand_key;
use crate::domain::entities::LlmProvider;
use crate::domain::enums::{ProviderType, ThrottleLevel};

/// Placement planner loop interval.
const PLANNER_INTERVAL: Duration = Duration::from_secs(5);

/// Scale-Out demand threshold (80% of eligible capacity).
const SCALE_OUT_THRESHOLD: f64 = 0.80;

/// Idle threshold before eviction (seconds).
const EVICT_IDLE_SECS: u64 = 180;

/// Shorter idle threshold for standby servers.
const STANDBY_EVICT_IDLE_SECS: u64 = 30;

/// Transition guard duration after STANDBY recovery (seconds).
const TRANSITION_GUARD_SECS: u64 = 30;

/// Scale-Out hold-down duration (seconds).
const SCALE_OUT_HOLDDOWN_SECS: u64 = 60;

/// Preload lock TTL in Valkey (seconds).
const PRELOAD_LOCK_TTL: i64 = 180;

/// Scale-Out decision lock TTL in Valkey (seconds).
const SCALEOUT_DECISION_TTL: i64 = 30;

fn preload_lock_key(model: &str, provider_id: Uuid) -> String {
    format!("veronex:preloading:{model}:{provider_id}")
}

fn scaleout_decision_key(model: &str) -> String {
    format!("veronex:scaleout:{model}")
}

/// Returns true if demand exceeds eligible capacity threshold (80%).
pub(crate) fn is_scale_out_needed(demand: u64, eligible_capacity: u32) -> bool {
    demand as f64 > eligible_capacity as f64 * SCALE_OUT_THRESHOLD
}

/// Returns true if an idle model should be evicted based on idle duration and standby state.
pub(crate) fn should_evict(idle_secs: u64, is_standby: bool) -> bool {
    let threshold = if is_standby { STANDBY_EVICT_IDLE_SECS } else { EVICT_IDLE_SECS };
    idle_secs >= threshold
}

/// Run the placement planner loop.
// 8 params: 5 Arc<dyn Port> registries + thermal + circuit_breaker + http_client.
// Grouping into a struct would require boxing all ports twice at the call site — not worth it.
#[allow(clippy::too_many_arguments)]
pub async fn run_placement_planner_loop(
    registry: Arc<dyn LlmProviderRegistry>,
    vram_pool: Arc<dyn VramPoolPort>,
    thermal: Arc<dyn ThermalPort>,
    circuit_breaker: Arc<dyn CircuitBreakerPort>,
    valkey: Arc<dyn ValkeyPort>,
    http_client: reqwest::Client,
    instance_id: Arc<str>,
    thermal_drain: Arc<dyn ThermalDrainPort>,
    shutdown: CancellationToken,
) {
    tracing::info!("placement planner started (interval=5s)");

    // Track Scale-Out hold-down per server to prevent immediate Scale-In
    let mut scale_out_holddown: HashMap<Uuid, u64> = HashMap::new();

    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = tokio::time::sleep(PLANNER_INTERVAL) => {}
        }

        if let Err(e) = planner_tick(
            &registry,
            &vram_pool,
            &thermal,
            &circuit_breaker,
            &valkey,
            &http_client,
            &instance_id,
            &thermal_drain,
            &mut scale_out_holddown,
            &shutdown,
        ).await {
            tracing::warn!("placement planner tick failed: {e}");
        }
    }

    tracing::info!("placement planner stopped");
}

#[allow(clippy::too_many_arguments)]
async fn planner_tick(
    registry: &Arc<dyn LlmProviderRegistry>,
    vram_pool: &Arc<dyn VramPoolPort>,
    thermal: &Arc<dyn ThermalPort>,
    circuit_breaker: &Arc<dyn CircuitBreakerPort>,
    valkey: &Arc<dyn ValkeyPort>,
    http_client: &reqwest::Client,
    instance_id: &str,
    thermal_drain: &Arc<dyn ThermalDrainPort>,
    scale_out_holddown: &mut HashMap<Uuid, u64>,
    shutdown: &CancellationToken,
) -> anyhow::Result<()> {
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;

    // Clean up expired hold-downs
    scale_out_holddown.retain(|_, until| *until > now_ms);

    // ── Pass 0: Read-only snapshot ──────────────────────────────────────
    let all_providers = registry.list_all().await.unwrap_or_default();
    let ollama_providers: Vec<&LlmProvider> = all_providers
        .iter()
        .filter(|p| p.is_active && p.provider_type == ProviderType::Ollama)
        .collect();

    if ollama_providers.is_empty() {
        return Ok(());
    }

    // Scale-Out candidates: healthy servers with free VRAM
    let scale_out_candidates: Vec<&LlmProvider> = ollama_providers
        .iter()
        .filter(|p| {
            let level = thermal.get_level(p.id);
            !matches!(level, ThrottleLevel::Soft | ThrottleLevel::Hard | ThrottleLevel::Cooldown)
                && circuit_breaker.is_allowed(p.id)
                && vram_pool.available_vram_mb(p.id) > 0
        })
        .copied()
        .collect();

    // Collect all unique models across providers
    let mut all_models: HashSet<String> = HashSet::new();
    for p in &ollama_providers {
        for model in vram_pool.loaded_model_names(p.id) {
            all_models.insert(model);
        }
    }

    // Get demand for each model
    let mut model_demand: HashMap<String, u64> = HashMap::new();
    for model in &all_models {
        let key = demand_key(model);
        let demand: u64 = valkey.kv_get(&key).await
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if demand > 0 {
            model_demand.insert(model.clone(), demand);
        }
    }

    // Compute eligible capacity per model (excluding standby, thermal-gated, etc.)
    let mut eligible_capacity: HashMap<String, u32> = HashMap::new();
    for p in &ollama_providers {
        let level = thermal.get_level(p.id);
        if matches!(level, ThrottleLevel::Soft | ThrottleLevel::Hard | ThrottleLevel::Cooldown) {
            continue;
        }
        if !circuit_breaker.is_allowed(p.id) {
            continue;
        }

        for model in vram_pool.loaded_model_names(p.id) {
            if vram_pool.is_pulling(p.id, &model) {
                continue;
            }
            if vram_pool.is_dispatch_blocked(p.id, &model) {
                continue;
            }
            let mc = vram_pool.max_concurrent(p.id, &model);
            *eligible_capacity.entry(model).or_insert(0) += mc;
        }
    }

    // Scale-Out needed: demand > eligible_capacity × 0.80
    let scale_out_needed: HashSet<String> = model_demand
        .iter()
        .filter(|(model, demand)| {
            let cap = eligible_capacity.get(*model).copied().unwrap_or(0);
            is_scale_out_needed(**demand, cap)
        })
        .map(|(model, _)| model.clone())
        .collect();

    // Provisional free VRAM tracking (prevents multi-model collision in same cycle)
    let mut provisional_free: HashMap<Uuid, u32> = HashMap::new();
    for p in &scale_out_candidates {
        provisional_free.insert(p.id, vram_pool.available_vram_mb(p.id));
    }

    // Track which servers were used for Scale-Out/Preload this cycle (protect from ⑤)
    let mut scale_out_servers: HashSet<Uuid> = HashSet::new();

    // ── Hard Gate Watchdog (SDD §3/§6) ─────────────────────────────────
    // Drives Hard → Cooldown when active==0. Logs at 60s/90s drain stalls.
    for p in &ollama_providers {
        if thermal.get_level(p.id) != ThrottleLevel::Hard {
            continue;
        }
        let active = vram_pool.provider_active_requests(p.id);
        if active == 0 {
            // Active drained naturally — start Cooldown timer now (SDD §3)
            thermal.set_cooldown(p.id);
            tracing::info!(provider_id = %p.id, "Hard gate: active=0, transitioning to Cooldown");
            continue;
        }
        if let Some(elapsed) = thermal.hard_since_elapsed_secs(p.id) {
            if elapsed >= 90 {
                tracing::error!(
                    provider_id = %p.id, elapsed_secs = elapsed, active_requests = active,
                    "WATCHDOG: Hard gate >90s drain stall — {active} active requests remain"
                );
            } else if elapsed >= 60 {
                // SDD §3: Hard 진입 후 60s 경과 + active>0 → in-flight jobs 강제 cancel.
                // Cancelling drops VramPermit → active_count → 0 → Cooldown transition.
                tracing::warn!(
                    provider_id = %p.id, elapsed_secs = elapsed, active_requests = active,
                    "Hard gate 60s: force-cancelling {active} in-flight jobs for thermal drain"
                );
                let cancelled = thermal_drain.cancel_jobs_for_provider(p.id);
                tracing::warn!(provider_id = %p.id, cancelled, "Hard gate: cancel signals sent");
            }
        }
    }

    // ── Step ④: STANDBY recovery ────────────────────────────────────────
    for p in &ollama_providers {
        if !vram_pool.is_standby(p.id) {
            continue;
        }
        if vram_pool.in_transition(p.id) {
            continue;
        }

        // Condition A: server has loaded model with demand>0
        let loaded = vram_pool.loaded_model_names(p.id);
        let has_demand = loaded.iter().any(|m| model_demand.contains_key(m));

        // Condition B: server is the best candidate (most free VRAM) for a scale_out_needed model.
        // Mirrors Step① best_server selection: max provisional_free among eligible candidates.
        let is_best_for_scaleout = scale_out_needed.iter().any(|model| {
            let my_free = provisional_free.get(&p.id).copied().unwrap_or(0);
            if my_free == 0 || vram_pool.loaded_model_names(p.id).contains(model) {
                return false;
            }
            // Check this server has the most free VRAM among all eligible scale_out_candidates.
            scale_out_candidates.iter().all(|c| {
                c.id == p.id
                    || vram_pool.loaded_model_names(c.id).contains(model)
                    || provisional_free.get(&c.id).copied().unwrap_or(0) <= my_free
            })
        });

        if has_demand || is_best_for_scaleout {
            vram_pool.set_standby(p.id, false);
            let guard_until = now_ms + TRANSITION_GUARD_SECS * 1000;
            vram_pool.set_transition_until(p.id, guard_until);
            scale_out_servers.insert(p.id);
            tracing::info!(provider_id = %p.id, "STANDBY recovery — server reactivated");
        }
    }

    // ── Step ①: Scale-Out ───────────────────────────────────────────────
    for model in &scale_out_needed {
        // Skip if preloading servers already cover demand
        let preloading_count = ollama_providers.iter()
            .filter(|p| vram_pool.is_preloading(p.id, model))
            .count();
        let avg_mc = eligible_capacity.get(model).copied().unwrap_or(1).max(1);
        let demand = model_demand.get(model).copied().unwrap_or(0);
        let needed_servers = ((demand as f64) / (avg_mc as f64)).ceil() as usize;
        if preloading_count >= needed_servers {
            continue;
        }

        // Find best server: most free VRAM, not already loaded, not preload-excluded,
        // not in transition (STANDBY recovery guard from Step ④ — prevents conflict).
        let best_server = scale_out_candidates.iter()
            .filter(|p| {
                !vram_pool.loaded_model_names(p.id).contains(&model.to_string())
                    && !vram_pool.is_preloading(p.id, model)
                    && !vram_pool.is_pulling(p.id, model)
                    && !vram_pool.is_preload_excluded(p.id, model)
                    && !vram_pool.in_transition(p.id)
                    && provisional_free.get(&p.id).copied().unwrap_or(0) > 0
            })
            .max_by_key(|p| {
                let free = provisional_free.get(&p.id).copied().unwrap_or(0);
                (free, std::cmp::Reverse(p.id)) // tie-break: provider_id ASC (Reverse for max_by)
            });

        let Some(server) = best_server else {
            continue; // No eligible server (single-server: no-op)
        };

        // Multi-instance dedup: Scale-Out decision lock
        let decision_key = scaleout_decision_key(model);
        match valkey.kv_get(&decision_key).await {
            Ok(Some(_)) => continue, // another replica is handling this model
            _ => {}
        }
        // Acquire decision lock (NX)
        if valkey.kv_set(&decision_key, instance_id, SCALEOUT_DECISION_TTL, false).await.is_err() {
            continue;
        }

        // Preload lock (NX): prevent duplicate preload of same model+server
        let preload_key = preload_lock_key(model, server.id);
        match valkey.kv_get(&preload_key).await {
            Ok(Some(_)) => {
                let _ = valkey.kv_del(&decision_key).await;
                continue;
            }
            _ => {}
        }
        if valkey.kv_set(&preload_key, "1", PRELOAD_LOCK_TTL, false).await.is_err() {
            let _ = valkey.kv_del(&decision_key).await;
            continue;
        }

        // Update provisional free VRAM with actual model weight (fallback to 2GB if unknown).
        if let Some(free) = provisional_free.get_mut(&server.id) {
            let model_weight = vram_pool.model_weight_mb(server.id, model);
            let weight_estimate = if model_weight > 0 { model_weight } else { 2048 };
            *free = free.saturating_sub(weight_estimate);
        }
        scale_out_servers.insert(server.id);
        scale_out_holddown.insert(server.id, now_ms + SCALE_OUT_HOLDDOWN_SECS * 1000);

        // Spawn preload task
        let url = server.url.clone();
        let model_c = model.clone();
        let provider_id = server.id;
        let np = server.num_parallel.max(1) as u32;
        let vram_c = vram_pool.clone();
        let http_c = http_client.clone();
        let valkey_c = valkey.clone();
        let preload_key_c = preload_key.clone();
        let decision_key_c = decision_key.clone();

        tokio::spawn(async move {
            let success = crate::infrastructure::outbound::ollama::preloader::preload_model(
                &http_c, &url, &model_c, provider_id, &vram_c, np,
            ).await;
            // Release locks
            let _ = valkey_c.kv_del(&preload_key_c).await;
            let _ = valkey_c.kv_del(&decision_key_c).await;

            if success {
                vram_c.mark_model_loaded(provider_id, &model_c, 0); // weight will be updated by sync loop
                tracing::info!(%provider_id, model = %model_c, "Scale-Out preload completed");
            }
        });

        tracing::info!(%model, provider_id = %server.id, "Scale-Out triggered — preloading");
    }

    // ── Step ②: Preload (demand>0 && !is_loaded && !is_preloading && has_room) ──
    for (model, _demand) in &model_demand {
        if scale_out_needed.contains(model) {
            continue; // already handled in ①
        }

        for p in &ollama_providers {
            // Skip thermally throttled providers (Soft/Hard/Cooldown).
            if matches!(thermal.get_level(p.id), ThrottleLevel::Soft | ThrottleLevel::Hard | ThrottleLevel::Cooldown) {
                continue;
            }
            // Skip standby providers — not eligible for new preloads until reactivated (Step ④).
            if vram_pool.is_standby(p.id) {
                continue;
            }
            if vram_pool.loaded_model_names(p.id).contains(model) {
                continue; // already loaded
            }
            if vram_pool.is_preloading(p.id, model) || vram_pool.is_pulling(p.id, model) {
                continue;
            }
            if vram_pool.is_preload_excluded(p.id, model) {
                continue;
            }
            let free = provisional_free.get(&p.id).copied().unwrap_or(
                vram_pool.available_vram_mb(p.id)
            );
            if free == 0 {
                continue;
            }

            // Preload lock
            let preload_key = preload_lock_key(model, p.id);
            match valkey.kv_get(&preload_key).await {
                Ok(Some(_)) => continue,
                _ => {}
            }
            if valkey.kv_set(&preload_key, "1", PRELOAD_LOCK_TTL, false).await.is_err() {
                continue;
            }

            if let Some(pf) = provisional_free.get_mut(&p.id) {
                let model_weight = vram_pool.model_weight_mb(p.id, model);
                let weight_estimate = if model_weight > 0 { model_weight } else { 2048 };
                *pf = pf.saturating_sub(weight_estimate);
            }
            scale_out_servers.insert(p.id);

            let url = p.url.clone();
            let model_c = model.clone();
            let provider_id = p.id;
            let np = p.num_parallel.max(1) as u32;
            let vram_c = vram_pool.clone();
            let http_c = http_client.clone();
            let valkey_c = valkey.clone();
            let preload_key_c = preload_key.clone();

            tokio::spawn(async move {
                let success = crate::infrastructure::outbound::ollama::preloader::preload_model(
                    &http_c, &url, &model_c, provider_id, &vram_c, np,
                ).await;
                let _ = valkey_c.kv_del(&preload_key_c).await;
                if success {
                    vram_c.mark_model_loaded(provider_id, &model_c, 0);
                }
            });

            tracing::debug!(provider_id = %p.id, %model, "Preload triggered for queued model");
            break; // one preload per model per cycle
        }
    }

    // ── Step ③: Evict (demand==0 && is_loaded && active==0 && idle threshold) ──
    for p in &ollama_providers {
        for model in vram_pool.loaded_model_names(p.id) {
            // Skip if demand > 0
            if model_demand.contains_key(&model) {
                continue;
            }
            // Skip if active requests
            if vram_pool.active_requests(p.id, &model) > 0 {
                continue;
            }
            // Skip if preloading or pulling
            if vram_pool.is_preloading(p.id, &model) || vram_pool.is_pulling(p.id, &model) {
                continue;
            }

            let idle_secs = vram_pool.idle_since_secs(p.id, &model);

            if should_evict(idle_secs, vram_pool.is_standby(p.id)) {
                vram_pool.mark_model_unloaded(p.id, &model);
                tracing::info!(provider_id = %p.id, %model, idle_secs, "model evicted (demand=0)");
            }
        }
    }

    // ── Step ⑤: Scale-In (server_idle && !last_server && !in_transition) ──
    // Skip if only one Ollama provider (last_server protection)
    if ollama_providers.len() > 1 {
        for p in &ollama_providers {
            // Skip if this server was used in Scale-Out/Preload this cycle
            if scale_out_servers.contains(&p.id) {
                continue;
            }
            // Skip if in hold-down period
            if scale_out_holddown.contains_key(&p.id) {
                continue;
            }
            // Check server_idle: no loaded models with demand, no active requests
            let loaded = vram_pool.loaded_model_names(p.id);
            // Fresh provider (never synced) — no models in VramPool yet.
            // Don't Scale-In until at least one capacity sync has occurred.
            if loaded.is_empty() {
                continue;
            }
            let has_demand = loaded.iter().any(|m| model_demand.contains_key(m));
            if has_demand {
                continue;
            }
            let total_active = vram_pool.provider_active_requests(p.id);
            if total_active > 0 {
                continue;
            }
            // Skip if any model is preloading
            if loaded.iter().any(|m| vram_pool.is_preloading(p.id, m)) {
                continue;
            }

            // Skip if already standby or in transition
            if vram_pool.is_standby(p.id) || vram_pool.in_transition(p.id) {
                continue;
            }

            // Mark as standby (Scale-In)
            vram_pool.set_standby(p.id, true);
            let guard_until = now_ms + TRANSITION_GUARD_SECS * 1000;
            vram_pool.set_transition_until(p.id, guard_until);
            tracing::info!(provider_id = %p.id, "Scale-In — provider marked standby");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_scale_out_needed ──────────────────────────────────────────────

    #[test]
    fn scale_out_not_needed_when_demand_below_threshold() {
        // demand=8, capacity=10 → 8 > 10*0.8=8.0 → false (not strictly greater)
        assert!(!is_scale_out_needed(8, 10));
    }

    #[test]
    fn scale_out_needed_when_demand_exceeds_threshold() {
        // demand=9, capacity=10 → 9 > 8.0 → true
        assert!(is_scale_out_needed(9, 10));
    }

    #[test]
    fn scale_out_not_needed_when_zero_demand() {
        assert!(!is_scale_out_needed(0, 10));
    }

    #[test]
    fn scale_out_needed_when_zero_capacity_and_any_demand() {
        // demand=1, capacity=0 → 1.0 > 0.0 → true
        assert!(is_scale_out_needed(1, 0));
    }

    // ── should_evict ────────────────────────────────────────────────────

    #[test]
    fn evict_when_idle_exceeds_normal_threshold() {
        assert!(should_evict(EVICT_IDLE_SECS, false));
        assert!(should_evict(EVICT_IDLE_SECS + 1, false));
    }

    #[test]
    fn no_evict_when_idle_below_normal_threshold() {
        assert!(!should_evict(EVICT_IDLE_SECS - 1, false));
    }

    #[test]
    fn evict_standby_uses_shorter_threshold() {
        // Standby threshold is 30s vs normal 180s
        assert!(should_evict(STANDBY_EVICT_IDLE_SECS, true));
        assert!(!should_evict(STANDBY_EVICT_IDLE_SECS - 1, true));
    }

    #[test]
    fn standby_threshold_shorter_than_normal() {
        assert!(STANDBY_EVICT_IDLE_SECS < EVICT_IDLE_SECS);
    }
}
