# Tasks: Whisper STT Provider

> L3: Detailed tasks | Scope: whisper-stt.md | **Last Updated**: 2026-03-19

## Task List

| # | Task | Scope | Status | File |
|---|------|-------|--------|------|
| T1 | DB migration: llm_providers CHECK constraint에 'whisper' 추가 | S7 | pending | migrations/postgres/ |
| T2 | ProviderType::Whisper 추가 (enums.rs) | S1 | pending | domain/enums.rs |
| T3 | SttProviderPort trait 정의 | S2 | pending | application/ports/outbound/stt_provider_port.rs |
| T4 | WhisperAdapter 구현 | S3 | pending | infrastructure/outbound/whisper/adapter.rs |
| T5 | AppState에 stt_port 필드 추가 | S3 | pending | infrastructure/inbound/http/state.rs |
| T6 | bootstrap/repositories.rs — WhisperAdapter wiring | S3 | pending | bootstrap/repositories.rs |
| T7 | health_checker.rs — Whisper GET / 체크 분기 | S4 | pending | infrastructure/outbound/health_checker.rs |
| T8 | provider_handlers.rs — Whisper 등록 validation 분기 | S5 | pending | infrastructure/inbound/http/provider_handlers.rs |
| T9 | /v1/audio/transcriptions 핸들러 구현 | S6 | pending | infrastructure/inbound/http/openai_media_handlers.rs |
| T10 | WHISPER_ASR_URL env var — AppConfig | S3 | pending | bootstrap/config.rs |
| T11 | domain/constants.rs — WHISPER_REQUEST_TIMEOUT 추가 | S2 | pending | domain/constants.rs |
| T12 | CDD doc 작성 | S8 | pending | docs/llm/providers/whisper-stt.md |

---

## T1 — DB Migration

**File**: `migrations/postgres/000010_whisper_provider.up.sql`

```sql
-- llm_providers.provider_type CHECK constraint 확장
-- 기존: CHECK (provider_type IN ('ollama', 'gemini'))
-- 신규: CHECK (provider_type IN ('ollama', 'gemini', 'whisper'))
ALTER TABLE llm_providers
  DROP CONSTRAINT IF EXISTS llm_providers_provider_type_check;

ALTER TABLE llm_providers
  ADD CONSTRAINT llm_providers_provider_type_check
  CHECK (provider_type IN ('ollama', 'gemini', 'whisper'));
```

**File**: `migrations/postgres/000010_whisper_provider.down.sql`

```sql
ALTER TABLE llm_providers
  DROP CONSTRAINT IF EXISTS llm_providers_provider_type_check;

ALTER TABLE llm_providers
  ADD CONSTRAINT llm_providers_provider_type_check
  CHECK (provider_type IN ('ollama', 'gemini'));
```

---

## T2 — ProviderType::Whisper

**File**: `crates/veronex/src/domain/enums.rs`

```rust
pub enum ProviderType {
    Ollama,
    Gemini,
    Whisper,   // STT provider
}

impl ProviderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ollama   => "ollama",
            Self::Gemini   => "gemini",
            Self::Whisper  => "whisper",
        }
    }

    pub fn resource_type(&self) -> &'static str {
        match self {
            Self::Ollama   => "ollama_provider",
            Self::Gemini   => "gemini_provider",
            Self::Whisper  => "whisper_provider",
        }
    }
}

impl std::str::FromStr for ProviderType {
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ollama"  => Ok(Self::Ollama),
            "gemini"  => Ok(Self::Gemini),
            "whisper" => Ok(Self::Whisper),
            other     => Err(format!("unknown provider type: {other}")),
        }
    }
}
```

기존 `match` 블록 전체에서 `ProviderType` exhaustive match 확인 필요:
- `health_checker.rs`
- `provider_handlers.rs`
- `provider_router.rs` (dispatch에서 Whisper 제외)
- `model_manager/` (Whisper 제외)

---

## T3 — SttProviderPort

