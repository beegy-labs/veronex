use axum::http::Method;
use axum::middleware;
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use super::backend_handlers;
use super::dashboard_handlers;
use super::docs_handlers;
use super::gemini_model_handlers;
use super::gemini_policy_handlers;
use super::gpu_server_handlers;
use super::handlers;
use super::key_handlers;
use super::metrics_handlers;
use super::middleware::api_key_auth::api_key_auth;
use super::middleware::rate_limiter::rate_limiter;
use super::ollama_model_handlers;
use super::openai_handlers;
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
        .route("/v1/keys/{id}", delete(key_handlers::delete_key))
        .route("/v1/keys/{id}", patch(key_handlers::toggle_key))
        // Usage routes
        .route("/v1/usage", get(usage_handlers::aggregate_usage))
        .route("/v1/usage/breakdown", get(usage_handlers::usage_breakdown))
        .route("/v1/usage/{key_id}", get(usage_handlers::key_usage))
        .route(
            "/v1/usage/{key_id}/jobs",
            get(usage_handlers::key_usage_jobs),
        )
        // Analytics route
        .route(
            "/v1/dashboard/analytics",
            get(usage_handlers::get_analytics),
        )
        // Dashboard routes
        .route("/v1/dashboard/stats", get(dashboard_handlers::get_stats))
        .route("/v1/dashboard/jobs", get(dashboard_handlers::list_jobs))
        .route("/v1/dashboard/jobs/{id}", get(dashboard_handlers::get_job_detail))
        .route(
            "/v1/dashboard/performance",
            get(dashboard_handlers::get_performance),
        )
        // Backend management routes
        .route("/v1/backends", post(backend_handlers::register_backend))
        .route("/v1/backends", get(backend_handlers::list_backends))
        .route("/v1/backends/{id}", delete(backend_handlers::delete_backend))
        .route("/v1/backends/{id}", patch(backend_handlers::update_backend))
        .route(
            "/v1/backends/{id}/healthcheck",
            post(backend_handlers::healthcheck_backend),
        )
        .route(
            "/v1/backends/{id}/models",
            get(backend_handlers::list_backend_models),
        )
        .route(
            "/v1/backends/{id}/models/sync",
            post(backend_handlers::sync_backend_models),
        )
        .route(
            "/v1/backends/{id}/key",
            get(backend_handlers::reveal_backend_key),
        )
        .route(
            "/v1/backends/{id}/selected-models",
            get(backend_handlers::list_selected_models),
        )
        .route(
            "/v1/backends/{id}/selected-models/{model_name}",
            patch(backend_handlers::set_model_enabled),
        )
        // OpenAI-compatible chat completions endpoint
        .route("/v1/chat/completions", post(openai_handlers::chat_completions))
        // Job stream replay (test reconnect — OpenAI SSE format)
        .route("/v1/jobs/{id}/stream", get(handlers::stream_job_openai))
        // GPU server management routes
        .route("/v1/servers", post(gpu_server_handlers::register_gpu_server))
        .route("/v1/servers", get(gpu_server_handlers::list_gpu_servers))
        .route("/v1/servers/{id}", patch(gpu_server_handlers::update_gpu_server))
        .route("/v1/servers/{id}", delete(gpu_server_handlers::delete_gpu_server))
        .route("/v1/servers/{id}/metrics", get(gpu_server_handlers::get_server_metrics))
        .route("/v1/servers/{id}/metrics/history", get(gpu_server_handlers::get_server_metrics_history))
        // Gemini rate-limit policy management
        .route("/v1/gemini/policies", get(gemini_policy_handlers::list_gemini_policies))
        .route("/v1/gemini/policies/{model_name}", put(gemini_policy_handlers::upsert_gemini_policy))
        // Gemini global model sync + status sync
        .route("/v1/gemini/sync-config", get(gemini_model_handlers::get_sync_config))
        .route("/v1/gemini/sync-config", put(gemini_model_handlers::set_sync_config))
        .route("/v1/gemini/models/sync", post(gemini_model_handlers::sync_models))
        .route("/v1/gemini/models", get(gemini_model_handlers::list_models))
        .route("/v1/gemini/sync-status", post(gemini_model_handlers::sync_status))
        // Ollama global model sync
        .route("/v1/ollama/models", get(ollama_model_handlers::list_models))
        .route("/v1/ollama/models/sync", post(ollama_model_handlers::sync_all_backends))
        .route("/v1/ollama/sync/status", get(ollama_model_handlers::get_sync_status))
        .route("/v1/ollama/models/{model_name}/backends", get(ollama_model_handlers::list_model_backends))
        .route("/v1/ollama/backends/{backend_id}/models", get(ollama_model_handlers::list_backend_models))
}

/// Build the full application router with health endpoints and middleware.
///
/// Applies API key auth and rate limiting to all API routes.
/// Health/readiness endpoints bypass authentication.
pub fn build_app(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS, Method::PATCH])
        .allow_headers(tower_http::cors::Any)
        .allow_origin(tower_http::cors::Any);

    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/readyz", get(|| async { "ok" }))
        // API documentation — no auth required.
        .route("/docs/openapi.json", get(docs_handlers::openapi_json))
        .route("/docs/swagger", get(docs_handlers::swagger_ui))
        .route("/docs/redoc", get(docs_handlers::redoc_ui))
        // Prometheus HTTP SD — consumed by OTel Collector, no auth required.
        .route(
            "/v1/metrics/targets",
            get(metrics_handlers::list_metrics_targets),
        )
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
