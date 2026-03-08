use uuid::Uuid;

/// Port for per-provider circuit breaker state.
///
/// Abstracts `CircuitBreakerMap` so the application use-case layer is
/// decoupled from the concrete failure isolation implementation.
pub trait CircuitBreakerPort: Send + Sync {
    /// Returns `true` if the provider is allowed to accept new requests.
    fn is_allowed(&self, provider_id: Uuid) -> bool;

    /// Record a successful inference — transitions Open/HalfOpen → Closed.
    fn on_success(&self, provider_id: Uuid);

    /// Record a failed inference — may transition Closed → Open.
    fn on_failure(&self, provider_id: Uuid);

    /// Record a request latency sample (ms). When P99 exceeds the threshold
    /// and enough samples exist, the circuit soft-degrades to HalfOpen.
    fn record_latency(&self, provider_id: Uuid, latency_ms: u64);
}
