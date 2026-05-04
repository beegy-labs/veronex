pub mod account;
pub use account::Account;

pub mod session;
pub use session::Session;

pub mod api_key;
pub use api_key::*;

pub mod role;
pub use role::Role;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use super::enums::{
    ApiFormat, FinishReason, JobSource, JobStatus, LlmProviderStatus, ProviderType,
};
use super::value_objects::{JobId, ModelName, Prompt, VisionAnalysis};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
pub struct InferenceJob {
    #[ts(type = "string")]
    pub id: JobId,
    pub prompt: Prompt,
    /// First ≤200 characters of the prompt (char boundary, CJK-safe).
    /// The only part of the prompt persisted to Postgres. Full prompt lives in S3.
    #[serde(default)]
    pub prompt_preview: Option<String>,
    pub model_name: ModelName,
    pub status: JobStatus,
    pub provider_type: ProviderType,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    /// Full concatenated output of the inference. Populated on completion so the
    /// result can be replayed after a server restart (since the token buffer is
    /// in-memory only).
    #[serde(default)]
    pub result_text: Option<String>,
    /// The API key that submitted this job. `None` for Test Run jobs.
    #[serde(default)]
    pub api_key_id: Option<Uuid>,
    /// The account that submitted this job via Test Run (JWT). `None` for API key jobs.
    #[serde(default)]
    pub account_id: Option<Uuid>,
    /// Inference execution latency in ms (started_at → completed_at).
    /// `None` while the job is pending/running.
    #[serde(default)]
    pub latency_ms: Option<i32>,
    /// Time to first token in ms (started_at → first token received).
    /// `None` while pending/running or not yet measured.
    #[serde(default)]
    pub ttft_ms: Option<i32>,
    /// Number of prompt (input) tokens.
    /// `None` while pending/running or if not reported by provider.
    #[serde(default)]
    pub prompt_tokens: Option<i32>,
    /// Number of completion (output) tokens generated.
    /// `None` while pending/running.
    #[serde(default)]
    pub completion_tokens: Option<i32>,
    /// Tokens served from cache (Gemini `cachedContentTokenCount`).
    /// Always `None` for Ollama (not exposed by API).
    #[serde(default)]
    pub cached_tokens: Option<i32>,
    /// Whether this job came from the test panel or a real API client.
    #[serde(default)]
    pub source: JobSource,
    /// The specific provider instance (Ollama server) that processed this job.
    /// `None` until dispatched. Set by the queue dispatcher before running.
    #[serde(default)]
    pub provider_id: Option<Uuid>,
    /// Which API format the inbound request arrived via (route-based discriminator).
    #[serde(default)]
    pub api_format: ApiFormat,
    /// Full LLM input context — complete messages array in Ollama `/api/chat` format.
    ///
    /// Contains: system prompt + prior turns (user/assistant/tool) + current user message.
    /// When Some, the OllamaAdapter routes to `/api/chat`; when None, to `/api/generate`.
    ///
    /// Stored in S3 `ConversationRecord.messages` (not persisted to Postgres).
    /// Serves as ground-truth training input: input=messages, output=result+tool_calls.
    /// Can reach 100–500 KB for agentic sessions with large file contents.
    #[serde(default)]
    pub messages: Option<serde_json::Value>,
    /// Tool/function definitions forwarded from the client (OpenAI or Ollama format).
    /// Passed to the provider so it can produce proper `tool_calls` responses.
    /// Not persisted to DB — in-memory only during dispatch.
    #[serde(default)]
    pub tools: Option<serde_json::Value>,
    /// The HTTP path of the inbound request that created this job.
    /// e.g. "/v1/chat/completions", "/api/chat", "/v1beta/models/gemini-2.0-flash:generateContent"
    /// Not set for jobs recovered on startup.
    #[serde(default)]
    pub request_path: Option<String>,
    /// Time the job spent waiting in the Valkey queue before dispatch (ms).
    /// Computed as `started_at - created_at` when the job transitions to Running.
    /// `None` while pending (not yet dispatched).
    #[serde(default)]
    pub queue_time_ms: Option<i32>,
    /// Timestamp when a cancellation request was received.
    /// Set by `InferenceUseCaseImpl::cancel()` for non-terminal jobs.
    /// `None` if the job was never cancelled.
    #[serde(default)]
    pub cancelled_at: Option<DateTime<Utc>>,
    /// Client-supplied conversation / thread ID (from X-Conversation-ID header).
    /// Groups all LLM turns that belong to one agent session.
    /// NULL for single-turn requests or clients that do not send the header.
    #[serde(default)]
    pub conversation_id: Option<Uuid>,
    /// Structured tool calls returned by the model (JSONB in DB).
    /// Ollama format: `[{function: {name, arguments}}]`
    /// Populated when the model made at least one tool call; None for text-only responses.
    #[serde(default)]
    pub tool_calls_json: Option<serde_json::Value>,
    /// Blake2b-256 hex hash of the full messages array.
    /// Used for session grouping (conversation chain detection).
    #[serde(default)]
    pub messages_hash: Option<String>,
    /// Blake2b-256 hex hash of messages[0..n-1] (all turns except last).
    /// Empty string = first turn (no parent). Used to link child → parent in a session.
    #[serde(default)]
    pub messages_prefix_hash: Option<String>,
    /// Machine-readable failure cause (G16). Set when status=Failed.
    /// Values: queue_full, no_eligible_provider, queue_wait_exceeded, provider_error,
    ///         token_budget_exceeded, lease_expired_max_attempts, lease_expired_reenqueue_failed
    #[serde(default)]
    pub failure_reason: Option<String>,
    /// Base64 images for vision inference (/api/generate).
    /// Not persisted to DB — in-memory only during dispatch.
    #[serde(default)]
    #[ts(skip)]
    pub images: Option<Vec<String>>,
    /// S3 keys for stored WebP images (full + thumbnail pairs).
    /// Populated after async image upload completes. Persisted in DB.
    #[serde(default)]
    pub image_keys: Option<Vec<String>>,
    /// Stop sequences for inference. Not persisted to DB — in-memory only during dispatch.
    #[serde(default)]
    #[ts(skip)]
    pub stop: Option<serde_json::Value>,
    /// Seed for reproducible outputs. Not persisted to DB.
    #[serde(default)]
    #[ts(skip)]
    pub seed: Option<u32>,
    /// Response format (json_object/text/json_schema). Not persisted to DB.
    #[serde(default)]
    #[ts(skip)]
    pub response_format: Option<serde_json::Value>,
    /// Frequency penalty. Not persisted to DB.
    #[serde(default)]
    #[ts(skip)]
    pub frequency_penalty: Option<f64>,
    /// Presence penalty. Not persisted to DB.
    #[serde(default)]
    #[ts(skip)]
    pub presence_penalty: Option<f64>,
    /// Groups all inference_jobs belonging to one MCP agentic loop run.
    /// NULL for non-MCP requests (single-turn, no tool calls).
    #[serde(default)]
    pub mcp_loop_id: Option<Uuid>,
    /// Max tokens (output limit) capped at the HTTP handler boundary.
    /// Passed to Ollama as `options.num_predict`. Not persisted to DB.
    #[serde(default)]
    #[ts(skip)]
    pub max_tokens: Option<u32>,
    /// Vision pre-processing result for image-bearing tasks.
    /// Set in-memory by the HTTP handler after the vision call completes,
    /// injected into `TurnRecord` by `finalize_job()` before S3 write.
    /// Not persisted to DB.
    #[serde(default)]
    #[ts(skip)]
    pub vision_analysis: Option<VisionAnalysis>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResult {
    pub job_id: JobId,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    /// Tokens served from cache (e.g. Gemini `cachedContentTokenCount`).
    /// Billed at a lower rate than regular prompt tokens.
    /// `None` if the provider does not expose this metric (Ollama).
    pub cached_tokens: Option<u32>,
    pub latency_ms: u32,
    pub ttft_ms: Option<u32>,
    pub tokens: Vec<String>,
    pub finish_reason: FinishReason,
}

/// Physical GPU server (one node-exporter per host).
///
/// `node_exporter_url` is the only connection point to the hardware.
/// CPU / memory / GPU metrics are fetched live from that endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
pub struct GpuServer {
    pub id: Uuid,
    pub name: String,
    /// node-exporter endpoint, e.g. `"http://192.168.1.10:9100"`.
    pub node_exporter_url: Option<String>,
    pub registered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
pub struct LlmProvider {
    pub id: Uuid,
    pub name: String,
    pub provider_type: ProviderType,
    pub url: String,
    pub api_key_encrypted: Option<String>,
    /// GPU VRAM capacity in MiB (manual). 0 = unknown → treat as unlimited for dispatch.
    pub total_vram_mb: i64,
    /// GPU index on this host (0-based). Correlates with node-exporter drm/hwmon metrics.
    /// `None` = GPU 0 / not specified.
    #[serde(default)]
    pub gpu_index: Option<i16>,
    /// FK → gpu_servers. `None` for cloud providers (Gemini, etc.).
    #[serde(default)]
    pub server_id: Option<Uuid>,
    /// true = key is on a Google free-tier project.
    /// RPM/RPD limits are read from `gemini_rate_limit_policies` (per model, shared).
    #[serde(default)]
    pub is_free_tier: bool,
    /// Maximum parallel requests per Ollama num_parallel setting.
    /// Used as AIMD upper bound. Default 4.
    #[serde(default = "default_num_parallel")]
    pub num_parallel: i16,
    pub status: LlmProviderStatus,
    pub registered_at: DateTime<Utc>,
}

impl LlmProvider {
    /// True for Ollama-typed providers. Used by handlers and helpers that
    /// filter the registry list to local GPU hosts (vs. Gemini cloud).
    pub fn is_ollama(&self) -> bool {
        self.provider_type == ProviderType::Ollama
    }

