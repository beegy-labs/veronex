//! Ollama model lifecycle: load probe + in-flight coalescing + stall detection.
//!
//! This module implements the Phase-1 path of the inference SoD split (SDD:
//! `.specs/veronex/inference-lifecycle-sod.md`). It is plumbed into
//! `OllamaAdapter` as a `ModelLifecyclePort` implementation.
//!
//! ## Why this is non-trivial
//!
//! ollama's `/api/generate` is a **single request-response** HTTP. During a
//! cold load there is no streamed progress — the response opens after the
//! model is fully resident in VRAM. For a 200K-context model that's 163 s of
//! silent loading on AI Max+ 395 / ROCm 7.2 (project memory:
//! `low_power_ollama_lifecycle`); future 1M-context models are projected at
//! 5–10 minutes. A naive "stall = N seconds without bytes from the probe"
//! detector therefore misfires on every cold load.
//!
//! ## Design (post-Tier-B fix, 2026-04-28)
//!
//! Three concurrent observers race the probe. Stall detection is fed from the
//! observer that actually has progress signal — `/api/ps` polling — not from
//! the silent probe socket.
//!
//! ```
//! ┌───────────────────────────── ensure_ready(model) ─────────────────────────┐
//! │                                                                            │
//! │  ① VramPool SSOT — already loaded? → AlreadyLoaded   (no HTTP)             │
//! │  ② In-flight slot coalesce         → LoadCoalesced   (followers wait)      │
//! │  ③ Leader runs run_probe_with_stall:                                       │
//! │                                                                            │
//! │     ┌───────── tokio::select! (biased) ─────────────────────────────────┐  │
//! │     │                                                                    │  │
//! │     │  probe_fut          POST /api/generate { num_predict:0 }           │  │
//! │     │                       returns when model is fully loaded            │  │
//! │     │                       on success → LoadCompleted                     │  │
//! │     │                       on error   → ProviderError                     │  │
//! │     │                                                                    │  │
//! │     │  ps_poller          GET /api/ps every 5 s                          │  │
//! │     │                       when our model appears → record_progress()    │  │
//! │     │                       (never resolves; runs until select winner)    │  │
//! │     │                                                                    │  │
//! │     │  stall_fut          ticks every 5 s                                │  │
//! │     │                       skip while last_progress_at == 0              │  │
//! │     │                       (= "no /api/ps confirmation yet, still loading")│
//! │     │                       once first_progress: stall when               │  │
//! │     │                       now − last_progress_at > STALL_INTERVAL       │  │
//! │     │                                                                    │  │
//! │     │  hard_cap           sleep(LIFECYCLE_LOAD_TIMEOUT + 5 s)             │  │
//! │     │                       fail-safe; never cancelled by a partial load  │  │
//! │     │                                                                    │  │
//! │     │  progress_log       every 30 s emit info!("still loading, T=Xs")    │  │
//! │     │                       observability only; never resolves            │  │
//! │     └───────────────────────────────────────────────────────────────────┘  │
//! └────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! - `last_progress_at` is initialised to **0** (sentinel meaning "no progress
//!   signal observed yet"). Stall detection is a no-op while it is 0.
//! - The probe HTTP request is **never aborted** by the select winners. Closing
//!   the connection mid-load tells ollama to abort the load (`client connection
//!   closed before server finished loading`). Stall and hard-cap therefore
//!   propagate up through the runner so the caller sees a typed
//!   `LifecycleError`, but the probe future continues to be driven (its
//!   `reqwest::timeout(LIFECYCLE_LOAD_TIMEOUT)` is the upper bound).
//! - All three concurrent observers (`probe_fut`, `ps_poller`, `progress_log`)
//!   hold only `&` references into the slot via `Arc<AtomicU64>`; no mutex.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::Deserialize;
use tokio::sync::{Notify, OnceCell};

