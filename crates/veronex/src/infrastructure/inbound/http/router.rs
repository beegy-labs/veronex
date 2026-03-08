use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, Method};
use axum::middleware;
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use super::account_handlers;
use super::audit_handlers;
use super::auth_handlers;
use super::model_selection_handlers;
use super::provider_handlers;
use super::dashboard_handlers;
use super::docs_handlers;
use super::gemini_compat_handlers;
use super::gemini_model_handlers;
use super::gemini_policy_handlers;
use super::gpu_server_handlers;
use super::handlers;
use super::key_handlers;
use super::metrics_handlers;
use super::middleware::api_key_auth::api_key_auth;
use super::middleware::jwt_auth::jwt_auth;
use super::middleware::rate_limiter::rate_limiter;
use super::ollama_compat_handlers;
use super::ollama_model_handlers;
use super::openai_handlers;
use super::test_handlers;
use super::state::AppState;
use super::usage_handlers;

/// Build the inference router (API key auth + rate limit).
///
/// Only inference endpoints — used directly in handler unit tests.
pub fn build_api_router() -> Router<AppState> {
    Router::new()
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
        // ── OpenAI-compatible (qwen-code, OpenAI SDK) ──────────────────
        .route("/v1/chat/completions", post(openai_handlers::chat_completions))

        // ── Ollama native API (OLLAMA_HOST=http://veronex:3001) ─────────
        // /api/tags uses Veronex-synchronized models; everything else proxies to provider.
        .route("/api/tags",        get(ollama_compat_handlers::list_local_models))
        .route("/api/version",     get(ollama_compat_handlers::version))
        .route("/api/ps",          get(ollama_compat_handlers::ps))
        .route("/api/generate",    post(ollama_compat_handlers::generate))
        .route("/api/chat",        post(ollama_compat_handlers::chat))
        .route("/api/show",        post(ollama_compat_handlers::show))
        .route("/api/embed",       post(ollama_compat_handlers::embed))
        .route("/api/embeddings",  post(ollama_compat_handlers::embeddings))
        .route("/api/pull",        post(ollama_compat_handlers::pull))
        .route("/api/push",        post(ollama_compat_handlers::push))
        .route("/api/delete",      delete(ollama_compat_handlers::delete))
        .route("/api/copy",        post(ollama_compat_handlers::copy))
        .route("/api/create",      post(ollama_compat_handlers::create))

        // ── Gemini API-compatible (GOOGLE_GEMINI_BASE_URL=http://veronex:3001) ──
        // Model listing uses enabled Ollama models; generation proxies to Ollama.
        // {*path} catch-all is used for both GET (get_model) and POST (handle_request)
        // to avoid a conflict between {model} and {*path} segments.
        .route("/v1beta/models",         get(gemini_compat_handlers::list_models))
        .route("/v1beta/models/{*path}", get(gemini_compat_handlers::get_model)
                                            .post(gemini_compat_handlers::handle_request))

        // ── Job stream replay (OpenAI SSE format) ──────────────────────
        .route("/v1/jobs/{id}/stream", get(handlers::stream_job_openai))
}

/// Build the JWT-protected test run router (no API key, no rate limit).
///
/// Each API format has a dedicated test path that returns its native response format:
/// - `/v1/test/completions`           → OpenAI SSE (web test panel)
/// - `/v1/test/api/chat`              → Ollama NDJSON
/// - `/v1/test/api/generate`          → Ollama NDJSON
/// - `/v1/test/v1beta/models/{*path}` → Gemini SSE
fn build_test_router() -> Router<AppState> {
    Router::new()
        // OpenAI-compat (web test panel)
        .route("/v1/test/completions", post(test_handlers::test_completions))
        .route("/v1/test/jobs/{job_id}/stream", get(test_handlers::stream_test_job))
        // Ollama native test endpoints
        .route("/v1/test/api/chat", post(test_handlers::test_ollama_chat))
        .route("/v1/test/api/generate", post(test_handlers::test_ollama_generate))
        // Gemini native test endpoints
        .route("/v1/test/v1beta/models/{*path}", post(test_handlers::test_gemini_request))
}

