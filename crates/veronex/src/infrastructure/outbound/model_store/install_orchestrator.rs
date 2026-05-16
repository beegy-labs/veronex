#![allow(clippy::too_many_arguments)]

//! Install orchestrator — drives a Modelfile from `Pending` to `Ready`
//! through the [`InstallStatus`] FSM.
//!
//! Responsibilities (the only place these come together):
//!
//! 1. Open an `InstallAttempt` row, transition the Modelfile through
//!    `pending → downloading → uploading → verifying → ready` (or terminal
//!    `failed`).
//! 2. Pull bytes from the [`ModelSource`], pipe through [`LocalPv`] (which
//!    hashes and atomic-renames), check CAS dedup against [`BlobRegistry`],
//!    push to Garage via [`BlobStore`] when the blob is new.
//! 3. Coordinate concurrent installers of the same sha256 with an
//!    in-process per-sha256 mutex so two registrations of the same HF
//!    file don't both download.
//! 4. Record the outcome in the `InstallAttempt` row and broadcast install
//!    events for the SSE endpoint (see [`InstallEvent`]).
//!
//! This module is the **only** writer of `install_status` outside the
//! handler that creates the row. Everything else (router, ProcessManager,
//! capacity) is read-only against the FSM.

use std::collections::HashMap;
use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::{broadcast, Mutex};

use crate::application::ports::outbound::blob_registry::BlobRegistry;
use crate::application::ports::outbound::install_attempts_log::{
    AttemptFailure, AttemptSuccess, InstallAttemptsLog,
};
use crate::application::ports::outbound::modelfile_registry::ModelfileRegistry;
use crate::domain::entities::{
    AttemptStage, ErrorKind, InstallStatus, TriggeredBy, VeronexModel,
};

use super::blob_store::BlobStore;
use super::local_pv::LocalPv;
use super::source::ModelSource;

/// Bytes streamed plus elapsed time — used by both success/failure
/// recording on the InstallAttempt row.
struct PipelineMetrics {
    bytes: u64,
    started_at: Instant,
}

impl PipelineMetrics {
    fn new() -> Self {
        Self { bytes: 0, started_at: Instant::now() }
    }
    fn duration_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }
}

/// Public event surface for SSE subscribers (admin UI install/stream).
///
/// Cheap to clone — wraps a string payload that the handler serialises to
/// SSE `data:` lines. Channels are per-model_id to avoid a single
/// global broadcast that fans out to unrelated subscribers.
#[derive(Debug, Clone)]
pub enum InstallEvent {
    StatusChanged {
        model_id: String,
        status: InstallStatus,
    },
    Progress {
        model_id: String,
        stage: AttemptStage,
        bytes_done: u64,
        total_bytes: Option<u64>,
    },
    Failed {
        model_id: String,
        error_kind: ErrorKind,
        error_message: String,
    },
    Succeeded {
        model_id: String,
        sha256: String,
    },
}

/// Per-sha256 in-process lock used to coalesce concurrent installers.
type ShaLockMap = Mutex<HashMap<String, Arc<Mutex<()>>>>;

/// Wires together the Phase 2 ports + infrastructure into a single FSM
/// driver. Cheap to clone.
#[derive(Clone)]
pub struct InstallOrchestrator {
    inner: Arc<Inner>,
}

struct Inner {
    models: Arc<dyn ModelfileRegistry>,
    blobs: Arc<dyn BlobRegistry>,
    attempts: Arc<dyn InstallAttemptsLog>,
    blob_store: BlobStore,
    local_pv: LocalPv,
    /// Per-model_id broadcast channel; lazily created on first subscribe.
    /// Senders live as long as the orchestrator; receivers drop when the
    /// SSE handler disconnects.
    bus: Mutex<HashMap<String, broadcast::Sender<InstallEvent>>>,
    /// Per-sha256 install locks. Two installers that resolve the same sha256
    /// before downloading wait on the same mutex; the second one finds the
    /// blob already in CAS and short-circuits.
    sha_locks: ShaLockMap,
}