use crate::application::ports::outbound::model_lifecycle::LifecycleOutcome;
use crate::domain::errors::LifecycleError;

// ── Constants ───────────────────────────────────────────────────────────────

/// Hard cap on a single load attempt. Must exceed ollama's longest cold load
/// path. Measured for `qwen3-coder-next-200k:latest` on AI Max+ 395 / ROCm 7.2:
/// `load_duration` = 163,671 ms. 600 s leaves headroom for 1M-context models;
/// override per provider via future capacity-repo lookup if needed.
pub(super) const LIFECYCLE_LOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// Maximum gap between observed progress updates (driven by `/api/ps` poller)
/// before the slot is poisoned. Semantics: "model loaded per /api/ps but the
/// probe HTTP is not returning" — i.e. ollama HTTP layer hung post-load. Until
/// /api/ps confirms a load, this detector is a no-op (cold loads observe no
/// progress signal until ollama finishes weight ingestion).
pub(super) const LIFECYCLE_STALL_INTERVAL: Duration = Duration::from_secs(60);

/// `/api/ps` polling cadence during an in-flight load. 5 s is short enough to
/// surface load completion within roughly one tick of network jitter and long
/// enough to add negligible load (one HTTP per provider per load attempt).
pub(super) const LIFECYCLE_PS_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Cadence for periodic "still loading" observability logs. Operators tail
/// veronex-api logs to see live progress on long cold loads.
pub(super) const LIFECYCLE_PROGRESS_LOG_INTERVAL: Duration = Duration::from_secs(30);

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
    /// Wall-clock millis at last observed progress signal.
    /// **Sentinel `0`** = no progress yet (load still in initial silent phase).
    /// Updated by:
    ///   1. `/api/ps` poller when our model first appears in the loaded set.
    ///   2. `probe_fut` belt-and-suspenders write on probe success.
    pub last_progress_at: Arc<AtomicU64>,
    pub result: OnceCell<Result<LifecycleOutcome, LifecycleError>>,
}

impl LoadInFlight {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
            notify: Arc::new(Notify::new()),
            // Sentinel 0 — `has_first_progress() == false` until /api/ps
            // confirms the model is loaded. Stall detection is a no-op in
            // this state (it would otherwise misfire on every cold load).
            last_progress_at: Arc::new(AtomicU64::new(0)),
            result: OnceCell::new(),
        }
    }

    pub fn record_progress(&self) {
        let now_ms = wall_clock_ms();
        // Use Release so the stall detector's Acquire load sees a coherent
        // value w.r.t. any prior writes that produced this signal.
        self.last_progress_at.store(now_ms, Ordering::Release);
    }

    /// `true` once at least one progress signal has been observed (typically
    /// `/api/ps` poller seeing our model in the loaded set).
    pub fn has_first_progress(&self) -> bool {
        self.last_progress_at.load(Ordering::Acquire) != 0
    }

    /// Millis since last progress signal. Returns `0` when no signal has been
    /// observed (sentinel state) — stall detection treats this as "not stalled,
    /// load is still in normal silent-loading phase".
    pub fn no_progress_ms(&self) -> u64 {
        let last = self.last_progress_at.load(Ordering::Acquire);
        if last == 0 {
            return 0;
        }
        wall_clock_ms().saturating_sub(last)
    }
}

fn wall_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── /api/ps progress source ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct PsResponse {
    #[serde(default)]
    models: Vec<PsEntry>,
}

#[derive(Deserialize)]
struct PsEntry {
    #[serde(default)]
    name: String,
}

