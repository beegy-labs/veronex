//! GET /health probe for llama-server.
//!
//! llama.cpp's `/health` endpoint shape varies across versions. We accept the
//! superset and treat missing fields as zero/empty:
//!
//! ```json
//! { "status": "ok", "slots_idle": 4, "slots_processing": 0 }
//! ```
//!
//! Older builds return only `{"status":"ok"}`; some MR builds expose
//! `slots_idle`/`slots_processing` only when launched with `--metrics`. Callers
//! that need authoritative slot counts should use GET /slots instead.

use std::time::Duration;

use anyhow::{Context as _, Result};
use serde::Deserialize;

/// Snapshot of a llama-server's `/health` response.
///
/// Conservative parser: any missing numeric field defaults to 0. `status` may
/// be absent in newer builds where HTTP 200 alone signals readiness.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct SlotStatus {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub slots_idle: u32,
    #[serde(default)]
    pub slots_processing: u32,
}

impl SlotStatus {
    /// True iff the server is accepting requests. We accept either the explicit
    /// `"ok"` status or an empty status (older builds where HTTP 200 implies ok).
    pub fn is_ok(&self) -> bool {
        self.status.is_empty() || self.status.eq_ignore_ascii_case("ok")
    }

    /// Total slots configured (`--parallel`).
    pub fn total_slots(&self) -> u32 {
        self.slots_idle + self.slots_processing
    }
}

/// Default timeout for the `/health` HTTP call. llama-server should respond
/// effectively instantly when alive; 5s leaves headroom for a saturated reverse
/// proxy without confusing dead with slow.
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);

/// Issue `GET {base_url}/health` and parse the response.
///
/// Returns `Err` on transport failure, non-2xx status, or JSON parse error.
/// Callers are expected to map this into `ModelInstanceState::NotLoaded` or a
/// score of 0 in the router (see `provider_router::pick_best_provider`).
pub async fn get_health(client: &reqwest::Client, base_url: &str) -> Result<SlotStatus> {
    let url = format!("{}/health", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .timeout(HEALTH_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("non-2xx from {url}"))?;
    let status: SlotStatus = resp.json().await.with_context(|| "parse /health JSON")?;
    Ok(status)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn slot_status_is_ok_explicit() {
        let s = SlotStatus { status: "ok".into(), ..Default::default() };
        assert!(s.is_ok());
    }

    #[test]
    fn slot_status_is_ok_empty_treated_as_alive() {
        // Some llama-server builds omit `status` and rely on HTTP 200.
        let s = SlotStatus::default();
        assert!(s.is_ok());
    }

    #[test]
    fn slot_status_is_ok_case_insensitive() {
        let s = SlotStatus { status: "OK".into(), ..Default::default() };
        assert!(s.is_ok());
    }

    #[test]
    fn slot_status_total_slots_sums_idle_and_processing() {
        let s = SlotStatus { slots_idle: 3, slots_processing: 1, ..Default::default() };
        assert_eq!(s.total_slots(), 4);
    }

    #[tokio::test]
    #[ignore = "sandbox blocks mock HTTP port binding"]
    async fn get_health_parses_full_response() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/health"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "ok",
                "slots_idle": 4,
                "slots_processing": 0,
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let s = get_health(&client, &server.uri()).await.unwrap();
        assert_eq!(s, SlotStatus { status: "ok".into(), slots_idle: 4, slots_processing: 0 });
    }

    #[tokio::test]
    #[ignore = "sandbox blocks mock HTTP port binding"]
    async fn get_health_parses_minimal_response() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/health"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "ok"
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let s = get_health(&client, &server.uri()).await.unwrap();
        assert!(s.is_ok());
        assert_eq!(s.total_slots(), 0); // missing fields default to 0
    }

    #[tokio::test]
    #[ignore = "sandbox blocks mock HTTP port binding"]
    async fn get_health_errors_on_5xx() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/health"))
            .respond_with(wiremock::ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let r = get_health(&client, &server.uri()).await;
        assert!(r.is_err());
    }
}
