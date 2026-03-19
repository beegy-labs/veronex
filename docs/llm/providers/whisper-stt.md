# Providers — Whisper STT: Registration, Routing & Health

> SSOT | **Last Updated**: 2026-03-19

## Task Guide

| Task | File | What to change |
|------|------|----------------|
| Add/edit Whisper provider | `POST /v1/providers` (`provider_type: "whisper"`) | No code change — use API |
| Change Whisper health check | `infrastructure/outbound/health_checker.rs` — `ProviderType::Whisper` arm | Update URL or timeout |
| Change transcription timeout | `domain/constants.rs` — `WHISPER_REQUEST_TIMEOUT` | Increase for long audio |
| Change max audio upload size | `infrastructure/inbound/http/openai_media_handlers.rs` — `MAX_AUDIO_BYTES` | Default 25 MB |
| Add language detection logic | `infrastructure/outbound/whisper/adapter.rs` — `transcribe()` | Extend multipart form |
| Change whisper-asr query params | `infrastructure/outbound/whisper/adapter.rs` — `transcribe()` | `output`, `encode`, `diarize` |
| Wire new Whisper provider at startup | `bootstrap/repositories.rs` — `build_repositories()` | First active Whisper provider is used |
| Exclude Whisper from LLM dispatch | `application/use_cases/inference/dispatcher.rs` | Already excluded via `ProviderType::Whisper => continue` |

## Key Files

| File | Purpose |
|------|---------|
| `crates/veronex/src/application/ports/outbound/stt_provider_port.rs` | `SttProviderPort` trait + `TranscriptionRequest` / `TranscriptionResult` |
| `crates/veronex/src/infrastructure/outbound/whisper/adapter.rs` | `WhisperAdapter` — multipart POST to `{url}/asr` |
| `crates/veronex/src/infrastructure/inbound/http/openai_media_handlers.rs` | `audio_transcriptions()` handler |
| `crates/veronex/src/bootstrap/repositories.rs` | Auto-wire first active Whisper provider → `stt_port` |
| `crates/veronex/src/infrastructure/outbound/health_checker.rs` | Health check: `GET {url}` with 10 s timeout |
| `crates/veronex/src/domain/constants.rs` | `WHISPER_HEALTH_CHECK_TIMEOUT` (10 s), `WHISPER_REQUEST_TIMEOUT` (300 s) |
| `migrations/postgres/000010_whisper_provider.up.sql` | Adds `'whisper'` to provider_type CHECK constraint |

---

## Whisper Provider Entity

Whisper providers share the `llm_providers` table with Ollama/Gemini.

**Required**: `name`, `provider_type: "whisper"`, `url` (Whisper ASR endpoint)
**Not applicable**: `api_key`, `total_vram_mb`, `gpu_index`, `server_id`, `is_free_tier`

---

## Transcription Endpoint

`POST /v1/audio/transcriptions` — OpenAI-compatible multipart/form-data.

- `file` (required): audio binary ≤ 25 MB
- `language` (optional): BCP-47 code e.g. `"ko"` — auto-detect if absent
- `diarize` (optional): `"true"` to enable speaker diarization
- `model` (optional): accepted but ignored

Backend forwards to: `POST {whisper_url}/asr?output=json&encode=true[&language=...][&diarize=true]`

**503** if no active Whisper provider is registered at startup. Re-deploy/restart to pick up a newly registered provider.

---

## Architecture Notes

- Whisper is **NOT** an LLM provider — it is excluded from the inference queue dispatcher
- Only the **first** active Whisper provider in the DB is used (single STT provider model)
- Health check: `GET {url}` — whisper-asr-webservice returns 200 on its root path
- CoreDNS rewrite: `whisper-asr-1.kr1.girok.dev` → `cilium-gateway-web-gateway.system-network.svc` (HTTPS via web-gateway)
