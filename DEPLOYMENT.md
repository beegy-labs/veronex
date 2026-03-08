# Veronex — Deployment Guide

## 1. Prerequisites

- Docker 24+ with Compose v2 — verify with `docker compose version`
- Ollama running on the host or a remote server

## 2. Quick Start (Docker Compose)

```bash
# 1. Clone and configure
git clone <repo>
cd inferq
cp .env.example .env
# Edit .env: set JWT_SECRET (required) and OLLAMA_URL (if not macOS)

# 2. Start core services
docker compose up -d

# 3. Open the dashboard
open http://localhost:3002
# Follow the setup wizard to create your admin account
```

## 3. Core Stack

```bash
docker compose up -d
```

모든 필수 서비스(API, Web, PostgreSQL, Valkey, ClickHouse, Redpanda, OTel Collector)가 함께 시작됩니다.
모니터링은 OTel→Redpanda→ClickHouse 파이프라인으로 내장되어 있습니다.

## 4. Platform Notes

- **macOS**: Default `OLLAMA_URL=http://host.docker.internal:11434` works with Docker Desktop
- **Linux**: Set `OLLAMA_URL=http://172.17.0.1:11434` in `.env` (docker0 bridge IP)
- **GPU access**: AMD GPU metrics require `/sys/class/drm` (Linux only, ignored on macOS)

## 5. First Run

- Visit `http://localhost:3002` — the setup wizard creates the super admin account
- Or set `BOOTSTRAP_SUPER_USER` + `BOOTSTRAP_SUPER_PASS` in `.env` for automated setup
- Add an Ollama backend in the Providers page
- Create an API key in the Keys page

## 6. Updating

```bash
git pull
docker compose pull   # or docker compose build for local images
docker compose up -d
# Migrations run automatically on startup
```

## 7. Ports Reference

| Service | Default Port | Notes |
|---------|-------------|-------|
| veronex API | 3001 | OpenAI-compatible + admin API |
| veronex Web | 3002 | Dashboard UI |
| PostgreSQL | 5433 | Direct DB access (local dev) |
| Valkey | 6380 | Redis-compatible |
| ClickHouse | 8123 (HTTP) | Analytics queries |
| OTel Collector | 4317 (gRPC) / 4318 (HTTP) | Telemetry ingestion |
