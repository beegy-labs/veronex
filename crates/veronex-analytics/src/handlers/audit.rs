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

    // Build optional filter conditions using ClickHouse parameterised queries.
    // We construct the SQL dynamically since ClickHouse doesn't support NULL
    // parameter binding for conditional WHERE clauses well.
    let mut conditions = vec![
        "LogAttributes['event.name'] = 'audit.action'".to_string(),
    ];
    if let Some(ref action) = q.action {
        conditions.push(format!(
            "LogAttributes['action'] = '{}'",
            action.replace('\'', "''")
        ));
    }
    if let Some(ref rt) = q.resource_type {
        conditions.push(format!(
            "LogAttributes['resource_type'] = '{}'",
            rt.replace('\'', "''")
        ));
    }
    let where_clause = conditions.join(" AND ");

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
