//! Ollama model lifecycle: load probe + in-flight coalescing + stall detection.
//!
//! This module implements the Phase-1 path of the inference SoD split (SDD:
//! `.specs/veronex/inference-lifecycle-sod.md`). It is plumbed into
//! `OllamaAdapter` as a `ModelLifecyclePort` implementation.
//!
//! Design highlights:
//!
//! - SSOT for "is loaded" is `VramPoolPort::loaded_model_names(provider_id)`.
//!   No parallel `/api/ps` cache lives here — `sync_loop` already polls and
//!   reconciles.
//! - Concurrent `ensure_ready(model)` calls on the same adapter coalesce on a
//!   `LoadInFlight` slot keyed by model name; only one HTTP probe runs and
//!   the rest receive `LoadCoalesced`.
//! - Probe = `POST /api/generate {prompt:"", num_predict:0, keep_alive:"30m"}`.
//!   ollama auto-loads the model on this empty-prompt request and returns 200
//!   when ready. `keep_alive: "30m"` aligns with the homelab burst window per
//!   project memory `low_power_ollama_lifecycle`.
//! - Stall detection runs concurrently with the probe: if `last_progress_at`
//!   is not bumped within `LIFECYCLE_STALL_INTERVAL`, the slot is poisoned with
//!   `LifecycleError::Stalled` so the next caller can retry.
//! - Hard cap: `LIFECYCLE_LOAD_TIMEOUT` bounds the worst case (e.g. ROCm OOM
//!   that never returns).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use tokio::sync::{Notify, OnceCell};

use crate::application::ports::outbound::model_lifecycle::LifecycleOutcome;
use crate::domain::errors::LifecycleError;

// ── Constants (sized for measured 200K-context cold load = 163,671 ms) ──────

/// Hard cap on a single load attempt. Must exceed ollama's longest cold load
/// path. Measured for `qwen3-coder-next-200k:latest` on AI Max+ 395 / ROCm 7.2:
/// `load_duration` = 163,671 ms. 600 s leaves headroom for 1M-context models.
pub(super) const LIFECYCLE_LOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// Maximum gap between observed progress updates before the slot is declared
/// stalled. Probe runner bumps `last_progress_at` after the HTTP `send` returns
/// (request accepted by ollama).
pub(super) const LIFECYCLE_STALL_INTERVAL: Duration = Duration::from_secs(60);

/// keep_alive value sent on the probe. Aligns with homelab burst policy
/// (project memory: `low_power_ollama_lifecycle` — idle unload after 10m
/// default, 30m keeps the model warm during an active conversation).
pub(super) const LIFECYCLE_KEEP_ALIVE: &str = "30m";

// ── Shared per-(provider, model) load slot ──────────────────────────────────

/// A load attempt currently in flight. Concurrent `ensure_ready(model)` calls
/// on the same `OllamaAdapter` share one slot; the first call drives the
/// probe, the rest wait on `notify` and read the cloned `result`.
pub(super) struct LoadInFlight {
    pub started_at: Instant,
    pub notify: Arc<Notify>,
    /// Updated by the probe runner each time meaningful progress is observed.
    /// Stall detection compares wall-clock now to this value.
    pub last_progress_at: Arc<AtomicU64>,
    pub result: OnceCell<Result<LifecycleOutcome, LifecycleError>>,
}

impl LoadInFlight {
    pub fn new() -> Self {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            started_at: Instant::now(),
            notify: Arc::new(Notify::new()),
            last_progress_at: Arc::new(AtomicU64::new(now_ms)),
            result: OnceCell::new(),
        }
    }

    pub fn record_progress(&self) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.last_progress_at.store(now_ms, Ordering::Release);
    }

    pub fn no_progress_ms(&self) -> u64 {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        now_ms.saturating_sub(self.last_progress_at.load(Ordering::Acquire))
    }
}

// ── Probe runner ─────────────────────────────────────────────────────────────

/// Run a zero-token probe against ollama to load `model` into VRAM.
///
/// Sends `POST /api/generate {prompt:"", num_predict:0, keep_alive}`. ollama
/// auto-loads the model and returns 200 when it has at least produced the
/// trivial empty response. The HTTP send itself may block for the full
/// `LIFECYCLE_LOAD_TIMEOUT` if the model is genuinely cold.
///
/// Returns `Ok(elapsed_ms)` on success, mapped error on failure.
pub(super) async fn probe_load(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    keep_alive: &str,
) -> Result<u64, LifecycleError> {
    let url = format!("{}/api/generate", base_url.trim_end_matches('/'));
    let started = Instant::now();
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "model": model,
            "prompt": "",
            "num_predict": 0,
            "keep_alive": keep_alive,
        }))
        .timeout(LIFECYCLE_LOAD_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("probe_load: POST {url}"))
        .map_err(|e| LifecycleError::ProviderError(e.to_string()))?;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    if !resp.status().is_success() {
        return Err(LifecycleError::ProviderError(format!(
            "probe_load: ollama returned {}",
            resp.status()
        )));
    }
    Ok(elapsed_ms)
}