    /// True for Gemini-typed providers (cloud).
    pub fn is_gemini(&self) -> bool {
        self.provider_type == ProviderType::Gemini
    }
}

fn default_num_parallel() -> i16 {
    4
}

/// Per-model Gemini rate limit policy (shared across all free-tier providers).
///
/// Stored in `gemini_rate_limit_policies` and editable from the admin UI.
/// `model_name = "*"` is the global fallback used when no model-specific row exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiRateLimitPolicy {
    pub id: Uuid,
    /// e.g. "gemini-2.5-flash" or "*" (global default)
    pub model_name: String,
    /// Max requests per minute (0 = no enforcement)
    pub rpm_limit: i32,
    /// Max requests per day (0 = no enforcement)
    pub rpd_limit: i32,
    /// Whether this model can be used on a Google free-tier project.
    /// When false: skip all free-tier providers and route directly to a paid provider.
    /// RPM/RPD counters are also suppressed for paid providers.
    pub available_on_free_tier: bool,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn make_inference_job() -> InferenceJob {
        InferenceJob {
            id: JobId::new(),
            prompt: Prompt::new("What is Rust?").unwrap(),
            prompt_preview: None,
            model_name: ModelName::new("llama3.2").unwrap(),
            status: JobStatus::Pending,
            provider_type: ProviderType::Ollama,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            error: None,
            result_text: None,
            api_key_id: None,
            account_id: None,
            latency_ms: None,
            ttft_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            cached_tokens: None,
            source: JobSource::Api,
            provider_id: None,
            api_format: ApiFormat::OpenaiCompat,
            messages: None,
            tools: None,
            max_tokens: None,
            request_path: None,
            queue_time_ms: None,
            cancelled_at: None,
            conversation_id: None,
            tool_calls_json: None,
            messages_hash: None,
            messages_prefix_hash: None,
            failure_reason: None,
            images: None,
            image_keys: None,
            stop: None,
            seed: None,
            response_format: None,
            frequency_penalty: None,
            presence_penalty: None,
            mcp_loop_id: None,
            vision_analysis: None,
        }
    }

