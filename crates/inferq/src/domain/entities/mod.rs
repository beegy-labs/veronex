pub mod account;
pub use account::Account;

pub mod session;
pub use session::Session;

pub mod api_key;
pub use api_key::*;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::{
    ApiFormat, BackendType, FinishReason, JobSource, JobStatus, LlmBackendStatus, ModelStatus,
};
use super::value_objects::{JobId, ModelName, Prompt};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceJob {
    pub id: JobId,
    pub prompt: Prompt,
    pub model_name: ModelName,
    pub status: JobStatus,
    pub backend: BackendType,
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
    /// `None` while pending/running or if not reported by backend.
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
    /// The specific backend instance (Ollama server) that processed this job.
    /// `None` until dispatched. Set by the queue dispatcher before running.
    #[serde(default)]
    pub backend_id: Option<Uuid>,
    /// Which API format the inbound request arrived via (route-based discriminator).
    #[serde(default)]
    pub api_format: ApiFormat,
    /// Multi-turn chat messages in Ollama `/api/chat` format.
    ///
    /// When Some, the OllamaAdapter uses `/api/chat` instead of `/api/generate`.
    /// Not persisted to DB — stored only in the in-memory DashMap during dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages: Option<serde_json::Value>,
    /// The HTTP path of the inbound request that created this job.
    /// e.g. "/v1/chat/completions", "/api/chat", "/v1beta/models/gemini-2.0-flash:generateContent"
    /// Not set for jobs recovered on startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub name: ModelName,
    pub backend_id: Uuid,
    pub backend_type: BackendType,
    pub vram_mb: i64,
    pub status: ModelStatus,
    pub last_used_at: Option<DateTime<Utc>>,
    pub active_calls: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResult {
    pub job_id: JobId,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    /// Tokens served from cache (e.g. Gemini `cachedContentTokenCount`).
    /// Billed at a lower rate than regular prompt tokens.
    /// `None` if the backend does not expose this metric (Ollama).
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuServer {
    pub id: Uuid,
    pub name: String,
    /// node-exporter endpoint, e.g. `"http://192.168.1.10:9100"`.
    pub node_exporter_url: Option<String>,
    pub registered_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmBackend {
    pub id: Uuid,
    pub name: String,
    pub backend_type: BackendType,
    pub url: String,
    pub api_key_encrypted: Option<String>,
    pub is_active: bool,
    /// GPU VRAM capacity in MiB (manual). 0 = unknown → treat as unlimited for dispatch.
    pub total_vram_mb: i64,
    /// GPU index on this host (0-based). Correlates with node-exporter drm/hwmon metrics.
    /// `None` = GPU 0 / not specified.
    #[serde(default)]
    pub gpu_index: Option<i16>,
    /// FK → gpu_servers. `None` for cloud backends (Gemini, etc.).
    #[serde(default)]
    pub server_id: Option<Uuid>,
    /// inferq-agent URL (Phase 2, currently unused).
    /// e.g. `http://192.168.1.10:9091`
    #[serde(default)]
    pub agent_url: Option<String>,
    /// true = key is on a Google free-tier project.
    /// RPM/RPD limits are read from `gemini_rate_limit_policies` (per model, shared).
    #[serde(default)]
    pub is_free_tier: bool,
    pub status: LlmBackendStatus,
    pub registered_at: DateTime<Utc>,
}

/// Per-model Gemini rate limit policy (shared across all free-tier backends).
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
    /// When false: skip all free-tier backends and route directly to a paid backend.
    /// RPM/RPD counters are also suppressed for paid backends.
    pub available_on_free_tier: bool,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_inference_job() -> InferenceJob {
        InferenceJob {
            id: JobId::new(),
            prompt: Prompt::new("What is Rust?").unwrap(),
            model_name: ModelName::new("llama3.2").unwrap(),
            status: JobStatus::Pending,
            backend: BackendType::Ollama,
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
            backend_id: None,
            api_format: ApiFormat::OpenaiCompat,
            messages: None,
            request_path: None,
        }
    }

    fn make_llm_backend() -> LlmBackend {
        LlmBackend {
            id: Uuid::now_v7(),
            name: "local-ollama".to_string(),
            backend_type: BackendType::Ollama,
            url: "http://localhost:11434".to_string(),
            api_key_encrypted: None,
            is_active: true,
            total_vram_mb: 24576,
            gpu_index: None,
            server_id: None,
            agent_url: None,
            is_free_tier: false,
            status: LlmBackendStatus::Online,
            registered_at: Utc::now(),
        }
    }

    fn make_model() -> Model {
        Model {
            name: ModelName::new("llama3.2").unwrap(),
            backend_id: Uuid::now_v7(),
            backend_type: BackendType::Ollama,
            vram_mb: 8192,
            status: ModelStatus::Loaded,
            last_used_at: Some(Utc::now()),
            active_calls: 2,
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
    fn inference_job_creation() {
        let job = make_inference_job();
        assert_eq!(job.status, JobStatus::Pending);
        assert_eq!(job.backend, BackendType::Ollama);
        assert_eq!(job.prompt.as_str(), "What is Rust?");
        assert_eq!(job.model_name.as_str(), "llama3.2");
        assert!(job.started_at.is_none());
        assert!(job.completed_at.is_none());
        assert!(job.error.is_none());
    }

    #[test]
    fn inference_job_with_all_fields() {
        let now = Utc::now();
        let job = InferenceJob {
            id: JobId::new(),
            prompt: Prompt::new("Explain quantum computing").unwrap(),
            model_name: ModelName::new("gemini-pro").unwrap(),
            status: JobStatus::Failed,
            backend: BackendType::Gemini,
            created_at: now,
            started_at: Some(now),
            completed_at: Some(now),
            error: Some("timeout".to_string()),
            result_text: None,
            api_key_id: None,
            account_id: None,
            latency_ms: None,
            ttft_ms: None,
            prompt_tokens: None,
            completion_tokens: None,
            cached_tokens: None,
            source: JobSource::Api,
            backend_id: None,
            api_format: ApiFormat::OpenaiCompat,
            messages: None,
            request_path: None,
        };
        assert_eq!(job.status, JobStatus::Failed);
        assert!(job.started_at.is_some());
        assert!(job.completed_at.is_some());
        assert_eq!(job.error.as_deref(), Some("timeout"));
    }

    #[test]
    fn llm_backend_creation_with_uuidv7() {
        let backend = make_llm_backend();
        assert_eq!(backend.id.get_version_num(), 7);
        assert_eq!(backend.name, "local-ollama");
        assert_eq!(backend.backend_type, BackendType::Ollama);
        assert!(backend.is_active);
        assert_eq!(backend.status, LlmBackendStatus::Online);
        assert!(backend.api_key_encrypted.is_none());
    }

    #[test]
    fn llm_backend_with_api_key() {
        let mut backend = make_llm_backend();
        backend.backend_type = BackendType::Gemini;
        backend.api_key_encrypted = Some("encrypted_key_data".to_string());
        assert!(backend.api_key_encrypted.is_some());
        assert_eq!(backend.backend_type, BackendType::Gemini);
    }

    #[test]
    fn model_creation() {
        let model = make_model();
        assert_eq!(model.name.as_str(), "llama3.2");
        assert_eq!(model.backend_type, BackendType::Ollama);
        assert_eq!(model.vram_mb, 8192);
        assert_eq!(model.status, ModelStatus::Loaded);
        assert!(model.last_used_at.is_some());
        assert_eq!(model.active_calls, 2);
    }

    #[test]
    fn model_backend_id_is_uuidv7() {
        let model = make_model();
        assert_eq!(model.backend_id.get_version_num(), 7);
    }

    #[test]
    fn inference_result_creation() {
        let result = make_inference_result();
        assert_eq!(result.prompt_tokens, 10);
        assert_eq!(result.completion_tokens, 50);
        assert_eq!(result.latency_ms, 1200);
        assert_eq!(result.ttft_ms, Some(150));
        assert_eq!(result.tokens.len(), 2);
        assert_eq!(result.finish_reason, FinishReason::Stop);
    }

    #[test]
    fn inference_result_without_ttft() {
        let result = InferenceResult {
            job_id: JobId::new(),
            prompt_tokens: 5,
            completion_tokens: 20,
            cached_tokens: None,
            latency_ms: 800,
            ttft_ms: None,
            tokens: vec!["response".to_string()],
            finish_reason: FinishReason::Length,
        };
        assert!(result.ttft_ms.is_none());
        assert_eq!(result.finish_reason, FinishReason::Length);
    }

    #[test]
    fn inference_job_serde_roundtrip() {
        let job = make_inference_job();
        let json = serde_json::to_string(&job).unwrap();
        let deserialized: InferenceJob = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, job.id);
        assert_eq!(deserialized.status, job.status);
        assert_eq!(deserialized.backend, job.backend);
        assert_eq!(deserialized.prompt.as_str(), job.prompt.as_str());
        assert_eq!(deserialized.model_name.as_str(), job.model_name.as_str());
    }

    #[test]
    fn llm_backend_serde_roundtrip() {
        let backend = make_llm_backend();
        let json = serde_json::to_string(&backend).unwrap();
        let deserialized: LlmBackend = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, backend.id);
        assert_eq!(deserialized.name, backend.name);
        assert_eq!(deserialized.backend_type, backend.backend_type);
        assert_eq!(deserialized.url, backend.url);
        assert_eq!(deserialized.is_active, backend.is_active);
        assert_eq!(deserialized.status, backend.status);
    }

    #[test]
    fn model_serde_roundtrip() {
        let model = make_model();
        let json = serde_json::to_string(&model).unwrap();
        let deserialized: Model = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name.as_str(), model.name.as_str());
        assert_eq!(deserialized.backend_id, model.backend_id);
        assert_eq!(deserialized.backend_type, model.backend_type);
        assert_eq!(deserialized.vram_mb, model.vram_mb);
        assert_eq!(deserialized.status, model.status);
        assert_eq!(deserialized.active_calls, model.active_calls);
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