**File**: `crates/veronex/src/application/ports/outbound/stt_provider_port.rs`

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionRequest {
    pub audio_bytes: Vec<u8>,
    pub language: Option<String>,   // None = auto-detect
    pub diarize: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    pub language: Option<String>,
    pub duration_seconds: Option<f64>,
}

#[async_trait]
pub trait SttProviderPort: Send + Sync {
    async fn transcribe(&self, req: TranscriptionRequest) -> anyhow::Result<TranscriptionResult>;
    async fn health_check(&self) -> anyhow::Result<()>;
}
```

`mod.rs`에 추가:
```rust
pub mod stt_provider_port;
pub use stt_provider_port::{SttProviderPort, TranscriptionRequest, TranscriptionResult};
```

---

## T4 — WhisperAdapter

**File**: `crates/veronex/src/infrastructure/outbound/whisper/adapter.rs`

```rust
pub struct WhisperAdapter {
    base_url: String,
    client:   reqwest::Client,
}

impl WhisperAdapter {
    pub fn new(base_url: impl Into<String>, client: reqwest::Client) -> Self {
        Self { base_url: base_url.into(), client }
    }
}

#[async_trait]
impl SttProviderPort for WhisperAdapter {
    async fn transcribe(&self, req: TranscriptionRequest) -> anyhow::Result<TranscriptionResult> {
        // POST {base_url}/asr?output=json&encode=true[&language=ko][&diarize=true]
        // multipart/form-data: audio_file=<bytes>
        // Response: { "text": "...", "language": "..." }
        let mut url = format!("{}/asr?output=json&encode=true", self.base_url);
        if let Some(ref lang) = req.language { url.push_str(&format!("&language={lang}")); }
        if req.diarize { url.push_str("&diarize=true"); }

        let part = reqwest::multipart::Part::bytes(req.audio_bytes)
            .file_name("audio")
            .mime_str("application/octet-stream")?;
        let form = reqwest::multipart::Form::new().part("audio_file", part);

        let resp = self.client
            .post(&url)
            .multipart(form)
            .send().await?
            .error_for_status()?
            .json::<serde_json::Value>().await?;

        Ok(TranscriptionResult {
            text:             resp["text"].as_str().unwrap_or("").to_string(),
            language:         resp["language"].as_str().map(str::to_string),
            duration_seconds: resp["duration"].as_f64(),
        })
    }

    async fn health_check(&self) -> anyhow::Result<()> {
        self.client
            .get(&self.base_url)
            .send().await?
            .error_for_status()?;
        Ok(())
    }
}
```

**File**: `crates/veronex/src/infrastructure/outbound/whisper/mod.rs`
```rust
pub mod adapter;
pub use adapter::WhisperAdapter;
```

`infrastructure/outbound/mod.rs`에 `pub mod whisper;` 추가.

---

## T5 — AppState

**File**: `infrastructure/inbound/http/state.rs`

```rust
pub struct AppState {
    // ... 기존 필드 ...
    pub stt_port: Option<Arc<dyn SttProviderPort>>,
}
```

---

## T6 — Wiring

**File**: `bootstrap/repositories.rs`

```rust
// Whisper provider가 DB에 active로 등록되어 있으면 WhisperAdapter 생성
let stt_port: Option<Arc<dyn SttProviderPort>> =
    repos.provider_registry
        .list_active().await?
        .iter()
        .find(|p| p.provider_type == ProviderType::Whisper)
        .map(|p| Arc::new(WhisperAdapter::new(&p.url, infra.http_client.clone()))
            as Arc<dyn SttProviderPort>);