/// Build the JWT-protected admin router.
fn build_jwt_router() -> Router<AppState> {
    Router::new()
        // Account management
        .route("/v1/accounts", get(account_handlers::list_accounts).post(account_handlers::create_account))
        .route("/v1/accounts/{id}", patch(account_handlers::update_account).delete(account_handlers::delete_account))
        .route("/v1/accounts/{id}/active", patch(account_handlers::set_account_active))
        .route("/v1/accounts/{id}/reset-link", post(account_handlers::create_reset_link))
        .route("/v1/accounts/{id}/sessions", get(account_handlers::list_account_sessions).delete(account_handlers::revoke_all_account_sessions))
        .route("/v1/sessions/{session_id}", delete(account_handlers::revoke_session))
        // Audit
        .route("/v1/audit", get(audit_handlers::list_audit_events))
        // Key management
        .route("/v1/keys", get(key_handlers::list_keys).post(key_handlers::create_key))
        .route("/v1/keys/{id}", delete(key_handlers::delete_key).patch(key_handlers::toggle_key))
        // Usage
        .route("/v1/usage", get(usage_handlers::aggregate_usage))
        .route("/v1/usage/breakdown", get(usage_handlers::usage_breakdown))
        .route("/v1/usage/{key_id}", get(usage_handlers::key_usage))
        .route("/v1/usage/{key_id}/jobs", get(usage_handlers::key_usage_jobs))
        .route("/v1/usage/{key_id}/models", get(usage_handlers::key_model_breakdown))
        // Analytics
        .route("/v1/dashboard/analytics", get(usage_handlers::get_analytics))
        // Dashboard
        .route("/v1/dashboard/overview", get(dashboard_handlers::get_dashboard_overview))
        .route("/v1/dashboard/stats", get(dashboard_handlers::get_stats))
        .route("/v1/dashboard/queue/depth", get(dashboard_handlers::get_queue_depth))
        .route("/v1/dashboard/jobs/stream", get(dashboard_handlers::job_events_sse))
        .route("/v1/dashboard/jobs", get(dashboard_handlers::list_jobs))
        .route(
            "/v1/dashboard/jobs/{id}",
            get(dashboard_handlers::get_job_detail).delete(dashboard_handlers::cancel_job),
        )
        .route("/v1/dashboard/performance", get(dashboard_handlers::get_performance))
        // Provider management
        .route("/v1/providers", get(provider_handlers::list_providers).post(provider_handlers::register_provider))
        .route("/v1/providers/{id}", delete(provider_handlers::delete_provider).patch(provider_handlers::update_provider))
        .route("/v1/providers/{id}/sync", post(provider_handlers::sync_single_provider))
        .route("/v1/providers/{id}/models", get(provider_handlers::list_provider_models))
        .route("/v1/providers/sync", post(provider_handlers::sync_all_providers_handler))
        .route("/v1/providers/{id}/key", get(provider_handlers::reveal_provider_key))
        .route("/v1/providers/{id}/selected-models", get(model_selection_handlers::list_selected_models))
        .route("/v1/providers/{id}/selected-models/{model_name}", patch(model_selection_handlers::set_model_enabled))
        // GPU server management
        .route("/v1/servers", get(gpu_server_handlers::list_gpu_servers).post(gpu_server_handlers::register_gpu_server))
        .route(
            "/v1/servers/{id}",
            patch(gpu_server_handlers::update_gpu_server).delete(gpu_server_handlers::delete_gpu_server),
        )
        .route("/v1/servers/{id}/metrics", get(gpu_server_handlers::get_server_metrics))
        .route("/v1/servers/{id}/metrics/history", get(gpu_server_handlers::get_server_metrics_history))
        // Gemini
        .route("/v1/gemini/policies", get(gemini_policy_handlers::list_gemini_policies))
        .route("/v1/gemini/policies/{model_name}", put(gemini_policy_handlers::upsert_gemini_policy))
        .route("/v1/gemini/sync-config", get(gemini_model_handlers::get_sync_config).put(gemini_model_handlers::set_sync_config))
        .route("/v1/gemini/models/sync", post(gemini_model_handlers::sync_models))
        .route("/v1/gemini/models", get(gemini_model_handlers::list_models))
        .route("/v1/gemini/sync-status", post(gemini_model_handlers::sync_status))
        // Ollama
        .route("/v1/ollama/models", get(ollama_model_handlers::list_models))
        .route("/v1/ollama/models/sync", post(ollama_model_handlers::sync_all_providers))
        .route("/v1/ollama/sync/status", get(ollama_model_handlers::get_sync_status))
        .route("/v1/ollama/models/{model_name}/providers", get(ollama_model_handlers::list_model_providers))
        .route("/v1/ollama/providers/{provider_id}/models", get(provider_handlers::list_provider_models))
        // Capacity / VRAM pool
        .route("/v1/dashboard/capacity", get(dashboard_handlers::get_capacity))
        .route(
            "/v1/dashboard/capacity/settings",
            get(dashboard_handlers::get_capacity_settings)
                .patch(dashboard_handlers::patch_capacity_settings),
        )
        // Session grouping
        .route(
            "/v1/dashboard/session-grouping/trigger",
            post(dashboard_handlers::trigger_session_grouping),
        )
        // Lab (experimental) features
        .route(
            "/v1/dashboard/lab",
            get(dashboard_handlers::get_lab_settings)
                .patch(dashboard_handlers::patch_lab_settings),
        )
}

