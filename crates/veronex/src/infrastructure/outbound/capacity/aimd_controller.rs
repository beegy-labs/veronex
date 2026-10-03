//! Phase 4 — AIMD window controller (pure function).
//!
//! Combines the dual-pressure signals with the Concur AIMD rule. Pure —
//! no I/O, no clock. The analyzer loop calls this every
//! `control_interval` (5s by default) and persists the new window via
//! [`AimdRegistry::set_window`] when it differs.
//!
//! Intent map:
//!
//! | Condition                                       | Action       | Source         |
//! |-------------------------------------------------|--------------|----------------|
//! | `U > u_high && H < h_thresh`                    | cut by β     | Concur §3.2     |
//! | `T > t_high`                                    | cut by β     | MARS §4.1       |
//! | `U < u_low && T < t_low`                        | grow by α    | MARS §4.1       |
//! | otherwise                                        | hold         | Concur          |
//!
//! Defaults: α = 2 (additive grow), β = 0.5 (multiplicative cut).

use crate::infrastructure::outbound::capacity::aimd_registry::AimdState;
use crate::infrastructure::outbound::capacity::pressure::{PressureSignals, Thresholds};

/// Concur AIMD additive-grow constant. Each grow adds `ALPHA` slots.
pub const ALPHA: u32 = 2;

/// Concur AIMD multiplicative-cut factor.
pub const BETA: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    /// New window equals previous; no DB write needed.
    Hold,
    /// Grow by `ALPHA`; new window clamped at `n_parallel`.
    Grow,
    /// Cut by `BETA`; new window clamped at 1.
    Cut,
}

/// Decide on `Hold | Grow | Cut` from the signals.
pub fn classify(signals: PressureSignals, t: Thresholds) -> WindowAction {
    let s = signals.clamp();

    // MARS dual-condition cut: either KV is hot AND cache reuse is low,
    // or the MCP tool concurrency is saturated. Prefix overlap acts as
    // a damper — high reuse means a "hot" cache is doing useful work,
    // so we don't shrink.
    let kv_overload = s.u_t > t.u_high && s.h_t < t.h_thresh;
    let tool_overload = s.t_t > t.t_high;
    if kv_overload || tool_overload {
        return WindowAction::Cut;
    }

    // MARS both-healthy grow.
    if s.u_t < t.u_low && s.t_t < t.t_low {
        return WindowAction::Grow;
    }

    WindowAction::Hold
}

/// Apply [`classify`] to a state and return the new window. Pure —
/// callers persist via `registry.set_window` when the value changes.
pub fn update_window(
    state: &AimdState,
    signals: PressureSignals,
    t: Thresholds,
) -> u32 {
    match classify(signals, t) {
        WindowAction::Hold => state.window,
        WindowAction::Grow => state
            .window
            .saturating_add(ALPHA)
            .min(state.n_parallel),
        WindowAction::Cut => {
            let cut = (state.window as f32 * BETA).floor() as u32;
            cut.max(1)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn signals(u: f32, h: f32, t: f32) -> PressureSignals {
        PressureSignals { u_t: u, h_t: h, t_t: t }
    }

    fn state(window: u32, n_parallel: u32) -> AimdState {
        AimdState { window, n_parallel, last_update: std::time::Instant::now() }
    }

    #[test]
    fn cut_when_kv_hot_and_cache_cold() {
        // U > 0.5, H < 0.2 → cut (Concur)
        let action = classify(signals(0.8, 0.1, 0.0), Thresholds::default());
        assert_eq!(action, WindowAction::Cut);
    }

    #[test]
    fn no_cut_when_kv_hot_but_cache_warm() {
        // High cache reuse damps the cut: hot but reuse > h_thresh → hold
        let action = classify(signals(0.8, 0.5, 0.0), Thresholds::default());
        assert_eq!(action, WindowAction::Hold);
    }

    #[test]
    fn cut_when_mcp_tool_saturated() {
        // T > 0.5 alone → cut (MARS)
        let action = classify(signals(0.1, 0.9, 0.7), Thresholds::default());
        assert_eq!(action, WindowAction::Cut);
    }

    #[test]
    fn grow_when_both_healthy() {
        // U < 0.2 AND T < 0.2 → grow (MARS)
        let action = classify(signals(0.1, 0.0, 0.05), Thresholds::default());
        assert_eq!(action, WindowAction::Grow);
    }

    #[test]
    fn hold_in_neutral_zone() {
        // U in 0.2..0.5 → hold
        let action = classify(signals(0.3, 0.0, 0.1), Thresholds::default());
        assert_eq!(action, WindowAction::Hold);
    }

    #[test]
    fn update_window_grow_caps_at_n_parallel() {
        let s = state(7, 8);
        let new = update_window(
            &s,
            signals(0.05, 0.0, 0.05),
            Thresholds::default(),
        );
        assert_eq!(new, 8); // 7 + 2 capped at 8
    }

    #[test]
    fn update_window_cut_floor_one() {
        let s = state(1, 8);
        let new = update_window(
            &s,
            signals(0.9, 0.0, 0.0),
            Thresholds::default(),
        );
        assert_eq!(new, 1); // already at floor
    }

    #[test]
    fn update_window_cut_halves() {
        let s = state(8, 16);
        let new = update_window(
            &s,
            signals(0.9, 0.0, 0.0),
            Thresholds::default(),
        );
        assert_eq!(new, 4); // 8 * 0.5
    }

    #[test]
    fn update_window_hold_preserves() {
        let s = state(5, 16);
        let new = update_window(
            &s,
            signals(0.3, 0.0, 0.3),
            Thresholds::default(),
        );
        assert_eq!(new, 5);
    }

    #[test]
    fn signals_out_of_range_are_clamped_before_compare() {
        // u_t = 2.0 should clamp to 1.0 → still > u_high → cut
        let action = classify(signals(2.0, 0.0, 0.0), Thresholds::default());
        assert_eq!(action, WindowAction::Cut);
    }
}
