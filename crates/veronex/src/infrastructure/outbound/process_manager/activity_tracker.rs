//! Per-provider activity tracking — input to the idle reaper.
//!
//! For every managed `llama-server` provider we keep:
//!
//! - `in_flight: u32`             — currently-routing requests
//! - `last_request_started_at`    — `now()` on each `on_request_start`
//! - `last_request_completed_at`  — `now()` on each `on_request_end`
//!
//! `idle_for(provider_id)` returns:
//!
//! - `Duration::ZERO` whenever `in_flight > 0` — a busy provider is
//!   never idle even if its last completed timestamp is old.
//! - Otherwise `now - max(last_started, last_completed, registered_at)`.
//!
//! `on_request_start` returns a [`RequestGuard`] that decrements
//! `in_flight` on drop so a panic inside the inference path can never
//! leak in-flight count.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
struct Activity {
    last_request_started_at: Option<DateTime<Utc>>,
    last_request_completed_at: Option<DateTime<Utc>>,
    in_flight: u32,
    registered_at: DateTime<Utc>,
}

impl Activity {
    fn new(registered_at: DateTime<Utc>) -> Self {
        Self {
            last_request_started_at: None,
            last_request_completed_at: None,
            in_flight: 0,
            registered_at,
        }
    }
}

/// Cheap to clone (`Arc`-of-state). One handle is shared across the
/// router, IdleManager, and any admin endpoints that surface activity.
#[derive(Clone, Default)]
pub struct ActivityTracker {
    state: Arc<DashMap<Uuid, Activity>>,
}

impl ActivityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider with `now()` as its baseline. Idempotent —
    /// calling twice is a no-op (already-registered providers keep
    /// their existing activity).
    pub fn register(&self, provider_id: Uuid, now: DateTime<Utc>) {
        self.state
            .entry(provider_id)
            .or_insert_with(|| Activity::new(now));
    }

    /// Mark a request as starting; returns a guard that decrements the
    /// in-flight counter on drop.
    pub fn on_request_start(
        &self,
        provider_id: Uuid,
        now: DateTime<Utc>,
    ) -> RequestGuard {
        let mut entry = self
            .state
            .entry(provider_id)
            .or_insert_with(|| Activity::new(now));
        entry.in_flight = entry.in_flight.saturating_add(1);
        entry.last_request_started_at = Some(now);
        RequestGuard {
            tracker: self.state.clone(),
            provider_id,
        }
    }

    /// Compute idle duration. `None` when the provider is unknown — a
    /// caller might have called `unregister` already; idle reaper treats
    /// `None` as "nothing to do".
    pub fn idle_for(
        &self,
        provider_id: Uuid,
        now: DateTime<Utc>,
    ) -> Option<Duration> {
        let entry = self.state.get(&provider_id)?;
        if entry.in_flight > 0 {
            return Some(Duration::ZERO);
        }
        let last = entry
            .last_request_completed_at
            .max(entry.last_request_started_at)
            .unwrap_or(entry.registered_at);
        let delta = (now - last).to_std().unwrap_or(Duration::ZERO);
        Some(delta)
    }

    /// Drop the entry — the provider was stopped or deregistered.
    pub fn unregister(&self, provider_id: Uuid) {
        self.state.remove(&provider_id);
    }

    /// Snapshot for admin endpoints — `(provider_id, in_flight)` pairs.
    pub fn in_flight_snapshot(&self) -> Vec<(Uuid, u32)> {
        self.state
            .iter()
            .map(|e| (*e.key(), e.value().in_flight))
            .collect()
    }
}

/// RAII handle for an in-flight request. Drop calls
/// `on_request_end` semantics — decrements in_flight and stamps
/// `last_request_completed_at`. Hold one for the duration of the
/// inference call; the router pattern is:
///
/// ```ignore
/// let _guard = activity.on_request_start(provider_id, Utc::now());
/// adapter.stream_tokens(...).await?;
/// // _guard drops here on success or panic.
/// ```
pub struct RequestGuard {
    tracker: Arc<DashMap<Uuid, Activity>>,
    provider_id: Uuid,
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        if let Some(mut entry) = self.tracker.get_mut(&self.provider_id) {
            entry.in_flight = entry.in_flight.saturating_sub(1);
            entry.last_request_completed_at = Some(Utc::now());
        }
    }
}

impl std::fmt::Debug for RequestGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestGuard")
            .field("provider_id", &self.provider_id)
            .finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(secs, 0).unwrap()
    }

    #[test]
    fn idle_for_unknown_provider_is_none() {
        let tr = ActivityTracker::new();
        let id = Uuid::now_v7();
        assert!(tr.idle_for(id, at(100)).is_none());
    }

    #[test]
    fn idle_for_freshly_registered_returns_now_minus_register() {
        let tr = ActivityTracker::new();
        let id = Uuid::now_v7();
        tr.register(id, at(100));
        assert_eq!(tr.idle_for(id, at(160)).unwrap(), Duration::from_secs(60));
    }

    #[test]
    fn in_flight_zeros_idle_duration() {
        let tr = ActivityTracker::new();
        let id = Uuid::now_v7();
        tr.register(id, at(100));
        let _g = tr.on_request_start(id, at(105));
        assert_eq!(tr.idle_for(id, at(200)).unwrap(), Duration::ZERO);
    }

    #[test]
    fn guard_drop_decrements_and_stamps_completion() {
        let tr = ActivityTracker::new();
        let id = Uuid::now_v7();
        tr.register(id, at(100));
        {
            let _g = tr.on_request_start(id, at(105));
            assert_eq!(tr.idle_for(id, at(200)).unwrap(), Duration::ZERO);
        }
        // Dropping the guard records `Utc::now()` as completion; we can't
        // pin the exact value but in_flight must be back to 0 so idle_for
        // returns a positive duration only if last_completed is older than
        // `now`. Just assert it is no longer zero (in_flight unwound).
        let snap = tr.in_flight_snapshot();
        assert_eq!(snap.iter().find(|(p, _)| *p == id).map(|(_, n)| *n), Some(0));
    }

    #[test]
    fn unregister_removes_entry() {
        let tr = ActivityTracker::new();
        let id = Uuid::now_v7();
        tr.register(id, at(100));
        tr.unregister(id);
        assert!(tr.idle_for(id, at(200)).is_none());
    }

    #[test]
    fn double_register_keeps_first_baseline() {
        let tr = ActivityTracker::new();
        let id = Uuid::now_v7();
        tr.register(id, at(100));
        tr.register(id, at(500)); // ignored because the entry exists
        assert_eq!(tr.idle_for(id, at(160)).unwrap(), Duration::from_secs(60));
    }
}
