# CLAUDE.md

> **Claude Entry Point** | **Last Updated**: 2026-03-03

## Quick Start

**Start here**: [.ai/README.md](.ai/README.md)

## Essential Reading

**For ANY task, read these first:**

1. **[.ai/rules.md](.ai/rules.md)** - Core DO/DON'T rules (CRITICAL)
2. **[.ai/git-flow.md](.ai/git-flow.md)** - Branch & commit policy
3. **[.ai/architecture.md](.ai/architecture.md)** - Hexagonal architecture overview

## Documentation Policy (CDD 4-Tier)

| Tier | Path | Role | Editable |
| ---- | ---- | ---- | -------- |
| 1 | `.ai/` | Indicator (≤50 lines) | Yes |
| 2 | `docs/llm/` | SSOT (domain-based) | Yes |
| 3 | `docs/en/` | Human-readable | Auto-gen |
| 4 | `docs/kr/` | Korean | Auto-gen |

**Edit Rules**: Edit `.ai/` + `docs/llm/` only. Never edit `docs/en/` or `docs/kr/` directly.

## Key Principles

- **Language**: English only (code, docs, commits)
- **Architecture**: Hexagonal (Ports & Adapters)
- **GitFlow**: `feat/* → develop → main` (see [.ai/git-flow.md](.ai/git-flow.md))
- **Commits**: Never mention AI assistance

## Tech Stack

| Layer | Tech |
| ----- | ---- |
| **Runtime** | Rust (Axum 0.8, tokio, Edition 2024) |
| **DB** | PostgreSQL 18 (sqlx 0.8, native uuidv7()) |
| **Queue** | Valkey (fred 10, BLPOP/RPUSH) |
| **Streaming** | SSE (Server-Sent Events) |
| **Analytics** | ClickHouse + OTel Collector |
| **Messaging** | Redpanda (Kafka-compatible) |
| **Deploy** | Kubernetes (Helm), Docker |
| **Web** | Next.js 16, Tailwind v4, shadcn/ui |

## Domain Docs (`docs/llm/`)

| Domain | Path | Topics |
| ------ | ---- | ------ |
| Policies | `policies/` | architecture, patterns, git-flow, terminology |
| Auth | `auth/` | jwt-sessions, api-keys, security |
| Inference | `inference/` | job-lifecycle, job-api, session-grouping, job-analytics, openai-compat, capacity, model-pricing, lab-features |
| Providers | `providers/` | ollama, ollama-models, gemini, gemini-models, hardware |
| Infra | `infra/` | deploy, otel-pipeline |
| Frontend | `frontend/` | design-system (core, i18n, components), charts, pages/* |
| Research | `research/` | 2026 best practices |

## Workflows (ADD)

| Action | Workflow | Reads |
| ------ | -------- | ----- |
| Code review / optimization | `.add/code-review.md` | Relevant CDD docs per domain |

---

**Start**: [.ai/README.md](.ai/README.md)
