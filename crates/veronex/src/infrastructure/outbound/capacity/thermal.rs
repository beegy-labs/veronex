use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;
use uuid::Uuid;

use crate::application::ports::outbound::thermal_port::ThermalPort;
pub use crate::domain::enums::ThrottleLevel;

struct ThrottleState {
    level:      ThrottleLevel,
    temp_c:     f32,
    hard_since: Option<Instant>, // when Hard throttle was first entered
}

/// Thread-safe map of backend_id → thermal throttle state.
///
/// Updated by the health checker every 30 s.
/// Read by the queue dispatcher on every job dispatch (~0.1 ms, in-memory only).
#[derive(Clone)]
pub struct ThermalThrottleMap {
    states:       Arc<DashMap<Uuid, ThrottleState>>,
    cooldown_secs: u64,
}

impl ThermalThrottleMap {
    pub fn new(cooldown_secs: u64) -> Self {
        Self {
            states: Arc::new(DashMap::new()),
            cooldown_secs,
        }
    }

    /// Update thermal state for a provider and return the new level.
    ///
    /// Should be called by the health checker after reading hw_metrics.
    pub fn update(&self, backend_id: Uuid, temp_c: f32) -> ThrottleLevel {
        let (prev_level, prev_hard_since) = self.states
            .get(&backend_id)
            .map(|s| (Some(s.level.clone()), s.hard_since))
            .unwrap_or((None, None));

        let in_cooldown = prev_hard_since
            .map(|t| t.elapsed().as_secs() < self.cooldown_secs)
            .unwrap_or(false);

        let next = if temp_c >= 92.0 {
            ThrottleLevel::Hard
        } else if temp_c >= 85.0 {
            ThrottleLevel::Soft
        } else if temp_c < 78.0 && !in_cooldown {
            ThrottleLevel::Normal
        } else {
            // Hysteresis zone (78–85°C) or cooldown active: keep previous level.
            prev_level.as_ref().cloned().unwrap_or(ThrottleLevel::Normal)
        };

        match &next {
            ThrottleLevel::Normal => {
                self.states.remove(&backend_id);
            }
            ThrottleLevel::Hard => {
                // Preserve hard_since timestamp if already in Hard state.
                let hard_since = if matches!(prev_level.as_ref(), Some(ThrottleLevel::Hard)) {
                    prev_hard_since
                } else {
                    Some(Instant::now())
                };
                self.states.insert(
                    backend_id,
                    ThrottleState { level: ThrottleLevel::Hard, temp_c, hard_since },
                );
            }
            ThrottleLevel::Soft => {
                self.states.insert(
                    backend_id,
                    ThrottleState { level: ThrottleLevel::Soft, temp_c, hard_since: None },
                );
            }
        }

        next
    }

    /// Current throttle level for a provider (Normal if no state recorded).
    pub fn get(&self, backend_id: Uuid) -> ThrottleLevel {
        self.states
            .get(&backend_id)
            .map(|s| s.level.clone())
            .unwrap_or(ThrottleLevel::Normal)
    }

    /// Last recorded temperature for a provider.
    pub fn temp_c(&self, backend_id: Uuid) -> Option<f32> {
        self.states.get(&backend_id).map(|s| s.temp_c)
    }
}

impl ThermalPort for ThermalThrottleMap {
    fn get_level(&self, provider_id: Uuid) -> ThrottleLevel {
        self.get(provider_id)
    }
}
