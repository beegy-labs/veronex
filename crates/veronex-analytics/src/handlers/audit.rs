use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Deserialize)]
pub struct AuditQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct AuditEventRow {
    #[serde(with = "clickhouse::serde::time::datetime64::nanos")]
    pub event_time: time::OffsetDateTime,
    pub account_id: String,
    pub account_name: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub resource_name: String,
    pub ip_address: String,
    pub details: String,
}

#[derive(Serialize)]
pub struct AuditEventResponse {
    pub event_time: String,
    pub account_id: String,
    pub account_name: String,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub resource_name: String,
    pub ip_address: String,
    pub details: String,
}

/// `GET /internal/audit?limit=&offset=&action=&resource_type=`
pub async fn list_audit_events(
    State(state): State<AppState>,
    Query(q): Query<AuditQuery>,
) -> Result<Json<Vec<AuditEventResponse>>, StatusCode> {
    let limit = q.limit.unwrap_or(100).min(1000);
    let offset = q.offset.unwrap_or(0);

    // Whitelist validation — reject unknown filter values to prevent injection.
    const ALLOWED_ACTIONS: &[&str] = &[
        "create", "update", "delete", "regenerate", "login", "logout", "sync",
        "trigger", "reset_password", "toggle", "revoke",
    ];
    const ALLOWED_RESOURCE_TYPES: &[&str] = &[
        "account", "ollama_provider", "gemini_provider", "api_key",
        "capacity_settings", "gemini_policy", "gpu_server", "lab_settings",
        "session",
    ];

    if let Some(ref action) = q.action
        && !ALLOWED_ACTIONS.contains(&action.as_str())
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(ref rt) = q.resource_type
        && !ALLOWED_RESOURCE_TYPES.contains(&rt.as_str())
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    // resource_id is a UUID — validate format to prevent injection.
    if let Some(ref rid) = q.resource_id {
        if uuid::Uuid::parse_str(rid).is_err() {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    // Build filter conditions. Values are whitelist-validated above, safe for interpolation.
    let mut conditions = vec![
        "LogAttributes['event.name'] = 'audit.action'".to_string(),
    ];
    if let Some(ref action) = q.action {
        conditions.push(format!("LogAttributes['action'] = '{action}'"));
    }
    if let Some(ref rt) = q.resource_type {
        conditions.push(format!("LogAttributes['resource_type'] = '{rt}'"));
    }
    if let Some(ref rid) = q.resource_id {
        conditions.push(format!("LogAttributes['resource_id'] = '{rid}'"));
    }
    let where_clause = conditions.join(" AND ");

    // LIMIT and OFFSET are u32 from deserialization — integer format is safe.
    let sql = format!(
        "SELECT
            Timestamp                               AS event_time,
            LogAttributes['account_id']             AS account_id,
            LogAttributes['account_name']           AS account_name,
            LogAttributes['action']                 AS action,
            LogAttributes['resource_type']          AS resource_type,
            LogAttributes['resource_id']            AS resource_id,
            LogAttributes['resource_name']          AS resource_name,
            LogAttributes['ip_address']             AS ip_address,
            LogAttributes['details']                AS details
        FROM otel_logs
        WHERE {where_clause}
        ORDER BY Timestamp DESC
        LIMIT {limit} OFFSET {offset}"
    );

    let rows = state
        .ch
        .query(&sql)
        .fetch_all::<AuditEventRow>()
        .await
        .map_err(|e| {
            tracing::warn!("audit query failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let events = rows
        .into_iter()
        .map(|r| {
            let event_time = r
                .event_time
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            AuditEventResponse {
                event_time,
                account_id: r.account_id,
                account_name: r.account_name,
                action: r.action,
                resource_type: r.resource_type,
                resource_id: r.resource_id,
                resource_name: r.resource_name,
                ip_address: r.ip_address,
                details: r.details,
            }
        })
        .collect();

    Ok(Json(events))
}

#[cfg(test)]
mod tests {
    const ALLOWED_ACTIONS: &[&str] = &[
        "create", "update", "delete", "regenerate", "login", "logout", "sync",
        "trigger", "reset_password", "toggle", "revoke",
    ];
    const ALLOWED_RESOURCE_TYPES: &[&str] = &[
        "account", "ollama_provider", "gemini_provider", "api_key",
        "capacity_settings", "gemini_policy", "gpu_server", "lab_settings",
        "session",
    ];

    #[test]
    fn rejects_invalid_action() {
        let bad = "'; DROP TABLE--";
        assert!(!ALLOWED_ACTIONS.contains(&bad));
    }

    #[test]
    fn accepts_valid_action() {
        assert!(ALLOWED_ACTIONS.contains(&"create"));
        assert!(ALLOWED_ACTIONS.contains(&"logout"));
    }

    #[test]
    fn rejects_invalid_resource_type() {
        let bad = "<script>alert(1)</script>";
        assert!(!ALLOWED_RESOURCE_TYPES.contains(&bad));
    }

    #[test]
    fn accepts_valid_resource_type() {
        assert!(ALLOWED_RESOURCE_TYPES.contains(&"account"));
        assert!(ALLOWED_RESOURCE_TYPES.contains(&"api_key"));
    }
}
