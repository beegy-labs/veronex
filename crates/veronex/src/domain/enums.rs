use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ── Account role (legacy — kept for backward compat during migration) ───────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum AccountRole {
    Super,
    Admin,
}

// ── Permission ──────────────────────────────────────────────────────────────

/// Permission identifiers stored in `roles.permissions TEXT[]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    DashboardView,
    ApiTest,
    ProviderManage,
    KeyManage,
    AccountManage,
    AuditView,
    SettingsManage,
    RoleManage,
    ModelManage,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DashboardView => "dashboard_view",
            Self::ApiTest => "api_test",
            Self::ProviderManage => "provider_manage",
            Self::KeyManage => "key_manage",
            Self::AccountManage => "account_manage",
            Self::AuditView => "audit_view",
            Self::SettingsManage => "settings_manage",
            Self::RoleManage => "role_manage",
            Self::ModelManage => "model_manage",
        }
    }
}

/// All valid permission strings — used for input validation.
pub const ALL_PERMISSIONS: &[&str] = &[
    "dashboard_view", "api_test", "provider_manage",
    "key_manage", "account_manage", "audit_view", "settings_manage",
    "role_manage", "model_manage",
];

// ── Menu ────────────────────────────────────────────────────────────────────

/// Menu identifiers stored in `roles.menus TEXT[]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "snake_case")]
pub enum MenuId {
    Dashboard,
    Flow,
    Jobs,
    Performance,
    Usage,
    Test,
    Providers,
    Servers,
    Keys,
    Accounts,
    Audit,
    ApiDocs,
}

/// All valid menu ID strings — used for input validation.
pub const ALL_MENUS: &[&str] = &[
    "dashboard", "flow", "jobs", "performance", "usage", "test",
    "providers", "servers", "keys", "accounts", "audit", "api_docs",
];

impl AccountRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Super => "super",
            Self::Admin => "admin",
        }
    }
}

impl std::fmt::Display for AccountRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for AccountRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "super" => Ok(Self::Super),
            "admin" => Ok(Self::Admin),
            other => Err(format!("invalid role: {other}")),
        }
    }
}

// ── Job / API enums ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum JobSource {
    #[default]
    Api,
    Test,
    /// Capacity analyzer LLM inference (VRAM probing, batch analysis).
    Analyzer,
}

/// Which API format the inbound request arrived via.
///
/// The discriminator is the matched route path — no header convention is used.
/// This is recorded on every queued job for per-API analytics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    /// POST /v1/chat/completions (OpenAI SDK, qwen-code, etc.)
    #[default]
    OpenaiCompat,
    /// POST /api/chat or /api/generate (OLLAMA_HOST=veronex clients)
    OllamaNative,
    /// POST /v1beta/models/{model}:generateContent (Gemini CLI, google-generativeai SDK)
    GeminiNative,
    /// POST /v1/inference (Veronex native SDK)
    VeronexNative,
}

impl ApiFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenaiCompat => "openai_compat",
            Self::OllamaNative => "ollama_native",
            Self::GeminiNative => "gemini_native",
            Self::VeronexNative => "veronex_native",
        }
    }
}

impl std::str::FromStr for ApiFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "openai_compat" => Ok(Self::OpenaiCompat),
            "ollama_native" => Ok(Self::OllamaNative),
            "gemini_native" => Ok(Self::GeminiNative),
            "veronex_native" => Ok(Self::VeronexNative),
            _ => Err(format!("unknown ApiFormat: {s}")),
        }
    }
}


impl JobSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Test => "test",
            Self::Analyzer => "analyzer",
        }
    }
}

impl std::str::FromStr for JobSource {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "api" => Ok(Self::Api),
            "test" => Ok(Self::Test),
            "analyzer" => Ok(Self::Analyzer),
            other => Err(format!("invalid job source: {other}")),
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    Ollama,
    Gemini,
}

impl ProviderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::Gemini => "gemini",
        }
    }

    /// Audit trail resource type string for this provider type.
    pub fn resource_type(&self) -> &'static str {
        match self {
            Self::Ollama => "ollama_provider",
            Self::Gemini => "gemini_provider",
        }
    }
}

impl std::str::FromStr for ProviderType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ollama" => Ok(Self::Ollama),
            "gemini" => Ok(Self::Gemini),
            other => Err(format!("unknown provider type: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum FinishReason {
    Stop,
    Length,
    Cancelled,
    Error,
}

impl FinishReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl std::str::FromStr for JobStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(format!("unknown job status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum LlmProviderStatus {
    Online,
    Offline,
    Degraded,
}

impl LlmProviderStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Offline => "offline",
            Self::Degraded => "degraded",
        }
    }
}

