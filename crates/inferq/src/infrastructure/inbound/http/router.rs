use axum::http::Method;
use axum::middleware;
use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use super::backend_handlers;
use super::dashboard_handlers;
use super::handlers;
use super::key_handlers;
use super::middleware::api_key_auth::api_key_auth;
use super::middleware::rate_limiter::rate_limiter;
use super::state::AppState;
use super::usage_handlers;

/// Build the versioned API router (routes only, no middleware).
///
/// Used directly in handler unit tests where middleware is not needed.
pub fn build_api_router() -> Router<AppState> {
    Router::new()
        // Inference routes
        .route("/v1/inference", post(handlers::submit_inference))
        .route(
            "/v1/inference/{job_id}/stream",
            get(handlers::stream_inference),
        )
        .route(
            "/v1/inference/{job_id}/status",
            get(handlers::get_status),
        )
        .route(
            "/v1/inference/{job_id}",
            delete(handlers::cancel_inference),
        )
        // Key management routes
        .route("/v1/keys", post(key_handlers::create_key))
        .route("/v1/keys", get(key_handlers::list_keys))
        .route("/v1/keys/{id}", delete(key_handlers::revoke_key))
        // Usage routes
        .route("/v1/usage", get(usage_handlers::aggregate_usage))
        .route("/v1/usage/{key_id}", get(usage_handlers::key_usage))
        .route(
            "/v1/usage/{key_id}/jobs",
            get(usage_handlers::key_usage_jobs),
        )
        // Dashboard routes
        .route("/v1/dashboard/stats", get(dashboard_handlers::get_stats))
        .route("/v1/dashboard/jobs", get(dashboard_handlers::list_jobs))
        .route(
            "/v1/dashboard/performance",
            get(dashboard_handlers::get_performance),
        )
        // Backend management routes
        .route("/v1/backends", post(backend_handlers::register_backend))
        .route("/v1/backends", get(backend_handlers::list_backends))
        .route("/v1/backends/{id}", delete(backend_handlers::delete_backend))
        .route(
            "/v1/backends/{id}/healthcheck",
            post(backend_handlers::healthcheck_backend),
        )
        .route(
            "/v1/backends/{id}/models",
            get(backend_handlers::list_backend_models),
        )
}

/// Build the full application router with health endpoints and middleware.
///
/// Applies API key auth and rate limiting to all API routes.
/// Health/readiness endpoints bypass authentication.
pub fn build_app(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers(tower_http::cors::Any)
        .allow_origin(tower_http::cors::Any);

    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/readyz", get(|| async { "ok" }))
        .merge(
            build_api_router()
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    rate_limiter,
                ))
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    api_key_auth,
                )),
        )
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
