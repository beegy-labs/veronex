use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::domain::enums::AccountRole;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/lib/generated/")]
pub struct Account {
    pub id: Uuid,
    pub username: String,
    #[serde(skip_serializing)]
    #[ts(skip)]
    pub password_hash: String,
    pub name: String,
    pub email: Option<String>,
    pub role: AccountRole,
    pub department: Option<String>,
    pub position: Option<String>,
    pub is_active: bool,
    #[ts(skip)]
    pub created_by: Option<Uuid>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    #[ts(skip)]
    pub deleted_at: Option<DateTime<Utc>>,
}
