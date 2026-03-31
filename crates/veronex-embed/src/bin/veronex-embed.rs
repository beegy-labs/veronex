//! veronex-embed — HTTP embedding service.
//!
//! Endpoints:
//!   POST /embed       — single text embedding
//!   POST /embed/batch — batch text embeddings
//!   GET  /health      — health check
//!   GET  /models      — available models
//!
//! Default port: 3200 (override with `PORT` env var).

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use tracing::info;

use veronex_embed::*;

type AppState = Arc<EmbedState>;

async fn embed_handler(
    State(state): State<AppState>,
    Json(req): Json<EmbedRequest>,
) -> impl IntoResponse {
    if req.text.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "text must not be empty"})),
        )
            .into_response();
    }

    match state.embed(&req.text) {
        Ok(vector) => {
            let dims = vector.len();
            (StatusCode::OK, Json(serde_json::json!(EmbedResponse { vector, dims }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

async fn embed_batch_handler(
    State(state): State<AppState>,
    Json(req): Json<EmbedBatchRequest>,
) -> impl IntoResponse {
    if req.texts.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "texts must not be empty"})),
        )
            .into_response();
    }

    match state.embed_batch(&req.texts) {
        Ok(vectors) => {
            let dims = vectors.first().map(|v| v.len()).unwrap_or(0);
            (
                StatusCode::OK,
                Json(serde_json::json!(EmbedBatchResponse { vectors, dims })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "service": "veronex-embed",
        "model": state.model_id.name(),
    }))
}

async fn models_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(ModelsResponse {
        models: vec![ModelInfo {
            name: state.model_id.name().to_string(),
            dims: state.model_id.dims(),
            loaded: true,
        }],
        default: ModelId::default_model().name().to_string(),
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3200);

    info!("Loading embedding model: multilingual-e5-large...");
    let state: AppState = Arc::new(
        EmbedState::new(ModelId::MultilingualE5Large).expect("failed to load embedding model"),
    );
    info!(model = state.model_id.name(), dims = state.model_id.dims(), "Model loaded");

    // PRELOAD_ONLY=1: bake model into Docker image at build time, then exit.
    if std::env::var("PRELOAD_ONLY").is_ok() {
        info!("PRELOAD_ONLY mode — model pre-loaded into cache, exiting");
        return;
    }

    let app = Router::new()
        .route("/embed", post(embed_handler))
        .route("/embed/batch", post(embed_batch_handler))
        .route("/health", get(health_handler))
        .route("/models", get(models_handler))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");
    info!(addr = %addr, "veronex-embed listening");
    axum::serve(listener, app).await.expect("Server error");
}
