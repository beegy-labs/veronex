use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub event_time: DateTime<Utc>,
    pub account_id: Uuid,
    pub account_name: String,
    /// `"create"` | `"update"` | `"delete"` | `"login"` | `"logout"` | `"reset_password"`
    pub action: String,
    /// `"api_key"` | `"ollama_backend"` | `"gemini_backend"` | `"account"` | `"gpu_server"`
    pub resource_type: String,
    pub resource_id: String,
    pub resource_name: String,
    pub ip_address: Option<String>,
    pub details: Option<String>,
}

#[async_trait]
pub trait AuditPort: Send + Sync {
    async fn record(&self, event: AuditEvent);
}