/// Single `GET /api/ps` query — returns the names of models currently resident
/// on the provider. Errors are non-fatal for the lifecycle path: the caller
/// (`run_ps_poller`) treats any error as "no signal this tick".
async fn query_loaded_models(
    client: &reqwest::Client,
    base_url: &str,
) -> Result<Vec<String>, anyhow::Error> {
    let url = format!("{}/api/ps", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .timeout(LIFECYCLE_PS_POLL_INTERVAL) // a poll must finish within one tick
        .send()
        .await
        .with_context(|| format!("query_loaded_models: GET {url}"))?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "query_loaded_models: status {}",
            resp.status()
        ));
    }
    let body: PsResponse = resp
        .json()
        .await
        .with_context(|| "query_loaded_models: deserialize PsResponse")?;
    Ok(body.models.into_iter().map(|m| m.name).collect())
}

// ── Probe runner ─────────────────────────────────────────────────────────────

/// Run a zero-token probe against ollama to load `model` into VRAM.
///
/// Sends `POST /api/generate {prompt:"", num_predict:0, keep_alive}`. ollama
/// auto-loads the model and returns 200 when load completes. The probe HTTP
/// request itself sets `reqwest::timeout(LIFECYCLE_LOAD_TIMEOUT)` as the upper
/// bound on a single attempt.
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

