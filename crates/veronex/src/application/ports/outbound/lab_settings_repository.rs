use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Snapshot of all lab (experimental) feature flags.
#[derive(Debug, Clone)]
pub struct LabSettings {
    // ── Existing image settings ──────────────────────────────────────────────
    /// Gemini function-calling (tool use) support.
    pub gemini_function_calling: bool,
    /// Max images per inference request. 0 = image input disabled.
    pub max_images_per_request: i32,
    /// Max base64 bytes per image (default 2 MB).
    pub max_image_b64_bytes: i32,

    // ── Context compression ──────────────────────────────────────────────────
    pub context_compression_enabled: bool,
    /// Model used for per-turn compression. `None` = reuse the inference model.
    pub compression_model: Option<String>,
    /// Fraction of `configured_ctx` reserved for the assembled inference input.
    pub context_budget_ratio: f32,
    /// Compress every N completed turns (1 = every turn).
    pub compression_trigger_turns: i32,
    /// Keep the last N turns verbatim (not compressed) in context assembly.
    pub recent_verbatim_window: i32,
    /// Timeout for a single compression call in seconds.
    pub compression_timeout_secs: i32,

    // ── Multi-turn eligibility gate ──────────────────────────────────────────
    /// Minimum model parameter count in billions. Below this → 400.
    pub multiturn_min_params: i32,
    /// Minimum `max_ctx` (model architecture maximum). Below this → 400.
    pub multiturn_min_ctx: i32,
    /// Explicit allowlist of model names. Empty = all qualifying models allowed.
    pub multiturn_allowed_models: Vec<String>,

    // ── Vision ───────────────────────────────────────────────────────────────
    /// Designated vision model. `None` = auto-select from available providers.
    pub vision_model: Option<String>,

    // ── Session handoff ──────────────────────────────────────────────────────
    pub handoff_enabled: bool,
    /// Fraction of `configured_ctx` that triggers session handoff (default 0.85).
    pub handoff_threshold: f32,

    pub updated_at: DateTime<Utc>,
}

impl Default for LabSettings {
    fn default() -> Self {
        Self {
            gemini_function_calling: false,
            max_images_per_request: 4,
            max_image_b64_bytes: 2 * 1024 * 1024,
            context_compression_enabled: false,
            compression_model: None,
            context_budget_ratio: 0.60,
            compression_trigger_turns: 1,
            recent_verbatim_window: 1,
            compression_timeout_secs: 10,
            multiturn_min_params: 7,
            multiturn_min_ctx: 16384,
            multiturn_allowed_models: Vec::new(),
            vision_model: None,
            handoff_enabled: true,
            handoff_threshold: 0.85,
            updated_at: Utc::now(),
        }
    }
}

/// Partial-update payload for `LabSettingsRepository::update()`.
/// All fields are `Option` — `None` means "keep current value" (COALESCE semantics).
/// For nullable text fields (e.g. `compression_model`), `Some(None)` clears the value.
#[derive(Debug, Default)]
pub struct LabSettingsUpdate {
    pub gemini_function_calling: Option<bool>,
    pub max_images_per_request: Option<i32>,
    pub max_image_b64_bytes: Option<i32>,
    pub context_compression_enabled: Option<bool>,
    pub compression_model: Option<Option<String>>,
    pub context_budget_ratio: Option<f32>,
    pub compression_trigger_turns: Option<i32>,
    pub recent_verbatim_window: Option<i32>,
    pub compression_timeout_secs: Option<i32>,
    pub multiturn_min_params: Option<i32>,
    pub multiturn_min_ctx: Option<i32>,
    pub multiturn_allowed_models: Option<Vec<String>>,
    pub vision_model: Option<Option<String>>,
    pub handoff_enabled: Option<bool>,
    pub handoff_threshold: Option<f32>,
}

#[async_trait]
pub trait LabSettingsRepository: Send + Sync {
    async fn get(&self) -> Result<LabSettings>;
    async fn update(&self, patch: LabSettingsUpdate) -> Result<LabSettings>;
}