    fn make_llm_provider() -> LlmProvider {
        LlmProvider {
            id: Uuid::now_v7(),
            name: "local-ollama".to_string(),
            provider_type: ProviderType::Ollama,
            url: "http://localhost:11434".to_string(),
            api_key_encrypted: None,
            total_vram_mb: 24576,
            gpu_index: None,
            server_id: None,
            is_free_tier: false,
            num_parallel: 4,
            status: LlmProviderStatus::Online,
            registered_at: Utc::now(),
        }
    }

    fn make_inference_result() -> InferenceResult {
        InferenceResult {
            job_id: JobId::new(),
            prompt_tokens: 10,
            completion_tokens: 50,
            cached_tokens: None,
            latency_ms: 1200,
            ttft_ms: Some(150),
            tokens: vec!["Hello".to_string(), " world".to_string()],
            finish_reason: FinishReason::Stop,
        }
    }

    #[test]
    fn inference_job_serde_roundtrip() {
        let job = make_inference_job();
        let json = serde_json::to_string(&job).unwrap();
        let deserialized: InferenceJob = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, job.id);
        assert_eq!(deserialized.status, job.status);
        assert_eq!(deserialized.provider_type, job.provider_type);
        assert_eq!(deserialized.prompt.as_str(), job.prompt.as_str());
        assert_eq!(deserialized.model_name.as_str(), job.model_name.as_str());
    }

    #[test]
    fn llm_provider_serde_roundtrip() {
        let provider = make_llm_provider();
        let json = serde_json::to_string(&provider).unwrap();
        let deserialized: LlmProvider = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, provider.id);
        assert_eq!(deserialized.name, provider.name);
        assert_eq!(deserialized.provider_type, provider.provider_type);
        assert_eq!(deserialized.url, provider.url);
        assert_eq!(deserialized.status, provider.status);
    }

    #[test]
    fn inference_job_with_images() {
        let mut job = make_inference_job();
        job.images = Some(vec!["abc123".to_string(), "def456".to_string()]);
        let json = serde_json::to_string(&job).unwrap();
        let deserialized: InferenceJob = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.images.as_ref().unwrap().len(), 2);
        assert_eq!(deserialized.images.as_ref().unwrap()[0], "abc123");
    }

    #[test]
    fn inference_result_serde_roundtrip() {
        let result = make_inference_result();
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: InferenceResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.job_id, result.job_id);
        assert_eq!(deserialized.prompt_tokens, result.prompt_tokens);
        assert_eq!(deserialized.completion_tokens, result.completion_tokens);
        assert_eq!(deserialized.latency_ms, result.latency_ms);
        assert_eq!(deserialized.ttft_ms, result.ttft_ms);
        assert_eq!(deserialized.tokens, result.tokens);
        assert_eq!(deserialized.finish_reason, result.finish_reason);
    }
}
