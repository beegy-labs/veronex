use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use uuid::Uuid;

use crate::application::ports::outbound::thermal_port::ThermalPort;
pub use crate::domain::enums::ThrottleLevel;

/// Per-provider thermal thresholds (°C).
#[derive(Debug, Clone, Copy)]
pub struct ThermalThresholds {
    /// Below this → Normal (if not in cooldown).
    pub normal_below: f32,
    /// At or above this → Soft throttle.
    pub soft_at: f32,
    /// At or above this → Hard throttle.
    pub hard_at: f32,
}

impl ThermalThresholds {
    /// GPU defaults (NVIDIA): 80/88/93°C
    pub const GPU: Self = Self { normal_below: 80.0, soft_at: 88.0, hard_at: 93.0 };
    /// CPU/iGPU defaults (Ryzen AI, NPU-class): 75/82/90°C
    pub const CPU: Self = Self { normal_below: 75.0, soft_at: 82.0, hard_at: 90.0 };
}

impl Default for ThermalThresholds {
    fn default() -> Self { Self::CPU }
}

/// Performance scaling factor based on temperature.
/// Used by queue scoring to reduce age_bonus on hot servers.
/// Linear interpolation: ≤75°C→1.0, 82°C→0.70, ≥90°C→0.0.
pub fn perf_factor(temp_c: f32) -> f32 {
    if temp_c <= 75.0 {
        1.0
    } else if temp_c >= 90.0 {
        0.0
    } else if temp_c <= 82.0 {
        // 75..=82: 1.0 → 0.70 (linear)
        1.0 - (temp_c - 75.0) * (0.30 / 7.0)
    } else {
        // 82..90: 0.70 → 0.0 (linear)
        0.70 - (temp_c - 82.0) * (0.70 / 8.0)
    }
}

struct ThrottleState {
    level: ThrottleLevel,
    temp_c: f32,
    /// When Hard throttle was first entered (for forced drain timeout).
    hard_since: Option<Instant>,
    /// When Cooldown state was entered (for cooldown_secs tracking).
    cooldown_entered_at: Option<Instant>,
    /// Σ max_concurrent across all loaded models at Hard entry.
    /// Carried through Hard → Cooldown → RampUp for the RampUp → Normal exit condition.
    pre_hard_total: u32,
}

/// Thread-safe map of provider_id → thermal throttle state.
///
/// Updated by the health checker every 30 s.
/// Read by the queue dispatcher on every job dispatch (~0.1 ms, in-memory only).
#[derive(Clone)]
pub struct ThermalThrottleMap {
    states:       Arc<DashMap<Uuid, ThrottleState>>,
    thresholds:   Arc<DashMap<Uuid, ThermalThresholds>>,
    default_thresholds: ThermalThresholds,
    cooldown_secs: u64,
}

impl ThermalThrottleMap {
    pub fn new(cooldown_secs: u64) -> Self {
        Self {
            states: Arc::new(DashMap::new()),
            thresholds: Arc::new(DashMap::new()),
            default_thresholds: ThermalThresholds::default(),
            cooldown_secs,
        }
    }

    /// Set custom thermal thresholds for a specific provider.
    pub fn set_thresholds(&self, provider_id: Uuid, t: ThermalThresholds) {
        self.thresholds.insert(provider_id, t);
    }

    fn get_thresholds(&self, provider_id: Uuid) -> ThermalThresholds {
        self.thresholds
            .get(&provider_id)
            .map(|t| *t)
            .unwrap_or(self.default_thresholds)
    }

