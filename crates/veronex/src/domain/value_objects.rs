use std::fmt;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::domain::errors::DomainError;

// ── Public ID encoding ───────────────────────────────────────────────────────
//
// All IDs stored internally as UUIDv7. External API exposes them as
// `{prefix}_{base62}` (e.g. `job_3X4aB...`). Each entity gets a typed
// newtype so the compiler prevents mixing IDs across entity boundaries.
//
// The macro generates: struct, Display (as prefix_base62), FromStr,
// Serialize (base62 string), Deserialize (from base62 string), From<Uuid>,
// Into<Uuid>, new(), from_uuid(), as_uuid().

macro_rules! define_entity_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name(pub Uuid);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            pub fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl From<Uuid> for $name {
            fn from(u: Uuid) -> Self {
                Self(u)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", $prefix, base62::encode(self.0.as_u128()))
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let encoded = s
                    .strip_prefix(concat!($prefix, "_"))
                    .ok_or_else(|| format!("expected prefix '{}_'", $prefix))?;
                let n = base62::decode(encoded)
                    .map_err(|e| format!("invalid base62: {e}"))?;
                Ok(Self(Uuid::from_u128(n)))
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                s.parse::<Self>().map_err(serde::de::Error::custom)
            }
        }
    };
}

// ── Entity ID types ──────────────────────────────────────────────────────────

define_entity_id!(JobId,        "job");
define_entity_id!(ConvId,       "conv");
define_entity_id!(AccountId,    "acct");
define_entity_id!(ApiKeyId,     "key");
define_entity_id!(RoleId,       "role");
define_entity_id!(SessionId,    "sess");
define_entity_id!(ProviderId,   "prov");
define_entity_id!(GpuServerId,  "gpu");
define_entity_id!(McpId,        "mcp");

/// Encode a UUID as a prefixed base62 public ID (e.g. `"job_3X4aB..."`).
pub fn pub_id_encode(prefix: &str, uuid: Uuid) -> String {
    format!("{}_{}", prefix, base62::encode(uuid.as_u128()))
}

/// Lightweight event fired on every job status transition.
/// Broadcast via tokio broadcast channel → SSE endpoint → network flow UI.
#[derive(Debug, Clone, Serialize)]
pub struct JobStatusEvent {
    pub id: String,
    pub status: String,
    pub model_name: String,
    pub provider_type: String,
    pub latency_ms: Option<i32>,
}

