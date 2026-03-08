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

struct ThrottleState {
    level:      ThrottleLevel,
    temp_c:     f32,
    hard_since: Option<Instant>, // when Hard throttle was first entered
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
    /// Should be called by the health checker after reading hw_metrics.
    pub fn update(&self, provider_id: Uuid, temp_c: f32) -> ThrottleLevel {
        let (prev_level, prev_hard_since) = self.states
            .get(&provider_id)
            .map(|s| (Some(s.level), s.hard_since))
            .unwrap_or((None, None));

        let in_cooldown = prev_hard_since
            .map(|t| t.elapsed().as_secs() < self.cooldown_secs)
            .unwrap_or(false);

        let th = self.get_thresholds(provider_id);

        let next = if temp_c >= th.hard_at {
            ThrottleLevel::Hard
        } else if temp_c >= th.soft_at {
            ThrottleLevel::Soft
        } else if temp_c < th.normal_below && !in_cooldown {
            ThrottleLevel::Normal
        } else {
            // Hysteresis zone or cooldown active: keep previous level.
            prev_level.as_ref().cloned().unwrap_or(ThrottleLevel::Normal)
        };

        match &next {
            ThrottleLevel::Normal => {
                self.states.remove(&provider_id);
            }
            ThrottleLevel::Hard => {
                // Preserve hard_since timestamp if already in Hard state.
                let hard_since = if matches!(prev_level.as_ref(), Some(ThrottleLevel::Hard)) {
                    prev_hard_since
                } else {
                    Some(Instant::now())
                };
                self.states.insert(
                    provider_id,
                    ThrottleState { level: ThrottleLevel::Hard, temp_c, hard_since },
                );
            }
            ThrottleLevel::Soft => {
                self.states.insert(
                    provider_id,
                    ThrottleState { level: ThrottleLevel::Soft, temp_c, hard_since: None },
                );
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
}

impl ThermalPort for ThermalThrottleMap {
    fn get_level(&self, provider_id: Uuid) -> ThrottleLevel {
        self.get(provider_id)
    }
}