/// Drive a probe to completion with concurrent stall detection. Returns the
/// final outcome (caller stores it in `slot.result`).
pub(super) async fn run_probe_with_stall(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    slot: &LoadInFlight,
) -> Result<LifecycleOutcome, LifecycleError> {
    let probe_fut = async {
        let elapsed = probe_load(client, base_url, model, LIFECYCLE_KEEP_ALIVE).await?;
        slot.record_progress();
        Ok::<LifecycleOutcome, LifecycleError>(LifecycleOutcome::LoadCompleted {
            duration_ms: elapsed,
        })
    };

    let stall_fut = async {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if slot.no_progress_ms() >= LIFECYCLE_STALL_INTERVAL.as_millis() as u64 {
                return LifecycleError::Stalled {
                    last_progress_ms: slot.no_progress_ms(),
                };
            }
        }
    };

    let hard_cap = tokio::time::sleep(LIFECYCLE_LOAD_TIMEOUT + Duration::from_secs(5));

    tokio::select! {
        biased;
        r = probe_fut => r,
        e = stall_fut => Err(e),
        _ = hard_cap => Err(LifecycleError::LoadTimeout {
            elapsed_ms: slot.started_at.elapsed().as_millis() as u64,
            max_ms: LIFECYCLE_LOAD_TIMEOUT.as_millis() as u64,
        }),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers, Mock, MockServer, ResponseTemplate};

    // Helper: spin up a wiremock that returns 200 OK after `delay_ms`.
    async fn ok_after(server: &MockServer, delay_ms: u64) {
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/api/generate"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"done": true}))
                    .set_delay(Duration::from_millis(delay_ms)),
            )
            .mount(server)
            .await;
    }

    async fn fail_with(server: &MockServer, status: u16) {
        Mock::given(matchers::method("POST"))
            .and(matchers::path("/api/generate"))
            .respond_with(ResponseTemplate::new(status))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn probe_load_returns_elapsed_on_success() {
        let server = MockServer::start().await;
        ok_after(&server, 50).await;
        let client = reqwest::Client::new();
        let elapsed = probe_load(&client, &server.uri(), "any", "30m")
            .await
            .unwrap();
        assert!(elapsed >= 40, "elapsed_ms = {elapsed}");
    }

    #[tokio::test]
    async fn probe_load_maps_502_to_provider_error() {
        let server = MockServer::start().await;
        fail_with(&server, 502).await;
        let client = reqwest::Client::new();
        let r = probe_load(&client, &server.uri(), "any", "30m").await;
        assert!(matches!(r, Err(LifecycleError::ProviderError(_))));
    }

    #[tokio::test]
    async fn run_probe_with_stall_completes_under_timeout() {
        let server = MockServer::start().await;
        ok_after(&server, 100).await;
        let client = reqwest::Client::new();
        let slot = LoadInFlight::new();
        let r = run_probe_with_stall(&client, &server.uri(), "any", &slot)
            .await
            .unwrap();
        match r {
            LifecycleOutcome::LoadCompleted { duration_ms } => {
                assert!(duration_ms >= 90, "duration_ms = {duration_ms}");
            }
            other => panic!("expected LoadCompleted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn load_inflight_records_progress() {
        let slot = LoadInFlight::new();
        let initial = slot.no_progress_ms();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mid = slot.no_progress_ms();
        slot.record_progress();
        let after = slot.no_progress_ms();
        assert!(mid >= initial, "expected mid >= initial");
        assert!(after < mid, "expected progress reset to lower value");
    }

    #[tokio::test]
    async fn lifecycle_constants_are_consistent() {
        // Stall must fire faster than the hard timeout, otherwise it's redundant.
        assert!(LIFECYCLE_STALL_INTERVAL < LIFECYCLE_LOAD_TIMEOUT);
        // 600 s hard cap covers the measured 200K cold load (163 s) with 4× headroom.
        assert!(
            LIFECYCLE_LOAD_TIMEOUT.as_secs() >= 600,
            "LIFECYCLE_LOAD_TIMEOUT regressed below 600s — would not cover 200K context"
        );
    }
}