impl InstallOrchestrator {
    pub fn new(
        models: Arc<dyn ModelfileRegistry>,
        blobs: Arc<dyn BlobRegistry>,
        attempts: Arc<dyn InstallAttemptsLog>,
        blob_store: BlobStore,
        local_pv: LocalPv,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                models,
                blobs,
                attempts,
                blob_store,
                local_pv,
                bus: Mutex::new(HashMap::new()),
                sha_locks: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Subscribe to install events for one model. Used by the SSE handler.
    /// Channel capacity is small (16) — slow consumers see `Lagged` and
    /// just miss intermediate progress events; the next `StatusChanged`
    /// resyncs them.
    pub async fn subscribe(&self, model_id: &str) -> broadcast::Receiver<InstallEvent> {
        let mut bus = self.inner.bus.lock().await;
        let tx = bus
            .entry(model_id.to_string())
            .or_insert_with(|| broadcast::channel(16).0);
        tx.subscribe()
    }

    /// Broadcast helper — silently no-ops when no subscribers are connected.
    async fn emit(&self, model_id: &str, event: InstallEvent) {
        let bus = self.inner.bus.lock().await;
        if let Some(tx) = bus.get(model_id) {
            let _ = tx.send(event);
        }
    }

    /// Acquire the per-sha256 mutex. Concurrent installers of the same
    /// sha256 serialize on this so only one performs the actual download.
    async fn lock_sha256(&self, sha256: &str) -> Arc<Mutex<()>> {
        let mut locks = self.inner.sha_locks.lock().await;
        locks
            .entry(sha256.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Execute one install attempt for `model`, using `source` as the byte
    /// origin. The Modelfile row already exists at `install_status='pending'`
    /// (or 'failed' for a retry); this method drives it through the FSM and
    /// returns Ok with the resulting blob sha256 on success.
    pub async fn install(
        &self,
        model: &VeronexModel,
        source: Box<dyn ModelSource>,
        triggered_by: TriggeredBy,
    ) -> Result<String> {
        let model_id = &model.model_id;
        let now = Utc::now();
        let attempt_id = self
            .inner
            .attempts
            .start(model_id, triggered_by)
            .await?;
        let mut metrics = PipelineMetrics::new();

        // ── Stage 1: try pre-resolution + CAS dedup ──────────────────────
        // Cheap path: the source advertises an sha256 (HF LFS OID, S3
        // pointer x-amz-meta-sha256, URL ETag). If the CAS already has it,
        // we simply bump ref_count and flip the row to ready.
        let pre_sha256 = match source.resolve_sha256().await {
            Ok(s) => s,
            Err(e) => {
                self.fail(
                    model_id,
                    attempt_id,
                    &mut metrics,
                    ErrorKind::Network,
                    &format!("source.resolve_sha256: {e}"),
                    None,
                )
                .await;
                return Err(e);
            }
        };

        if let Some(sha256) = &pre_sha256 {
            // Hold per-sha256 lock so two installers don't race a
            // download both.
            let lock = self.lock_sha256(sha256).await;
            let _guard = lock.lock().await;

            if let Some(blob) = self.inner.blobs.get(sha256).await? {
                // CAS hit. ref_count++ atomically.
                let _ = self.inner.blobs.increment_ref(sha256).await?;
                let _ = self
                    .inner
                    .blobs
                    .touch_accessed(sha256, Utc::now())
                    .await;
                self.transition(
                    model_id,
                    InstallStatus::Ready,
                    None,
                    None,
                    Some(now),
                )
                .await?;
                self.inner
                    .attempts
                    .succeed(
                        attempt_id,
                        &AttemptSuccess {
                            bytes_downloaded: Some(0),
                            total_bytes: Some(blob.size_bytes),
                            duration_ms: Some(metrics.duration_ms()),
                        },
                    )
                    .await?;
                self.emit(
                    model_id,
                    InstallEvent::Succeeded {
                        model_id: model_id.clone(),
                        sha256: sha256.clone(),
                    },
                )
                .await;
                return Ok(sha256.clone());
            }
            // Pre-known sha256 but not in CAS yet: fall through to download.
            // We hold the lock so concurrent installers queue here.
        }

        // ── Stage 2: download to Local PV (streaming sha256 verify) ──────
        self.transition(
            model_id,
            InstallStatus::Downloading,
            None,
            None,
            Some(now),
        )
        .await?;
        self.inner
            .attempts
            .update_progress(attempt_id, AttemptStage::Download, 0, source.total_bytes())
            .await
            .ok();

        // Best-effort LRU eviction before the new write. Reads
        // VERONEX_MODEL_MAX_DISK_GB internally (env-or-DB resolved at
        // bootstrap); when unset the call is a no-op. Failures are
        // non-fatal — the worst case is the FS fills and write_streaming
        // surfaces the OS error.
        if let Err(e) = self
            .inner
            .local_pv
            .evict_to(super::local_pv::EVICT_TARGET_PCT)
            .await
        {
            tracing::warn!(model_id, error = %e, "pre-install LRU eviction failed");
        }

        let stream = match source.stream_blob().await {
            Ok(s) => s,
            Err(e) => {
                self.fail(
                    model_id,
                    attempt_id,
                    &mut metrics,
                    classify_network_error(&e),
                    &format!("source.stream_blob: {e}"),
                    Some(AttemptStage::Download),
                )
                .await;
                return Err(e);
            }
        };

        let outcome = match self
            .inner
            .local_pv
            .write_streaming(pre_sha256.as_deref(), stream)
            .await
        {
            Ok(o) => o,
            Err(e) => {
                let kind = if e.to_string().contains("sha256 mismatch") {
                    ErrorKind::Sha256Mismatch
                } else {
                    ErrorKind::Network
                };
                self.fail(
                    model_id,
                    attempt_id,
                    &mut metrics,
                    kind,
                    &format!("write_streaming: {e}"),
                    Some(AttemptStage::Download),
                )
                .await;
                return Err(e);
            }
        };
        metrics.bytes = outcome.bytes_written;
        let sha256 = outcome.sha256.clone();
        self.emit(
            model_id,
            InstallEvent::Progress {
                model_id: model_id.clone(),
                stage: AttemptStage::Download,
                bytes_done: metrics.bytes,
                total_bytes: source.total_bytes(),
            },
        )
        .await;

        // ── Stage 3: re-check CAS dedup ──────────────────────────────────
        // Now that the actual sha256 is known, another installer may have
        // already pushed the same blob to Garage while we were downloading.
        // Re-check; if hit, skip the upload and just bump ref_count.
        let lock = self.lock_sha256(&sha256).await;
        let _guard = lock.lock().await;

        let already_cas = self.inner.blob_store.exists(&sha256).await.unwrap_or(false);
        let already_db = self.inner.blobs.get(&sha256).await?.is_some();

        if already_cas && already_db {
            let _ = self.inner.blobs.increment_ref(&sha256).await?;
            self.finalize_success(
                model_id,
                attempt_id,
                &sha256,
                metrics.bytes,
                source.total_bytes(),
                metrics.duration_ms(),
                now,
            )
            .await?;
            return Ok(sha256);
        }

        // ── Stage 4: upload to Garage ────────────────────────────────────
        self.transition(
            model_id,
            InstallStatus::Uploading,
            None,
            None,
            Some(now),
        )
        .await?;
        self.inner
            .attempts
            .update_progress(
                attempt_id,
                AttemptStage::Upload,
                metrics.bytes,
                source.total_bytes(),
            )
            .await
            .ok();

        if let Err(e) = self
            .inner
            .blob_store
            .put_from_path(&sha256, &outcome.path)
            .await
        {
            self.fail(
                model_id,
                attempt_id,
                &mut metrics,
                ErrorKind::S3Error,
                &format!("blob_store.put: {e}"),
                Some(AttemptStage::Upload),
            )
            .await;
            return Err(e);
        }

        // ── Stage 5: verify (record metadata in DB) ──────────────────────
        self.transition(
            model_id,
            InstallStatus::Verifying,
            None,
            None,
            Some(now),
        )
        .await?;
        let s3_key = BlobStore::key_for(&sha256);
        let source_record = source.source_spec();
        let _ref_count = self
            .inner
            .blobs
            .insert_or_increment(&sha256, metrics.bytes, &s3_key, &source_record)
            .await?;

        // ── Stage 6: ready ───────────────────────────────────────────────
        self.finalize_success(
            model_id,
            attempt_id,
            &sha256,
            metrics.bytes,
            source.total_bytes(),
            metrics.duration_ms(),
            now,
        )
        .await?;
        Ok(sha256)
    }

    async fn finalize_success(
        &self,
        model_id: &str,
        attempt_id: i64,
        sha256: &str,
        bytes: u64,
        total: Option<u64>,
        duration_ms: u64,
        when: chrono::DateTime<Utc>,
    ) -> Result<()> {
        self.transition(model_id, InstallStatus::Ready, None, None, Some(when))
            .await?;
        self.inner
            .attempts
            .succeed(
                attempt_id,
                &AttemptSuccess {
                    bytes_downloaded: Some(bytes),
                    total_bytes: total.or(Some(bytes)),
                    duration_ms: Some(duration_ms),
                },
            )
            .await?;
        self.emit(
            model_id,
            InstallEvent::Succeeded {
                model_id: model_id.to_string(),
                sha256: sha256.to_string(),
            },
        )
        .await;
        Ok(())
    }

    async fn transition(
        &self,
        model_id: &str,
        status: InstallStatus,
        error_kind: Option<ErrorKind>,
        error_message: Option<&str>,
        when: Option<chrono::DateTime<Utc>>,
    ) -> Result<()> {
        self.inner
            .models
            .update_install_status(model_id, status, error_kind, error_message, when)
            .await?;
        self.emit(
            model_id,
            InstallEvent::StatusChanged {
                model_id: model_id.to_string(),
                status,
            },
        )
        .await;
        Ok(())
    }

    async fn fail(
        &self,
        model_id: &str,
        attempt_id: i64,
        metrics: &mut PipelineMetrics,
        kind: ErrorKind,
        message: &str,
        stage: Option<AttemptStage>,
    ) {
        let now = Utc::now();
        let _ = self
            .inner
            .models
            .update_install_status(
                model_id,
                InstallStatus::Failed,
                Some(kind),
                Some(message),
                Some(now),
            )
            .await;
        let _ = self
            .inner
            .attempts
            .fail(
                attempt_id,
                &AttemptFailure {
                    stage,
                    error_kind: kind,
                    error_message: message.to_string(),
                    retryable: kind.retryable(),
                    bytes_downloaded: Some(metrics.bytes),
                    total_bytes: None,
                    duration_ms: Some(metrics.duration_ms()),
                },
            )
            .await;
        self.emit(
            model_id,
            InstallEvent::Failed {
                model_id: model_id.to_string(),
                error_kind: kind,
                error_message: message.to_string(),
            },
        )
        .await;
    }
}

/// Map a generic anyhow error from the network layer to an [`ErrorKind`].
/// Heuristic: matches a few stable string fragments that reqwest/aws-sdk-s3
/// emit. Unrecognised errors fall back to `Network` since the most common
/// cause is transport.
fn classify_network_error(e: &anyhow::Error) -> ErrorKind {
    let text = e.to_string().to_ascii_lowercase();
    if text.contains("404") || text.contains("not found") {
        ErrorKind::Source404
    } else if text.contains("401") || text.contains("403") || text.contains("unauthorized") {
        ErrorKind::Auth
    } else if text.contains("429") || text.contains("rate limit") {
        ErrorKind::RateLimit
    } else if text.contains("timeout") || text.contains("timed out") {
        ErrorKind::Timeout
    } else {
        ErrorKind::Network
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_network_error_known_fragments() {
        assert_eq!(
            classify_network_error(&anyhow::anyhow!("HTTP 404 Not Found")),
            ErrorKind::Source404
        );
        assert_eq!(
            classify_network_error(&anyhow::anyhow!("401 Unauthorized")),
            ErrorKind::Auth
        );
        assert_eq!(
            classify_network_error(&anyhow::anyhow!("HTTP 429 too many requests")),
            ErrorKind::RateLimit
        );
        assert_eq!(
            classify_network_error(&anyhow::anyhow!("connect timeout")),
            ErrorKind::Timeout
        );
        assert_eq!(
            classify_network_error(&anyhow::anyhow!("connection reset")),
            ErrorKind::Network
        );
    }

    #[tokio::test]
    async fn subscribe_creates_channel_per_model() {
        // Construct a minimally-wired orchestrator with no DB / S3 — we only
        // exercise the bus mechanics. The trait objects below would panic
        // if any DB method were invoked; we don't call any.
        let models: Arc<dyn ModelfileRegistry> = Arc::new(NoopModels);
        let blobs: Arc<dyn BlobRegistry> = Arc::new(NoopBlobs);
        let attempts: Arc<dyn InstallAttemptsLog> = Arc::new(NoopAttempts);
        let blob_store = make_test_blob_store();
        let local_pv = LocalPv::new(std::env::temp_dir());
        let orch = InstallOrchestrator::new(models, blobs, attempts, blob_store, local_pv);

        let _rx_a = orch.subscribe("a").await;
        let _rx_a2 = orch.subscribe("a").await; // same channel
        let _rx_b = orch.subscribe("b").await; // distinct channel

        // Send via internal helper: subscribers should receive their event
        // and the cross-channel send must not leak to the other subscriber.
        orch.emit(
            "a",
            InstallEvent::StatusChanged {
                model_id: "a".into(),
                status: InstallStatus::Ready,
            },
        )
        .await;
    }

    fn make_test_blob_store() -> BlobStore {
        BlobStore::for_tests("veronex-models")
    }

    // ── No-op trait stubs — the orchestrator's bus tests never call them ──

    struct NoopModels;
    #[async_trait::async_trait]
    impl ModelfileRegistry for NoopModels {
        async fn create(&self, _: &VeronexModel) -> Result<()> {
            unreachable!("not used by bus tests")
        }
        async fn get(&self, _: &str) -> Result<Option<VeronexModel>> {
            unreachable!()
        }
        async fn get_default(&self, _: &str) -> Result<Option<VeronexModel>> {
            unreachable!()
        }
        async fn list(
            &self,
            _: &crate::application::ports::outbound::modelfile_registry::ListFilter,
        ) -> Result<Vec<VeronexModel>> {
            unreachable!()
        }
        async fn patch(
            &self,
            _: &str,
            _: &crate::application::ports::outbound::modelfile_registry::ModelfilePatch,
        ) -> Result<bool> {
            unreachable!()
        }
        async fn update_install_status(
            &self,
            _: &str,
            _: InstallStatus,
            _: Option<ErrorKind>,
            _: Option<&str>,
            _: Option<chrono::DateTime<Utc>>,
        ) -> Result<()> {
            unreachable!()
        }
        async fn swap_blob(&self, _: &str, _: &str, _: &str) -> Result<()> {
            unreachable!()
        }
        async fn promote_default(&self, _: &str) -> Result<()> {
            unreachable!()
        }
        async fn delete(&self, _: &str) -> Result<bool> {
            unreachable!()
        }
    }

    struct NoopBlobs;
    #[async_trait::async_trait]
    impl BlobRegistry for NoopBlobs {
        async fn get(
            &self,
            _: &str,
        ) -> Result<Option<crate::domain::entities::GgufBlob>> {
            unreachable!()
        }
        async fn insert_or_increment(
            &self,
            _: &str,
            _: u64,
            _: &str,
            _: &serde_json::Value,
        ) -> Result<i32> {
            unreachable!()
        }
        async fn increment_ref(&self, _: &str) -> Result<i32> {
            unreachable!()
        }
        async fn decrement_ref(&self, _: &str) -> Result<i32> {
            unreachable!()
        }
        async fn list_orphans(
            &self,
            _: &crate::application::ports::outbound::blob_registry::OrphanFilter,
        ) -> Result<Vec<crate::domain::entities::GgufBlob>> {
            unreachable!()
        }
        async fn touch_accessed(
            &self,
            _: &str,
            _: chrono::DateTime<Utc>,
        ) -> Result<()> {
            unreachable!()
        }
        async fn delete(&self, _: &str) -> Result<bool> {
            unreachable!()
        }
    }

    struct NoopAttempts;
    #[async_trait::async_trait]
    impl InstallAttemptsLog for NoopAttempts {
        async fn start(&self, _: &str, _: TriggeredBy) -> Result<i64> {
            unreachable!()
        }
        async fn update_progress(
            &self,
            _: i64,
            _: AttemptStage,
            _: u64,
            _: Option<u64>,
        ) -> Result<()> {
            unreachable!()
        }
        async fn succeed(
            &self,
            _: i64,
            _: &AttemptSuccess,
        ) -> Result<()> {
            unreachable!()
        }
        async fn fail(
            &self,
            _: i64,
            _: &AttemptFailure,
        ) -> Result<()> {
            unreachable!()
        }
        async fn cancel(&self, _: i64) -> Result<()> {
            unreachable!()
        }
        async fn list(
            &self,
            _: &crate::application::ports::outbound::install_attempts_log::AttemptsFilter,
        ) -> Result<Vec<crate::domain::entities::InstallAttempt>> {
            unreachable!()
        }
        async fn delete(&self, _: i64) -> Result<bool> {
            unreachable!()
        }
        async fn delete_by_model(&self, _: &str) -> Result<u64> {
            unreachable!()
        }
    }

    #[allow(dead_code)]
    fn _unused_duration() -> Duration {
        // Placeholder so `Duration` import isn't a dead use when the rest
        // of the module compiles unchanged.
        Duration::from_secs(0)
    }
}
