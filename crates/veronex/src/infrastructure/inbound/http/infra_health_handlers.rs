use std::collections::HashMap;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::infrastructure::outbound::valkey_keys;
use super::error::AppError;
use super::middleware::jwt_auth::RequireDashboardView;
use super::state::AppState;

// ── Service health (infrastructure + pods) ────────────────────────

#[derive(Serialize)]
pub struct ServiceHealthResponse {
    pub infrastructure: Vec<ServiceStatus>,
    pub api_pods: Vec<PodStatus>,
    pub agent_pods: Vec<PodStatus>,
}

#[derive(Serialize)]
pub struct ServiceStatus {
    pub name: String,
    /// "ok" | "degraded" | "unavailable"
    pub status: String,
    pub latency_ms: Option<u32>,
    pub checked_at: Option<i64>,
}

#[derive(Serialize)]
pub struct PodStatus {
    pub id: String,
    /// "online" | "offline"
    pub status: String,
    pub last_heartbeat_ms: Option<i64>,
}

/// Compact JSON stored by health_checker in per-instance HASH.
#[derive(serde::Deserialize)]
struct SvcProbeEntry {
    s: String,
    ms: u32,
    t: i64,
}

/// GET /v1/dashboard/services — Infrastructure services + HPA pod status.
pub async fn get_service_health(
    State(state): State<AppState>,
) -> Result<Json<ServiceHealthResponse>, AppError> {
    use fred::prelude::*;

    let pool = state.valkey_pool.as_ref()
        .ok_or_else(|| AppError::ServiceUnavailable("Valkey not configured".into()))?;

    // ── 1. Infrastructure: merge service probes from all pods ──────
    let instance_ids: Vec<String> = pool.smembers(valkey_keys::instances_set()).await
        .unwrap_or_default();

    let mut all_probes: HashMap<String, Vec<SvcProbeEntry>> = HashMap::new();
    for iid in &instance_ids {
        let entries: HashMap<String, String> = pool
            .hgetall(valkey_keys::service_health(iid))
            .await
            .unwrap_or_default();
        for (svc_name, json) in entries {
            if let Ok(probe) = serde_json::from_str::<SvcProbeEntry>(&json) {
                all_probes.entry(svc_name).or_default().push(probe);
            }
        }
    }

    let svc_names = ["postgresql", "valkey", "clickhouse", "s3", "vespa", "embed"];
    let infrastructure: Vec<ServiceStatus> = svc_names.iter().filter_map(|name| {
        let probes = all_probes.get(*name)?;
        let ok_count = probes.iter().filter(|p| p.s == "ok").count();
        let status = if ok_count == probes.len() {
            "ok"
        } else if ok_count > 0 {
            "degraded"
        } else {
            "unavailable"
        };
        let latest = probes.iter().max_by_key(|p| p.t)?;
        Some(ServiceStatus {
            name: name.to_string(),
            status: status.to_string(),
            latency_ms: Some(latest.ms),
            checked_at: Some(latest.t),
        })
    }).collect();

    // ── 2. API pods: check heartbeat TTL ──────────────────────────
    let api_pods: Vec<PodStatus> = {
        let mut pods = Vec::with_capacity(instance_ids.len());
        let now_ms = chrono::Utc::now().timestamp_millis();
        for iid in &instance_ids {
            let ttl: i64 = pool.ttl(valkey_keys::heartbeat(iid)).await.unwrap_or(-2);
            if ttl <= 0 {
                let _: Result<(), _> = pool
                    .srem(valkey_keys::instances_set(), iid.as_str())
                    .await;
                continue;
            }
            let elapsed_ms = (30 - ttl) * 1000;
            pods.push(PodStatus {
                id: iid.clone(),
                status: "online".into(),
                last_heartbeat_ms: Some(now_ms - elapsed_ms),
            });
        }
        pods
    };

    // ── 3. Agent pods ───────────────────────────────────────────────
    let agent_ids: Vec<String> = pool
        .smembers(valkey_keys::agent_instances_set())
        .await
        .unwrap_or_default();

    let agent_pods: Vec<PodStatus> = {
        let mut pods = Vec::with_capacity(agent_ids.len());
        let now_ms = chrono::Utc::now().timestamp_millis();
        for hostname in &agent_ids {
            let hb_key = valkey_keys::agent_heartbeat(hostname);
            let ttl: i64 = pool.ttl(&hb_key).await.unwrap_or(-2);
            if ttl <= 0 {
                let _: Result<(), _> = pool
                    .srem(valkey_keys::agent_instances_set(), hostname.as_str())
                    .await;
                continue;
            }
            let elapsed_ms = (180 - ttl) * 1000;
            pods.push(PodStatus {
                id: hostname.clone(),
                status: "online".into(),
                last_heartbeat_ms: Some(now_ms - elapsed_ms),
            });
        }
        pods
    };

    Ok(Json(ServiceHealthResponse { infrastructure, api_pods, agent_pods }))
}

