//! Outbound port — GGUF blob registry (Phase 2 CAS metadata).
//!
//! Companion to [`crate::application::ports::outbound::modelfile_registry`].
//! This port owns the `gguf_blobs` table: the CAS metadata layer that records
//! which sha256 lives in Garage, how many models reference it, and how long
//! it has been orphaned.
//!
//! Note that the CAS object itself lives in Garage (via the `BlobStore`
//! infrastructure adapter); this trait does not move bytes. It just records
//! their identity and reference count. The two layers are kept separate so
//! tests for ref-count semantics don't need a live S3.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::GgufBlob;

/// Filter for [`BlobRegistry::list_orphans`].
#[derive(Debug, Default, Clone)]
pub struct OrphanFilter {
    /// Only return blobs orphaned at least this many days. `None` returns all.
    pub min_orphan_days: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[async_trait]
pub trait BlobRegistry: Send + Sync {
    /// Look up a blob by sha256.
    async fn get(&self, sha256: &str) -> Result<Option<GgufBlob>>;

    /// Insert a new blob row with `ref_count = 1` (the implicit incrementing
    /// model counts as the first reference). Idempotent under retry: if a
    /// row for this sha256 already exists it's an `ON CONFLICT DO UPDATE
    /// SET ref_count = ref_count + 1, orphan_since = NULL` so concurrent
    /// installers converge on a single blob without losing a reference.
    ///
    /// Returns the resulting `ref_count` (>= 1) for diagnostics.
    async fn insert_or_increment(
        &self,
        sha256: &str,
        size_bytes: u64,
        s3_key: &str,
        source_record: &serde_json::Value,
    ) -> Result<i32>;

    /// Increment ref_count for an existing blob (does not insert). Used by
    /// PATCH source-swap when the new sha256 already has a row. Sets
    /// `orphan_since = NULL` so a previously orphaned blob is "rescued".
    /// Returns the new ref_count.
    async fn increment_ref(&self, sha256: &str) -> Result<i32>;

    /// Decrement ref_count atomically. If the new value is 0, set
    /// `orphan_since = now()` in the same statement. Returns the new
    /// ref_count (>= 0). The CAS object is NOT deleted automatically —
    /// admin handles physical GC via the orphan listing.
    async fn decrement_ref(&self, sha256: &str) -> Result<i32>;

    /// List orphaned blobs (ref_count = 0). Admin GC view.
    async fn list_orphans(&self, filter: &OrphanFilter) -> Result<Vec<GgufBlob>>;

    /// Update `last_accessed_at` for LRU-style stats. Cheap UPDATE; called by
    /// the install orchestrator on each ref_count bump and by the inference
    /// router on each successful blob resolution.
    async fn touch_accessed(&self, sha256: &str, at: DateTime<Utc>) -> Result<()>;

    /// Delete a blob row. Caller MUST ensure ref_count == 0; otherwise the
    /// physical CAS object would be deleted while a Modelfile still
    /// references it. Returns `Ok(false)` if the row didn't exist.
    async fn delete(&self, sha256: &str) -> Result<bool>;
}
