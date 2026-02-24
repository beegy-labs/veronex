use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::state::AppState;

// ── Performance types ───────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct LatencyStatsRow {
    pub avg_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub total_requests: u64,
    pub success_count: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct HourlyThroughputRow {
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub hour: time::OffsetDateTime,
    pub request_count: u64,
    pub success_count: u64,
    pub avg_latency_ms: f64,
    pub total_tokens: u64,
}

#[derive(Serialize)]
pub struct HourlyThroughputResponse {
    pub hour: String,
    pub request_count: u64,
    pub success_count: u64,
    pub avg_latency_ms: f64,
    pub total_tokens: u64,
}

#[derive(Serialize)]
pub struct PerformanceResponse {
    pub avg_latency_ms: f64,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub total_requests: u64,
    pub success_rate: f64,
    pub total_tokens: u64,
    pub hourly: Vec<HourlyThroughputResponse>,
}

// ── Query parameters ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct JobsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    pub status: Option<String>,
}

fn default_limit() -> i64 {
    50
}

// ── Response types ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct DashboardStats {
    pub total_keys: i64,
    pub active_keys: i64,
    pub total_jobs: i64,
    pub jobs_last_24h: i64,
    pub jobs_by_status: HashMap<String, i64>,
}

#[derive(Serialize)]
pub struct JobSummary {
    pub id: String,
    pub model_name: String,
    pub backend: String,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub latency_ms: Option<i64>,
}

#[derive(Serialize)]
pub struct JobsResponse {
    pub jobs: Vec<JobSummary>,
    pub total: i64,
}

// ── Handlers ───────────────────────────────────────────────────────

