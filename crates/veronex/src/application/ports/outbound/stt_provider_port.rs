use async_trait::async_trait;

/// Request payload for speech-to-text transcription.
pub struct TranscriptionRequest {
    pub audio_bytes: Vec<u8>,
    /// BCP-47 language code (e.g. "ko", "en"). `None` = auto-detect.
    pub language: Option<String>,
    /// Enable speaker diarization (pyannote-audio).
    pub diarize: bool,
}

/// Result of a successful transcription.
pub struct TranscriptionResult {
    pub text: String,
    pub language: Option<String>,
    pub duration_seconds: Option<f64>,
}

/// Outbound port for a Whisper-compatible STT provider.
#[async_trait]
pub trait SttProviderPort: Send + Sync {
    async fn transcribe(&self, req: TranscriptionRequest) -> anyhow::Result<TranscriptionResult>;
}
