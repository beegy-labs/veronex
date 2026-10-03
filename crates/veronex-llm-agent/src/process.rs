//! In-memory registry of running llama-server children.
//!
//! Keyed by [`uuid::Uuid`] (stable across pid renames). Drop tracking
//! is deliberate — when the agent restarts, every child is lost; the
//! API server reconciles by listing `/process` then re-spawning what's
//! missing.
//!
//! Phase 3 ships the registry data structure; the platform-specific
//! `Command::spawn` lives in `mac/` and `linux/` sub-modules that land
//! when the agent gets its real launch logic.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct RunningProcess {
    pub agent_handle: Uuid,
    pub model_id: String,
    pub blob_sha256: String,
    pub port: u16,
    pub pid: u32,
    pub started_at: DateTime<Utc>,
}

#[derive(Clone, Default)]
pub struct ProcessRegistry {
    inner: Arc<Mutex<HashMap<Uuid, RunningProcess>>>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, p: RunningProcess) {
        if let Ok(mut g) = self.inner.lock() {
            g.insert(p.agent_handle, p);
        }
    }

    pub fn remove(&self, handle: Uuid) -> Option<RunningProcess> {
        self.inner.lock().ok().and_then(|mut g| g.remove(&handle))
    }

    pub fn get(&self, handle: Uuid) -> Option<RunningProcess> {
        self.inner.lock().ok().and_then(|g| g.get(&handle).cloned())
    }

    pub fn list(&self) -> Vec<RunningProcess> {
        self.inner
            .lock()
            .ok()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(handle: Uuid, port: u16) -> RunningProcess {
        RunningProcess {
            agent_handle: handle,
            model_id: "qwen3:q4".into(),
            blob_sha256: "abc".into(),
            port,
            pid: 1234,
            started_at: Utc::now(),
        }
    }

    #[test]
    fn insert_get_remove_round_trip() {
        let reg = ProcessRegistry::new();
        let h = Uuid::now_v7();
        reg.insert(proc(h, 11430));
        assert_eq!(reg.get(h).unwrap().port, 11430);
        assert!(reg.remove(h).is_some());
        assert!(reg.get(h).is_none());
    }

    #[test]
    fn list_returns_all_entries() {
        let reg = ProcessRegistry::new();
        let h1 = Uuid::now_v7();
        let h2 = Uuid::now_v7();
        reg.insert(proc(h1, 11430));
        reg.insert(proc(h2, 11431));
        let mut ports: Vec<u16> = reg.list().iter().map(|p| p.port).collect();
        ports.sort();
        assert_eq!(ports, vec![11430, 11431]);
    }
}
