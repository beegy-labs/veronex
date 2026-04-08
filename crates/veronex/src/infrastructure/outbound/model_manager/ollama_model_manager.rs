use std::collections::VecDeque;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::application::ports::outbound::model_manager_port::ModelManagerPort;

// ── Ollama /api/ps response ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PsResponse {
    models: Vec<PsModel>,
}

#[derive(Deserialize)]
struct PsModel {
    name: String,
}

// ── LRU state ──────────────────────────────────────────────────────────────────

struct LruState {
    /// Model names in LRU order: front = most recently used.
    loaded: VecDeque<String>,
    /// Maximum number of models to keep loaded simultaneously.
    max_loaded: usize,
}

impl LruState {
    fn new(max_loaded: usize) -> Self {
        Self {
            loaded: VecDeque::new(),
            max_loaded,
        }
    }

    fn is_loaded(&self, model: &str) -> bool {
        self.loaded.iter().any(|m| m == model)
    }

    fn touch(&mut self, model: &str) {
        if let Some(pos) = self.loaded.iter().position(|m| m == model) {
            self.loaded.remove(pos);
        }
        self.loaded.push_front(model.to_string());
    }

    /// Pop the least-recently-used model, returning its name.
    fn evict_lru(&mut self) -> Option<String> {
        self.loaded.pop_back()
    }

    /// True if adding one more model would exceed the limit.
    fn needs_eviction(&self) -> bool {
        self.loaded.len() >= self.max_loaded
    }

    fn remove(&mut self, model: &str) {
        self.loaded.retain(|m| m != model);
    }
}

// ── Adapter ────────────────────────────────────────────────────────────────────

pub struct OllamaModelManager {
    base_url: String,
    client: reqwest::Client,
    state: Arc<Mutex<LruState>>,
}

impl OllamaModelManager {
    /// Create a new manager for the given Ollama base URL.
    ///
    /// `max_loaded` controls how many models may stay loaded in GPU memory
    /// simultaneously.  Set to `1` for single-GPU deployments (greedy allocation).
    /// `client` should be a shared `reqwest::Client` from `AppState`.
    pub fn new(base_url: impl Into<String>, max_loaded: usize, client: reqwest::Client) -> Self {
        Self {
            base_url: base_url.into(),
            client,
            state: Arc::new(Mutex::new(LruState::new(max_loaded))),
        }
    }

    /// Fetch currently-running models from Ollama `/api/ps` without holding any lock.
    ///
    /// Returns `None` on network/parse failure (caller keeps cached LRU state as-is).
    async fn fetch_running_models(&self) -> Option<Vec<String>> {
        match self
            .client
            .get(format!("{}/api/ps", self.base_url))
            .send()
            .await
        {
            Ok(resp) => match resp.json::<PsResponse>().await {
                Ok(ps) => Some(ps.models.into_iter().map(|m| m.name).collect()),
                Err(e) => {
                    tracing::warn!("model manager: /api/ps parse failed (using cached state): {e}");
                    None
                }
            },
            Err(e) => {
                tracing::warn!("model manager: /api/ps query failed (using cached state): {e}");
                None
            }
        }
    }

    /// Apply a fresh model list to the in-memory LRU state (caller holds the lock).
    fn apply_running_models(state: &mut LruState, running: &[String]) {
        state.loaded.retain(|m| running.contains(m));
        for m in running {
            if !state.is_loaded(m) {
                state.loaded.push_back(m.clone());
            }
        }
    }

    /// Explicitly unload `model_name` by sending a zero-keep_alive generate request.
    async fn unload(&self, model_name: &str) {
        let url = format!("{}/api/generate", self.base_url);
        match self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "model": model_name,
                "keep_alive": 0,
            }))
            .send()
            .await
        {
            Ok(_) => {
                tracing::info!(model = model_name, "model evicted from GPU memory");
            }
            Err(e) => {
                tracing::warn!(model = model_name, "model eviction request failed: {e}");
            }
        }
    }
}

#[async_trait]
impl ModelManagerPort for OllamaModelManager {
    async fn ensure_loaded(&self, model_name: &str) -> Result<()> {
        // Fetch /api/ps WITHOUT holding the lock — avoids holding tokio::Mutex across I/O.
        let running = self.fetch_running_models().await;

        let mut state = self.state.lock().await;

        // Sync in-memory LRU with actual Ollama state (pure, no I/O)
        if let Some(ref models) = running {
            Self::apply_running_models(&mut state, models);
        }

        if state.is_loaded(model_name) {
            // Already loaded — bump to MRU position
            state.touch(model_name);
            return Ok(());
        }

        // Evict LRU models until we have room for the new one
        while state.needs_eviction() {
            if let Some(victim) = state.evict_lru() {
                if victim != model_name {
                    drop(state); // release lock while doing I/O
                    self.unload(&victim).await;
                    state = self.state.lock().await;
                    state.remove(&victim);
                } else {
                    // Victim is the model we want — keep it
                    state.loaded.push_front(victim);
                    return Ok(());
                }
            } else {
                break;
            }
        }

        // Register model as MRU; Ollama will load it on the next inference call
        state.touch(model_name);
        tracing::debug!(model = model_name, "model registered for loading");

        Ok(())
    }

    async fn record_used(&self, model_name: &str) {
        let mut state = self.state.lock().await;
        state.touch(model_name);
    }

    async fn loaded_models(&self) -> Vec<String> {
        let state = self.state.lock().await;
        state.loaded.iter().cloned().collect()
    }
}

// ── Unit tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(max_loaded: usize) -> LruState {
        LruState::new(max_loaded)
    }

    #[test]
    fn lru_touch_promotes_to_front() {
        let mut s = make_state(3);
        s.touch("a");
        s.touch("b");
        s.touch("c");
        // c is MRU, a is LRU
        assert_eq!(s.loaded.front().map(String::as_str), Some("c"));
        // Touch a — it should become MRU
        s.touch("a");
        assert_eq!(s.loaded.front().map(String::as_str), Some("a"));
        assert_eq!(s.loaded.back().map(String::as_str), Some("b"));
    }

    #[test]
    fn lru_evict_removes_back() {
        let mut s = make_state(2);
        s.touch("a");
        s.touch("b");
        let evicted = s.evict_lru();
        assert_eq!(evicted.as_deref(), Some("a")); // a was LRU
        assert_eq!(s.loaded.len(), 1);
        assert_eq!(s.loaded.front().map(String::as_str), Some("b"));
    }

    #[test]
    fn lru_needs_eviction_when_at_capacity() {
        let mut s = make_state(1);
        assert!(!s.needs_eviction());
        s.touch("a");
        assert!(s.needs_eviction());
    }

    #[test]
    fn lru_is_loaded() {
        let mut s = make_state(2);
        s.touch("llama3.2");
        assert!(s.is_loaded("llama3.2"));
        assert!(!s.is_loaded("mistral"));
    }

    #[test]
    fn lru_remove_by_name() {
        let mut s = make_state(3);
        s.touch("a");
        s.touch("b");
        s.touch("c");
        s.remove("b");
        assert!(!s.is_loaded("b"));
        assert!(s.is_loaded("a"));
        assert!(s.is_loaded("c"));
        assert_eq!(s.loaded.len(), 2);
    }
}
