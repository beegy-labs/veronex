mod compression_router;
mod context_compressor;
mod dispatcher;
mod helpers;
mod runner;
mod use_case;

pub use helpers::record_tpm;
pub use use_case::InferenceUseCaseImpl;

// ── Shared types (visible to sibling submodules via `super::`) ──────────────

use std::sync::Arc;

use tokio::sync::Notify;
use uuid::Uuid;

use crate::application::ports::outbound::message_store::VisionAnalysis;
use crate::domain::enums::{JobStatus, KeyTier};
use crate::domain::entities::InferenceJob;
use crate::domain::value_objects::StreamToken;

pub(crate) struct JobEntry {
    pub job: InferenceJob,
    pub status: JobStatus,
    pub tokens: Vec<StreamToken>,
    pub done: bool,
    pub api_key_id: Option<Uuid>,
    pub notify: Arc<Notify>,
    pub cancel_notify: Arc<Notify>,
    pub gemini_tier: Option<String>,
    pub key_tier: Option<KeyTier>,
    pub tpm_reservation_minute: Option<i64>,
    /// Provider this job was dispatched to (set at dispatch time, used for Hard drain cancel).
    pub assigned_provider_id: Option<Uuid>,
    /// Vision pre-processing result. Set by HTTP handler, consumed by finalize_job().
    pub vision_analysis: Option<VisionAnalysis>,
    /// Resources for per-turn compression. `None` when compression is not configured.
    pub compression_handle: Option<Arc<compression_router::CompressionHandle>>,
}
