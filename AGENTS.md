# AGENTS.md

> Universal LLM entry point | **Last Updated**: 2026-03-15

Read [.ai/README.md](.ai/README.md)

<!-- BEGIN: STANDARD POLICY -->
## Identity

| Term | Definition |
| ---- | ---------- |
| CDD | System SSOT and reconstruction baseline |
| SDD | CDD-derived change plan |
| ADD | Autonomous execution and policy selection engine |

Core loop: `CDD → SDD → ADD → CDD (feedback)` | Full definitions: [docs/llm/policies/identity.md](docs/llm/policies/identity.md)

## Frameworks

| Directory | Framework | Role |
| --------- | --------- | ---- |
| `.ai/` + `docs/llm/` | CDD | System SSOT — rules, patterns, architecture, constraints |
| `.specs/` | SDD | Change plans — specs, tasks, scope |
| `.add/` | ADD | Execution — workflow prompts, policy selection |

## Commit Rules

| Rule | Detail |
| ---- | ------ |
| No AI mention | Never reference Claude, GPT, Copilot, AI, LLM in commits, PR titles, PR bodies |
| No AI co-author | No Co-Authored-By AI trailers |
| Full spec | [.ai/git-flow.md](.ai/git-flow.md) |

## Doc Formatting

Applies to `.ai/`, `docs/llm/`, `.add/`. Full spec: [docs/llm/policies/token-optimization.md](docs/llm/policies/token-optimization.md)

| Rule | Detail |
| ---- | ------ |
| No emoji | No Unicode emoji |
| No decorative ASCII | No borders, box-drawing chars |
| No prose/filler | Tables over sentences |
| Indent / Headers | 2-space max 2 levels; H1+H2+H3 only |
| Format priority | Tables > YAML > bullets > code > prose |
<!-- END: STANDARD POLICY -->

<!-- BEGIN: PROJECT CUSTOM -->
## Architecture and Stack

| Layer | Tech |
| ----- | ---- |
| Backend | Rust + Axum (hexagonal) |
| Frontend | Next.js 16 + React 19 |
| Cache/Queue | Valkey (ZSET priority queue) |
| RDBMS / Analytics | PostgreSQL 18, ClickHouse |
| Streaming / Observability | Redpanda (Kafka API), OpenTelemetry Collector |

## Core Rules

| Rule | Detail |
| ---- | ------ |
| Queue dispatch | ZSET priority queue (`veronex:queue:zset`, tier-scored) |
| Capacity control | AIMD + p95 fast adapt, LLM Batch tuning |
| Thermal | Auto-detect gpu_vendor (nvidia->GPU, amd->CPU), per-provider thresholds |
| Lab settings | Gate features via `useLabSettings()` / `LabSettingsRepository` |

## Integration Points

| System | Protocol | Doc |
| ------ | -------- | --- |
| Ollama | HTTP + SSE streaming | `providers/ollama.md` |
| Gemini | REST + SSE | `providers/gemini.md` |
| OTel Collector | gRPC OTLP | `infra/otel-pipeline.md` |
| Redpanda | Kafka protocol | `infra/otel-pipeline-ops.md` |
<!-- END: PROJECT CUSTOM -->

## Workflows and Config

| Type | Key | Value |
| ---- | --- | ----- |
| ADD | Code review | `.add/code-review.md` |
| ADD | Doc sync | `.add/doc-sync.md` |
| LLM | Claude Code | `CLAUDE.md` |
| LLM | OpenAI Codex | `AGENTS.md` |
| LLM | Gemini CLI | `GEMINI.md` (future) |
| LLM | Cursor | `.cursorrules` (future) |