    /// Update thermal state for a provider and return the new level.
    ///
    /// Implements the 5-state thermal machine:
    /// Normal → Soft → Hard → Cooldown → RampUp → Normal
    ///
    /// `active_count`       — provider-wide in-flight request count (for Soft hysteresis).
    /// `sum_max_concurrent` — Σ max_concurrent across loaded models (for RampUp → Normal check).
    pub fn update(
        &self,
        provider_id: Uuid,
        temp_c: f32,
        active_count: u32,
        sum_max_concurrent: u32,
    ) -> ThrottleLevel {
        let prev = self.states.get(&provider_id)
            .map(|s| (s.level, s.hard_since, s.cooldown_entered_at, s.pre_hard_total));

        let (prev_level, prev_hard_since, prev_cooldown_entered, prev_pre_hard_total) = prev
            .unwrap_or((ThrottleLevel::Normal, None, None, 0));

        let th = self.get_thresholds(provider_id);

        let next = match prev_level {
            ThrottleLevel::Normal => {
                if temp_c >= th.hard_at {
                    ThrottleLevel::Hard
                } else if temp_c >= th.soft_at {
                    ThrottleLevel::Soft
                } else {
                    ThrottleLevel::Normal
                }
            }
            ThrottleLevel::Soft => {
                if temp_c >= th.hard_at {
                    ThrottleLevel::Hard
                } else if temp_c < th.normal_below && active_count == 0 {
                    // Hysteresis: temp must drop below normal_below AND all in-flight must finish.
                    // active_count == 0 prevents releasing the gate mid-stream (SDD §3).
                    ThrottleLevel::Normal
                } else {
                    ThrottleLevel::Soft
                }
            }
            ThrottleLevel::Hard => {
                if temp_c >= th.hard_at {
                    ThrottleLevel::Hard
                } else {
                    // Remain Hard until placement_planner calls set_cooldown() when active==0.
                    // Fallback: after cooldown_secs (300s) auto-advance to avoid infinite Hard.
                    let force_advance = prev_hard_since
                        .map(|t| t.elapsed().as_secs() >= self.cooldown_secs)
                        .unwrap_or(false);
                    if force_advance {
                        ThrottleLevel::Cooldown
                    } else {
                        ThrottleLevel::Hard
                    }
                }
            }
            ThrottleLevel::Cooldown => {
                if temp_c >= th.hard_at {
                    // Temp re-surged: reset cooldown timer (re-enter Cooldown).
                    ThrottleLevel::Cooldown
                } else {
                    let cooldown_elapsed = prev_cooldown_entered
                        .map(|t| t.elapsed().as_secs() >= self.cooldown_secs)
                        .unwrap_or(false);
                    let max_wait_exceeded = prev_cooldown_entered
                        .map(|t| t.elapsed().as_secs() >= self.cooldown_secs * 3)
                        .unwrap_or(false);

                    if cooldown_elapsed && temp_c < th.soft_at {
                        ThrottleLevel::RampUp
                    } else if max_wait_exceeded {
                        // Force transition based on current temp.
                        if temp_c >= th.soft_at {
                            ThrottleLevel::Soft
                        } else {
                            ThrottleLevel::RampUp
                        }
                    } else {
                        ThrottleLevel::Cooldown
                    }
                }
            }
            ThrottleLevel::RampUp => {
                if temp_c >= th.hard_at {
                    ThrottleLevel::Hard
                } else if temp_c >= th.soft_at {
                    ThrottleLevel::Soft
                } else if temp_c < th.normal_below {
                    // AIMD restoration check: current Σ max_concurrent must reach pre-Hard level.
                    // pre_hard_total == 0 means Hard was never entered (e.g. direct RampUp test) → allow.
                    if prev_pre_hard_total == 0 || sum_max_concurrent >= prev_pre_hard_total {
                        ThrottleLevel::Normal
                    } else {
                        ThrottleLevel::RampUp // AIMD still restoring
                    }
                } else {
                    ThrottleLevel::RampUp
                }
            }
        };

        // Persist state.
        match next {
            ThrottleLevel::Normal => {
                self.states.remove(&provider_id);
            }
            ThrottleLevel::Soft => {
                self.states.insert(provider_id, ThrottleState {
                    level: next, temp_c, hard_since: None, cooldown_entered_at: None,
                    pre_hard_total: 0,
                });
            }
            ThrottleLevel::Hard => {
                let hard_since = if prev_level == ThrottleLevel::Hard {
                    prev_hard_since
                } else {
                    Some(Instant::now())
                };
                // Snapshot Σ max_concurrent at first Hard entry.
                // On re-entry (RampUp → Hard) preserve the existing snapshot so
                // the RampUp exit condition is anchored to the original pre-Hard level.
                let pre_hard_total = if prev_level == ThrottleLevel::Hard {
                    prev_pre_hard_total
                } else {
                    sum_max_concurrent
                };
                self.states.insert(provider_id, ThrottleState {
                    level: next, temp_c, hard_since, cooldown_entered_at: None,
                    pre_hard_total,
                });
            }
            ThrottleLevel::Cooldown => {
                let cooldown_entered_at = if prev_level == ThrottleLevel::Cooldown && temp_c < th.hard_at {
                    // Keep existing cooldown timer (no reset unless temp re-surges).
                    prev_cooldown_entered
                } else {
                    // New Cooldown entry or timer reset on temp re-surge.
                    Some(Instant::now())
                };
                // Carry pre_hard_total through Cooldown for RampUp → Normal exit check.
                self.states.insert(provider_id, ThrottleState {
                    level: next, temp_c, hard_since: None, cooldown_entered_at,
                    pre_hard_total: prev_pre_hard_total,
                });
            }
            ThrottleLevel::RampUp => {
                // Carry pre_hard_total through RampUp until Normal is reached.
                self.states.insert(provider_id, ThrottleState {
                    level: next, temp_c, hard_since: None, cooldown_entered_at: None,
                    pre_hard_total: prev_pre_hard_total,
                });
            }
        }

        next
    }