// ── ClickHouse HTTP query helper ──────────────────────────────────────────────

async fn ch_get(
    client: &reqwest::Client,
    base_url: &str,
    user: &str,
    password: &str,
    query: &str,
) -> Option<reqwest::Response> {
    let url = format!(
        "{base_url}/?user={}&password={}&query={}",
        percent_encode(user),
        percent_encode(password),
        percent_encode(query),
    );
    client.get(&url).send().await.ok()
        .filter(|r| r.status().is_success())
}

fn percent_encode(s: &str) -> String {
    s.bytes().flat_map(|b| {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' => vec![b as char],
            _ => format!("%{b:02X}").chars().collect(),
        }
    }).collect()
}

// ── Pipeline health (Kafka consumer lag + TPM) ────────────────────────────────

#[derive(Serialize)]
pub struct TopicPipelineStats {
    pub topic: String,
    pub consumer_offset: i64,
    pub log_end_offset: i64,
    pub lag: i64,
    pub tpm_1m: i64,
    pub tpm_5m: i64,
    pub last_poll_secs: Option<i64>,
    pub is_active: bool,
    pub last_error: Option<String>,
    pub consumer_count: u32,
}

#[derive(Serialize)]
pub struct PipelineHealthResponse {
    pub topics: Vec<TopicPipelineStats>,
    pub available: bool,
}

