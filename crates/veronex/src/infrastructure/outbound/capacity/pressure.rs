//! Phase 4 — dual-pressure signal computation.
//!
//! Combines three normalized 0..1 indicators that the AIMD controller
//! turns into window changes:
//!
//! - `U_t` — KV-cache utilization across all slots (Concur).
//! - `H_t` — prefix overlap with active prompts; high values mean cache
//!   reuse is winning (MARS dual-pressure).
//! - `T_t` — MCP tool in-flight ratio (MARS); cuts even when KV is fine.
//!
//! Sources:
//! - Concur (arXiv 2601.22705)        — KV utilization & cut/grow rules.
//! - MARS  (arXiv 2604.26963)        — dual-pressure structure.
//! - TokenScale (arXiv 2512.03416)    — TTFT/TPOT SLO buckets (in `slo.rs`).

use serde::{Deserialize, Serialize};

/// Three independent pressure indicators in `[0.0, 1.0]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct PressureSignals {
    /// KV-cache utilization. `sum(slot.tokens) / (n_ctx * n_parallel)`.
    pub u_t: f32,
    /// Prefix-overlap hit rate over the recent moving window. Values
    /// closer to 1.0 mean the same conversation is reusing slot caches.
    pub h_t: f32,
    /// MCP tool concurrency pressure. `inflight / capacity`. EMA over
    /// the analyzer interval.
    pub t_t: f32,
}

impl PressureSignals {
    /// Clamp every component into `[0, 1]`. The analyzer feeds raw
    /// ratios; this guards against transient counter weirdness.
    pub fn clamp(self) -> Self {
        Self {
            u_t: self.u_t.clamp(0.0, 1.0),
            h_t: self.h_t.clamp(0.0, 1.0),
            t_t: self.t_t.clamp(0.0, 1.0),
        }
    }
}

/// Tunable thresholds. Defaults are the Concur + MARS recommended
/// values; the analyzer reads overrides from `system_settings` so an
/// operator can re-tune without a code change.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    /// Below `u_low` (Concur 0.2) → grow allowed when other signals OK.
    pub u_low: f32,
    /// Above `u_high` (Concur 0.5) → cut when prefix overlap also low.
    pub u_high: f32,
    /// `H_t < h_thresh` (MARS 0.2) qualifies "low cache reuse" — paired
    /// with high U for the cut condition.
    pub h_thresh: f32,
    /// MCP tool grow ceiling (MARS 0.2).
    pub t_low: f32,
    /// MCP tool cut floor (MARS 0.5).
    pub t_high: f32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            u_low: 0.2,
            u_high: 0.5,
            h_thresh: 0.2,
            t_low: 0.2,
            t_high: 0.5,
        }
    }
}

/// Per Concur §3.2 — slot-aggregate KV utilization. Returns 0 when
/// `n_ctx * n_parallel` is zero (uninitialized provider).
pub fn kv_utilization(
    sum_slot_tokens: u64,
    n_ctx: u32,
    n_parallel: u32,
) -> f32 {
    let denom = (n_ctx as u64).saturating_mul(n_parallel as u64);
    if denom == 0 {
        return 0.0;
    }
    (sum_slot_tokens as f64 / denom as f64).clamp(0.0, 1.0) as f32
}

/// Exponential moving average — used by the analyzer to smooth the MCP
/// tool ratio and the prefix overlap indicator. `alpha` is the new
/// sample weight; 0.3 is the MARS recommendation for this update rate.
pub fn ema(prev: f32, sample: f32, alpha: f32) -> f32 {
    prev + alpha * (sample - prev)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn kv_utilization_zero_when_uninitialized() {
        assert_eq!(kv_utilization(0, 0, 0), 0.0);
        assert_eq!(kv_utilization(100, 0, 4), 0.0);
        assert_eq!(kv_utilization(100, 1024, 0), 0.0);
    }

    #[test]
    fn kv_utilization_basic_ratio() {
        // half full: 2 slots × 512 used out of 1024 ctx × 2 parallel
        let u = kv_utilization(1024, 1024, 2);
        assert!((u - 0.5).abs() < 1e-6);
    }

    #[test]
    fn kv_utilization_clamps_at_one() {
        // overflow scenario shouldn't blow past 1.0
        let u = kv_utilization(10_000, 100, 1);
        assert_eq!(u, 1.0);
    }

    #[test]
    fn clamp_bounds_signals() {
        let s = PressureSignals { u_t: -0.5, h_t: 1.5, t_t: 0.5 }.clamp();
        assert_eq!(s.u_t, 0.0);
        assert_eq!(s.h_t, 1.0);
        assert_eq!(s.t_t, 0.5);
    }

    #[test]
    fn ema_converges_toward_sample() {
        let mut x = 0.0;
        for _ in 0..50 {
            x = ema(x, 1.0, 0.3);
        }
        assert!(x > 0.99);
    }

    #[test]
    fn defaults_match_concur_and_mars_recommendations() {
        let t = Thresholds::default();
        assert_eq!(t.u_low, 0.2);
        assert_eq!(t.u_high, 0.5);
        assert_eq!(t.h_thresh, 0.2);
        assert_eq!(t.t_low, 0.2);
        assert_eq!(t.t_high, 0.5);
    }
}