/// Set security headers on every response (H2 fix).
#[allow(clippy::expect_used)]
async fn security_headers(mut response: axum::response::Response) -> axum::response::Response {
    let headers = response.headers_mut();
    headers.insert(
        axum::http::header::STRICT_TRANSPORT_SECURITY,
        "max-age=31536000; includeSubDomains".parse().expect("static"),
    );
    headers.insert(
        axum::http::header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().expect("static"),
    );
    headers.insert(
        axum::http::header::X_FRAME_OPTIONS,
        "DENY".parse().expect("static"),
    );
    headers.insert(
        axum::http::header::REFERRER_POLICY,
        "strict-origin-when-cross-origin".parse().expect("static"),
    );
    response
}

/// Build the full application router with health endpoints and middleware.
///
/// Applies API key auth and rate limiting to all API routes.
/// Health/readiness endpoints bypass authentication.
///
/// `cors_origins`: empty = allow any origin (`*`);
/// non-empty = restrict to the provided list (set via `CORS_ALLOWED_ORIGINS`).
pub fn build_app(state: AppState, cors_origins: Vec<HeaderValue>) -> Router {
    let cors = {
        let base = CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS, Method::PATCH])
            .allow_headers([
                axum::http::header::CONTENT_TYPE,
                axum::http::header::AUTHORIZATION,
                axum::http::header::ACCEPT,
                axum::http::header::HeaderName::from_static("x-conversation-id"),
            ])
            .allow_credentials(true);
        if cors_origins.is_empty() {
            // credentials:true requires an explicit origin — wildcard (*) is
            // forbidden by the CORS spec when credentials are included.
            // Default to localhost:3000 (Next.js dev server) when no origins
            // are configured; production MUST set CORS_ALLOWED_ORIGINS.
            #[allow(clippy::expect_used)]
            base.allow_origin(AllowOrigin::list([
                "http://localhost:3000".parse::<HeaderValue>().expect("static"),
            ]))
        } else {
            base.allow_origin(AllowOrigin::list(cors_origins))
        }
    };

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
        // First-run setup (no auth — only usable before any account exists)
        .route("/v1/setup/status", get(auth_handlers::setup_status))
        .route("/v1/setup", post(auth_handlers::setup))
        // Public auth routes (no middleware)
        .route("/v1/auth/login", post(auth_handlers::login))
        .route("/v1/auth/logout", post(auth_handlers::logout))
        .route("/v1/auth/refresh", post(auth_handlers::refresh))
        .route("/v1/auth/reset-password", post(auth_handlers::reset_password))
        // JWT-protected admin routes
        .merge(
            build_jwt_router()
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    jwt_auth,
                )),
        )
        // JWT-protected test run routes (no API key, no rate limit)
        .merge(
            build_test_router()
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    jwt_auth,
                )),
        )
        // API key-authenticated routes (existing, unchanged)
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
        .layer(DefaultBodyLimit::max(1024 * 1024)) // 1 MB — reject oversized bodies before deserialization
        .layer(cors)
        .layer(middleware::map_response(security_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