/// Five-level thermal throttling state for GPU/APU providers.
///
/// Updated by the health checker every 30 s; read by the queue dispatcher
/// on every job dispatch to gate concurrency limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleLevel {
    /// Temp < normal_below — full concurrency allowed.
    Normal,
    /// soft_at ≤ temp < hard_at — new requests blocked (503).
    Soft,
    /// temp ≥ hard_at — all blocked, forced drain after 60s.
    Hard,
    /// After Hard: waiting cooldown_secs for hardware to cool down. No new requests.
    Cooldown,
    /// After Cooldown: gradually restoring. New requests allowed, max_concurrent=1.
    RampUp,
}

// ── API Key enums ────────────────────────────────────────────────────────────

/// Billing tier for an API key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum KeyTier {
    Free,
    #[default]
    Paid,
}

impl KeyTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Paid => "paid",
        }
    }
}


impl std::fmt::Display for KeyTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for KeyTier {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "free" => Ok(Self::Free),
            "paid" => Ok(Self::Paid),
            other => Err(format!("invalid tier: {other}")),
        }
    }
}

/// API key category: standard (production) or test (dev/testing).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
#[serde(rename_all = "lowercase")]
pub enum KeyType {
    #[default]
    Standard,
    Test,
}

impl KeyType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Test => "test",
        }
    }

    pub fn is_test(&self) -> bool {
        matches!(self, Self::Test)
    }
}


impl std::fmt::Display for KeyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for KeyType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "standard" => Ok(Self::Standard),
            "test" => Ok(Self::Test),
            other => Err(format!("invalid key_type: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_permissions_count() {
        assert_eq!(ALL_PERMISSIONS.len(), 9);
    }

    #[test]
    fn all_menus_count() {
        assert_eq!(ALL_MENUS.len(), 12);
    }

    #[test]
    fn permission_as_str_roundtrip() {
        for &p in ALL_PERMISSIONS {
            assert!(!p.is_empty());
        }
    }

    #[test]
    fn menu_as_str_roundtrip() {
        for &m in ALL_MENUS {
            assert!(!m.is_empty());
        }
    }

    #[test]
    fn role_manage_in_all_permissions() {
        assert!(ALL_PERMISSIONS.contains(&"role_manage"));
    }

    // ── FromStr roundtrip tests ─────────────────────────────────────────

    #[test]
    fn account_role_roundtrip() {
        for role in &[AccountRole::Super, AccountRole::Admin] {
            let s = role.as_str();
            let parsed: AccountRole = s.parse().unwrap();
            assert_eq!(*role, parsed);
        }
    }

    #[test]
    fn account_role_invalid() {
        assert!("viewer".parse::<AccountRole>().is_err());
    }

    #[test]
    fn provider_type_roundtrip() {
        for pt in &[ProviderType::Ollama, ProviderType::Gemini] {
            let s = pt.as_str();
            let parsed: ProviderType = s.parse().unwrap();
            assert_eq!(*pt, parsed);
        }
    }

    #[test]
    fn job_status_roundtrip() {
        for status in &[JobStatus::Pending, JobStatus::Running, JobStatus::Completed, JobStatus::Failed, JobStatus::Cancelled] {
            let s = status.as_str();
            let parsed: JobStatus = s.parse().unwrap();
            assert_eq!(*status, parsed);
        }
    }

    #[test]
    fn job_source_roundtrip() {
        for src in &[JobSource::Api, JobSource::Test, JobSource::Analyzer] {
            let s = src.as_str();
            let parsed: JobSource = s.parse().unwrap();
            assert_eq!(*src, parsed);
        }
    }

    #[test]
    fn api_format_roundtrip() {
        for fmt in &[ApiFormat::OpenaiCompat, ApiFormat::OllamaNative, ApiFormat::GeminiNative, ApiFormat::VeronexNative] {
            let s = fmt.as_str();
            let parsed: ApiFormat = s.parse().unwrap();
            assert_eq!(*fmt, parsed);
        }
    }

    #[test]
    fn key_tier_roundtrip() {
        for tier in &[KeyTier::Free, KeyTier::Paid] {
            let s = tier.as_str();
            let parsed: KeyTier = s.parse().unwrap();
            assert_eq!(*tier, parsed);
        }
    }

    #[test]
    fn key_type_roundtrip() {
        for kt in &[KeyType::Standard, KeyType::Test] {
            let s = kt.as_str();
            let parsed: KeyType = s.parse().unwrap();
            assert_eq!(*kt, parsed);
        }
    }

    #[test]
    fn key_type_is_test() {
        assert!(!KeyType::Standard.is_test());
        assert!(KeyType::Test.is_test());
    }

    #[test]
    fn provider_type_resource_type() {
        assert_eq!(ProviderType::Ollama.resource_type(), "ollama_provider");
        assert_eq!(ProviderType::Gemini.resource_type(), "gemini_provider");
    }
}