/// GET /v1/dashboard/stats — Overview statistics.
pub async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<DashboardStats>, StatusCode> {
    let pool = &state.pg_pool;

    // Key counts
    let key_row = sqlx::query(
        "SELECT
            COUNT(*) AS total_keys,
            COUNT(*) FILTER (WHERE is_active = true) AS active_keys
         FROM api_keys",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    use sqlx::Row;
    let total_keys: i64 = key_row.try_get("total_keys").unwrap_or(0);
    let active_keys: i64 = key_row.try_get("active_keys").unwrap_or(0);

    // Job counts
    let job_row = sqlx::query(
        "SELECT
            COUNT(*) AS total_jobs,
            COUNT(*) FILTER (WHERE created_at >= now() - interval '24 hours') AS jobs_last_24h
         FROM inference_jobs",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total_jobs: i64 = job_row.try_get("total_jobs").unwrap_or(0);
    let jobs_last_24h: i64 = job_row.try_get("jobs_last_24h").unwrap_or(0);

    // Jobs by status
    let status_rows = sqlx::query(
        "SELECT status, COUNT(*) AS cnt
         FROM inference_jobs
         GROUP BY status",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut jobs_by_status: HashMap<String, i64> = HashMap::new();
    // Ensure all known statuses are present
    for s in &["pending", "running", "completed", "failed", "cancelled"] {
        jobs_by_status.insert(s.to_string(), 0);
    }
    for row in status_rows {
        let status: String = row.try_get("status").unwrap_or_default();
        let cnt: i64 = row.try_get("cnt").unwrap_or(0);
        jobs_by_status.insert(status, cnt);
    }

    Ok(Json(DashboardStats {
        total_keys,
        active_keys,
        total_jobs,
        jobs_last_24h,
        jobs_by_status,
    }))
}

/// GET /v1/dashboard/jobs — Paginated job list.
pub async fn list_jobs(
    State(state): State<AppState>,
    Query(params): Query<JobsQuery>,
) -> Result<Json<JobsResponse>, StatusCode> {
    use sqlx::Row;
    let pool = &state.pg_pool;

    // Total count (with optional status filter)
    let total: i64 = if let Some(ref status) = params.status {
        let row = sqlx::query("SELECT COUNT(*) AS cnt FROM inference_jobs WHERE status = $1")
            .bind(status)
            .fetch_one(pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        row.try_get("cnt").unwrap_or(0)
    } else {
        let row = sqlx::query("SELECT COUNT(*) AS cnt FROM inference_jobs")
            .fetch_one(pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        row.try_get("cnt").unwrap_or(0)
    };

    // Paginated rows
    let rows = if let Some(ref status) = params.status {
        sqlx::query(
            "SELECT id, model_name, backend, status, created_at, completed_at,
                    CASE
                        WHEN completed_at IS NOT NULL
                        THEN EXTRACT(EPOCH FROM (completed_at - created_at)) * 1000
                        ELSE NULL
                    END AS latency_ms
             FROM inference_jobs
             WHERE status = $1
             ORDER BY created_at DESC
             LIMIT $2 OFFSET $3",
        )
        .bind(status)
        .bind(params.limit)
        .bind(params.offset)
        .fetch_all(pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        sqlx::query(
            "SELECT id, model_name, backend, status, created_at, completed_at,
                    CASE
                        WHEN completed_at IS NOT NULL
                        THEN EXTRACT(EPOCH FROM (completed_at - created_at)) * 1000
                        ELSE NULL
                    END AS latency_ms
             FROM inference_jobs
             ORDER BY created_at DESC
             LIMIT $1 OFFSET $2",
        )
        .bind(params.limit)
        .bind(params.offset)
        .fetch_all(pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    let jobs: Vec<JobSummary> = rows
        .iter()
        .map(|row| {
            let id: uuid::Uuid = row.try_get("id").unwrap_or_default();
            let model_name: String = row.try_get("model_name").unwrap_or_default();
            let backend: String = row.try_get("backend").unwrap_or_default();
            let status: String = row.try_get("status").unwrap_or_default();
            let created_at: chrono::DateTime<chrono::Utc> =
                row.try_get("created_at").unwrap_or_default();
            let completed_at: Option<chrono::DateTime<chrono::Utc>> =
                row.try_get("completed_at").unwrap_or(None);
            let latency_ms: Option<f64> = row.try_get("latency_ms").unwrap_or(None);

            JobSummary {
                id: id.to_string(),
                model_name,
                backend,
                status,
                created_at: created_at.to_rfc3339(),
                completed_at: completed_at.map(|dt| dt.to_rfc3339()),
                latency_ms: latency_ms.map(|v| v as i64),
            }
        })
        .collect();

    Ok(Json(JobsResponse { jobs, total }))
}

/// GET /v1/dashboard/performance — Latency percentiles + hourly throughput.
///
/// Queries ClickHouse `inference_logs` for P50/P95/P99 latency and
/// per-hour request/token throughput. Returns 503 if ClickHouse is disabled.
pub async fn get_performance(
    State(state): State<AppState>,
    Query(params): Query<super::usage_handlers::UsageQuery>,
) -> Result<Json<PerformanceResponse>, StatusCode> {
    let Some(ref client) = state.clickhouse_client else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };

    // Aggregate latency percentiles
    let stats = client
        .query(
            "SELECT
                avgOrDefault(latency_ms)                AS avg_latency_ms,
                quantileOrDefault(0.50)(latency_ms)     AS p50_latency_ms,
                quantileOrDefault(0.95)(latency_ms)     AS p95_latency_ms,
                quantileOrDefault(0.99)(latency_ms)     AS p99_latency_ms,
                count()                                 AS total_requests,
                countIf(finish_reason = 'stop')         AS success_count,
                sum(completion_tokens)                  AS total_tokens
            FROM inference_logs
            WHERE event_time >= now() - INTERVAL ? HOUR",
        )
        .bind(params.hours)
        .fetch_one::<LatencyStatsRow>()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Hourly throughput
    let hourly_rows = client
        .query(
            "SELECT
                toStartOfHour(event_time)               AS hour,
                count()                                 AS request_count,
                countIf(finish_reason = 'stop')         AS success_count,
                avgOrDefault(latency_ms)                AS avg_latency_ms,
                sum(completion_tokens)                  AS total_tokens
            FROM inference_logs
            WHERE event_time >= now() - INTERVAL ? HOUR
            GROUP BY hour
            ORDER BY hour ASC",
        )
        .bind(params.hours)
        .fetch_all::<HourlyThroughputRow>()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let success_rate = if stats.total_requests > 0 {
        stats.success_count as f64 / stats.total_requests as f64
    } else {
        0.0
    };

    let hourly = hourly_rows
        .into_iter()
        .map(|r| HourlyThroughputResponse {
            hour: r
                .hour
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            request_count: r.request_count,
            success_count: r.success_count,
            avg_latency_ms: r.avg_latency_ms,
            total_tokens: r.total_tokens,
        })
        .collect();

    Ok(Json(PerformanceResponse {
        avg_latency_ms: stats.avg_latency_ms,
        p50_latency_ms: stats.p50_latency_ms,
        p95_latency_ms: stats.p95_latency_ms,
        p99_latency_ms: stats.p99_latency_ms,
        total_requests: stats.total_requests,
        success_rate,
        total_tokens: stats.total_tokens,
        hourly,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_query_defaults() {
        let json = serde_json::json!({});
        let q: JobsQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.limit, 50);
        assert_eq!(q.offset, 0);
        assert!(q.status.is_none());
    }

    #[test]
    fn jobs_query_with_status() {
        let json = serde_json::json!({ "status": "completed", "limit": 10, "offset": 20 });
        let q: JobsQuery = serde_json::from_value(json).unwrap();
        assert_eq!(q.limit, 10);
        assert_eq!(q.offset, 20);
        assert_eq!(q.status.as_deref(), Some("completed"));
    }

    #[test]
    fn dashboard_stats_serialization() {
        let mut jobs_by_status = HashMap::new();
        jobs_by_status.insert("completed".to_string(), 100_i64);
        jobs_by_status.insert("failed".to_string(), 5_i64);

        let stats = DashboardStats {
            total_keys: 10,
            active_keys: 8,
            total_jobs: 105,
            jobs_last_24h: 20,
            jobs_by_status,
        };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["total_keys"], 10);
        assert_eq!(json["active_keys"], 8);
        assert_eq!(json["total_jobs"], 105);
        assert_eq!(json["jobs_last_24h"], 20);
        assert_eq!(json["jobs_by_status"]["completed"], 100);
    }

    #[test]
    fn job_summary_serialization() {
        let job = JobSummary {
            id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            model_name: "llama3.2".to_string(),
            backend: "ollama".to_string(),
            status: "completed".to_string(),
            created_at: "2026-02-22T12:00:00Z".to_string(),
            completed_at: Some("2026-02-22T12:00:01.2Z".to_string()),
            latency_ms: Some(1200),
        };
        let json = serde_json::to_value(&job).unwrap();
        assert_eq!(json["model_name"], "llama3.2");
        assert_eq!(json["backend"], "ollama");
        assert_eq!(json["latency_ms"], 1200);
    }

    #[test]
    fn jobs_response_serialization() {
        let resp = JobsResponse {
            jobs: vec![],
            total: 0,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["total"], 0);
        assert!(json["jobs"].as_array().unwrap().is_empty());
    }
}
