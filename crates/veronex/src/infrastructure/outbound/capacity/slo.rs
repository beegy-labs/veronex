//! Phase 4 — TokenScale SLO buckets and trigger evaluation.
//!
//! TokenScale (arXiv 2512.03416) splits inference requests into three
//! latency buckets by input token count, each with its own TTFT
//! target. Output tokens have a single TPOT SLO. The analyzer ticks
//! every 5s; an SLO breach must persist for `sustained_window` before
//! it fires a `scale_out` event.
//!
//! Pure module — no I/O. The analyzer collects p95 from the metrics
//! pipeline (ClickHouse) and feeds it here.

use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestBucket {
    /// Input < 256 tokens. TTFT target 250ms.
    Short,
    /// Input < 1024 tokens. TTFT target 400ms.
    Medium,
    /// Input ≥ 1024 tokens. TTFT target 2000ms.
    Long,
}

impl RequestBucket {
    pub fn classify(input_tokens: u32) -> Self {
        if input_tokens < 256 {
            Self::Short
        } else if input_tokens < 1024 {
            Self::Medium
        } else {
            Self::Long
        }
    }

    /// Per-bucket TTFT SLO from TokenScale Table 2.
    pub fn ttft_slo_ms(&self) -> u32 {
        match self {
            Self::Short => 250,
            Self::Medium => 400,
            Self::Long => 2000,
        }
    }

    /// All buckets share the same TPOT SLO.
    pub const TPOT_SLO_MS: u32 = 100;
}

/// Operator-tunable thresholds. Defaults match TokenScale §4 / §5
/// recommendations. Stored under canonical `system_settings` keys so
/// admin can adjust without a code change.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SloThresholds {
    /// Multiplier on the bucket TTFT SLO above which a sample counts
    /// as a breach. 1.5× absorbs measurement jitter without missing
    /// real regressions.
    pub ttft_breach_multiplier: f32,
    /// Sustained breach window before firing `scale_out`.
    pub sustained_breach: Duration,
    /// Queue depth threshold (`pending_requests > n_parallel * factor`).
    pub queue_depth_factor: f32,
    /// Sustained queue-overflow window before firing.
    pub sustained_queue: Duration,
}

impl Default for SloThresholds {
    fn default() -> Self {
        Self {
            ttft_breach_multiplier: 1.5,
            sustained_breach: Duration::from_secs(30),
            queue_depth_factor: 1.0,
            sustained_queue: Duration::from_secs(15),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleSignal {
    /// All within SLO; nothing to do.
    Steady,
    /// Some breach is accumulating but hasn't yet sustained.
    Pressuring,
    /// Sustained breach long enough to act.
    ScaleOut,
}

/// Evaluate one tick of TTFT SLO state.
///
/// Returns `ScaleSignal::ScaleOut` only when the breach has been
/// sustained for `sustained_breach`; otherwise returns `Pressuring`
/// while the breach is active or `Steady` otherwise.
///
/// `sustained_for` is the duration the breach has already been
/// accumulating (callers track this externally so the function stays
/// pure).
pub fn evaluate_ttft(
    bucket: RequestBucket,
    p95_ttft_ms: u32,
    sustained_for: Duration,
    t: SloThresholds,
) -> ScaleSignal {
    let breach_threshold =
        (bucket.ttft_slo_ms() as f32 * t.ttft_breach_multiplier) as u32;
    if p95_ttft_ms <= breach_threshold {
        return ScaleSignal::Steady;
    }
    if sustained_for >= t.sustained_breach {
        ScaleSignal::ScaleOut
    } else {
        ScaleSignal::Pressuring
    }
}

/// Evaluate queue-depth pressure. `queue_depth` is the number of
/// pending requests waiting on the provider; `n_parallel` is the
/// provider's hard upper bound.
pub fn evaluate_queue_depth(
    queue_depth: u32,
    n_parallel: u32,
    sustained_for: Duration,
    t: SloThresholds,
) -> ScaleSignal {
    let factor = (n_parallel as f32 * t.queue_depth_factor).max(1.0);
    let threshold = factor as u32;
    if queue_depth <= threshold {
        return ScaleSignal::Steady;
    }
    if sustained_for >= t.sustained_queue {
        ScaleSignal::ScaleOut
    } else {
        ScaleSignal::Pressuring
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_classification_matches_tokenscale_table_2() {
        assert_eq!(RequestBucket::classify(0), RequestBucket::Short);
        assert_eq!(RequestBucket::classify(255), RequestBucket::Short);
        assert_eq!(RequestBucket::classify(256), RequestBucket::Medium);
        assert_eq!(RequestBucket::classify(1023), RequestBucket::Medium);
        assert_eq!(RequestBucket::classify(1024), RequestBucket::Long);
        assert_eq!(RequestBucket::classify(8192), RequestBucket::Long);
    }

    #[test]
    fn ttft_slo_per_bucket() {
        assert_eq!(RequestBucket::Short.ttft_slo_ms(), 250);
        assert_eq!(RequestBucket::Medium.ttft_slo_ms(), 400);
        assert_eq!(RequestBucket::Long.ttft_slo_ms(), 2000);
        assert_eq!(RequestBucket::TPOT_SLO_MS, 100);
    }

    #[test]
    fn ttft_steady_when_within_slo() {
        let s = evaluate_ttft(
            RequestBucket::Short,
            200, // < 250 × 1.5 = 375
            Duration::from_secs(60),
            SloThresholds::default(),
        );
        assert_eq!(s, ScaleSignal::Steady);
    }

    #[test]
    fn ttft_pressuring_below_sustained() {
        // 250 × 1.5 = 375 → 500 is breach; only 10s sustained
        let s = evaluate_ttft(
            RequestBucket::Short,
            500,
            Duration::from_secs(10),
            SloThresholds::default(),
        );
        assert_eq!(s, ScaleSignal::Pressuring);
    }

    #[test]
    fn ttft_scale_out_at_sustained() {
        let s = evaluate_ttft(
            RequestBucket::Short,
            500,
            Duration::from_secs(30),
            SloThresholds::default(),
        );
        assert_eq!(s, ScaleSignal::ScaleOut);
    }

    #[test]
    fn queue_depth_steady_within_capacity() {
        let s = evaluate_queue_depth(
            3,
            4,
            Duration::from_secs(30),
            SloThresholds::default(),
        );
        assert_eq!(s, ScaleSignal::Steady);
    }

    #[test]
    fn queue_depth_scale_out_at_sustained() {
        let s = evaluate_queue_depth(
            10,
            4,
            Duration::from_secs(15),
            SloThresholds::default(),
        );
        assert_eq!(s, ScaleSignal::ScaleOut);
    }
}