/// Drive a probe to completion with `/api/ps`-fed progress signal, stall
/// detection, observability log, and hard cap. Returns the final outcome
/// (caller stores it in `slot.result`).
///
/// See module-level docs for the design rationale.
pub(super) async fn run_probe_with_stall(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    slot: &LoadInFlight,
) -> Result<LifecycleOutcome, LifecycleError> {
    // ── (1) Canonical success/error path — probe response decides. ──
    let probe_fut = async {
        let elapsed = probe_load(client, base_url, model, LIFECYCLE_KEEP_ALIVE).await?;
        slot.record_progress(); // belt-and-suspenders even if poller already fired
        Ok::<LifecycleOutcome, LifecycleError>(LifecycleOutcome::LoadCompleted {
            duration_ms: elapsed,
        })
    };

    // ── (2) /api/ps poller — the sole progress signal source. ──
    let ps_poller = async {
        let mut interval = tokio::time::interval(LIFECYCLE_PS_POLL_INTERVAL);
        // Skip the immediate-fire first tick — ps wouldn't show a model that
        // ollama has only just been asked to load.
        interval.tick().await;
        loop {
            interval.tick().await;
            match query_loaded_models(client, base_url).await {
                Ok(loaded) if loaded.iter().any(|m| m == model) => {
                    if !slot.has_first_progress() {
                        // First confirmation — log once for observability so
                        // operators see when ollama actually finished load.
                        tracing::info!(
                            %model,
                            elapsed_ms = slot.started_at.elapsed().as_millis() as u64,
                            "lifecycle.probe — /api/ps confirms model loaded; awaiting probe HTTP return"
                        );
                    }
                    slot.record_progress();
                }
                Ok(_) | Err(_) => {} // best-effort
            }
        }
    };

    // ── (3) Stall detector — fires only AFTER first progress confirmation. ──
    let stall_fut = async {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.tick().await;
        loop {
            interval.tick().await;
            if !slot.has_first_progress() {
                continue; // load still in initial silent-loading phase
            }
            let gap_ms = slot.no_progress_ms();
            if gap_ms >= LIFECYCLE_STALL_INTERVAL.as_millis() as u64 {
                return LifecycleError::Stalled {
                    last_progress_ms: gap_ms,
                };
            }
        }
    };

    // ── (4) Periodic observability — emits "still loading, T=Xs" on long loads. ──
    let progress_log = async {
        let mut interval = tokio::time::interval(LIFECYCLE_PROGRESS_LOG_INTERVAL);
        interval.tick().await;
        loop {
            interval.tick().await;
            tracing::info!(
                %model,
                elapsed_s = slot.started_at.elapsed().as_secs(),
                first_progress = slot.has_first_progress(),
                "lifecycle.probe — still loading"
            );
        }
    };

    // ── (5) Hard cap — fail-safe upper bound. ──
    let hard_cap = tokio::time::sleep(LIFECYCLE_LOAD_TIMEOUT + Duration::from_secs(5));

    tokio::select! {
        biased;
        r = probe_fut => r,
        e = stall_fut => Err(e),
        _ = hard_cap => Err(LifecycleError::LoadTimeout {
            elapsed_ms: slot.started_at.elapsed().as_millis() as u64,
            max_ms: LIFECYCLE_LOAD_TIMEOUT.as_millis() as u64,
        }),
        // ps_poller and progress_log are infinite loops — they're polled solely
        // for their side effects (slot.record_progress / tracing::info!). If
        // they ever return, that is a bug; surface it loudly.
        _ = ps_poller => unreachable!("ps_poller exited its infinite loop"),
        _ = progress_log => unreachable!("progress_log exited its infinite loop"),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers, Mock, MockServer, ResponseTemplate};

    // Helper: spin up a wiremock that returns 200 OK after `delay_ms` on
    // `/api/generate`.
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

    /// Mount a `/api/ps` mock that returns the given model names.
    async fn ps_returns(server: &MockServer, models: &[&str]) {
        let body = serde_json::json!({
            "models": models.iter().map(|n| serde_json::json!({"name": n})).collect::<Vec<_>>(),
        });
        Mock::given(matchers::method("GET"))
            .and(matchers::path("/api/ps"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
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
        ps_returns(&server, &["any"]).await;
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

    /// Critical regression test: stall detector must NOT fire while the probe
    /// is in its initial silent-loading phase (no `/api/ps` confirmation yet).
    /// Pre-fix behaviour misfired here on every 200K cold load.
    #[tokio::test]
    async fn stall_does_not_fire_before_first_progress_signal() {
        let server = MockServer::start().await;
        // probe takes 8 s — longer than the 5 s stall tick — but we never
        // mount /api/ps to confirm load, so stall must NOT fire.
        ok_after(&server, 8_000).await;
        let client = reqwest::Client::new();
        let slot = LoadInFlight::new();
        let r = tokio::time::timeout(
            Duration::from_secs(15),
            run_probe_with_stall(&client, &server.uri(), "any", &slot),
        )
        .await
        .expect("must not exceed test timeout")
        .expect("must complete via probe_fut, not stall");
        assert!(matches!(r, LifecycleOutcome::LoadCompleted { .. }));
        assert!(slot.has_first_progress(), "probe success records progress");
    }

    #[tokio::test]
    async fn load_inflight_records_progress() {
        let slot = LoadInFlight::new();
        // Sentinel: no progress yet.
        assert!(!slot.has_first_progress());
        assert_eq!(slot.no_progress_ms(), 0);

        slot.record_progress();
        assert!(slot.has_first_progress());

        tokio::time::sleep(Duration::from_millis(50)).await;
        let after = slot.no_progress_ms();
        assert!(
            after >= 40,
            "expected ≥40 ms gap after record + 50 ms sleep, got {after}"
        );
    }

    #[tokio::test]
    async fn lifecycle_constants_are_consistent() {
        // /api/ps poll cadence < stall interval — otherwise stall would race
        // ahead of progress signals.
        assert!(LIFECYCLE_PS_POLL_INTERVAL < LIFECYCLE_STALL_INTERVAL);
        // Stall < hard cap — stall is the fast-fail path.
        assert!(LIFECYCLE_STALL_INTERVAL < LIFECYCLE_LOAD_TIMEOUT);
        // 600 s hard cap covers measured 200K cold load (163 s) with 4× headroom.
        assert!(
            LIFECYCLE_LOAD_TIMEOUT.as_secs() >= 600,
            "LIFECYCLE_LOAD_TIMEOUT regressed below 600s — would not cover 200K context"
        );
        // Progress log cadence >= ps poll cadence — prevents log spam.
        assert!(LIFECYCLE_PROGRESS_LOG_INTERVAL >= LIFECYCLE_PS_POLL_INTERVAL);
    }
}
