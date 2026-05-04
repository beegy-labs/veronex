//! Outbound port: model lifecycle (load/unload/health).
//!
//! Separates Phase 1 (resource acquisition — model weights + KV cache + warmup)
//! from Phase 2 (token generation, see `InferenceProviderPort`).
//!
//! Caller contract:
//!   1. invoke `ensure_ready(model)` and wait for Ok
//!   2. then proceed to `InferenceProviderPort::stream_tokens` / `infer`
//!
//! Implementations MUST resolve `num_ctx` internally from the same SSOT the
//! inference path uses (Valkey `ollama_model_ctx` cache → fabricate fallback)
//! and send it to the provider. ollama's scheduler treats the same model with
//! different `KvSize` (== num_ctx) as separate runner subprocesses
//! (`OLLAMA_NUM_PARALLEL=1`); a Phase 1 / Phase 2 mismatch triggers a second
//! cold-load. SDD: `.specs/veronex/lifecycle-num-ctx-ssot-alignment.md`.
//!
//! Implementations MUST coalesce concurrent same-model calls within a single
//! provider (idempotent in-flight dedup) and update the VramPool SSOT on
//! load completion / failure.
//!
//! SDD reference: `.specs/veronex/history/inference-lifecycle-sod.md`.

use async_trait::async_trait;

use crate::domain::errors::LifecycleError;
use crate::domain::value_objects::{EvictionReason, ModelInstanceState};

#[async_trait]
pub trait ModelLifecyclePort: Send + Sync {
    /// Postcondition: returns Ok ⇒ the model is in `Loaded` state on this provider.
    /// The caller may proceed to `stream_tokens` immediately on Ok.
    ///
    /// **Implementation contract**: ollama-backed impls MUST resolve `num_ctx`
    /// from the same source the inference port uses (sync SSOT → fabricate
    /// fallback) and include `options.num_ctx` in the probe body. A Phase 1 /
    /// Phase 2 mismatch causes ollama to spawn a second runner subprocess for
    /// the same model (verified 2026-04-30: 220 + 232 s instead of 220 s).
    /// SDD: `.specs/veronex/lifecycle-num-ctx-ssot-alignment.md`.
    ///
    /// Concurrent same-model calls coalesce on a per-(provider, model) in-flight
    /// slot; only one HTTP probe runs and the rest receive a `LoadCoalesced`
    /// outcome carrying the wait time.
    async fn ensure_ready(&self, model: &str) -> Result<LifecycleOutcome, LifecycleError>;

    /// Read-only snapshot — used by VramPool / capacity planner / dashboards.
    /// Does NOT trigger a load.
    async fn instance_state(&self, model: &str) -> ModelInstanceState;

    /// Operator-driven eviction (e.g. model unenrolled, VRAM pressure rebalance).
    /// Implementations MAY ignore for cloud providers (no-op).
    async fn evict(&self, model: &str, reason: EvictionReason) -> Result<(), LifecycleError>;
}

/// Outcome of `ensure_ready`. Carries enough information for the runner to
/// emit a single tracing span with phase + duration without a second call.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleOutcome {
    /// VramPool SSOT said the model is already loaded. Returns in <1 ms.
    AlreadyLoaded,

    /// We triggered the load; ollama returned 200 OK after `duration_ms`.
    LoadCompleted { duration_ms: u64 },

    /// Another in-flight load completed for us. We waited `waited_ms`.
    LoadCoalesced { waited_ms: u64 },
}

#[cfg(test)]
pub mod mock {
    //! In-memory `ModelLifecyclePort` mock for use case / runner tests.
    //! Reproduces the real adapter's contract minus HTTP / VramPool side-effects.

    use std::collections::HashMap;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;

    /// Behaviour to inject for a given model name.
    #[derive(Debug, Clone)]
    pub enum MockBehaviour {
        AlreadyLoaded,
        LoadAfter { duration_ms: u64 },
        Fail(LifecycleError),
    }

    pub struct MockLifecycle {
        behaviour: Mutex<HashMap<String, MockBehaviour>>,
        ensure_ready_calls: Mutex<HashMap<String, u32>>,
        evict_calls: Mutex<HashMap<String, u32>>,
    }

    impl MockLifecycle {
        pub fn new() -> Self {
            Self {
                behaviour: Mutex::new(HashMap::new()),
                ensure_ready_calls: Mutex::new(HashMap::new()),
                evict_calls: Mutex::new(HashMap::new()),
            }
        }

        pub fn set(&self, model: &str, b: MockBehaviour) {
            self.behaviour.lock().unwrap().insert(model.to_string(), b);
        }

        pub fn call_count(&self, model: &str) -> u32 {
            *self.ensure_ready_calls.lock().unwrap().get(model).unwrap_or(&0)
        }

        pub fn evict_count(&self, model: &str) -> u32 {
            *self.evict_calls.lock().unwrap().get(model).unwrap_or(&0)
        }
    }

    impl Default for MockLifecycle {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl ModelLifecyclePort for MockLifecycle {
        async fn ensure_ready(&self, model: &str) -> Result<LifecycleOutcome, LifecycleError> {
            *self
                .ensure_ready_calls
                .lock()
                .unwrap()
                .entry(model.to_string())
                .or_insert(0) += 1;

            let b = self
                .behaviour
                .lock()
                .unwrap()
                .get(model)
                .cloned()
                .unwrap_or(MockBehaviour::AlreadyLoaded);

            match b {
                MockBehaviour::AlreadyLoaded => Ok(LifecycleOutcome::AlreadyLoaded),
                MockBehaviour::LoadAfter { duration_ms } => {
                    Ok(LifecycleOutcome::LoadCompleted { duration_ms })
                }
                MockBehaviour::Fail(e) => Err(e),
            }
        }

        async fn instance_state(&self, _model: &str) -> ModelInstanceState {
            ModelInstanceState::NotLoaded
        }

        async fn evict(
            &self,
            model: &str,
            _reason: EvictionReason,
        ) -> Result<(), LifecycleError> {
            *self
                .evict_calls
                .lock()
                .unwrap()
                .entry(model.to_string())
                .or_insert(0) += 1;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock::{MockBehaviour, MockLifecycle};
    use super::*;

    #[tokio::test]
    async fn mock_default_returns_already_loaded() {
        let m = MockLifecycle::new();
        let r = m.ensure_ready("any").await.unwrap();
        assert_eq!(r, LifecycleOutcome::AlreadyLoaded);
        assert_eq!(m.call_count("any"), 1);
    }

    #[tokio::test]
    async fn mock_set_load_after_returns_load_completed() {
        let m = MockLifecycle::new();
        m.set("big-model", MockBehaviour::LoadAfter { duration_ms: 163_000 });
        let r = m.ensure_ready("big-model").await.unwrap();
        assert!(matches!(r, LifecycleOutcome::LoadCompleted { duration_ms } if duration_ms == 163_000));
    }

    #[tokio::test]
    async fn mock_set_fail_propagates_error_variant() {
        let m = MockLifecycle::new();
        m.set(
            "broken",
            MockBehaviour::Fail(LifecycleError::CircuitOpen),
        );
        let r = m.ensure_ready("broken").await;
        assert!(matches!(r, Err(LifecycleError::CircuitOpen)));
    }

    #[tokio::test]
    async fn mock_evict_increments_counter() {
        let m = MockLifecycle::new();
        let _ = m.evict("model", EvictionReason::Operator).await.unwrap();
        let _ = m.evict("model", EvictionReason::VramPressure).await.unwrap();
        assert_eq!(m.evict_count("model"), 2);
    }
}