    /// Current throttle level for a provider (Normal if no state recorded).
    pub fn get(&self, provider_id: Uuid) -> ThrottleLevel {
        self.states
            .get(&provider_id)
            .map(|s| s.level)
            .unwrap_or(ThrottleLevel::Normal)
    }

    /// Last recorded temperature for a provider.
    pub fn temp_c(&self, provider_id: Uuid) -> Option<f32> {
        self.states.get(&provider_id).map(|s| s.temp_c)
    }

    /// Seconds elapsed since the provider entered Hard state (None if not in Hard).
    pub fn hard_since_elapsed_secs(&self, provider_id: Uuid) -> Option<u64> {
        self.states
            .get(&provider_id)
            .and_then(|s| s.hard_since.map(|t| t.elapsed().as_secs()))
    }

    /// Force Hard → Cooldown transition (called by placement_planner when active==0).
    pub fn set_cooldown(&self, provider_id: Uuid) {
        if let Some(mut state) = self.states.get_mut(&provider_id) {
            if state.level == ThrottleLevel::Hard {
                state.level = ThrottleLevel::Cooldown;
                state.hard_since = None;
                state.cooldown_entered_at = Some(Instant::now());
            }
        }
    }
}

impl ThermalPort for ThermalThrottleMap {
    fn get_level(&self, provider_id: Uuid) -> ThrottleLevel {
        self.get(provider_id)
    }

    fn perf_factor(&self, provider_id: Uuid) -> f32 {
        self.states
            .get(&provider_id)
            .map(|s| perf_factor(s.temp_c))
            .unwrap_or(1.0)
    }

    fn global_perf_factor(&self) -> f32 {
        let mut min_pf = 1.0_f32;
        for entry in self.states.iter() {
            let pf = perf_factor(entry.value().temp_c);
            if pf < min_pf {
                min_pf = pf;
            }
        }
        min_pf
    }

    fn hard_since_elapsed_secs(&self, provider_id: Uuid) -> Option<u64> {
        self.hard_since_elapsed_secs(provider_id)
    }

