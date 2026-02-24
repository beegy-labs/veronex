use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::enums::FinishReason;

/// Telemetry event emitted after each inference job completes (success, failure, or cancel).
#[derive(Debug, Clone)]
pub struct InferenceEvent {
    pub event_time: DateTime<Utc>,
    pub request_id: Uuid,         // = InferenceJob.id.0
    pub api_key_id: Option<Uuid>, // present after rate-limiting PR
    pub tenant_id: String,        // empty string if unknown
    pub model_name: String,
    pub backend: String,          // "ollama" | "gemini"
    pub prompt_tokens: u32,       // 0 if unknown (streaming)
    pub completion_tokens: u32,   // count of token events
    pub latency_ms: u32,          // completed_at - started_at
    pub ttft_ms: Option<u32>,     // None for now (future)
    pub finish_reason: FinishReason,
    pub status: String,           // "completed" | "failed" | "cancelled"
    pub error_msg: Option<String>,
}

#[async_trait]
pub trait ObservabilityPort: Send + Sync {
    async fn record_inference(&self, event: &InferenceEvent) -> Result<()>;
}