/// Real-time aggregate stats pushed to all SSE clients every second.
/// All connected users receive the same values simultaneously.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
pub struct FlowStats {
    /// Enqueue events in the last 10 seconds (raw count).
    /// Client divides by 10 for req/s display.
    pub incoming: u32,
    /// Enqueue events in the last 60 seconds (raw count) = req/m.
    pub incoming_60s: u32,
    /// Jobs currently waiting in the queue.
    pub queued: u32,
    /// Jobs currently being processed by a provider.
    pub running: u32,
    /// Jobs that completed (any terminal status) in the last 60 seconds.
    pub completed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
pub struct ModelName(String);

impl ModelName {
    pub fn new(s: &str) -> Result<Self, DomainError> {
        if s.is_empty() {
            return Err(DomainError::Validation("model name cannot be empty".to_string()));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
pub struct Prompt(String);

impl Prompt {
    pub fn new(s: &str) -> Result<Self, DomainError> {
        if s.is_empty() {
            return Err(DomainError::Validation("prompt cannot be empty".to_string()));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct StreamToken {
    pub value: String,
    pub is_final: bool,
    /// Actual prompt token count from the provider's usage metadata.
    /// Only populated on the final token when the provider reports real counts.
    pub prompt_tokens: Option<u32>,
    /// Actual completion token count from the provider's usage metadata.
    /// Only populated on the final token when the provider reports real counts.
    pub completion_tokens: Option<u32>,
    /// Tokens served from cache (Gemini `cachedContentTokenCount`).
    /// Only populated on the final token; `None` for Ollama.
    pub cached_tokens: Option<u32>,
    /// Tool calls returned by the model (Ollama `/api/chat` format).
    /// When Some, this token carries tool call data instead of text content.
    /// Handlers must convert to the appropriate wire format (OpenAI vs Ollama NDJSON).
    pub tool_calls: Option<serde_json::Value>,
    /// Finish reason from the provider ("stop", "length", "tool_calls").
    /// Only set on the final token. `None` for intermediate tokens.
    pub finish_reason: Option<String>,
}

impl StreamToken {
    pub fn text(value: String) -> Self {
        Self { value, is_final: false, prompt_tokens: None, completion_tokens: None, cached_tokens: None, tool_calls: None, finish_reason: None }
    }
    pub fn done() -> Self {
        Self { value: String::new(), is_final: true, prompt_tokens: None, completion_tokens: None, cached_tokens: None, tool_calls: None, finish_reason: None }
    }
}

// ── Validated value objects (backend-only) ──────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Username(String);

impl Username {
    pub fn new(s: &str) -> Result<Self, DomainError> {
        if s.is_empty() {
            return Err(DomainError::Validation("username cannot be empty".to_string()));
        }
        if s.len() > 255 {
            return Err(DomainError::Validation("username exceeds 255 characters".to_string()));
        }
        if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(DomainError::Validation(
                "username must contain only alphanumeric characters, underscores, or hyphens".to_string(),
            ));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Email(String);

impl Email {
    pub fn new(s: &str) -> Result<Self, DomainError> {
        if !s.contains('@') {
            return Err(DomainError::Validation("email must contain '@'".to_string()));
        }
        if s.len() > 254 {
            return Err(DomainError::Validation("email exceeds 254 characters".to_string()));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderUrl(String);

impl ProviderUrl {
    pub fn new(s: &str) -> Result<Self, DomainError> {
        if !(s.starts_with("http://") || s.starts_with("https://")) {
            return Err(DomainError::Validation(
                "provider URL must start with http:// or https://".to_string(),
            ));
        }
        Ok(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// ── Vision analysis ──────────────────────────────────────────────────────────

/// Result of the vision pre-processing call for an image-bearing turn.
///
/// Stored in `TurnRecord` / `InferenceJob` before inference runs so that future
/// compression can preserve the image context as text rather than losing it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionAnalysis {
    /// Image analysis text produced by the vision model (~200 tokens).
    pub analysis: String,
    /// Model used for analysis (e.g. "llava:7b").
    pub vision_model: String,
    /// Number of images analyzed.
    pub image_count: u32,
    /// Token count of the analysis output.
    pub analysis_tokens: u32,
}

// ── Model instance lifecycle (per-provider, per-model) ───────────────────────
//
// Tracked in VramPool (SSOT) and updated by ModelLifecyclePort adapters and
// the sync_loop background task. State transitions are constrained to the
// closure documented on `ModelInstanceState::can_transition_to`.
//
// SDD reference: `.specs/veronex/inference-lifecycle-sod.md`.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelInstanceState {
    NotLoaded,
    Loading {
        started_at: SystemTime,
        last_progress_at: SystemTime,
    },
    Loaded {
        loaded_at: SystemTime,
        weight_bytes: u64,
    },
    Failed {
        failed_at: SystemTime,
        reason: String,
        retry_after: SystemTime,
    },
    Evicted {
        evicted_at: SystemTime,
        reason: EvictionReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvictionReason {
    /// ollama unloaded the model to make room for another.
    VramPressure,
    /// ollama TTL expired (low-power keep-alive policy).
    KeepAliveExpired,
    /// Explicit `evict()` call (operator action / model unenrollment).
    Operator,
    /// Cleanup after a load attempt failed.
    LoadFailed,
}

impl ModelInstanceState {
    /// Returns `true` when `self` may transition to `next` per the lifecycle
    /// invariants. Callers MUST check this before persisting a new state.
    ///
    /// Allowed transitions:
    /// ```text
    /// NotLoaded → Loading | Failed
    /// Loading   → Loaded  | Failed
    /// Loaded    → Evicted              (must go via Evicted, not directly NotLoaded)
    /// Failed    → Loading              (retry after retry_after)
    /// Evicted   → NotLoaded | Loading
    /// ```
    pub fn can_transition_to(&self, next: &Self) -> bool {
        use ModelInstanceState::*;
        matches!(
            (self, next),
            (NotLoaded, Loading { .. })
                | (NotLoaded, Failed { .. })
                | (Loading { .. }, Loaded { .. })
                | (Loading { .. }, Failed { .. })
                | (Loaded { .. }, Evicted { .. })
                | (Failed { .. }, Loading { .. })
                | (Evicted { .. }, NotLoaded)
                | (Evicted { .. }, Loading { .. })
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ── Username ─────────────────────────────────────────────────────────

    /// Concrete examples kept as documentation.
    #[test]
    fn username_valid_examples() {
        assert!(Username::new("alice").is_ok());
        assert!(Username::new("bob-123").is_ok());
        assert!(Username::new("c_d").is_ok());
    }

    #[test]
    fn username_empty() {
        assert!(Username::new("").is_err());
    }

    proptest! {
        #[test]
        fn username_valid_chars_always_accepted(
            s in "[a-zA-Z0-9_-]{1,255}"
        ) {
            let u = Username::new(&s).unwrap();
            prop_assert_eq!(u.as_str(), s.as_str());
        }

        #[test]
        fn username_too_long_always_rejected(extra in 1u16..500) {
            let s: String = "a".repeat(255 + extra as usize);
            prop_assert!(Username::new(&s).is_err());
        }

        #[test]
        fn username_with_special_chars_rejected(
            prefix in "[a-z]{1,5}",
            bad_char in "[^a-zA-Z0-9_-]",
            suffix in "[a-z]{0,5}",
        ) {
            let s = format!("{prefix}{bad_char}{suffix}");
            prop_assert!(Username::new(&s).is_err());
        }
    }

    // ── Email ────────────────────────────────────────────────────────────

    /// Concrete example kept as documentation.
    #[test]
    fn email_valid_example() {
        assert!(Email::new("a@b.com").is_ok());
    }

    proptest! {
        #[test]
        fn email_with_at_sign_and_valid_length_accepted(
            local in "[a-z]{1,50}",
            domain in "[a-z]{1,50}\\.[a-z]{2,4}",
        ) {
            let email = format!("{local}@{domain}");
            prop_assume!(email.len() <= 254);
            let e = Email::new(&email).unwrap();
            prop_assert_eq!(e.as_str(), email.as_str());
        }

        #[test]
        fn email_without_at_rejected(s in "[a-zA-Z0-9.]{1,100}") {
            prop_assume!(!s.contains('@'));
            prop_assert!(Email::new(&s).is_err());
        }

        #[test]
        fn email_too_long_rejected(extra in 1u16..500) {
            let local = "a".repeat(250 + extra as usize);
            let email = format!("{local}@b.c");
            prop_assert!(Email::new(&email).is_err());
        }
    }

    // ── ProviderUrl ──────────────────────────────────────────────────────

    /// Concrete example kept as documentation.
    #[test]
    fn provider_url_valid_example() {
        assert!(ProviderUrl::new("http://localhost:11434").is_ok());
    }

    proptest! {
        #[test]
        fn provider_url_http_https_accepted(
            host in "[a-z]{3,10}(\\.[a-z]{2,5})?",
            port in prop::option::of(1000u16..65535),
            use_https in proptest::bool::ANY,
        ) {
            let scheme = if use_https { "https" } else { "http" };
            let url = match port {
                Some(p) => format!("{scheme}://{host}:{p}"),
                None => format!("{scheme}://{host}"),
            };
            let p = ProviderUrl::new(&url).unwrap();
            prop_assert_eq!(p.as_str(), url.as_str());
        }

        #[test]
        fn provider_url_non_http_scheme_rejected(
            scheme in "(ftp|ssh|ws|wss|file)",
            host in "[a-z]{3,10}",
        ) {
            let url = format!("{scheme}://{host}");
            prop_assert!(ProviderUrl::new(&url).is_err());
        }
    }

    // ── ModelName ────────────────────────────────────────────────────────

    #[test]
    fn model_name_valid() {
        let m = ModelName::new("llama3.2:latest").unwrap();
        assert_eq!(m.as_str(), "llama3.2:latest");
    }

    #[test]
    fn model_name_empty_rejected() {
        assert!(ModelName::new("").is_err());
    }

    // ── Prompt ──────────────────────────────────────────────────────────

    #[test]
    fn prompt_valid() {
        let p = Prompt::new("hello").unwrap();
        assert_eq!(p.as_str(), "hello");
    }

    #[test]
    fn prompt_empty_rejected() {
        assert!(Prompt::new("").is_err());
    }

    // ── ModelInstanceState transitions ───────────────────────────────────

    use std::time::{Duration, SystemTime};

    fn now() -> SystemTime {
        SystemTime::now()
    }

    #[test]
    fn model_state_not_loaded_to_loading_allowed() {
        let from = ModelInstanceState::NotLoaded;
        let to = ModelInstanceState::Loading {
            started_at: now(),
            last_progress_at: now(),
        };
        assert!(from.can_transition_to(&to));
    }

    #[test]
    fn model_state_loading_to_loaded_allowed() {
        let from = ModelInstanceState::Loading {
            started_at: now(),
            last_progress_at: now(),
        };
        let to = ModelInstanceState::Loaded {
            loaded_at: now(),
            weight_bytes: 58 * 1024 * 1024 * 1024,
        };
        assert!(from.can_transition_to(&to));
    }

    #[test]
    fn model_state_loaded_direct_to_not_loaded_forbidden() {
        // Must go via Evicted — invariant prevents losing the eviction reason
        let from = ModelInstanceState::Loaded {
            loaded_at: now(),
            weight_bytes: 0,
        };
        let to = ModelInstanceState::NotLoaded;
        assert!(!from.can_transition_to(&to));
    }

    #[test]
    fn model_state_loaded_to_evicted_allowed() {
        let from = ModelInstanceState::Loaded {
            loaded_at: now(),
            weight_bytes: 0,
        };
        let to = ModelInstanceState::Evicted {
            evicted_at: now(),
            reason: EvictionReason::KeepAliveExpired,
        };
        assert!(from.can_transition_to(&to));
    }

    #[test]
    fn model_state_failed_to_loading_allowed_for_retry() {
        let from = ModelInstanceState::Failed {
            failed_at: now(),
            reason: "stalled".into(),
            retry_after: now() + Duration::from_secs(60),
        };
        let to = ModelInstanceState::Loading {
            started_at: now(),
            last_progress_at: now(),
        };
        assert!(from.can_transition_to(&to));
    }

    #[test]
    fn model_state_evicted_to_loading_allowed() {
        let from = ModelInstanceState::Evicted {
            evicted_at: now(),
            reason: EvictionReason::VramPressure,
        };
        let to = ModelInstanceState::Loading {
            started_at: now(),
            last_progress_at: now(),
        };
        assert!(from.can_transition_to(&to));
    }

    #[test]
    fn model_state_serde_roundtrip_loaded() {
        let s = ModelInstanceState::Loaded {
            loaded_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000),
            weight_bytes: 12_345_678,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: ModelInstanceState = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn eviction_reason_serde_each_variant() {
        for r in [
            EvictionReason::VramPressure,
            EvictionReason::KeepAliveExpired,
            EvictionReason::Operator,
            EvictionReason::LoadFailed,
        ] {
            let json = serde_json::to_string(&r).unwrap();
            let back: EvictionReason = serde_json::from_str(&json).unwrap();
            assert_eq!(r, back);
        }
    }
}
