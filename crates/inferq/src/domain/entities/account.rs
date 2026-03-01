use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: Uuid,
    pub username: String,
    pub password_hash: String,
    pub name: String,
    pub email: Option<String>,
    /// `"super"` | `"admin"`
    pub role: String,
    pub department: Option<String>,
    pub position: Option<String>,
    pub is_active: bool,
    pub created_by: Option<Uuid>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}