    fn set_cooldown(&self, provider_id: Uuid) {
        self.set_cooldown(provider_id);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn perf_factor_boundaries() {
        assert_eq!(perf_factor(70.0), 1.0);
        assert_eq!(perf_factor(75.0), 1.0);
        assert!((perf_factor(82.0) - 0.70).abs() < 0.01);
        assert_eq!(perf_factor(90.0), 0.0);
        assert_eq!(perf_factor(95.0), 0.0);
    }

    #[test]
    fn perf_factor_interpolation() {
        // Midpoint of 75..82 range (78.5°C)
        let mid = perf_factor(78.5);
        assert!(mid > 0.70 && mid < 1.0, "mid={mid}");

        // Midpoint of 82..90 range (86°C)
        let mid2 = perf_factor(86.0);
        assert!(mid2 > 0.0 && mid2 < 0.70, "mid2={mid2}");
    }

    // Helper: call update with default active_count=0, sum_mc=10 (assume AIMD restored).
    fn upd(map: &ThermalThrottleMap, id: Uuid, temp: f32) -> ThrottleLevel {
        map.update(id, temp, 0, 10)
    }

    #[test]
    fn normal_to_soft_to_hard() {
        let map = ThermalThrottleMap::new(300);
        let id = Uuid::now_v7();

        assert_eq!(upd(&map, id, 70.0), ThrottleLevel::Normal);
        assert_eq!(upd(&map, id, 83.0), ThrottleLevel::Soft);
        assert_eq!(upd(&map, id, 91.0), ThrottleLevel::Hard);
    }

    #[test]
    fn soft_hysteresis_requires_active_zero() {
        let map = ThermalThrottleMap::new(300);
        let id = Uuid::now_v7();

        upd(&map, id, 83.0); // → Soft
        // Below normal_below but active_count > 0 → stays Soft
        assert_eq!(map.update(id, 74.0, 1, 10), ThrottleLevel::Soft);
        // Below normal_below AND active_count == 0 → Normal
        assert_eq!(map.update(id, 74.0, 0, 10), ThrottleLevel::Normal);
    }

    #[test]
    fn soft_hysteresis_temp_zone() {
        let map = ThermalThrottleMap::new(300);
        let id = Uuid::now_v7();

        upd(&map, id, 83.0); // → Soft
        // Still in hysteresis zone (between normal_below and soft_at), active=0
        assert_eq!(map.update(id, 76.0, 0, 10), ThrottleLevel::Soft);
        // Below normal_below, active=0 → Normal
        assert_eq!(map.update(id, 74.0, 0, 10), ThrottleLevel::Normal);
    }

    #[test]
    fn rampup_to_normal_requires_pre_hard_total() {
        let map = ThermalThrottleMap::new(0); // 0s cooldown for test
        let id = Uuid::now_v7();

        // Enter Hard with sum_mc=8
        map.update(id, 91.0, 0, 8); // → Hard, pre_hard_total=8
        map.update(id, 80.0, 0, 8); // → Cooldown
        // With 0s cooldown elapsed and temp < soft_at → RampUp
        assert_eq!(map.update(id, 70.0, 0, 8), ThrottleLevel::RampUp);
        // AIMD not restored yet (sum_mc=4 < pre_hard_total=8) → stays RampUp
        assert_eq!(map.update(id, 70.0, 0, 4), ThrottleLevel::RampUp);
        // AIMD restored (sum_mc=8 >= pre_hard_total=8) → Normal
        assert_eq!(map.update(id, 70.0, 0, 8), ThrottleLevel::Normal);
    }

    #[test]
    fn rampup_re_enters_hard_on_spike() {
        let map = ThermalThrottleMap::new(0);
        let id = Uuid::now_v7();

        upd(&map, id, 91.0); // → Hard
        upd(&map, id, 80.0); // → Cooldown
        upd(&map, id, 70.0); // → RampUp
        assert_eq!(upd(&map, id, 91.0), ThrottleLevel::Hard);
    }

    #[test]
    fn cooldown_timer_reset_on_temp_surge() {
        let map = ThermalThrottleMap::new(300);
        let id = Uuid::now_v7();

        upd(&map, id, 91.0); // → Hard
        // Force into Cooldown by manipulating state directly.
        map.states.insert(id, ThrottleState {
            level: ThrottleLevel::Cooldown,
            temp_c: 80.0,
            hard_since: None,
            cooldown_entered_at: Some(Instant::now()),
            pre_hard_total: 8,
        });

        // Temp re-surges → Cooldown timer resets
        assert_eq!(upd(&map, id, 91.0), ThrottleLevel::Cooldown);
    }
}
