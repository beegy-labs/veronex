//! Modelfile registry domain entities (Phase 2 — AI BaaS).
//!
//! Three concepts:
//!
//! - [`GgufBlob`] — content-addressable GGUF binary keyed by sha256. Multiple
//!   `VeronexModel` rows may reference the same blob (dedup). Lifetime tracked
//!   via `ref_count`; `orphan_since` marks the moment it became unreferenced
//!   (admin manually deletes — no automatic GC).
//!
//! - [`VeronexModel`] — Modelfile spec: a lightweight metadata row pointing at
//!   a `GgufBlob` by sha256, plus runtime/defaults/chat_template etc. The
//!   `model_id` PK has the form `"{family}:{quantization}"`.
//!
//! - [`InstallAttempt`] — append-only history row for each install attempt.
//!   Survives model deletion (no FK). Admin manually deletes via API.
//!
//! All three are read-only domain types; persistence layer (SQLx repo)
//! constructs them from query results and assigns them via plain field access.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Modelfile (veronex_models row) ──────────────────────────────────────────

/// A registered model — Modelfile + blob_sha256 reference.
///
/// `model_id` is `"{family}:{quantization}"` (e.g. `"qwen3-coder:q4_K_M"`).
/// Inference requests look up by `model_id` (precise) or by `family` (resolves
/// to the row with `is_default = true`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VeronexModel {
    pub model_id: String,
    pub family: String,
    pub quantization: String,
    pub blob_sha256: String,
    pub display_name: Option<String>,
    /// Original source spec (for re-install on PATCH). One of:
    /// `{type:"hf", repo, filename, revision}`, `{type:"upload",...}`,
    /// `{type:"url",...}`, `{type:"s3_pointer", s3_key}`.
    pub source_spec: serde_json::Value,
    /// llama-server CLI flags: `{n_ctx, n_gpu_layers, n_batch, flash_attn,
    /// split_mode, cache_type_k, cache_type_v, ...}`.
    pub runtime: serde_json::Value,
    /// Default sampling params: `{temperature, top_p, top_k, repeat_penalty,
    /// min_p, mirostat}`. Request-time params override.
    pub defaults: serde_json::Value,
    /// `{type: "preset"|"jinja_inline"|"jinja_file", value}`.
    pub chat_template: serde_json::Value,
    pub stop_tokens: Option<serde_json::Value>,
    pub system_prompt: Option<String>,
    pub is_default: bool,
    pub install_status: InstallStatus,
    pub last_error_kind: Option<String>,
    pub last_error_message: Option<String>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub tags: Option<Vec<String>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Finite state for an install pipeline.
///
/// `pending` → start. `downloading` → fetching from source. `uploading` →
/// pushing to Garage (only when source is `upload`). `verifying` → sha256
/// checksum step. `ready` → terminal success. `failed` → terminal failure
/// (admin must `POST /install/retry` to start a new attempt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    Pending,
    Downloading,
    Uploading,
    Verifying,
    Ready,
    Failed,
}

impl InstallStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Downloading => "downloading",
            Self::Uploading => "uploading",
            Self::Verifying => "verifying",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }

    /// True for the in-progress states (router returns 503 + Retry-After).
    pub fn is_in_progress(&self) -> bool {
        matches!(self, Self::Pending | Self::Downloading | Self::Uploading | Self::Verifying)
    }

    /// True only for `Ready` — the only state that admits inference dispatch.
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

impl std::str::FromStr for InstallStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "downloading" => Ok(Self::Downloading),
            "uploading" => Ok(Self::Uploading),
            "verifying" => Ok(Self::Verifying),
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            other => Err(format!("invalid install_status: {other}")),
        }
    }
}

// ── GGUF blob (gguf_blobs row) ──────────────────────────────────────────────

/// Content-addressable GGUF binary. The `sha256` is both the PK and the file
/// identity in Garage (`s3_key = "gguf-blobs/{sha256}.gguf"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GgufBlob {
    pub sha256: String,
    pub size_bytes: u64,
    pub s3_key: String,
    /// History of source records that produced/registered this blob (audit
    /// trail). Each entry is `{type, ..., fetched_at}`.
    pub source_history: serde_json::Value,
    pub ref_count: i32,
    /// `Some(t)` iff `ref_count == 0` and admin has not yet deleted. Reset to
    /// `None` if a new model registration increments `ref_count` back above 0.
    pub orphan_since: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_accessed_at: Option<DateTime<Utc>>,
}

impl GgufBlob {
    /// Days the blob has been orphaned, or `None` if currently referenced.
    /// Admin UI uses this to decide GC candidates.
    pub fn orphan_days(&self) -> Option<i64> {
        self.orphan_since.map(|since| (Utc::now() - since).num_days())
    }
}

// ── Install attempt (veronex_model_install_attempts row) ────────────────────

/// Append-only history row for a single install attempt. Survives model
/// deletion — `model_id` is a free-form text reference, not a foreign key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallAttempt {
    pub id: i64,
    pub model_id: String,
    pub attempt_no: i32,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: AttemptStatus,
    pub stage: Option<AttemptStage>,
    pub error_kind: Option<ErrorKind>,
    pub error_message: Option<String>,
    pub bytes_downloaded: Option<u64>,
    pub total_bytes: Option<u64>,
    pub duration_ms: Option<u64>,
    /// Hint shown in admin UI — true means the error class is generally
    /// network/transient. Veronex never auto-retries; admin clicks `[Retry]`.
    pub retryable: Option<bool>,
    pub triggered_by: Option<TriggeredBy>,
}

