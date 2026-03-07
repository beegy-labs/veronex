use chrono::Utc;
use uuid::Uuid;

use crate::application::ports::outbound::audit_port::AuditEvent;
use crate::infrastructure::inbound::http::middleware::jwt_auth::Claims;

use super::state::AppState;

/// Emit an audit event using JWT claims as the actor identity.
///
/// No-op when the audit port is not configured (i.e. `state.audit_port` is `None`).
pub async fn emit_audit(
    state: &AppState,
    actor: &Claims,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    resource_name: &str,
    details: &str,
) {
    emit_audit_raw(
        state,
        actor.sub,
        &actor.sub.to_string(),
        action,
        resource_type,
        resource_id,
        resource_name,
        details,
    )
    .await;
}

/// Emit an audit event with an explicit account ID and name (for pre-auth flows
/// like login / password reset where JWT claims are not yet available).
///
/// No-op when the audit port is not configured.
#[allow(clippy::too_many_arguments)]
pub async fn emit_audit_raw(
    state: &AppState,
    account_id: Uuid,
    account_name: &str,
    action: &str,
    resource_type: &str,
    resource_id: &str,
    resource_name: &str,
    details: &str,
) {
    if let Some(ref port) = state.audit_port {
        port.record(AuditEvent {
            event_time: Utc::now(),
            account_id,
            account_name: account_name.to_string(),
            action: action.to_string(),
            resource_type: resource_type.to_string(),
            resource_id: resource_id.to_string(),
            resource_name: resource_name.to_string(),
            ip_address: None,
            details: Some(details.to_string()),
        })
        .await;
    }
}
