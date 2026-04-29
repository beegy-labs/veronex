# Lab Features (Experimental)

> SSOT | **Last Updated**: 2026-04-06

Lab features are experimental capabilities disabled by default.
Enabled in Settings → Lab Features.

→ Port, API, and Frontend details: `inference/lab-features-impl.md`

---

## Current Lab Features

| Feature | Default | Status |
|---------|---------|--------|
| `gemini_function_calling` | `false` | In development |
| `context_compression_enabled` | `false` | Lab |
| `compression_model` | `null` | Lab |
| `context_budget_ratio` | `0.60` | Lab — also drives the gateway-side pre-flight pruner (S17) |
| `compression_trigger_turns` | `1` | Lab |
| `recent_verbatim_window` | `1` | Lab |
| `compression_timeout_secs` | `10` | Lab |
| `multiturn_min_params` | `7` | Lab |
| `multiturn_min_ctx` | `16384` | Lab |
| `multiturn_allowed_models` | `[]` (all) | Lab |
| `vision_model` | `null` | Lab |
| `handoff_enabled` | `true` | Lab |
| `handoff_threshold` | `0.85` | Lab |

---

## Gateway shims — capability adapters

The gateway's product promise (per `.ai/README.md`): feature-richness depends on the gateway's shims, not on each underlying model's intrinsic capabilities. Two shims are active today:

| Capability | Shim mechanism | Trigger |
|---|---|---|
| Vision | `inference_helpers.rs::analyze_images_for_context` delegates the image to `lab_settings.vision_model` (default `qwen3-vl:8b`) and prepends the resulting text description to the user prompt | Non-vision model (per `is_vision_model` heuristic) + image input |
| Tool calling | `mcp::react_prompt` injects ReAct system prompt; `mcp::react_parser` extracts `Action: name\nAction Input: {...}` blocks; `mcp::bridge::run_loop_react` executes via the same `execute_calls` machinery the native path uses | Non-native-tool-calling model (per `ollama::capability::heuristic_supports_native`) + active MCP session. SDD: `.specs/veronex/history/mcp-react-shim.md`. |
| Context budget | `context_pruner::prune_to_budget` trims oldest messages in-memory before LLM submission, preserving system + last 5 turns + a placeholder marker | Accumulated `messages[]` token count exceeds `configured_ctx × context_budget_ratio − 1024`. SDD: `.specs/veronex/history/conversation-context-compression.md`. |

All three shims preserve the canonical flow's invariants — circuit breaker, ACL, S3 ConversationRecord, observability spans apply uniformly across native and shim paths.

---

## `gemini_function_calling`

**Scope**: SSOT for all Gemini integration visibility.
Disabling hides Gemini everywhere — not only function calling —
because the Gemini-compatible API is itself experimental.

**When disabled** (default) — suppressed:

| Layer | Behaviour |
|-------|-----------|
| **API** | Requests with `tools[]` → `501 Not Implemented` |
| **Nav** | Gemini child item hidden from the Providers nav group |
| **Providers page** | `?s=gemini` falls back to `?s=ollama`; `GeminiTab` not rendered |
| **Overview dashboard** | Provider Status KPI counts Ollama only; "API Services" row hidden; Gemini legend hidden |
| **Network Flow panel** | Gemini octagon node, Queue→Gemini path, and Gemini response arc removed |

**When enabled**:
- `tools[].functionDeclarations` converted to Ollama format and forwarded with the job.
- Model can return `functionCall` parts in its response.
- Tool calls streamed back in Gemini SSE format.
- Tool calls stored in `tool_calls_json` for training data.
- All Gemini UI (nav item, providers tab, dashboard stats, flow panel) visible.

---

## DB Schema

```sql
CREATE TABLE lab_settings (
    id                          INT         PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    gemini_function_calling     BOOLEAN     NOT NULL DEFAULT false,
    max_images_per_request      INT         NOT NULL DEFAULT 4,
    max_image_b64_bytes         INT         NOT NULL DEFAULT 2097152,
    context_compression_enabled BOOLEAN     NOT NULL DEFAULT false,
    compression_model           TEXT,
    context_budget_ratio        REAL        NOT NULL DEFAULT 0.60,
    compression_trigger_turns   INT         NOT NULL DEFAULT 1,
    recent_verbatim_window      INT         NOT NULL DEFAULT 1,
    compression_timeout_secs    INT         NOT NULL DEFAULT 10,
    multiturn_min_params        INT         NOT NULL DEFAULT 7,
    multiturn_min_ctx           INT         NOT NULL DEFAULT 16384,
    multiturn_allowed_models    TEXT[]      NOT NULL DEFAULT '{}',
    vision_model                TEXT,
    handoff_enabled             BOOLEAN     NOT NULL DEFAULT true,
    handoff_threshold           REAL        NOT NULL DEFAULT 0.85,
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO lab_settings (id) VALUES (1) ON CONFLICT DO NOTHING;
```

Singleton table (id=1, enforced by CHECK). Managed via `docker/postgres/init.sql`.

---

## Adding a New Lab Feature

1. Add column to `lab_settings` in `docker/postgres/init.sql`.
2. Add field to `LabSettings` struct in `application/ports/outbound/lab_settings_repository.rs`.
3. Add field to `LabSettingsUpdate` struct (same file).
4. Update `Default` impl for `LabSettings`.
5. Update `PostgresLabSettingsRepository` SQL (get + update).
6. Add field to `LabSettingsResponse` and `PatchLabSettingsBody` in `dashboard_handlers.rs`.
7. Update `lab_settings_to_response()` mapping.
8. Add the API-level check in the relevant handler(s).
9. Add `useLabSettings()` in every gated UI component.
10. Add i18n keys (en / ko / ja).
11. Add toggle or control in the relevant settings UI.
12. Document here and in `lab-features-impl.md`.