/// `GET /v1/dashboard/pipeline`
pub async fn get_pipeline_health(
    RequireDashboardView(_): RequireDashboardView,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let Some(ref redpanda_admin_url) = state.kafka_broker_admin_url else {
        return Json(PipelineHealthResponse { topics: vec![], available: false }).into_response();
    };
    let Some(ref ch_url) = state.clickhouse_http_url else {
        return Json(PipelineHealthResponse { topics: vec![], available: false }).into_response();
    };

    let ch_user = state.clickhouse_user.as_deref().unwrap_or("default");
    let ch_pass = state.clickhouse_password.as_deref().unwrap_or("");
    let ch_db   = state.clickhouse_db.as_deref().unwrap_or("veronex");

    // ── 1. Redpanda Prometheus metrics → high-watermark per topic ──────────
    let metrics_url = format!("{redpanda_admin_url}/metrics");
    let metrics_text = match state.http_client.get(&metrics_url).send().await {
        Ok(r) if r.status().is_success() => r.text().await.unwrap_or_default(),
        _ => String::new(),
    };

    let mut high_watermarks: HashMap<String, i64> = HashMap::new();
    for line in metrics_text.lines() {
        if !line.starts_with("vectorized_cluster_partition_high_watermark{") { continue }
        let topic = line
            .split("topic=\"").nth(1)
            .and_then(|s| s.split('"').next())
            .map(|s| s.to_string());
        let value = line.rsplit(' ').next()
            .and_then(|v| v.trim().parse::<f64>().ok())
            .map(|f| f as i64);
        if let (Some(t), Some(v)) = (topic, value) && t.starts_with("otel-") {
            high_watermarks.insert(t, v);
        }
    }

    // ── 2. ClickHouse → consumer offsets + last_poll + errors ──────────────
    let consumer_query = format!(
        "SELECT \
            table, \
            arrayElement(assignments.topic, 1) AS topic, \
            arrayElement(assignments.current_offset, 1) AS consumer_offset, \
            last_poll_time, \
            if(length(exceptions.text) > 0, \
               substring(arrayElement(exceptions.text, length(exceptions.text)), 1, 200), \
               '') AS last_error \
         FROM system.kafka_consumers \
         WHERE database='{ch_db}' \
           AND table IN ('kafka_otel_logs', 'kafka_otel_metrics') \
         FORMAT JSONEachRow"
    );

    let ch_consumer_resp = ch_get(&state.http_client, ch_url, ch_user, ch_pass, &consumer_query).await;

    #[derive(serde::Deserialize)]
    struct ChConsumerRow {
        topic: String,
        consumer_offset: i64,
        last_poll_time: String,
        last_error: String,
    }

    let mut consumer_map: HashMap<String, ChConsumerRow> = HashMap::new();
    if let Some(resp) = ch_consumer_resp && let Ok(body) = resp.text().await {
        for line in body.lines() {
            if let Ok(row) = serde_json::from_str::<ChConsumerRow>(line) {
                consumer_map.insert(row.topic.clone(), row);
            }
        }
    }

    // ── 3. ClickHouse → consumer count per topic ──────────────────────────
    let consumer_count_query = format!(
        "SELECT table, count() AS cnt \
         FROM system.kafka_consumers \
         WHERE database='{ch_db}' \
           AND table IN ('kafka_otel_logs', 'kafka_otel_metrics') \
         GROUP BY table \
         FORMAT JSONEachRow"
    );

    let ch_count_resp = ch_get(&state.http_client, ch_url, ch_user, ch_pass, &consumer_count_query).await;

    #[derive(serde::Deserialize)]
    struct ChCountRow {
        table: String,
        cnt: u32,
    }

    let table_to_topic = [
        ("kafka_otel_logs",    "otel-logs"),
        ("kafka_otel_metrics", "otel-metrics"),
    ];
    let mut consumer_count_map: HashMap<&str, u32> = HashMap::new();
    if let Some(resp) = ch_count_resp && let Ok(body) = resp.text().await {
        for line in body.lines() {
            if let Ok(row) = serde_json::from_str::<ChCountRow>(line)
                && let Some(&topic) = table_to_topic.iter().find(|(t, _)| *t == row.table).map(|(_, tp)| tp)
            {
                consumer_count_map.insert(topic, row.cnt);
            }
        }
    }

    // ── 4. ClickHouse → TPM ────────────────────────────────────────────────
    let tpm_query = format!(
        "SELECT 'otel-logs' AS topic, \
                countIf(timestamp >= now() - INTERVAL 1 MINUTE) AS t1m, \
                countIf(timestamp >= now() - INTERVAL 5 MINUTE) AS t5m \
         FROM {ch_db}.otel_logs \
         UNION ALL \
         SELECT 'otel-metrics', \
                countIf(timestamp >= now() - INTERVAL 1 MINUTE), \
                countIf(timestamp >= now() - INTERVAL 5 MINUTE) \
         FROM {ch_db}.otel_metrics \
         FORMAT JSONEachRow"
    );

    let ch_tpm_resp = ch_get(&state.http_client, ch_url, ch_user, ch_pass, &tpm_query).await;

    #[derive(serde::Deserialize)]
    struct ChTpmRow {
        topic: String,
        t1m: i64,
        t5m: i64,
    }

    let mut tpm_map: HashMap<String, (i64, i64)> = HashMap::new();
    if let Some(resp) = ch_tpm_resp && let Ok(body) = resp.text().await {
        for line in body.lines() {
            if let Ok(row) = serde_json::from_str::<ChTpmRow>(line) {
                tpm_map.insert(row.topic, (row.t1m, row.t5m));
            }
        }
    }

    // ── 5. Assemble response ───────────────────────────────────────────────
    let now = chrono::Utc::now();
    let topics_config = [
        ("otel-logs",    "kafka_otel_logs"),
        ("otel-metrics", "kafka_otel_metrics"),
    ];

    let topics: Vec<TopicPipelineStats> = topics_config.iter().map(|(topic, _table)| {
        let log_end_offset = high_watermarks.get(*topic).copied().unwrap_or(0);

        let (consumer_offset, last_poll_secs, is_active, last_error) = if let Some(row) = consumer_map.get(*topic) {
            let last_poll_secs = chrono::NaiveDateTime::parse_from_str(&row.last_poll_time, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|dt| now.signed_duration_since(dt.and_utc()).num_seconds())
                .filter(|&s| s >= 0);
            let err = if row.last_error.is_empty() { None } else { Some(row.last_error.clone()) };
            let is_active = last_poll_secs.map(|s| s < 120).unwrap_or(false);
            (row.consumer_offset, last_poll_secs, is_active, err)
        } else {
            (0, None, false, None)
        };

        let lag = (log_end_offset - consumer_offset).max(0);
        let (tpm_1m, tpm_5m) = tpm_map.get(*topic).copied().unwrap_or((0, 0));
        let consumer_count = consumer_count_map.get(*topic).copied().unwrap_or(0);

        TopicPipelineStats {
            topic: topic.to_string(),
            consumer_offset,
            log_end_offset,
            lag,
            tpm_1m,
            tpm_5m,
            last_poll_secs,
            is_active,
            last_error,
            consumer_count,
        }
    }).collect();

    let available = !metrics_text.is_empty() || !consumer_map.is_empty();
    Json(PipelineHealthResponse { topics, available }).into_response()
}
