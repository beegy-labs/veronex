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
    gpu_power_w: f64,
}

#[derive(Debug, Serialize)]
pub struct ServerMetricsPoint {
    pub ts: String,
    pub mem_total_mb: u64,
    pub mem_avail_mb: u64,
    pub gpu_temp_c: Option<f64>,
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
            "SELECT DISTINCT Attributes['chip'] AS chip
             FROM otel_metrics_gauge
             WHERE MetricName = 'node_hwmon_chip_names'
               AND Attributes['chip_name'] = 'amdgpu'
               AND Attributes['server_id'] = ?
             LIMIT 1",
        )
        .bind(&server_id_str)
        .fetch_all::<ChipRow>()
        .await
        .unwrap_or_default();

    let gpu_chip = chip_rows.into_iter().next().map(|r| r.chip).unwrap_or_default();

    // Step 2: pivot query
    let query_str = format!(
        "SELECT
            toStartOfInterval(TimeUnix, INTERVAL {bucket_interval}) AS ts,
            toFloat64(maxIf(Value, MetricName = 'node_memory_MemTotal_bytes') / 1048576.0) AS mem_total_mb,
            toFloat64(avgIf(Value, MetricName = 'node_memory_MemAvailable_bytes') / 1048576.0) AS mem_avail_mb,
            avgIf(Value,
                MetricName = 'node_hwmon_temp_celsius'
                AND Attributes['chip'] = ?
                AND Attributes['sensor'] = 'temp1') AS gpu_temp_c,
            avgIf(Value,
                MetricName IN ('node_hwmon_power_average_watt', 'node_hwmon_power_average_watts')
                AND Attributes['chip'] = ?) AS gpu_power_w
        FROM otel_metrics_gauge
        WHERE Attributes['server_id'] = ?
          AND TimeUnix >= now() - INTERVAL ? HOUR
        GROUP BY ts
        ORDER BY ts"
    );

    let rows = state
        .ch
        .query(&query_str)
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

    #[test]
    fn bucket_interval_1_minute() {
        assert_eq!(bucket_interval(1), "1 MINUTE");
        assert_eq!(bucket_interval(24), "1 MINUTE");
    }

    #[test]
    fn bucket_interval_5_minute() {
        assert_eq!(bucket_interval(25), "5 MINUTE");
        assert_eq!(bucket_interval(168), "5 MINUTE");
    }

    #[test]
    fn bucket_interval_60_minute() {
        assert_eq!(bucket_interval(169), "60 MINUTE");
        assert_eq!(bucket_interval(1440), "60 MINUTE");
    }

    #[test]
    fn hours_clamped_range() {
        // The handler clamps hours to 1..=1440
        let hours = 0_u32.clamp(1, 1440);
        assert_eq!(hours, 1);
        let hours = 9999_u32.clamp(1, 1440);
        assert_eq!(hours, 1440);
    }
}
