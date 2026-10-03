//! Outbound port — Modelfile registry (Phase 2).
//!
//! Owns CRUD for `veronex_models`. The repo intentionally does **not** know
//! anything about source adapters, blob storage, or the install state
//! machine — those concerns live in the install orchestrator. This port is
//! pure database access.
//!
//! Read paths fan out to the inference router (lookup by model_id or
//! family-default) and the admin UI (list with filters, single-row detail).
//! Write paths are limited to the orchestrator (status transitions) and the
//! admin handlers (registration, PATCH, DELETE, promote).

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::{ErrorKind, InstallStatus, VeronexModel};

/// Filter for [`ModelfileRegistry::list`].
#[derive(Debug, Default, Clone)]
pub struct ListFilter {
    pub family: Option<String>,
    pub install_status: Option<InstallStatus>,
    pub is_default: Option<bool>,
    /// Limit and offset for pagination. `None` means "no limit".
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Subset of a [`VeronexModel`] that can be patched by `PATCH /v1/admin/models/:id`.
///
/// `family` and `quantization` are deliberately absent — changing those is a
/// new model identity (the handler returns 405). `source_spec` triggers a
/// re-install; the other fields take effect on the next ProcessManager start
/// without re-downloading the blob.
#[derive(Debug, Clone, Default)]
pub struct ModelfilePatch {
    pub display_name: Option<Option<String>>,
    pub source_spec: Option<serde_json::Value>,
    pub runtime: Option<serde_json::Value>,
    pub defaults: Option<serde_json::Value>,
    pub chat_template: Option<serde_json::Value>,
    pub stop_tokens: Option<Option<serde_json::Value>>,
    pub system_prompt: Option<Option<String>>,
    pub tags: Option<Option<Vec<String>>>,
}

#[async_trait]
pub trait ModelfileRegistry: Send + Sync {
    /// Insert a new Modelfile row.
    ///
    /// Caller is responsible for ensuring the referenced `blob_sha256` exists
    /// (the FK fires otherwise). For initial registrations the orchestrator
    /// inserts the Modelfile *after* the blob row is in place, both inside
    /// one transaction.
    async fn create(&self, model: &VeronexModel) -> Result<()>;

    /// Fetch a single Modelfile by `model_id` (`"{family}:{quantization}"`).
    /// Returns `None` if absent — the inference router maps this to 404.
    async fn get(&self, model_id: &str) -> Result<Option<VeronexModel>>;

    /// Fetch the default Modelfile for a family — used when the inference
    /// request specifies just `family` without `:quantization`. Driven by
    /// the partial unique index `uq_veronex_models_family_default`.
    async fn get_default(&self, family: &str) -> Result<Option<VeronexModel>>;

    /// List Modelfiles matching the filter.
    async fn list(&self, filter: &ListFilter) -> Result<Vec<VeronexModel>>;

    /// Apply a PATCH. Fields set to `Some(_)` overwrite; `None` keeps the
    /// existing value. The double-`Option` shape (`Option<Option<T>>`)
    /// distinguishes "leave unchanged" from "set to NULL".
    ///
    /// Returns `Ok(false)` if no row was found (admin handler maps to 404).
    async fn patch(&self, model_id: &str, patch: &ModelfilePatch) -> Result<bool>;

    /// Update only the install state. Atomic; safe to call from the
    /// orchestrator's FSM without coordinating with PATCH writers.
    async fn update_install_status(
        &self,
        model_id: &str,
        status: InstallStatus,
        error_kind: Option<ErrorKind>,
        error_message: Option<&str>,
        last_attempt_at: Option<DateTime<Utc>>,
    ) -> Result<()>;

    /// Atomically swap the referenced blob. Used after a PATCH that changed
    /// `source_spec` and the new install completed successfully.
    ///
    /// Implementation MUST run as a single transaction:
    /// 1. UPDATE veronex_models SET blob_sha256 = new_sha256, install_status = 'ready'
    /// 2. INSERT/UPDATE gguf_blobs ref_count for new_sha256 (+1)
    /// 3. UPDATE gguf_blobs ref_count for old_sha256 (-1, set orphan_since if 0)
    async fn swap_blob(
        &self,
        model_id: &str,
        old_sha256: &str,
        new_sha256: &str,
    ) -> Result<()>;

    /// Promote a Modelfile to be the family default. The implementation MUST
    /// clear `is_default` on any other row in the same family in the same
    /// transaction (the partial unique index would otherwise reject).
    async fn promote_default(&self, model_id: &str) -> Result<()>;

    /// Delete a Modelfile and decrement its blob's ref_count atomically.
    /// Install attempt history rows are intentionally retained.
    /// Returns `Ok(false)` if no row was found.
    async fn delete(&self, model_id: &str) -> Result<bool>;
}
