# Changelog

All notable changes to **Veronex** — an OpenAI-compatible LLM inference gateway with multi-backend routing, VRAM-aware concurrency, and a Next.js monitoring dashboard — will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-03-02

### Added

- OpenAI-compatible chat completions API (`POST /v1/chat/completions`)
- Ollama and Gemini multi-backend routing with per-request backend selection
- VRAM-aware dynamic concurrency (`ConcurrencySlotMap`: `(backend_id, model_name)` → `Arc<Semaphore>`)
- Thermal throttle with three tiers: Normal (<78°C) / Soft (≥85°C) / Hard (≥92°C) with 60s hysteresis
- Capacity analyzer loop (30s tick): Ollama `/api/ps` + `/api/show` → KV cache formula → recommended slots
- SSE real-time job streaming (`GET /v1/dashboard/jobs/stream`, `GET /v1/test/jobs/{id}/stream`)
- API key management: standard and test key types, BLAKE2b hash storage, per-key rate limits
- JWT authentication with rolling refresh tokens and Valkey-backed revocation blocklist
- Rate limiting: RPM sliding window and TPM budget per API key
- Job queue with priority lanes (paid / standard / test) via Valkey BLPOP/RPUSH
- Multi-tenant accounts with RBAC: super / admin / user roles
- Audit trail ingested through OTel pipeline → Redpanda → ClickHouse
- GPU server metrics via node-exporter scrape with ClickHouse history (`GET /v1/servers/{id}/metrics`)
- Model pricing table and per-job `estimated_cost_usd` tracking
- Lab settings as feature flags (e.g., Gemini gating behind `gemini_function_calling`)
- Next.js 16 dashboard with pages: Overview, Usage, Performance, Jobs, API Keys, Providers, Servers, Accounts, Audit
- Real-time network flow visualization (ArgoCD-style SVG panel with live SSE feed)
- i18n support: English and Korean
- Docker Compose deployment with all core services

[Unreleased]: https://github.com/beegy-labs/inferq/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/beegy-labs/inferq/releases/tag/v0.1.0
