//! OpenAI-compatible media endpoints.
//!
//! - `POST /v1/audio/transcriptions` — proxied to the active Whisper STT provider.
//! - All other media endpoints return 501 Not Implemented (TTS, image gen, moderation).

use axum::extract::Multipart;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use axum::Json;
use serde::Serialize;
use tracing::instrument;

use crate::application::ports::outbound::stt_provider_port::TranscriptionRequest;
use crate::domain::entities::ApiKey;
use super::error::AppError;
use super::state::AppState;

/// Maximum audio file size accepted (matches OpenAI's 25 MB limit).
const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;

fn not_implemented(feature: &str) -> AppError {
    AppError::NotImplemented(format!("{feature} is not supported by this server"))
}

// ── Audio transcription ───────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct AudioTranscriptionResponse {
    pub text: String,
}

/// `POST /v1/audio/transcriptions` — Whisper speech-to-text.
///
/// Accepts OpenAI-compatible multipart/form-data:
/// - `file`     — audio file (required, ≤ 25 MB)
/// - `model`    — accepted but ignored (server uses its configured model)
/// - `language` — BCP-47 code, e.g. "ko" (optional, auto-detect if absent)
/// - `diarize`  — "true" to enable speaker diarization (optional)
#[instrument(skip(state, multipart), fields(api_key_id = %api_key.id))]
pub async fn audio_transcriptions(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    mut multipart: Multipart,
) -> Result<Json<AudioTranscriptionResponse>, AppError> {
    let stt = state.stt_port.as_ref().ok_or_else(|| {
        AppError::ServiceUnavailable("No active Whisper provider registered".into())
    })?;

    let mut audio_bytes: Option<Vec<u8>> = None;
    let mut language: Option<String> = None;
    let mut diarize = false;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        match field.name() {
            Some("file") => {
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                if data.len() > MAX_AUDIO_BYTES {
                    return Err(AppError::BadRequest("Audio file exceeds 25 MB".into()));
                }
                audio_bytes = Some(data.to_vec());
            }
            Some("language") => {
                let val = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                if !val.is_empty() {
                    language = Some(val);
                }
            }
            Some("diarize") => {
                let val = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                diarize = val == "true";
            }
            // `model`, `response_format`, `prompt` and other OpenAI fields are
            // accepted but ignored — Whisper server uses its own configuration.
            _ => {}
        }
    }

    let audio = audio_bytes
        .ok_or_else(|| AppError::BadRequest("Missing required field: file".into()))?;

    let result = stt
        .transcribe(TranscriptionRequest {
            audio_bytes: audio,
            language,
            diarize,
        })
        .await
        .map_err(|e| AppError::ServiceUnavailable(format!("Whisper error: {e}")))?;

    Ok(Json(AudioTranscriptionResponse { text: result.text }))
}

// ── Unimplemented stubs ───────────────────────────────────────────────────────

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
