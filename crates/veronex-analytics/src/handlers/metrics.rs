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
struct ChipRow {
    chip: String,
}

#[derive(Debug, Deserialize, clickhouse::Row)]
struct ServerMetricsHistoryRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    ts: time::OffsetDateTime,
    mem_total_mb: f64,
    mem_avail_mb: f64,
    gpu_temp_c: f64,
    gpu_temp_junction_c: f64,
    gpu_temp_mem_c: f64,
    gpu_power_w: f64,
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
}

/// `GET /internal/metrics/history/{server_id}?hours=`
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

    // Step 1: find amdgpu chip label
    let chip_rows = state
        .ch
        .query(
            "SELECT DISTINCT attributes['chip'] AS chip
             FROM otel_metrics_gauge
             WHERE metric_name = 'node_hwmon_chip_names'
               AND attributes['chip_name'] = 'amdgpu'
               AND server_id = ?
             LIMIT 1",
        )
        .bind(&server_id_str)
        .fetch_all::<ChipRow>()
        .await
        .unwrap_or_default();

    let gpu_chip = chip_rows.into_iter().next().map(|r| r.chip).unwrap_or_default();

    // Step 2: pivot query — edge(temp1), junction(temp2), memory(temp3)
    let query_str = format!(
        "SELECT
            toStartOfInterval(ts, INTERVAL {bucket_interval}) AS ts,
            toFloat64(maxIf(value, metric_name IN ('node_memory_MemTotal_bytes', 'node_memory_total_bytes')) / 1048576.0) AS mem_total_mb,
            toFloat64(avgIf(value, metric_name IN ('node_memory_MemAvailable_bytes', 'node_memory_free_bytes')) / 1048576.0) AS mem_avail_mb,
            avgIf(value,
                metric_name = 'node_hwmon_temp_celsius'
                AND attributes['chip'] = ?
                AND attributes['sensor'] = 'temp1') AS gpu_temp_c,
            avgIf(value,
                metric_name = 'node_hwmon_temp_celsius'
                AND attributes['chip'] = ?
                AND attributes['sensor'] = 'temp2') AS gpu_temp_junction_c,
            avgIf(value,
                metric_name = 'node_hwmon_temp_celsius'
                AND attributes['chip'] = ?
                AND attributes['sensor'] = 'temp3') AS gpu_temp_mem_c,
            avgIf(value,
                metric_name IN ('node_hwmon_power_average_watt', 'node_hwmon_power_average_watts')
                AND attributes['chip'] = ?) AS gpu_power_w
        FROM otel_metrics_gauge
        WHERE server_id = ?
          AND ts >= now() - INTERVAL ? HOUR
        GROUP BY ts
        ORDER BY ts"
    );

    let rows = state
        .ch
        .query(&query_str)
        .bind(&gpu_chip)
        .bind(&gpu_chip)
        .bind(&gpu_chip)
        .bind(&gpu_chip)
        .bind(&server_id_str)
        .bind(hours)
        .fetch_all::<ServerMetricsHistoryRow>()
        .await
        .map_err(|e| {
            tracing::warn!(%server_id, "metrics history query failed: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let points: Vec<ServerMetricsPoint> = rows
        .into_iter()
        .map(|r| ServerMetricsPoint {
            ts: r
                .ts
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            mem_total_mb: r.mem_total_mb as u64,
            mem_avail_mb: r.mem_avail_mb as u64,
            gpu_temp_c: if r.gpu_temp_c > 0.0 { Some(r.gpu_temp_c) } else { None },
            gpu_temp_junction_c: if r.gpu_temp_junction_c > 0.0 { Some(r.gpu_temp_junction_c) } else { None },
            gpu_temp_mem_c: if r.gpu_temp_mem_c > 0.0 { Some(r.gpu_temp_mem_c) } else { None },
            gpu_power_w: if r.gpu_power_w > 0.0 { Some(r.gpu_power_w) } else { None },
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

    /// Concrete boundary examples kept as documentation.
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

        /// Clamp always produces a value in [1, 1440].
        #[test]
        fn hours_clamp_always_in_range(hours in 0u32..=100000) {
            let clamped = hours.clamp(1, 1440);
            prop_assert!(clamped >= 1);
            prop_assert!(clamped <= 1440);
        }
    }
}
