use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct MetricsHistoryQuery {
    pub hours: Option<u32>,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct ServerMetricsHistoryRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    ts: time::OffsetDateTime,
    mem_total_mb: f64,
    mem_avail_mb: f64,
    gpu_temp_edge_c: f64,
    gpu_temp_junction_c: f64,
    gpu_temp_mem_c: f64,
    gpu_power_w: f64,
    cpu_usage_pct: f64,
    cpu_temp_c: f64,
}

#[derive(Debug, Serialize)]
pub struct ServerMetricsPoint {
    pub ts: String,
    pub mem_total_mb: u64,
    pub mem_avail_mb: u64,
    pub gpu_temp_c: Option<f64>,
    pub gpu_temp_junction_c: Option<f64>,
    pub gpu_temp_mem_c: Option<f64>,
    pub gpu_power_w: Option<f64>,
    pub cpu_usage_pct: Option<f64>,
    pub cpu_temp_c: Option<f64>,
}

/// `GET /internal/metrics/history/{server_id}?hours=`
///
/// Uses agent-enriched `hw_type`/`hw_role` labels for hardware-agnostic queries.
/// Works with any vendor: AMD (amdgpu/k10temp), Intel (coretemp), NVIDIA (nouveau).
pub async fn get_server_metrics_history(
    State(state): State<AppState>,
    Path(server_id): Path<Uuid>,
    Query(params): Query<MetricsHistoryQuery>,
) -> Result<Json<Vec<ServerMetricsPoint>>, StatusCode> {
    let hours = params.hours.unwrap_or(1).clamp(1, 1440);
    let server_id_str = server_id.to_string();

    let bucket_interval = if hours <= 24 {
        "1 MINUTE"
    } else if hours <= 168 {
        "5 MINUTE"
    } else {
        "60 MINUTE"
    };

    // Query uses hw_type/hw_vendor/hw_role labels enriched by veronex-agent.
    // No chip_name hardcoding — works with any hardware vendor.
    let query_str = format!(
        "WITH cpu_buckets AS (
            SELECT
                toStartOfInterval(ts, INTERVAL {bucket_interval}) AS bucket,
                sumIf(value, attributes['mode'] = 'idle') AS idle_total,
                sum(value) AS all_total
            FROM otel_metrics_gauge
            WHERE metric_name = 'node_cpu_seconds_total'
              AND server_id = ?
              AND ts >= now() - INTERVAL ? HOUR
            GROUP BY bucket
            ORDER BY bucket
        ),
        cpu_pct AS (
            SELECT
                bucket,
                if(
                    all_total - lagInFrame(all_total) OVER (ORDER BY bucket) > 0,
                    (1.0 - (idle_total - lagInFrame(idle_total) OVER (ORDER BY bucket))
                          / (all_total - lagInFrame(all_total) OVER (ORDER BY bucket))) * 100.0,
                    0
                ) AS usage_pct
            FROM cpu_buckets
        )
        SELECT
            toStartOfInterval(g.ts, INTERVAL {bucket_interval}) AS ts,
            -- Memory
            toFloat64(maxIf(g.value, g.metric_name IN ('node_memory_MemTotal_bytes', 'node_memory_total_bytes')) / 1048576.0) AS mem_total_mb,
            toFloat64(avgIf(g.value, g.metric_name IN ('node_memory_MemAvailable_bytes', 'node_memory_free_bytes')) / 1048576.0) AS mem_avail_mb,
            -- GPU temperature (hw_type=gpu): edge, junction, memory
            avgIf(g.value,
                g.metric_name = 'node_hwmon_temp_celsius'
                AND g.attributes['hw_type'] = 'gpu'
                AND g.attributes['hw_role'] = 'temp_edge') AS gpu_temp_edge_c,
            avgIf(g.value,
                g.metric_name = 'node_hwmon_temp_celsius'
                AND g.attributes['hw_type'] = 'gpu'
                AND g.attributes['hw_role'] = 'temp_junction') AS gpu_temp_junction_c,
            avgIf(g.value,
                g.metric_name = 'node_hwmon_temp_celsius'
                AND g.attributes['hw_type'] = 'gpu'
                AND g.attributes['hw_role'] = 'temp_mem') AS gpu_temp_mem_c,
            -- GPU power (hw_type=gpu)
            avgIf(g.value,
                g.metric_name IN ('node_hwmon_power_average_watt', 'node_hwmon_power_average_watts', 'node_hwmon_power_watt')
                AND g.attributes['hw_type'] = 'gpu') AS gpu_power_w,
            -- CPU usage % (from counter deltas)
            coalesce(c.usage_pct, 0) AS cpu_usage_pct,
            -- CPU temperature (hw_type=cpu, hw_role=temp_package)
            avgIf(g.value,
                g.metric_name = 'node_hwmon_temp_celsius'
                AND g.attributes['hw_type'] = 'cpu'
                AND g.attributes['hw_role'] = 'temp_package') AS cpu_temp_c
        FROM otel_metrics_gauge g
        LEFT JOIN cpu_pct c ON c.bucket = toStartOfInterval(g.ts, INTERVAL {bucket_interval})
        WHERE g.server_id = ?
          AND g.ts >= now() - INTERVAL ? HOUR
          AND g.metric_name != 'node_cpu_seconds_total'
        GROUP BY ts, c.usage_pct
        HAVING ts > toDateTime64(0, 9)
        ORDER BY ts"
    );

    let rows = state
        .ch
        .query(&query_str)
        .bind(&server_id_str)  // cpu_buckets server_id
        .bind(hours)           // cpu_buckets hours
        .bind(&server_id_str)  // main query server_id
        .bind(hours)           // main query hours
        .fetch_all::<ServerMetricsHistoryRow>()
        .await
        .map_err(|e| {
            tracing::warn!(%server_id, "metrics history query failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let points: Vec<ServerMetricsPoint> = rows
        .into_iter()
        .map(|r| {
            let opt = |v: f64| if v > 0.0 { Some((v * 10.0).round() / 10.0) } else { None };
            ServerMetricsPoint {
                ts: r.ts.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
                mem_total_mb: r.mem_total_mb as u64,
                mem_avail_mb: r.mem_avail_mb as u64,
                gpu_temp_c: opt(r.gpu_temp_edge_c),
                gpu_temp_junction_c: opt(r.gpu_temp_junction_c),
                gpu_temp_mem_c: opt(r.gpu_temp_mem_c),
                gpu_power_w: opt(r.gpu_power_w),
                cpu_usage_pct: opt(r.cpu_usage_pct),
                cpu_temp_c: opt(r.cpu_temp_c),
            }
        })
        .collect();

    Ok(Json(points))
}

/// Select bucket interval based on the requested hour range.
#[cfg(test)]
fn bucket_interval(hours: u32) -> &'static str {
    if hours <= 24 {
        "1 MINUTE"
    } else if hours <= 168 {
        "5 MINUTE"
    } else {
        "60 MINUTE"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn bucket_interval_boundary_examples() {
        assert_eq!(bucket_interval(1), "1 MINUTE");
        assert_eq!(bucket_interval(24), "1 MINUTE");
        assert_eq!(bucket_interval(25), "5 MINUTE");
        assert_eq!(bucket_interval(168), "5 MINUTE");
        assert_eq!(bucket_interval(169), "60 MINUTE");
    }

    proptest! {
        #[test]
        fn bucket_interval_1min_range(hours in 1u32..=24) {
            prop_assert_eq!(bucket_interval(hours), "1 MINUTE");
        }
        #[test]
        fn bucket_interval_5min_range(hours in 25u32..=168) {
            prop_assert_eq!(bucket_interval(hours), "5 MINUTE");
        }
        #[test]
        fn bucket_interval_60min_range(hours in 169u32..=10000) {
            prop_assert_eq!(bucket_interval(hours), "60 MINUTE");
        }
        #[test]
        fn hours_clamp_always_in_range(hours in 0u32..=100000) {
            let clamped = hours.clamp(1, 1440);
            prop_assert!(clamped >= 1);
            prop_assert!(clamped <= 1440);
        }
    }
}