```

> 주의: 복수 Whisper 인스턴스 지원은 Multi-server Scale-Out 스펙에서 처리.
> 현재는 첫 번째 active Whisper provider를 사용.

---

## T7 — health_checker.rs

**기존 패턴** (Ollama: `GET /api/version`):
```rust
match provider.provider_type {
    ProviderType::Ollama  => check_ollama(&client, &provider.url).await,
    ProviderType::Gemini  => check_gemini(&client, &provider).await,
    ProviderType::Whisper => check_whisper(&client, &provider.url).await,
}
```

`check_whisper()`:
```rust
async fn check_whisper(client: &reqwest::Client, url: &str) -> anyhow::Result<()> {
    client.get(url)
        .timeout(WHISPER_HEALTH_TIMEOUT)
        .send().await?
        .error_for_status()?;
    Ok(())
}
```

`WHISPER_HEALTH_TIMEOUT = Duration::from_secs(10)` — health_checker.rs 상단에 추가.

---

## T8 — provider_handlers.rs

Whisper 등록 시 validation 분기:
```rust
// RegisterProviderRequest 처리에서
match req.provider_type {
    ProviderType::Ollama => {
        // url 필수, SSRF check
    }
    ProviderType::Gemini => {
        // api_key 필수
    }
    ProviderType::Whisper => {
        // url 필수, SSRF check
        // api_key 불필요
        // total_vram_mb, gpu_index, server_id 무시
    }
}
```

`/v1/providers/{id}/models` — Whisper인 경우 400 반환:
```rust
ProviderType::Whisper => return Err(AppError::BadRequest(
    "Whisper providers do not have model lists. Use POST /v1/audio/transcriptions".into()
)),
```

---

## T9 — /v1/audio/transcriptions

**File**: `infrastructure/inbound/http/openai_media_handlers.rs`

```rust
use axum::extract::{Multipart, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use tracing::instrument;

const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024; // 25 MB (OpenAI 기준)

#[derive(Serialize)]
pub struct AudioTranscriptionResponse {
    pub text: String,
}

#[instrument(skip(state, multipart))]
pub async fn audio_transcriptions(
    State(state): State<AppState>,
    Extension(api_key): Extension<ApiKey>,
    mut multipart: Multipart,
) -> Result<Json<AudioTranscriptionResponse>, AppError> {
    let stt = state.stt_port.as_ref()
        .ok_or_else(|| AppError::ServiceUnavailable(
            "No active Whisper provider registered".into()
        ))?;

    // multipart 파싱
    let mut audio_bytes: Option<Vec<u8>> = None;
    let mut language: Option<String> = None;
    let mut diarize = false;

    while let Some(field) = multipart.next_field().await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        match field.name() {
            Some("file") => {
                let data = field.bytes().await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                if data.len() > MAX_AUDIO_BYTES {
                    return Err(AppError::BadRequest("Audio file exceeds 25 MB".into()));
                }
                audio_bytes = Some(data.to_vec());
            }
            Some("language") => {
                language = Some(field.text().await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?);
            }
            Some("diarize") => {
                let val = field.text().await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                diarize = val == "true";
            }
            _ => {}
        }
    }

    let audio = audio_bytes.ok_or_else(|| AppError::BadRequest("Missing audio file".into()))?;

    let result = stt.transcribe(TranscriptionRequest { audio_bytes: audio, language, diarize })
        .await
        .map_err(|e| AppError::ServiceUnavailable(format!("Whisper error: {e}")))?;

    Ok(Json(AudioTranscriptionResponse { text: result.text }))
}
```

---

## T10 — AppConfig

`WHISPER_ASR_URL` env var는 **불필요**.
Whisper provider URL은 DB (`llm_providers` 테이블)에서 관리.
config.rs 변경 없음.

---

## T11 — domain/constants.rs

```rust
/// Whisper ASR transcription request timeout (large audio files).
pub const WHISPER_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
```

> T4 WhisperAdapter 생성 시 client timeout으로 사용.

---

## T12 — CDD Doc

**File**: `docs/llm/providers/whisper-stt.md`

내용:
- 등록 방법 (`POST /v1/providers {provider_type: "whisper", url: "..."}`)
- `/v1/audio/transcriptions` 요청/응답 형식
- Whisper ASR 엔드포인트 spec (POST /asr params)
- 헬스체크 방식
- 제약 (단일 인스턴스, diarization 응답 미포함)
