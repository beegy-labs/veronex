//! OpenAI-compatible media endpoint stubs.
//!
//! These endpoints exist for API compatibility (Open WebUI, LiteLLM, etc.) but
//! Veronex does not support audio or image generation natively.
//! Clients receive a proper 501 Not Implemented response in OpenAI error format.

use axum::response::{IntoResponse, Response};
use tracing::instrument;

use super::error::AppError;

fn not_implemented(feature: &str) -> AppError {
    AppError::NotImplemented(format!("{feature} is not supported by this server"))
}

/// `POST /v1/audio/transcriptions` — Whisper speech-to-text (not supported).
#[instrument]
pub async fn audio_transcriptions() -> Response {
    not_implemented("Audio transcription").into_response()
}

/// `POST /v1/audio/speech` — Text-to-speech (not supported).
#[instrument]
pub async fn audio_speech() -> Response {
    not_implemented("Audio speech synthesis").into_response()
}

/// `POST /v1/images/generations` — Image generation (not supported).
#[instrument]
pub async fn image_generations() -> Response {
    not_implemented("Image generation").into_response()
}

/// `POST /v1/moderations` — Content moderation (not supported).
#[instrument]
pub async fn moderations() -> Response {
    not_implemented("Content moderation").into_response()
}
