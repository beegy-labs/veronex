use async_trait::async_trait;

use crate::application::ports::outbound::stt_provider_port::{
    SttProviderPort, TranscriptionRequest, TranscriptionResult,
};
use crate::domain::constants::WHISPER_REQUEST_TIMEOUT;

/// HTTP adapter for the openai-whisper-asr-webservice API.
///
/// Sends audio to `POST {base_url}/asr` as multipart/form-data and
/// parses the JSON response into a [`TranscriptionResult`].
pub struct WhisperAdapter {
    base_url: String,
    client: reqwest::Client,
}

impl WhisperAdapter {
    pub fn new(base_url: impl Into<String>, client: reqwest::Client) -> Self {
        // Use a dedicated client with the Whisper-specific timeout so long
        // audio files do not hit the shared client's shorter timeout.
        let client = reqwest::Client::builder()
            .timeout(WHISPER_REQUEST_TIMEOUT)
            .use_rustls_tls()
            .build()
            .unwrap_or(client);
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client,
        }
    }
}

#[async_trait]
impl SttProviderPort for WhisperAdapter {
    async fn transcribe(&self, req: TranscriptionRequest) -> anyhow::Result<TranscriptionResult> {
        let mut url = format!("{}/asr?output=json&encode=true", self.base_url);
        if let Some(ref lang) = req.language {
            url.push_str(&format!("&language={lang}"));
        }
        if req.diarize {
            url.push_str("&diarize=true");
        }

        let part = reqwest::multipart::Part::bytes(req.audio_bytes)
            .file_name("audio")
            .mime_str("application/octet-stream")?;
        let form = reqwest::multipart::Form::new().part("audio_file", part);

        let resp = self
            .client
            .post(&url)
            .multipart(form)
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await?;

        Ok(TranscriptionResult {
            text: resp["text"].as_str().unwrap_or("").to_string(),
            language: resp["language"].as_str().map(str::to_string),
            duration_seconds: resp["duration"].as_f64(),
        })
    }
}