/// Attempt outcome. `in_progress` is the only non-terminal state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    InProgress,
    Succeeded,
    Failed,
    Cancelled,
}

impl AttemptStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Pipeline stage where an attempt currently is (or where it failed). `None`
/// for attempts that completed without progressing past resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStage {
    Download,
    Upload,
    Verify,
}

impl AttemptStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Download => "download",
            Self::Upload => "upload",
            Self::Verify => "verify",
        }
    }
}

/// Classification of install failures. Maps to operator-facing
/// retryable-or-not signaling in the admin UI. There is no automatic retry —
/// the `retryable` boolean is purely a hint for human decision-making.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Transient: timeout, connection reset, DNS fail.
    Network,
    /// HF 429 or similar.
    RateLimit,
    /// Garage transient failure.
    S3Error,
    /// Computed sha256 ≠ expected (HF LFS OID mismatch or upload corruption).
    Sha256Mismatch,
    /// Download stalled (no progress for N seconds).
    Timeout,
    /// Recovery-marker for installs in progress when the app crashed/restarted.
    StartupRecovery,
    /// Catch-all (fallback for unclassified panics).
    Unknown,
    /// HF/URL 404 — source spec needs editing.
    Source404,
    /// Auth failure (HF token missing/expired, S3 creds wrong).
    Auth,
    /// PV/Garage capacity exceeded.
    DiskFull,
    /// File parses but is not a valid GGUF.
    InvalidGguf,
    /// llama.cpp does not support this quantization.
    QuantizationUnsupported,
}

impl ErrorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::RateLimit => "rate_limit",
            Self::S3Error => "s3_error",
            Self::Sha256Mismatch => "sha256_mismatch",
            Self::Timeout => "timeout",
            Self::StartupRecovery => "startup_recovery",
            Self::Unknown => "unknown",
            Self::Source404 => "source_404",
            Self::Auth => "auth",
            Self::DiskFull => "disk_full",
            Self::InvalidGguf => "invalid_gguf",
            Self::QuantizationUnsupported => "quantization_unsupported",
        }
    }

    /// Operator-facing retry hint. Network/transient errors are flagged true;
    /// errors that require source/credential edits are flagged false.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Network
                | Self::RateLimit
                | Self::S3Error
                | Self::Sha256Mismatch
                | Self::Timeout
                | Self::StartupRecovery
                | Self::Unknown
        )
    }
}

/// What initiated this attempt. Used in dashboards to distinguish initial
/// register-time installs from admin retries vs auto-installs that follow a
/// PATCH that changed the source spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggeredBy {
    Register,
    AdminRetry,
    PatchSource,
}

impl TriggeredBy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::AdminRetry => "admin_retry",
            Self::PatchSource => "patch_source",
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn install_status_roundtrip_via_str() {
        for s in &[
            InstallStatus::Pending,
            InstallStatus::Downloading,
            InstallStatus::Uploading,
            InstallStatus::Verifying,
            InstallStatus::Ready,
            InstallStatus::Failed,
        ] {
            let parsed: InstallStatus = s.as_str().parse().unwrap();
            assert_eq!(*s, parsed);
        }
    }

    #[test]
    fn install_status_in_progress_excludes_terminal() {
        assert!(InstallStatus::Pending.is_in_progress());
        assert!(InstallStatus::Downloading.is_in_progress());
        assert!(InstallStatus::Uploading.is_in_progress());
        assert!(InstallStatus::Verifying.is_in_progress());
        assert!(!InstallStatus::Ready.is_in_progress());
        assert!(!InstallStatus::Failed.is_in_progress());
    }

    #[test]
    fn install_status_is_ready_only_ready() {
        assert!(InstallStatus::Ready.is_ready());
        for other in &[
            InstallStatus::Pending,
            InstallStatus::Downloading,
            InstallStatus::Uploading,
            InstallStatus::Verifying,
            InstallStatus::Failed,
        ] {
            assert!(!other.is_ready());
        }
    }

    #[test]
    fn error_kind_retryable_classification() {
        // Transient → retryable.
        assert!(ErrorKind::Network.retryable());
        assert!(ErrorKind::RateLimit.retryable());
        assert!(ErrorKind::S3Error.retryable());
        assert!(ErrorKind::Sha256Mismatch.retryable());
        assert!(ErrorKind::Timeout.retryable());
        assert!(ErrorKind::StartupRecovery.retryable());

        // Requires human action → not retryable as-is.
        assert!(!ErrorKind::Source404.retryable());
        assert!(!ErrorKind::Auth.retryable());
        assert!(!ErrorKind::DiskFull.retryable());
        assert!(!ErrorKind::InvalidGguf.retryable());
        assert!(!ErrorKind::QuantizationUnsupported.retryable());
    }

    #[test]
    fn orphan_days_none_when_referenced() {
        let blob = GgufBlob {
            sha256: "x".into(),
            size_bytes: 0,
            s3_key: "gguf-blobs/x.gguf".into(),
            source_history: serde_json::json!([]),
            ref_count: 1,
            orphan_since: None,
            created_at: Utc::now(),
            last_accessed_at: None,
        };
        assert_eq!(blob.orphan_days(), None);
    }

    #[test]
    fn orphan_days_counts_when_orphaned() {
        let blob = GgufBlob {
            sha256: "x".into(),
            size_bytes: 0,
            s3_key: "gguf-blobs/x.gguf".into(),
            source_history: serde_json::json!([]),
            ref_count: 0,
            orphan_since: Some(Utc::now() - chrono::Duration::days(5)),
            created_at: Utc::now(),
            last_accessed_at: None,
        };
        assert_eq!(blob.orphan_days(), Some(5));
    }
}
