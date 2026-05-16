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

## Agent Role Split

ADD is realized by two cooperating agents. Each request is routed by role; the same agent never owns both roles in the same scope.

| Agent | Role | Reads | Writes | Token Profile |
| ----- | ---- | ----- | ------ | ------------- |
| Claude Code | **Planner / Reviewer** | CDD (`.ai/`, `docs/llm/`), SDD scope/tasks, Codex summaries | SDD (`.specs/veronex/{scope}/`), CDD feedback | High-context reasoning, no source-body reads |
| Codex (via MCP) | **Implementer** | SDD scope/tasks, source code, build/test output | Code, migrations, test files | Bulk file I/O, build loops, retry cycles |

| Step | Owner | Action |
| ---- | ----- | ------ |
| 1 | Claude | Approve scope, author `.specs/veronex/{scope}/spec.md` + `tasks.md` |
| 2 | Claude | Delegate via `mcp__codex__codex` per [.add/codex-delegate.md](.add/codex-delegate.md) |
| 3 | Codex | Read source, implement, run QA gate, retry up to 3× |
| 4 | Codex | Return compressed summary (≤400 tokens, no diffs) |
| 5 | Claude | Verify summary vs scope; trigger Layer 2 fresh-eyes Codex review or `doc-sync.md` |

Stay-in-Claude exceptions: architecture/scope debate, one-line config edits, `.ai/` or `docs/llm/` doc edits, bug with unknown root cause. See [.add/codex-delegate.md](.add/codex-delegate.md#decision-matrix).

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
| llama-server | HTTP + SSE streaming | `providers/llama-server.md` |
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

## Token & Cache Hygiene (2026)

Anthropic prompt cache TTL dropped from 60 min to 5 min in 2026 Q1. To preserve hit rate (Claude Code achieves ~92% with proper layout):

| Rule | Reason |
| ---- | ------ |
| Static prefix first | This file, `.ai/` SSOT, tool defs — placed before any per-request material |
| No timestamps in static sections | A single shifting byte invalidates the entire prefix cache |
| `Last Updated` lives only in the header line | Never embed timestamps inside rule tables |
| Living document | Encode every recurring correction as a rule — AI-generated rules show no measurable benefit |
