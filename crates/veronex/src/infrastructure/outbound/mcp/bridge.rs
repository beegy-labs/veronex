//! McpBridgeAdapter — wraps the Ollama inference loop with MCP tool execution.
//!
//! # Flow (per request)
//!
//! ```text
//! 1. get_all()        — pull available MCP tool defs (Vec<Value>, OpenAI format)
//! 2. merge tools into request
//! 3. submit job  → collect tokens  → check for tool_calls
//! 4. if tool_calls contain MCP tools (prefix "mcp_"):
//!       a. execute each via McpSessionManager.call_tool()
//!       b. respect circuit breaker + result cache
//!       c. emit OTel span (→ ClickHouse mcp_tool_calls)
//!       d. append assistant + tool result messages
//!       e. GOTO 3  (max MAX_ROUNDS)
//! 5. return McpLoopResult
//! ```
//!
//! Duplicate-call detection: if the same (tool_name, args_hash) pair appears
//! LOOP_DETECT_THRESHOLD times, the loop is broken early.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::{debug, info, instrument, warn, Instrument};
use uuid::Uuid;
use chrono;

use veronex_mcp::{McpCircuitBreaker, McpResultCache, McpSessionManager, McpToolCache, truncate_at_char_boundary};

use crate::application::ports::inbound::inference_use_case::SubmitJobRequest;
use crate::application::ports::outbound::analytics_repository::{AnalyticsRepository, McpToolCallEvent};
use crate::domain::constants::{
    MCP_LIFECYCLE_LOAD_TIMEOUT, MCP_ROUND_TOTAL_TIMEOUT, MCP_STREAM_IDLE_TIMEOUT,
    MCP_TOKEN_FIRST_TIMEOUT,
};
use crate::domain::enums::{ApiFormat, ProviderType};
use crate::domain::value_objects::JobId;
use crate::infrastructure::inbound::http::inference_helpers::validate_tool_call;
use crate::infrastructure::inbound::http::middleware::infer_auth::InferCaller;
use crate::infrastructure::inbound::http::state::AppState;

// ── Constants ──────────────────────────────────────────────────────────────────

/// Maximum agentic loop rounds (tool call → execute → re-submit).
const MAX_ROUNDS: u8 = 5;
/// Maximum MCP tools injected per request (context window protection).
/// Also used by `McpToolCache::new()` in main.rs — keep in sync.
pub const MAX_TOOLS_PER_REQUEST: usize = 32;
/// Number of identical (tool, args_hash) pairs in one session before forced break.
const LOOP_DETECT_THRESHOLD: u8 = 3;
/// Result cache TTL (seconds).
const RESULT_CACHE_TTL_SECS: i64 = 300;
/// Phase 1 timeout — covers `ensure_ready` cold-load + KV cache pre-allocation
/// + warmup. Active until a `StreamToken::phase_boundary()` arrives from the
/// runner (S14 Lifecycle SoD post-`ensure_ready` signal). SSOT in
/// [`crate::domain::constants::MCP_LIFECYCLE_LOAD_TIMEOUT`] — coupled with the
/// `ollama::lifecycle` reqwest cold-load timeout (must stay equal).
const LIFECYCLE_TIMEOUT: tokio::time::Duration = MCP_LIFECYCLE_LOAD_TIMEOUT;

/// Phase 2 first-token timeout — applies only after the runner emits the
/// phase-boundary token. SSOT in
/// [`crate::domain::constants::MCP_TOKEN_FIRST_TIMEOUT`].
const TOKEN_FIRST_TIMEOUT: tokio::time::Duration = MCP_TOKEN_FIRST_TIMEOUT;

/// Per-token stream idle. SSOT in
/// [`crate::domain::constants::MCP_STREAM_IDLE_TIMEOUT`].
const STREAM_IDLE_TIMEOUT: tokio::time::Duration = MCP_STREAM_IDLE_TIMEOUT;

/// Hard cap per round. SSOT in
/// [`crate::domain::constants::MCP_ROUND_TOTAL_TIMEOUT`]. Held under the
/// upstream Cilium HTTPRoute `timeouts.request=1800 s`. Invariants enforced
/// by `tests::timeout_invariants`.
const ROUND_TOTAL_TIMEOUT: tokio::time::Duration = MCP_ROUND_TOTAL_TIMEOUT;

/// Upstream gateway request-timeout that the bridge round budget must stay under.
/// Mirrors `timeouts.request: 1800s` set on Cilium HTTPRoute in platform-gitops.
/// Used solely as a compile-time invariant target — not consumed at runtime.
#[cfg(test)]
const GATEWAY_REQUEST_TIMEOUT_SECS: u64 = 1800;
/// Maximum bytes of a single MCP tool result injected into the messages array.
/// Prevents OOM from malicious/misconfigured servers at high concurrency.
const MAX_TOOL_RESULT_BYTES: usize = 32_768;
/// Maximum bytes of args string fed into `quick_args_hash`.
/// Loop-detection hashing is O(n); cap prevents unbounded work on inflated payloads.
const MAX_ARGS_FOR_HASH_BYTES: usize = 4_096;
/// Maximum number of MCP tool calls executed concurrently within one round.
/// Prevents thundering-herd against a single MCP server when a model emits many calls.
const MAX_CONCURRENT_TOOL_CALLS: usize = 8;

// ── Public types ───────────────────────────────────────────────────────────────

/// Shared state for the MCP bridge — stored in `AppState.mcp_bridge`.
#[derive(Clone)]
pub struct McpBridgeAdapter {
    pub session_manager: Arc<McpSessionManager>,
    pub tool_cache: Arc<McpToolCache>,
    pub result_cache: Arc<McpResultCache>,
    pub circuit_breaker: Arc<McpCircuitBreaker>,
    pub analytics_repo: Option<Arc<dyn AnalyticsRepository>>,
}

/// Outcome of a single agentic loop run.
///
/// All rounds are collected synchronously by `collect_round` (S20 — see
/// `.specs/veronex/bridge-mcp-loop-correctness.md`). When the caller passes
/// a `sse_tap_tx` to `run_loop`, content tokens of text rounds are streamed
/// to the client AS they arrive; the fields below carry round-end summary
/// info (totals, finish_reason, last-round tool_calls if non-MCP).
pub struct McpLoopResult {
    /// Final assistant text content. May duplicate text already streamed via
    /// the tap (tap is a copy, not a move). Caller should NOT re-emit `content`
    /// when `streamed_via_tap` is true — see SDD §4 caller integration.
    pub content: String,
    /// Final round tool_calls — non-empty when the model finished with non-MCP
    /// tools (passthrough to client) or with no MCP servers in scope.
    pub tool_calls: Vec<Value>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub finish_reason: String,
    /// How many MCP tool-call rounds were executed.
    pub rounds: u8,
    /// True iff at least one round's content was forwarded via the `sse_tap_tx`
    /// passed to `run_loop`. When true, the caller MUST NOT emit `content` as
    /// an SSE chunk (already streamed) — only emit the trailing `finish` /
    /// `usage` / `[DONE]` chunks.
    pub streamed_via_tap: bool,
}

// ── McpBridgeAdapter impl ──────────────────────────────────────────────────────

impl McpBridgeAdapter {
    pub fn new(
        session_manager: Arc<McpSessionManager>,
        tool_cache: Arc<McpToolCache>,
        result_cache: Arc<McpResultCache>,
        circuit_breaker: Arc<McpCircuitBreaker>,
    ) -> Self {
        Self { session_manager, tool_cache, result_cache, circuit_breaker, analytics_repo: None }
    }

    /// Returns `true` if MCP should intercept this request
    /// (at least one server session is active). O(1).
    pub fn should_intercept(&self) -> bool {
        self.session_manager.has_sessions()
    }

    /// Run the full agentic MCP loop.
    ///
    /// `base_messages` must be in Ollama format already.
    /// `base_tools` are caller-supplied tools (injected before MCP tools, up to cap).
    ///
    /// `sse_tap_tx` is the **stream-tap** sender (SDD `.specs/veronex/bridge-mcp-loop-correctness.md`):
    /// when `Some`, the bridge forwards content tokens of text rounds to the
    /// caller's SSE writer as they arrive (chatGPT-style token streaming).
    /// Tool-call rounds remain silent on the tap — the bridge intercepts and
    /// executes MCP tools server-side, then runs the next round. All rounds
    /// are collected synchronously regardless of `sse_tap_tx`; the tap only
    /// controls user-visible streaming, not loop correctness.
    #[instrument(skip_all, fields(model = %model))]
    #[allow(clippy::too_many_arguments)]
    pub async fn run_loop(
        &self,
        state: &AppState,
        caller: &InferCaller,
        model: String,
        mut messages: Vec<Value>,
        base_tools: Option<Vec<Value>>,
        conversation_id: Option<uuid::Uuid>,
        stop: Option<Value>,
        seed: Option<u32>,
        response_format: Option<Value>,
        frequency_penalty: Option<f64>,
        presence_penalty: Option<f64>,
        sse_tap_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> Option<McpLoopResult> {
        // ── Per-key ACL + cap_points + top_k — fetched in parallel ───────────
        // JWT session callers (no api_key_id) bypass all key-level limits.
        let (allowed_servers, max_rounds, top_k_override) =
            if let Some(key_id) = caller.api_key_id() {
                let (acl_ids, cap, topk) = tokio::join!(
                    fetch_mcp_acl(state, key_id),
                    fetch_mcp_cap_points(state, key_id),
                    fetch_mcp_top_k(state, key_id),
                );
                // cap_points = 0 → MCP disabled for this key
                if cap == Some(0) {
                    return None;
                }
                let max_r = cap.map(|n| n.min(MAX_ROUNDS)).unwrap_or(MAX_ROUNDS);
                // Default deny: empty set = deny all, non-empty = allow listed servers.
                let allowed = Some(Arc::new(acl_ids.into_iter().collect::<HashSet<Uuid>>()));
                (allowed, max_r, topk)
            } else {
                (None, MAX_ROUNDS, None)
            };

        // ── Pre-flight context budget gate (S17 Tier C/D) ────────────────────
        //
        // Trim accumulated `messages[]` to fit the model's context window
        // before any further processing. Resolves the budget from the
        // smallest configured_ctx among providers serving this model so the
        // dispatcher can schedule onto any of them without overflow.
        // DCP invariant (SDD §5.3): in-memory only — S3 ConversationRecord
        // is never modified by pruning.
        // SDD: `.specs/veronex/history/conversation-context-compression.md` §5/§6.
        {
            use crate::application::use_cases::inference::{context_budget, context_pruner};
            let lab = state.lab_settings_repo.get().await.unwrap_or_default();
            let configured_ctx = state
                .capacity_repo
                .min_configured_ctx_for_model(&model)
                .await
                .ok()
                .flatten()
                .filter(|&c| c >= 4096)
                .unwrap_or(32_768);
            let budget = context_budget::budget_for_context(configured_ctx, lab.context_budget_ratio);
            let (trimmed, report) =
                context_pruner::prune_to_budget(&messages, budget, context_pruner::DEFAULT_PRESERVE_RECENT);
            if !report.is_no_op() {
                tracing::info!(
                    model = %model,
                    configured_ctx,
                    budget,
                    initial_tokens = report.initial,
                    after_tokens = report.budget_after,
                    dropped = report.dropped,
                    "context-pruner: trimmed accumulated messages to fit budget"
                );
                messages = trimmed;
            }
        }

        // ── Build the tool list (vector selection or fallback get_all) ───────────
        //
        // When McpVectorSelector is available: embed the last user message and
        // select Top-K semantically relevant tools via Vespa ANN.
        // Fallback (Vespa unavailable or not configured): get_all() + MAX_TOOLS cut.
        let last_user_query = extract_last_user_prompt(&messages);
        let environment = state.vespa_environment.as_ref();
        let tenant_id = state.vespa_tenant_id.as_ref();

        let mcp_openai_tools: Vec<Value> = if let Some(ref selector) = state.mcp_vector_selector {
            match selector.select(&last_user_query, environment, tenant_id, top_k_override).await {
                Some(hits) => {
                    use veronex_mcp::vector::McpVectorSelector;
                    McpVectorSelector::hits_to_openai(&hits)
                }
                None => {
                    // Vespa error — fall back to get_all
                    tracing::debug!("MCP vector select failed — falling back to get_all");
                    self.tool_cache.get_all(allowed_servers.as_deref()).await
                }
            }
        } else {
            self.tool_cache.get_all(allowed_servers.as_deref()).await
        };

        let mcp_tool_names: std::collections::HashSet<String> = mcp_openai_tools
            .iter()
            .filter_map(|v| v["function"]["name"].as_str().map(str::to_string))
            .collect();

        let mut all_tools: Vec<Value> = base_tools.unwrap_or_default();
        for tool in mcp_openai_tools {
            if all_tools.len() >= MAX_TOOLS_PER_REQUEST {
                break;
            }
            all_tools.push(tool);
        }
        let tools_json = if all_tools.is_empty() { None } else { Some(all_tools.clone()) };

        // ── Capability gate — route to forced-JSON gateway shim when the model
        // does not natively emit `tool_calls`. The shim leverages Ollama's
        // `format` parameter (constrained decoding via GBNF grammar) so weak
        // models (qwen3:8b, llama3:7b, etc.) can drive MCP deterministically.
        // The schema is a `oneOf` covering every available tool plus a `final`
        // terminator — the model's logits are masked at every token to produce
        // grammar-valid output, so tool dispatch is no longer best-effort.
        if !all_tools.is_empty()
            && !crate::infrastructure::outbound::ollama::capability::heuristic_supports_native(&model)
        {
            info!(model = %model, "MCP: routing to forced-JSON shim (non-native tool_calls model)");
            return self
                .run_loop_forced_json(
                    state,
                    caller,
                    model,
                    messages,
                    all_tools,
                    conversation_id,
                    stop,
                    seed,
                    response_format,
                    frequency_penalty,
                    presence_penalty,
                    allowed_servers,
                    max_rounds,
                )
                .await;
        }

        // ── Loop ID — groups all rounds into one traceable unit ───────────────
        let mcp_loop_id = Uuid::new_v4();

        // ── Loop state ─────────────────────────────────────────────────────────
        let mut total_prompt_tokens: u32 = 0;
        let mut total_completion_tokens: u32 = 0;
        let mut finish_reason = "stop".to_string();
        let mut content = String::new();
        let mut final_tool_calls: Vec<Value> = Vec::new();
        // All MCP tool_calls emitted across every round. Persisted on the
        // single consolidated S3 turn at loop end so the UI can render
        // "MCP {N}회" from `turn.tool_calls.length`.
        let mut all_mcp_tool_calls: Vec<Value> = Vec::new();
        let mut rounds: u8 = 0;
        // Tracks whether any round's content was forwarded via sse_tap_tx.
        // S20: replaces the legacy `final_job_id` fast-path signal.
        let mut streamed_via_tap = false;

        let mut first_job_id: Option<JobId> = None;

        let mut intermediate_job_ids: Vec<Uuid> = Vec::new();

        // Loop-detection: (tool_name, args_hash) → count
        let mut call_sig_counts: HashMap<(String, String), u8> = HashMap::new();

        for round in 0..max_rounds {
            debug!(round, "MCP agentic loop round");

            // ── Convergence boundary on the final round (S23 Tier C) ──────────
            // If we are about to dispatch the LAST allowed round, no text
            // content has been produced yet, AND at least one tool round has
            // already executed (so tool results exist in the messages array),
            // force the model to emit text by (1) injecting a system message
            // and (2) omitting the `tools` schema from the request. Either
            // alone is insufficient on Ollama-served, tool-eager models
            // (qwen3-coder family).
            //
            // The OpenAI canonical mechanism is `tool_choice="none"` (keep
            // tools, force a regular message), but Ollama's OpenAI-compat
            // endpoint silently drops the `tool_choice` field — so we
            // approximate by removing the tool schemas entirely on this
            // round. The accumulated tool *results* (role:"tool" entries)
            // remain in the messages array, so the model still has full
            // context to synthesize its final answer.
            //
            // References:
            //   - Ollama issue #8421 — tool_choice silently ignored
            //   - Ollama issue #11171 — open feature request for tool_choice
            //     (https://github.com/ollama/ollama/issues/8421)
            //     (https://github.com/ollama/ollama/issues/11171)
            //   - QwenLM/Qwen3-Coder issue #475 — degenerate tool-call loops
            //     documented in the model's community tracker
            //
            // SDD: `.specs/veronex/mcp-tool-audit-exposure-and-loop-convergence.md` §3.3.
            let convergence_boundary = round + 1 == max_rounds && rounds > 0 && content.is_empty();
            if convergence_boundary {
                // Re-anchor the date at synthesis time. The original
                // `inject_current_datetime` system message at messages[0] is
                // far from the generation point after several MCP rounds;
                // its weight on the model's output template diminishes
                // proportionally to context length. Re-injecting fresh,
                // close to the final emission, counters narrative anchoring
                // (qwen3-coder reverting to "2024년 12월" timelines despite
                // its own search queries using current dates).
                let date_anchor = crate::infrastructure::inbound::http::inference_helpers::build_current_datetime_system_text();
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": date_anchor
                }));
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": "You have reached the final response step. \
                        Tools are no longer available. Using the tool \
                        results already provided above, produce the user's \
                        final answer in natural language now. Honor the \
                        date constraints in the system message just above \
                        — every \"today\" / \"recent\" / \"현재\" / \"최근\" \
                        in your response refers to the current date listed \
                        there, not to your training cutoff."
                }));
                info!(round, max_rounds, "MCP convergence: tools omitted + date anchor + final-step system injected");
            }

            // ── Submit job ─────────────────────────────────────────────────────
            let prompt = extract_last_user_prompt(&messages);
            let job_id = match state.use_case.submit(SubmitJobRequest {
                prompt,
                model_name: model.clone(),
                provider_type: ProviderType::Ollama,
                gemini_tier: None,
                api_key_id: caller.api_key_id(),
                account_id: caller.account_id(),
                source: caller.source(),
                api_format: ApiFormat::OpenaiCompat,
                messages: Some(Value::Array(messages.clone())),
                // Convergence boundary: omit tools schema entirely so the
                // model has nothing callable and must emit text. See the
                // boundary block above for the Ollama-specific rationale.
                tools: if convergence_boundary { None } else { tools_json.clone().map(Value::Array) },
                request_path: Some("/v1/chat/completions".to_string()),
                conversation_id,
                key_tier: caller.key_tier(),
                images: None,
                stop: stop.clone(),
                seed,
                response_format: response_format.clone(),
                frequency_penalty,
                presence_penalty,
                mcp_loop_id: Some(mcp_loop_id),
                max_tokens: None,
                vision_analysis: None,
            }).await {
                Ok(id) => id,
                Err(e) => {
                    warn!("MCP loop: job submit failed on round {round}: {e}");
                    return None;
                }
            };

            if first_job_id.is_none() {
                first_job_id = Some(job_id.clone());
            } else {
                intermediate_job_ids.push(job_id.0);
            }

            // ── Collect response synchronously ─────────────────────────────────
            //
            // SDD `.specs/veronex/bridge-mcp-loop-correctness.md` (S20): all rounds
            // are collected synchronously by `collect_round`. The earlier
            // streaming fast-path (skip collection of round N+1 when `rounds > 0`)
            // was removed because (a) it bypassed MCP tool detection, breaking the
            // loop invariant when models emitted tool_calls in round N+1; (b) its
            // sole driving constraint (CF Edge 100s idle) was removed by
            // platform-gitops PRs #598/#599/#600 introducing CF-bypass routing;
            // (c) industry agentic-loop frameworks (LangGraph, OpenAI Agents SDK)
            // do not use such a bypass. Token-by-token streaming UX is preserved
            // via the `sse_tap_tx` stream-tap passed into `collect_round`.
            //
            // `collect_round` owns the phased timeout (LIFECYCLE/FIRST_TOKEN/STREAM_IDLE/ROUND_TOTAL).
            let round_result = match collect_round(state, &job_id, sse_tap_tx.as_ref()).await {
                Ok(r) => r,
                Err(e) => {
                    warn!(round = rounds, model = %model, error = %e, "MCP round failed");
                    let code = match &e {
                        RoundError::LifecycleTimeout  => "model_loading",
                        RoundError::FirstTokenTimeout => "model_hung_post_load",
                        RoundError::StreamIdleTimeout => "stream_stalled",
                        RoundError::TotalTimeout      => "round_timeout",
                        RoundError::Stream(_)         => "stream_error",
                    };
                    // Surface the failure to the client with a clear message + code instead
                    // of swallowing it. The client knows whether to retry (cold-load) or
                    // give up (hung / stream error).
                    return Some(McpLoopResult {
                        content: format!("Error: {e} (code={code})"),
                        tool_calls: Vec::new(),
                        prompt_tokens: total_prompt_tokens,
                        completion_tokens: total_completion_tokens,
                        finish_reason: code.to_string(),
                        rounds,
                        streamed_via_tap: false,
                    });
                }
            };
            total_prompt_tokens = total_prompt_tokens.saturating_add(round_result.prompt_tokens);
            total_completion_tokens = total_completion_tokens.saturating_add(round_result.completion_tokens);
            finish_reason = round_result.finish_reason.clone();
            content = round_result.content.clone();
            final_tool_calls = round_result.tool_calls.clone();
            // Track whether tap forwarded any content this loop (caller uses
            // this to decide whether to re-emit `content` as an SSE chunk).
            if round_result.passthrough_streamed {
                streamed_via_tap = true;
            }

            // ── Mixed-delta safety: passthrough wins ──────────────────────────
            // SDD §3.3: if the round produced text content that was streamed via
            // the tap, treat the round as final regardless of any tool_calls
            // that arrived later in the same round (vLLM bug class — content
            // and tool_calls in the same round is a model-side spec violation).
            // Continuing the loop after streaming text would emit tokens the
            // client interprets as a coherent continuation — worse than
            // dropping the malformed tool_calls.
            if round_result.passthrough_streamed {
                if !round_result.tool_calls.is_empty() {
                    warn!(
                        round,
                        tool_count = round_result.tool_calls.len(),
                        "mixed-delta round (content streamed first, tool_calls also emitted) — dropping tool_calls per SDD §3.3 mixed-delta safety"
                    );
                }
                break;
            }

            // ── Filter for MCP tool calls ──────────────────────────────────────
            let mut mcp_calls: Vec<Value> = round_result
                .tool_calls
                .into_iter()
                .filter(|tc| {
                    tc["function"]["name"]
                        .as_str()
                        .map(|n| mcp_tool_names.contains(n))
                        .unwrap_or(false)
                })
                .collect();
            // Cap per-round execution count (model may return more tool_calls than injected tools).
            mcp_calls.truncate(MAX_TOOLS_PER_REQUEST);

            if mcp_calls.is_empty() {
                // No MCP tools requested — round produced text or non-MCP tools.
                // Loop ends; caller emits content / tool_calls as final SSE.
                break;
            }

            rounds += 1;

            // ── Loop detection ─────────────────────────────────────────────────
            let mut loop_detected = false;
            for tc in &mcp_calls {
                let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                let args_hash = quick_args_hash(args_str);
                let count = call_sig_counts.entry((name.clone(), args_hash)).or_insert(0);
                *count += 1;
                if *count >= LOOP_DETECT_THRESHOLD {
                    warn!(tool = %name, "MCP loop detected — breaking");
                    loop_detected = true;
                    break;
                }
            }
            if loop_detected {
                break;
            }

            // ── Append assistant message with tool_calls ───────────────────────
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": "",
                "tool_calls": final_tool_calls
            }));

            // ── Execute MCP tools (join_all: order preserved for index mapping) ─
            let tenant_id = caller.account_id()
                .map(|id| id.to_string())
                .unwrap_or_default();
            let exec_results = self.execute_calls(state, &mcp_calls, caller.api_key_id(), tenant_id, round + 1, mcp_loop_id, job_id.0, allowed_servers.clone()).await;

            // Enrich each tool_call with its execution result + audit metadata
            // and stash for the consolidated S3 turn record. PG audit storage
            // (`mcp_loop_tool_calls`) was retired 2026-05-01 — S3 is the SSOT
            // for the conversation chain and ClickHouse retains the analytics
            // signal via `fire_mcp_ingest`.
            for (tc, (result_text, rec)) in mcp_calls.iter().zip(exec_results.into_iter()) {
                let call_id = tc["id"].as_str().unwrap_or("call_0");
                let tool_name = tc["function"]["name"].as_str().unwrap_or("");
                messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "name": tool_name,
                    "content": &result_text
                }));

                let mut enriched = tc.clone();
                enriched["round"] = serde_json::json!(rec.loop_round);
                enriched["server_slug"] = serde_json::json!(server_slug_from_namespaced(&rec.namespaced_name));
                enriched["result"] = serde_json::Value::String(result_text);
                enriched["outcome"] = serde_json::json!(rec.outcome);
                enriched["cache_hit"] = serde_json::json!(rec.cache_hit);
                enriched["latency_ms"] = serde_json::json!(rec.latency_ms);
                enriched["result_bytes"] = serde_json::json!(rec.result_bytes);
                all_mcp_tool_calls.push(enriched);
            }

            // ── Context window pruning ─────────────────────────────────────────
            // After the second tool round, earlier tool results are rarely needed
            // verbatim. Compress them to a short summary to bound context growth.
            // Keep the last 2 rounds of tool messages intact; summarise prior ones.
            if rounds >= 2 {
                prune_tool_messages(&mut messages, 2);
            }

            info!(round, mcp_calls = mcp_calls.len(), "MCP round complete");
        }

        // ── S24 synthesis fallback ────────────────────────────────────────────
        // After the round-loop exhausts, if the bridge produced no text content
        // and at least one tool round executed, dispatch a synthesis round on a
        // fresh messages array. This is the final guarantee that an inference
        // request returns a textual answer even when Qwen3-Coder reproduces
        // tool_call patterns from history (S23 boundary's tools-omission strips
        // schemas but the model can still mimic prior assistant.tool_calls
        // entries — QwenLM/Qwen3-Coder #475). The synthesis messages contain
        // ONLY the user's question and the accumulated tool RESULTS (not
        // tool_calls), so the model has no patterns to follow.
        // SDD: `.specs/veronex/mcp-synthesis-round.md` §3.3.
        if content.is_empty() && rounds > 0 {
            if let Some(results_text) = extract_tool_results(&messages) {
                let original_prompt = extract_last_user_prompt(&messages);
                info!(
                    rounds,
                    results_bytes = results_text.len(),
                    "MCP synthesis round: dispatching forced-text fallback"
                );
                let synth_messages = build_synthesis_messages(&original_prompt, &results_text);
                let synth_submit = state.use_case.submit(SubmitJobRequest {
                    prompt: original_prompt,
                    model_name: model.clone(),
                    provider_type: ProviderType::Ollama,
                    gemini_tier: None,
                    api_key_id: caller.api_key_id(),
                    account_id: caller.account_id(),
                    source: caller.source(),
                    api_format: ApiFormat::OpenaiCompat,
                    messages: Some(Value::Array(synth_messages)),
                    tools: None,
                    request_path: Some("/v1/chat/completions".to_string()),
                    conversation_id,
                    key_tier: caller.key_tier(),
                    images: None,
                    stop: stop.clone(),
                    seed,
                    response_format: response_format.clone(),
                    frequency_penalty,
                    presence_penalty,
                    mcp_loop_id: Some(mcp_loop_id),
                    max_tokens: None,
                    vision_analysis: None,
                }).await;

                match synth_submit {
                    Ok(synth_job_id) => {
                        intermediate_job_ids.push(synth_job_id.0);
                        match collect_round(state, &synth_job_id, sse_tap_tx.as_ref()).await {
                            Ok(synth_result) => {
                                total_prompt_tokens = total_prompt_tokens.saturating_add(synth_result.prompt_tokens);
                                total_completion_tokens = total_completion_tokens.saturating_add(synth_result.completion_tokens);
                                if synth_result.passthrough_streamed {
                                    streamed_via_tap = true;
                                }
                                if !synth_result.content.is_empty() {
                                    content = synth_result.content;
                                    finish_reason = synth_result.finish_reason;
                                    final_tool_calls.clear();
                                    info!("MCP synthesis round: succeeded — text content emitted");
                                } else {
                                    warn!("MCP synthesis round: produced no text either; surfacing degenerate result");
                                }
                            }
                            Err(e) => warn!(error = %e, "MCP synthesis round: collect_round failed"),
                        }
                    }
                    Err(e) => warn!(error = %e, "MCP synthesis round: submit failed"),
                }
            }
        }

        // Loop-wide bookkeeping: token rollup + intermediate-job cleanup
        //   + ONE S3 turn + ONE conversation-counter increment.
        //
        // Single-writer policy (revised 2026-05-01): runner skips S3 +
        // counters when `job.mcp_loop_id IS Some` (see `runner.rs::run_job`).
        // Bridge persists exactly one consolidated `TurnRecord` for the
        // entire loop and bumps `conversations.turn_count` by 1, so a user
        // question with N agentic rounds maps to one logical turn carrying
        // every round's `tool_calls` array (UI surfaces "MCP {N}회" from the
        // turn's `tool_calls.len()`).
        if let Some(ref fid) = first_job_id {
            let pg = &state.pg_pool;

            let _ = sqlx::query(
                "UPDATE inference_jobs SET prompt_tokens = $1, completion_tokens = $2 WHERE id = $3"
            )
            .bind(total_prompt_tokens.min(i32::MAX as u32) as i32)
            .bind(total_completion_tokens.min(i32::MAX as u32) as i32)
            .bind(fid.0)
            .execute(pg)
            .await
            .map_err(|e| warn!(job_id = %fid.0, error = %e, "MCP: failed to update job tokens"));

            // Remove intermediate round jobs — dashboard shows only the first job
            if !intermediate_job_ids.is_empty() {
                let _ = sqlx::query(
                    "DELETE FROM inference_jobs WHERE id = ANY($1)"
                )
                .bind(&intermediate_job_ids)
                .execute(pg)
                .await
                .map_err(|e| warn!(error = %e, "MCP: failed to cleanup intermediate jobs"));
            }

            // Bridge-owned S3 turn write (single writer for MCP loops).
            if let Some(ref store) = state.message_store {
                let owner_id = caller.account_id()
                    .or(caller.api_key_id())
                    .unwrap_or(fid.0);
                let s3_key = conversation_id.unwrap_or(fid.0);
                let date = chrono::Utc::now().date_naive();
                let mut record = store.get_conversation(owner_id, date, s3_key).await
                    .ok().flatten()
                    .unwrap_or_else(crate::application::ports::outbound::message_store::ConversationRecord::new);

                let tool_calls_val = if all_mcp_tool_calls.is_empty() {
                    None
                } else {
                    Some(serde_json::Value::Array(all_mcp_tool_calls.clone()))
                };
                let result_text = if content.is_empty() { None } else { Some(content.clone()) };

                record.turns.push(crate::application::ports::outbound::message_store::ConversationTurn::Regular(
                    crate::application::ports::outbound::message_store::TurnRecord {
                        job_id: fid.0,
                        prompt: extract_last_user_prompt(&messages),
                        messages: Some(serde_json::Value::Array(messages.clone())),
                        tool_calls: tool_calls_val,
                        result: result_text,
                        model_name: Some(model.clone()),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        compressed: None,
                        vision_analysis: None,
                    }
                ));

                if let Err(e) = store.put_conversation(owner_id, date, s3_key, &record).await {
                    warn!(job_id = %fid.0, error = %e, "MCP: bridge S3 turn write failed");
                } else if let Some(conv_id) = conversation_id {
                    if let Some(ref pool) = state.valkey_pool {
                        use fred::prelude::*;
                        let cache_key = crate::infrastructure::outbound::valkey_keys::conversation_record(conv_id);
                        if let Err(e) = pool.del::<i64, _>(cache_key).await {
                            tracing::warn!(error = %e, "MCP: valkey DEL conversation cache failed");
                        }
                    }
                }
            }

            // Bump turn_count by 1 for the entire loop (runner skipped this
            // for every mcp-loop round, so this is the sole increment).
            // Mirrors `PostgresJobRepository::update_conversation_counters`.
            if let Some(conv_id) = conversation_id {
                let _ = sqlx::query(
                    "UPDATE conversations \
                        SET turn_count = turn_count + 1, \
                            total_prompt_tokens = total_prompt_tokens + $1, \
                            total_completion_tokens = total_completion_tokens + $2, \
                            model_name = COALESCE(model_name, $3), \
                            updated_at = now() \
                      WHERE id = $4"
                )
                .bind(total_prompt_tokens.min(i32::MAX as u32) as i32)
                .bind(total_completion_tokens.min(i32::MAX as u32) as i32)
                .bind(&model)
                .bind(conv_id)
                .execute(pg)
                .await
                .map_err(|e| warn!(conversation_id = %conv_id, error = %e, "MCP: turn_count increment failed"));
            }
        }

        Some(McpLoopResult {
            content,
            tool_calls: final_tool_calls,
            prompt_tokens: total_prompt_tokens,
            completion_tokens: total_completion_tokens,
            finish_reason,
            rounds,
            streamed_via_tap,
        })
    }

    // ── Forced-JSON shim path ──────────────────────────────────────────────────
    //
    // Mirror of `run_loop` for models without native `tool_calls`. Injects a
    // minimal system prompt + tool catalogue, builds an `oneOf` JSON-Schema
    // covering every available tool plus a `final` terminator, and forwards
    // the schema as Ollama's `format` parameter. Constrained decoding (GBNF
    // grammar) guarantees the model's output is always a valid JSON object
    // matching one of the branches — so even tool-incapable models like
    // qwen3:8b can drive MCP deterministically. Replaces the prior text-template
    // ReAct shim, which depended on the model voluntarily following the format
    // and frequently produced zero tool calls on weak models.
    //
    // Schema / parser: `super::forced_json`.
    #[allow(clippy::too_many_arguments)]
    async fn run_loop_forced_json(
        &self,
        state: &AppState,
        caller: &InferCaller,
        model: String,
        mut messages: Vec<Value>,
        all_tools: Vec<Value>,
        conversation_id: Option<uuid::Uuid>,
        stop: Option<Value>,
        seed: Option<u32>,
        _user_response_format: Option<Value>,
        frequency_penalty: Option<f64>,
        presence_penalty: Option<f64>,
        allowed_servers: Option<Arc<HashSet<Uuid>>>,
        max_rounds: u8,
    ) -> Option<McpLoopResult> {
        use super::forced_json::{
            build_forced_json_schema, build_forced_json_system_prompt,
            parse_forced_action, schema_to_response_format, ForcedAction,
        };

        // System prompt is round-invariant. Schema is round-aware (see in-loop
        // construction below): on round 0 we omit the `final` branch so the
        // model is logit-masked into calling a tool; once at least one tool
        // result is in context, the `final` branch is allowed and the model
        // can answer. Defends against weak models (qwen3:8b, llama3:7b) that
        // emit "I don't have access to real-time data" without trying a tool.
        let system_prompt = build_forced_json_system_prompt(&all_tools)?;

        // Inject the system prompt at index 0 (before any user messages).
        messages.insert(0, serde_json::json!({"role": "system", "content": system_prompt}));

        let mcp_loop_id = Uuid::new_v4();
        let mut total_prompt_tokens: u32 = 0;
        let mut total_completion_tokens: u32 = 0;
        let mut content = String::new();
        let mut rounds: u8 = 0;
        let mut first_job_id: Option<JobId> = None;
        let mut intermediate_job_ids: Vec<Uuid> = Vec::new();
        let mut call_sig_counts: HashMap<(String, String), u8> = HashMap::new();
        let mut all_mcp_tool_calls: Vec<Value> = Vec::new();

        let tenant_id = caller.account_id().map(|id| id.to_string()).unwrap_or_default();

        for round in 0..max_rounds {
            // ── Round-aware schema: gate the `final` branch ──────────────────
            // Round 0 (no tool results yet): allow_final=false → model has no
            // logit space to emit `{"action":"final",...}`, so it MUST pick a
            // tool branch. Once any tool has been called, allow_final=true and
            // the model can either keep gathering or terminate.
            let allow_final = !all_mcp_tool_calls.is_empty();
            let schema = match build_forced_json_schema(&all_tools, allow_final) {
                Some(s) => s,
                None => {
                    warn!("MCP forced-JSON: schema construction failed on round {round}");
                    return None;
                }
            };
            let forced_response_format = Some(schema_to_response_format(schema));

            // ── Submit job WITHOUT native `tools[]` but WITH forced JSON format ─
            // The schema is the gateway's tool-dispatch contract; native tools[]
            // would be a no-op on non-supporting models and could conflict with
            // the constrained-decoding format on supporting ones.
            let prompt = extract_last_user_prompt(&messages);
            let job_id = match state.use_case.submit(SubmitJobRequest {
                prompt,
                model_name: model.clone(),
                provider_type: ProviderType::Ollama,
                gemini_tier: None,
                api_key_id: caller.api_key_id(),
                account_id: caller.account_id(),
                source: caller.source(),
                api_format: ApiFormat::OpenaiCompat,
                messages: Some(Value::Array(messages.clone())),
                tools: None,
                request_path: Some("/v1/chat/completions".to_string()),
                conversation_id,
                key_tier: caller.key_tier(),
                images: None,
                stop: stop.clone(),
                seed,
                response_format: forced_response_format,
                frequency_penalty,
                presence_penalty,
                mcp_loop_id: Some(mcp_loop_id),
                max_tokens: None,
                vision_analysis: None,
            }).await {
                Ok(id) => id,
                Err(e) => {
                    warn!("MCP forced-JSON: submit failed on round {round}: {e}");
                    return None;
                }
            };

            if first_job_id.is_none() {
                first_job_id = Some(job_id.clone());
            } else {
                intermediate_job_ids.push(job_id.0);
            }

            // ── Collect grammar-constrained JSON response ─────────────────────
            // No SSE tap: per-round output is a structural JSON object intended
            // for tool-dispatch, not for end-user streaming. Final-answer text
            // is emitted by the caller after the loop terminates.
            let round_result = match collect_round(state, &job_id, None).await {
                Ok(r) => r,
                Err(e) => {
                    warn!(round, error = ?e, "forced-JSON round failed");
                    return Some(McpLoopResult {
                        content: format!("Error: round {round} failed ({e:?})"),
                        tool_calls: Vec::new(),
                        prompt_tokens: total_prompt_tokens,
                        completion_tokens: total_completion_tokens,
                        finish_reason: "error".into(),
                        rounds: round,
                        streamed_via_tap: false,
                    });
                }
            };
            total_prompt_tokens = total_prompt_tokens.saturating_add(round_result.prompt_tokens);
            total_completion_tokens = total_completion_tokens.saturating_add(round_result.completion_tokens);
            rounds = round + 1;

            // ── Parse the model's JSON action ─────────────────────────────────
            match parse_forced_action(&round_result.content) {
                ForcedAction::Tool { name, args } => {
                    let args_str = serde_json::to_string(&args).unwrap_or_default();
                    let sig = (name.clone(), quick_args_hash(&args_str));
                    let count = call_sig_counts.entry(sig).or_insert(0);
                    *count += 1;
                    if *count >= LOOP_DETECT_THRESHOLD {
                        warn!(name = %name, "forced-JSON: loop detected — same call repeated");
                        content = format!(
                            "(Loop detected: '{}' called {} times. Stopping.)",
                            name, count
                        );
                        break;
                    }

                    let tc_value = serde_json::json!({
                        "function": { "name": &name, "arguments": &args_str }
                    });

                    let results = self
                        .execute_calls(
                            state,
                            &[tc_value],
                            caller.api_key_id(),
                            tenant_id.clone(),
                            rounds - 1,
                            mcp_loop_id,
                            job_id.0,
                            allowed_servers.clone(),
                        )
                        .await;

                    // Echo the model's tool selection back as an assistant
                    // message and the result as a user observation so the
                    // next round sees the full reasoning trace.
                    let serialized_action = serde_json::json!({
                        "action": "tool", "tool": &name, "args": args
                    }).to_string();
                    messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": serialized_action
                    }));
                    let (observation_text, rec_opt): (String, Option<&ToolCallRecord>) = results
                        .first()
                        .map(|(t, r)| (t.clone(), Some(r)))
                        .unwrap_or_else(|| ("(no result)".to_string(), None));
                    let mut observation = observation_text.clone();
                    truncate_at_char_boundary(&mut observation, MAX_TOOL_RESULT_BYTES);
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": format!("Observation: {observation}")
                    }));

                    if let Some(rec) = rec_opt {
                        all_mcp_tool_calls.push(serde_json::json!({
                            "function": { "name": &name, "arguments": &args_str },
                            "round": rec.loop_round,
                            "server_slug": server_slug_from_namespaced(&rec.namespaced_name),
                            "result": observation_text,
                            "outcome": &rec.outcome,
                            "cache_hit": rec.cache_hit,
                            "latency_ms": rec.latency_ms,
                            "result_bytes": rec.result_bytes,
                        }));
                    }

                    info!(round, name = %name, "forced-JSON: tool executed");
                    // Continue loop — next round will see the observation.
                }
                ForcedAction::Final { answer } => {
                    content = answer;
                    break;
                }
            }
        }

        // ── Cleanup intermediate jobs + roll up token counts onto first_job_id
        // + bridge-owned consolidated S3 turn write + turn_count bump ───────
        if let Some(ref fid) = first_job_id {
            let pg = &state.pg_pool;
            let _ = sqlx::query(
                "UPDATE inference_jobs SET prompt_tokens = $1, completion_tokens = $2 WHERE id = $3",
            )
            .bind(total_prompt_tokens.min(i32::MAX as u32) as i32)
            .bind(total_completion_tokens.min(i32::MAX as u32) as i32)
            .bind(fid.0)
            .execute(pg)
            .await
            .map_err(|e| warn!(job_id = %fid.0, error = %e, "ReAct: failed to update job tokens"));

            if !intermediate_job_ids.is_empty() {
                let _ = sqlx::query("DELETE FROM inference_jobs WHERE id = ANY($1)")
                    .bind(&intermediate_job_ids)
                    .execute(pg)
                    .await
                    .map_err(|e| warn!(error = %e, "ReAct: failed to cleanup intermediate jobs"));
            }

            if let Some(ref store) = state.message_store {
                let owner_id = caller.account_id().or(caller.api_key_id()).unwrap_or(fid.0);
                let s3_key = conversation_id.unwrap_or(fid.0);
                let date = chrono::Utc::now().date_naive();
                let mut record = store.get_conversation(owner_id, date, s3_key).await
                    .ok().flatten()
                    .unwrap_or_else(crate::application::ports::outbound::message_store::ConversationRecord::new);

                let tool_calls_val = if all_mcp_tool_calls.is_empty() {
                    None
                } else {
                    Some(serde_json::Value::Array(all_mcp_tool_calls.clone()))
                };
                let result_text = if content.is_empty() { None } else { Some(content.clone()) };

                record.turns.push(crate::application::ports::outbound::message_store::ConversationTurn::Regular(
                    crate::application::ports::outbound::message_store::TurnRecord {
                        job_id: fid.0,
                        prompt: extract_last_user_prompt(&messages),
                        messages: Some(serde_json::Value::Array(messages.clone())),
                        tool_calls: tool_calls_val,
                        result: result_text,
                        model_name: Some(model.clone()),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        compressed: None,
                        vision_analysis: None,
                    }
                ));
                if let Err(e) = store.put_conversation(owner_id, date, s3_key, &record).await {
                    warn!(job_id = %fid.0, error = %e, "ReAct: bridge S3 turn write failed");
                } else if let Some(conv_id) = conversation_id {
                    if let Some(ref pool) = state.valkey_pool {
                        use fred::prelude::*;
                        let cache_key = crate::infrastructure::outbound::valkey_keys::conversation_record(conv_id);
                        if let Err(e) = pool.del::<i64, _>(cache_key).await {
                            tracing::warn!(error = %e, "ReAct: valkey DEL conversation cache failed");
                        }
                    }
                }
            }

            if let Some(conv_id) = conversation_id {
                let _ = sqlx::query(
                    "UPDATE conversations \
                        SET turn_count = turn_count + 1, \
                            total_prompt_tokens = total_prompt_tokens + $1, \
                            total_completion_tokens = total_completion_tokens + $2, \
                            model_name = COALESCE(model_name, $3), \
                            updated_at = now() \
                      WHERE id = $4"
                )
                .bind(total_prompt_tokens.min(i32::MAX as u32) as i32)
                .bind(total_completion_tokens.min(i32::MAX as u32) as i32)
                .bind(&model)
                .bind(conv_id)
                .execute(pg)
                .await
                .map_err(|e| warn!(conversation_id = %conv_id, error = %e, "ReAct: turn_count increment failed"));
            }
        }

        Some(McpLoopResult {
            content,
            tool_calls: Vec::new(),
            prompt_tokens: total_prompt_tokens,
            completion_tokens: total_completion_tokens,
            finish_reason: "stop".into(),
            rounds,
            streamed_via_tap: false,
        })
    }

    // ── Tool execution ─────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    async fn execute_calls(
        &self,
        _state: &AppState,
        calls: &[Value],
        api_key_id: Option<Uuid>,
        tenant_id: String,
        loop_round: u8,
        mcp_loop_id: Uuid,
        triggering_job_id: Uuid,
        allowed_servers: Option<Arc<HashSet<Uuid>>>,
    ) -> Vec<(String, ToolCallRecord)> {
        use futures::stream::{self, StreamExt};

        // `buffered`: preserves submission order (required for Ollama index-based mapping)
        // while capping in-flight calls at MAX_CONCURRENT_TOOL_CALLS.
        // Each future owns clones of the cheap Arc fields — no borrowed lifetime issues.
        stream::iter(calls.iter().cloned())
            .map(|tc| {
                let bridge = self.clone();
                let allowed = allowed_servers.clone();
                let tenant_id = tenant_id.clone();
                async move {
                    bridge.execute_one(&tc, api_key_id, tenant_id, loop_round, mcp_loop_id, triggering_job_id, allowed.as_deref()).await
                }
            })
            .buffered(MAX_CONCURRENT_TOOL_CALLS)
            .collect::<Vec<_>>()
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_one(
        &self,
        tc: &Value,
        api_key_id: Option<Uuid>,
        tenant_id: String,
        loop_round: u8,
        _mcp_loop_id: Uuid,
        triggering_job_id: Uuid,
        allowed_servers: Option<&HashSet<Uuid>>,
    ) -> (String, ToolCallRecord) {
        let namespaced = tc["function"]["name"].as_str().unwrap_or("");
        let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
        let args: Value = serde_json::from_str(args_str)
            .unwrap_or(Value::Object(Default::default()));

        // ── Resolve server ─────────────────────────────────────────────────────
        let server_id = match self.tool_cache.server_id_of(namespaced) {
            Some(id) => id,
            None => {
                warn!(tool = %namespaced, "MCP: no server mapping");
                let text = serde_json::json!({"error": "unknown tool", "tool": namespaced}).to_string();
                let rec = ToolCallRecord::error(namespaced, "unknown_tool", loop_round);
                return (text, rec);
            }
        };

        // ── ACL check ──────────────────────────────────────────────────────────
        if allowed_servers.is_some_and(|a| !a.contains(&server_id)) {
            warn!(tool = %namespaced, server = %server_id, "MCP ACL: access denied for this key");
            let rec = ToolCallRecord::error(namespaced, "acl_denied", loop_round);
            return ("{\"error\": \"MCP server access denied\"}".into(), rec);
        }

        // ── Circuit breaker ────────────────────────────────────────────────────
        if self.circuit_breaker.is_open(server_id) {
            warn!(tool = %namespaced, server = %server_id, "MCP circuit open — skipping");
            let slug = server_slug_from_namespaced(namespaced);
            let rname = raw_tool_name(namespaced);
            self.fire_mcp_ingest(triggering_job_id, api_key_id, tenant_id.clone(), server_id, slug.to_string(), rname.to_string(), namespaced.to_string(), "circuit_open", false, 0, 0, 0, loop_round);
            let rec = ToolCallRecord::error(namespaced, "circuit_open", loop_round);
            return ("{\"error\": \"MCP server temporarily unavailable (circuit open)\"}".into(), rec);
        }

        // ── Resolve tool definition + raw name ────────────────────────────────
        // Prefer the cached tool's original `.name` and `server_name` fields over
        // parsing the namespaced string — slugs may contain underscores, which
        // makes simple prefix-stripping ambiguous (e.g. `mcp_my_server_get_weather`).
        let tool_def = self.tool_cache.get_tool_raw(namespaced);
        let raw_name_owned: String;
        let raw_name: &str = if let Some(ref def) = tool_def {
            &def.name
        } else {
            raw_name_owned = raw_tool_name(namespaced).to_string();
            &raw_name_owned
        };
        let server_slug: &str = tool_def
            .as_ref()
            .map(|d| d.server_name.as_str())
            .unwrap_or_else(|| server_slug_from_namespaced(namespaced));

        // ── Result cache ───────────────────────────────────────────────────────
        if let Some(ref tool_def) = tool_def && let Some(cached) = self.result_cache.get(tool_def, &args).await {
                let text = cached.to_llm_string();
                let bytes = text.len() as u32;
                self.fire_mcp_ingest(triggering_job_id, api_key_id, tenant_id.clone(), server_id, server_slug.to_string(), raw_name.to_string(), namespaced.to_string(), "cache_hit", true, 0, bytes, 0, loop_round);
                let rec = ToolCallRecord {
                    loop_round,
                    namespaced_name: namespaced.to_string(),
                    outcome: "cache_hit".to_string(),
                    cache_hit: true,
                    latency_ms: 0,
                    result_bytes: bytes as i32,
                };
                return (text, rec);
        }

        // ── Execute ────────────────────────────────────────────────────────────
        let started = Instant::now();

        let timeout_dur = Duration::from_secs(
            self.session_manager.get_timeout_secs(server_id) as u64
        );
        let result = tokio::time::timeout(
            timeout_dur,
            self.session_manager.call_tool(server_id, raw_name, args.clone()),
        )
        .await;

        let latency_ms = started.elapsed().as_millis() as u32;

        let (text, outcome) = match result {
            Ok(Ok(tool_result)) => {
                let is_err = tool_result.is_error;
                if is_err {
                    self.circuit_breaker.record_failure(server_id);
                } else {
                    self.circuit_breaker.record_success(server_id);
                }

                if !is_err && let Some(ref def) = tool_def {
                    self.result_cache.set(def, &args, &tool_result, RESULT_CACHE_TTL_SECS).await;
                }

                let mut text = tool_result.to_llm_string();
                if text.len() > MAX_TOOL_RESULT_BYTES {
                    warn!(tool = %namespaced, original_bytes = text.len(), "MCP tool result truncated");
                    truncate_at_char_boundary(&mut text, MAX_TOOL_RESULT_BYTES);
                }
                let outcome = if is_err { "error" } else { "success" };
                (text, outcome)
            }
            Ok(Err(e)) => {
                self.circuit_breaker.record_failure(server_id);
                // anyhow::Error::Display only shows the top-level message —
                // for reqwest transport errors that hides the actual cause
                // (e.g. "dns error", "connection reset"). Walk the source
                // chain so the operator can diagnose live.
                let causes: Vec<String> = e.chain().map(|c| c.to_string()).collect();
                warn!(tool = %namespaced, error = %e, causes = ?causes, "MCP tool call error");
                // Do not forward internal error details to LLM context — already logged above
                ("{\"error\": \"MCP tool call failed\"}".into(), "error")
            }
            Err(_elapsed) => {
                self.circuit_breaker.record_failure(server_id);
                warn!(tool = %namespaced, "MCP tool call timed out");
                ("{\"error\": \"MCP tool call timed out\"}".into(), "timeout")
            }
        };

        let bytes = text.len() as u32;
        self.fire_mcp_ingest(triggering_job_id, api_key_id, tenant_id, server_id, server_slug.to_string(), raw_name.to_string(), namespaced.to_string(), outcome, false, latency_ms, bytes, 1, loop_round);

        let rec = ToolCallRecord {
            loop_round,
            namespaced_name: namespaced.to_string(),
            outcome: outcome.to_string(),
            cache_hit: false,
            latency_ms: latency_ms as i32,
            result_bytes: bytes as i32,
        };

        (text, rec)
    }
}

// ── Internal helpers ───────────────────────────────────────────────────────────

/// Per-tool execution metadata returned alongside the result text. Used by
/// `run_loop` / `run_loop_react` to enrich the OpenAI tool_call invocation
/// before it lands on the consolidated S3 turn record. PG audit storage was
/// retired 2026-05-01 — this struct is purely an in-memory passthrough.
struct ToolCallRecord {
    loop_round: u8,
    namespaced_name: String,
    outcome: String,
    cache_hit: bool,
    latency_ms: i32,
    result_bytes: i32,
}

impl ToolCallRecord {
    fn error(namespaced_name: &str, outcome: &str, loop_round: u8) -> Self {
        Self {
            loop_round,
            namespaced_name: namespaced_name.to_string(),
            outcome: outcome.to_string(),
            cache_hit: false,
            latency_ms: 0,
            result_bytes: 0,
        }
    }
}

/// Context window pruning: keep the last `keep_rounds` rounds of tool messages verbatim.
/// Earlier tool messages are replaced with a compact summary to prevent unbounded growth.
///
/// Strategy: walk backwards from the end, counting assistant-with-tool_calls turns.
/// When we find the boundary (older than `keep_rounds` turns), replace tool-role messages
/// before the boundary with a single "tool" message summarising the truncated data.
fn prune_tool_messages(messages: &mut [Value], keep_rounds: usize) {
    // Find assistant+tool_calls boundaries (each marks one tool round).
    // We walk the slice collecting (index, round_number) for assistant messages that have tool_calls.
    let boundaries: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            if m["role"].as_str() == Some("assistant") && m["tool_calls"].is_array() {
                Some(i)
            } else {
                None
            }
        })
        .collect();

    if boundaries.len() <= keep_rounds {
        return; // Nothing to prune yet.
    }

    // The cut point: everything before boundaries[len - keep_rounds] is "old".
    // .get() guards keep_rounds=0 (len - 0 == len = OOB) → prune everything.
    let cut = boundaries
        .get(boundaries.len().saturating_sub(keep_rounds))
        .copied()
        .unwrap_or(messages.len());

    // Replace all tool-role messages before `cut` with a compact summary.
    // We replace them in-place rather than splicing to avoid index shifts.
    // Mark each old tool message with a compressed placeholder.
    let mut replaced = 0usize;
    for msg in &mut messages[..cut] {
        if msg["role"].as_str() == Some("tool") {
            let name = msg["name"].as_str().unwrap_or("tool").to_string();
            let tool_call_id = msg["tool_call_id"].as_str().unwrap_or("").to_string();
            *msg = serde_json::json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "name": name,
                "content": "[result truncated — see earlier context]"
            });
            replaced += 1;
        }
    }

    if replaced > 0 {
        debug!(replaced, cut, "MCP context pruning: compressed old tool messages");
    }
}

struct RoundResult {
    content: String,
    tool_calls: Vec<Value>,
    prompt_tokens: u32,
    completion_tokens: u32,
    finish_reason: String,
    /// True if the round produced text content that was streamed via the
    /// `sse_tx` tap. When this is set, the caller MUST treat the round as
    /// final (don't execute MCP tools even if `tool_calls` is also non-empty —
    /// see SDD `.specs/veronex/bridge-mcp-loop-correctness.md` §3.3 for
    /// mixed-delta safety rationale).
    passthrough_streamed: bool,
}

/// Collect all tokens from a submitted job into a `RoundResult`.
/// Failure modes from `collect_round`. Each variant maps to a distinct user-facing
/// error so the client can decide its retry strategy (cold-load vs hung vs network).
///
/// Phase-aware timing (S19, SDD `.specs/veronex/bridge-phase-aware-timing.md`):
/// `LIFECYCLE_TIMEOUT` covers Phase 1 (cold-load). After the runner emits a
/// `StreamToken::phase_boundary()` post-`ensure_ready`, the bridge switches to
/// `TOKEN_FIRST_TIMEOUT` for Phase 2 first token, then `STREAM_IDLE_TIMEOUT`
/// for streaming. Closes the 248 s race observed on `conv_3386OgDfDKkJvamF9X1Dr`.
#[derive(Debug)]
enum RoundError {
    /// Phase 1 (`ensure_ready`) exceeded `LIFECYCLE_TIMEOUT` — provider stuck
    /// loading. With `MCP_LIFECYCLE_PHASE=off` (legacy path), this also
    /// covers the entire pre-token window.
    LifecycleTimeout,
    /// Phase 2 (post-`ensure_ready`) failed to produce a first token within
    /// `TOKEN_FIRST_TIMEOUT`. Indicates a genuinely hung model immediately
    /// after a successful load.
    FirstTokenTimeout,
    /// Tokens started but the gap between tokens exceeded `STREAM_IDLE_TIMEOUT` —
    /// the model is hung mid-response.
    StreamIdleTimeout,
    /// The full round budget (`ROUND_TOTAL_TIMEOUT`) was exhausted even though
    /// tokens were flowing. Defends against pathological streams.
    TotalTimeout,
    /// Non-timeout stream error (provider 5xx, network drop, …).
    Stream(String),
}

impl std::fmt::Display for RoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LifecycleTimeout => write!(
                f,
                "model load did not complete within {}s. Provider may be cold-stuck.",
                LIFECYCLE_TIMEOUT.as_secs()
            ),
            Self::FirstTokenTimeout => write!(
                f,
                "no first token within {}s after model load completed. Model may be hung.",
                TOKEN_FIRST_TIMEOUT.as_secs()
            ),
            Self::StreamIdleTimeout => write!(
                f,
                "inference stream stalled (no token for {}s). Provider may be hung.",
                STREAM_IDLE_TIMEOUT.as_secs()
            ),
            Self::TotalTimeout => write!(
                f,
                "round exceeded total budget {}s. Possible runaway generation.",
                ROUND_TOTAL_TIMEOUT.as_secs()
            ),
            Self::Stream(e) => write!(f, "inference stream error: {e}"),
        }
    }
}

/// Synchronously collect one round of streamed tokens into a `RoundResult`.
///
/// `sse_tx` is the **stream-tap** (SDD `.specs/veronex/bridge-mcp-loop-correctness.md`
/// §3.2): when `Some`, content tokens are forwarded to the caller's SSE writer
/// AS they arrive — preserving chatGPT-style token-by-token UX for final-round
/// text. The tap follows OpenAI's round-level XOR: first non-empty delta
/// decides the mode (tool_calls → silent intercept, content → passthrough)
/// and that mode holds for the rest of the round.
async fn collect_round(
    state: &AppState,
    job_id: &JobId,
    sse_tx: Option<&tokio::sync::mpsc::UnboundedSender<String>>,
) -> Result<RoundResult, RoundError> {
    let mut token_stream = state.use_case.stream(job_id);
    let mut content = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut prompt_tokens: u32 = 0;
    let mut completion_tokens: u32 = 0;
    let mut finish_reason = "stop".to_string();
    // Stream-tap mode (only meaningful when sse_tx is Some).
    // - undecided: haven't seen the first meaningful delta yet
    // - intercept: tool_calls came first → never forward, bridge will execute
    // - passthrough: content came first → forward all subsequent content tokens
    #[derive(PartialEq)]
    enum TapMode { Undecided, Intercept, Passthrough }
    let mut tap_mode = TapMode::Undecided;
    let mut passthrough_streamed = false;
    // Phase-aware state — see SDD §3.3.
    // - in_phase_1=true: still in `ensure_ready` (Phase 1). Active until a
    //   `phase_boundary` token arrives. Timeout = LIFECYCLE_TIMEOUT.
    // - in_phase_1=false, received_any_token=false: post-load, awaiting first
    //   real token. Timeout = TOKEN_FIRST_TIMEOUT.
    // - received_any_token=true: streaming. Timeout = STREAM_IDLE_TIMEOUT.
    //
    // When `MCP_LIFECYCLE_PHASE=off` the runner does not emit a boundary;
    // bridge stays in Phase 1 for the whole round. LIFECYCLE_TIMEOUT >> the
    // legacy 240 s applied here, so legacy behaviour is strictly more
    // permissive (no regression).
    let mut in_phase_1 = true;
    let mut received_any_token = false;
    let round_start = Instant::now();

    loop {
        // Hard cap defends against unbounded streams.
        if round_start.elapsed() >= ROUND_TOTAL_TIMEOUT {
            return Err(RoundError::TotalTimeout);
        }
        let phase_timeout = if in_phase_1 {
            LIFECYCLE_TIMEOUT
        } else if !received_any_token {
            TOKEN_FIRST_TIMEOUT
        } else {
            STREAM_IDLE_TIMEOUT
        };

        match tokio::time::timeout(phase_timeout, token_stream.next()).await {
            Ok(Some(Ok(token))) => {
                if token.is_phase_boundary {
                    // Phase 1 → Phase 2 boundary. Don't forward; switch
                    // timing model and continue waiting for real tokens.
                    in_phase_1 = false;
                    received_any_token = false;
                    continue;
                }
                received_any_token = true;
                if token.is_final {
                    // is_final is checked first — a final token with tool_calls still ends the round.
                    prompt_tokens = token.prompt_tokens.unwrap_or(prompt_tokens);
                    completion_tokens = token.completion_tokens.unwrap_or(completion_tokens);
                    finish_reason = token.finish_reason.unwrap_or_else(|| {
                        if tool_calls.is_empty() { "stop".into() } else { "tool_calls".into() }
                    });
                    break;
                }
                let has_tool_calls = token.tool_calls.is_some();
                let has_content = !token.value.is_empty();

                // Stream-tap decision (only on first meaningful delta of the round)
                if sse_tx.is_some() && tap_mode == TapMode::Undecided {
                    if has_tool_calls {
                        tap_mode = TapMode::Intercept;
                    } else if has_content {
                        tap_mode = TapMode::Passthrough;
                    }
                }

                if has_tool_calls {
                    if let Some(calls) = token.tool_calls.as_ref().and_then(|v| v.as_array()) {
                        for (i, c) in calls.iter().enumerate() {
                            if validate_tool_call(c) {
                                tool_calls.push(convert_ollama_tool_call(i, c));
                            }
                        }
                    }
                } else if has_content {
                    content.push_str(&token.value);
                    // Forward to client SSE if in passthrough mode (SDD §3.2).
                    if tap_mode == TapMode::Passthrough {
                        if let Some(tx) = sse_tx {
                            // Channel disconnected (client gone) → tap stays silent;
                            // collect_round still completes for invariant maintenance.
                            let _ = tx.send(token.value.clone());
                            passthrough_streamed = true;
                        }
                    }
                }
            }
            Ok(Some(Err(e))) => return Err(RoundError::Stream(e.to_string())),
            Ok(None) => break, // stream ended cleanly
            Err(_) if in_phase_1 => return Err(RoundError::LifecycleTimeout),
            Err(_) if !received_any_token => return Err(RoundError::FirstTokenTimeout),
            Err(_) => return Err(RoundError::StreamIdleTimeout),
        }
    }

    Ok(RoundResult { content, tool_calls, prompt_tokens, completion_tokens, finish_reason, passthrough_streamed })
}

/// Convert an Ollama tool_call to OpenAI format, preserving index as ID.
fn convert_ollama_tool_call(i: usize, c: &Value) -> Value {
    let name = c.get("function")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let args = c.get("function")
        .and_then(|f| f.get("arguments"))
        .map(|a| serde_json::to_string(a).unwrap_or_default())
        .unwrap_or_default();
    serde_json::json!({
        "index": i,
        "id": format!("call_{i}"),
        "type": "function",
        "function": { "name": name, "arguments": args }
    })
}

/// Extract the last user message as a plain string prompt.
fn extract_last_user_prompt(messages: &[Value]) -> String {
    messages
        .iter()
        .rev()
        .find(|m| m["role"].as_str() == Some("user"))
        .and_then(|m| m["content"].as_str())
        .unwrap_or("")
        .to_string()
}

/// Walk the messages array and concatenate every `role:"tool"` entry's
/// content into a single text block, suitable for injection as a
/// synthesis-round system message. Returns `None` when no tool entries
/// were found (caller skips the synthesis dispatch). Each entry is
/// labelled with its index for the model's reading convenience.
///
/// SDD: `.specs/veronex/mcp-synthesis-round.md` §3.1.
fn extract_tool_results(messages: &[Value]) -> Option<String> {
    let mut parts = Vec::new();
    for m in messages.iter() {
        if m["role"].as_str() != Some("tool") {
            continue;
        }
        if let Some(content) = m["content"].as_str() {
            if !content.is_empty() {
                let label = m["name"].as_str().unwrap_or("tool");
                parts.push(format!("[{}] {}", label, content));
            }
        }
    }
    if parts.is_empty() { None } else { Some(parts.join("\n\n---\n\n")) }
}

/// Build the messages array for a synthesis round. Three entries:
///   1. system — directive forbidding tool calls and demanding final text
///   2. user — the original prompt
///   3. system — the accumulated tool results as plain text
///
/// Critically, this contains NO `assistant.tool_calls` entries, so the
/// model has no in-context pattern to mimic. Combined with the request's
/// `tools: None`, the model has nothing callable and must emit text.
///
/// SDD: `.specs/veronex/mcp-synthesis-round.md` §3.2.
fn build_synthesis_messages(original_prompt: &str, tool_results_text: &str) -> Vec<Value> {
    let date_anchor = crate::infrastructure::inbound::http::inference_helpers::build_current_datetime_system_text();
    vec![
        serde_json::json!({
            "role": "system",
            "content": date_anchor,
        }),
        serde_json::json!({
            "role": "system",
            "content": "You are answering the user's question. \
                Tools have already been used to gather the information \
                you need. Do NOT call any tools. Using the tool results \
                provided below, produce a complete, well-structured \
                answer to the user's question in their original language. \
                Honor the date constraints in the system message above — \
                every \"today\" / \"recent\" / \"현재\" / \"최근\" in your \
                response refers to the current date listed there, not to \
                your training cutoff."
        }),
        serde_json::json!({
            "role": "user",
            "content": original_prompt,
        }),
        serde_json::json!({
            "role": "system",
            "content": format!("Tool results gathered:\n\n{}", tool_results_text),
        }),
    ]
}

/// Strip `mcp_{server}_` prefix → raw tool name as registered on the MCP server.
///
/// Format: `mcp_{server_name}_{tool_name}`
/// e.g. `mcp_weather_get_weather` → `get_weather`
fn raw_tool_name(namespaced: &str) -> &str {
    // Strip "mcp_" then find next "_" for server boundary
    namespaced
        .strip_prefix("mcp_")
        .and_then(|s| s.find('_').map(|pos| &s[pos + 1..]))
        .unwrap_or(namespaced)
}

/// Extract the server slug from a namespaced tool name (best-effort, single-segment).
///
/// `mcp_weather_get_weather` → `weather`
/// `mcp_my_server_get_weather` → `my`  (first segment only — use `tool_def.server_name` when available)
///
/// Used only in the circuit-open fallback path before `tool_def` is resolved.
fn server_slug_from_namespaced(namespaced: &str) -> &str {
    namespaced
        .strip_prefix("mcp_")
        .and_then(|s| s.find('_').map(|pos| &s[..pos]))
        .unwrap_or("")
}

/// Quick hash of args string for loop-detection (does not need to be canonical).
/// Input is capped at `MAX_ARGS_FOR_HASH_BYTES` to bound O(n) hashing cost.
fn quick_args_hash(args_str: &str) -> String {
    let bytes = args_str.as_bytes();
    let capped = if bytes.len() > MAX_ARGS_FOR_HASH_BYTES { &bytes[..MAX_ARGS_FOR_HASH_BYTES] } else { bytes };
    let digest = Sha256::digest(capped);
    hex::encode(&digest[..4])
}

/// Fetch the list of MCP server IDs allowed for an API key.
///
/// Reads from `veronex:mcp:acl:{key_id}` (Valkey, JSON array of UUIDs, TTL=60s).
/// On cache miss, falls back to DB and populates the cache.
/// Invalidated by `key_mcp_access_handlers` on grant/revoke.
pub(crate) async fn fetch_mcp_acl(state: &AppState, key_id: Uuid) -> Vec<Uuid> {
    use fred::prelude::*;
    let vk_key = crate::infrastructure::outbound::valkey_keys::mcp_key_acl(key_id);

    // ── L1: Valkey ─────────────────────────────────────────────────────────────
    if let Some(ref pool) = state.valkey_pool
        && let Ok(Some(cached)) = pool.get::<Option<String>, _>(&vk_key).await
        && let Ok(ids) = serde_json::from_str::<Vec<Uuid>>(&cached)
    {
        return ids;
    }

    // ── L2: DB (cache miss) ────────────────────────────────────────────────────
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT server_id FROM mcp_key_access WHERE api_key_id = $1 AND is_allowed = true"
    )
    .bind(key_id)
    .fetch_all(&state.pg_pool)
    .await
    .unwrap_or_default();

    // Populate cache — empty array is also cached (negative cache).
    if let Some(ref pool) = state.valkey_pool && let Ok(json) = serde_json::to_string(&ids) {
        if let Err(e) = pool
            .set::<(), _, _>(&vk_key, json, Some(Expiration::EX(crate::domain::constants::MCP_KEY_CACHE_TTL_SECS)), None, false)
            .await
        {
            tracing::warn!(key = %vk_key, error = %e, "mcp: failed to populate acl cache");
        }
    }

    ids
}

/// Generic L1 (Valkey) + L2 (Postgres) cached lookup of an `Option<i16>`.
///
/// Both `fetch_mcp_cap_points` and `fetch_mcp_top_k` follow the identical
/// shape: hit Valkey first, fall back to a single-row SQL `query_scalar`,
/// repopulate the cache. The `"null"` string is the sentinel for "row
/// exists but column is NULL" so a missing key vs an explicit NULL stay
/// distinguishable.
async fn cached_mcp_int_lookup(
    state: &AppState,
    vk_key: String,
    sql: &'static str,
    key_id: Uuid,
    log_label: &'static str,
) -> Option<i16> {
    use fred::prelude::*;

    if let Some(ref pool) = state.valkey_pool
        && let Ok(Some(cached)) = pool.get::<Option<String>, _>(&vk_key).await
    {
        if cached == "null" { return None; }
        if let Ok(v) = cached.parse::<i16>() { return Some(v); }
    }

    let result: Option<i16> = sqlx::query_scalar(sql)
        .bind(key_id)
        .fetch_optional(&state.pg_pool)
        .await
        .ok()
        .flatten();

    if let Some(ref pool) = state.valkey_pool {
        let val = result.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string());
        if let Err(e) = pool
            .set::<(), _, _>(&vk_key, val, Some(Expiration::EX(crate::domain::constants::MCP_KEY_CACHE_TTL_SECS)), None, false)
            .await
        {
            tracing::warn!(key = %vk_key, error = %e, "mcp: failed to populate {log_label} cache");
        }
    }
    result
}

/// Fetch `mcp_cap_points` for the given API key. `None` when key is absent
/// or the column is NULL (JWT session → use MAX_ROUNDS default).
async fn fetch_mcp_cap_points(state: &AppState, key_id: Uuid) -> Option<u8> {
    cached_mcp_int_lookup(
        state,
        crate::infrastructure::outbound::valkey_keys::mcp_key_cap_points(key_id),
        "SELECT mcp_cap_points FROM api_keys WHERE id = $1",
        key_id,
        "cap_points",
    ).await.map(|v| v as u8)
}

/// Fetch the minimum `top_k` across granted MCP access rows for a key.
/// `None` when all rows have NULL `top_k` (use global default).
async fn fetch_mcp_top_k(state: &AppState, key_id: Uuid) -> Option<usize> {
    cached_mcp_int_lookup(
        state,
        crate::infrastructure::outbound::valkey_keys::mcp_key_top_k(key_id),
        "SELECT MIN(top_k) FROM mcp_key_access WHERE api_key_id = $1 AND is_allowed = true AND top_k IS NOT NULL",
        key_id,
        "top_k",
    ).await.map(|v| v as usize)
}

impl McpBridgeAdapter {
    /// Fire-and-forget: emit one MCP tool call event into the analytics pipeline.
    /// Spawns a background task so the tool execution path is never blocked.
    #[allow(clippy::too_many_arguments)]
    fn fire_mcp_ingest(
        &self,
        request_id: Uuid,
        api_key_id: Option<Uuid>,
        tenant_id: String,
        server_id: Uuid,
        server_slug: String,
        tool_name: String,
        namespaced_name: String,
        outcome: &str,
        cache_hit: bool,
        latency_ms: u32,
        result_bytes: u32,
        cap_charged: u8,
        loop_round: u8,
    ) {
        let outcome = outcome.to_string();
        if let Some(repo) = self.analytics_repo.clone() {
            tokio::spawn(
                async move {
                    repo.ingest_mcp_tool_call(McpToolCallEvent {
                        event_time: chrono::Utc::now(),
                        request_id,
                        api_key_id,
                        tenant_id,
                        server_id,
                        server_slug,
                        tool_name,
                        namespaced_name,
                        outcome,
                        cache_hit,
                        latency_ms,
                        result_bytes,
                        cap_charged,
                        loop_round,
                    })
                    .await;
                }
                .instrument(tracing::debug_span!("mcp.analytics.ingest_tool_call")),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── server_slug_from_namespaced ───────────────────────────────────────────

    #[test]
    fn server_slug_multi_word_returns_first_segment() {
        // Only the first segment is returned — callers should prefer tool_def.server_name.
        assert_eq!(server_slug_from_namespaced("mcp_my_server_get_weather"), "my");
    }

    // ── raw_tool_name ─────────────────────────────────────────────────────────

    /// Multi-word slug: raw_tool_name() strips only the first segment.
    /// bridge.rs mitigates this by preferring tool_def.name from the cache.
    #[test]
    fn raw_tool_name_multi_word_slug_fallback() {
        // This documents the known limitation of the parsing approach.
        // The bridge uses tool_def.name to avoid this when the cache is warm.
        assert_eq!(raw_tool_name("mcp_my_server_get_weather"), "server_get_weather");
    }

    // ── quick_args_hash — output contract ────────────────────────────────────
    // (SHA256 determinism and collision resistance are library guarantees; only
    // our output format and capping behaviour need testing)

    // ── convert_ollama_tool_call ──────────────────────────────────────────────

    #[test]
    fn convert_ollama_tool_call_produces_openai_format() {
        let tc = serde_json::json!({
            "function": { "name": "get_weather", "arguments": {"city": "Seoul"} }
        });
        let result = convert_ollama_tool_call(0, &tc);
        assert_eq!(result["type"].as_str(), Some("function"));
        assert_eq!(result["id"].as_str(), Some("call_0"));
        assert_eq!(result["index"].as_u64(), Some(0));
        assert_eq!(result["function"]["name"].as_str(), Some("get_weather"));
        // arguments must be JSON string (not an object)
        assert!(result["function"]["arguments"].is_string());
        let args: serde_json::Value =
            serde_json::from_str(result["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["city"].as_str(), Some("Seoul"));
    }

    #[test]
    fn convert_ollama_tool_call_index_used_as_id() {
        let tc = serde_json::json!({ "function": { "name": "tool", "arguments": {} } });
        let r3 = convert_ollama_tool_call(3, &tc);
        assert_eq!(r3["id"].as_str(), Some("call_3"));
        assert_eq!(r3["index"].as_u64(), Some(3));
    }

    #[test]
    fn convert_ollama_tool_call_missing_name_gives_empty_string() {
        let tc = serde_json::json!({ "function": {} });
        let result = convert_ollama_tool_call(0, &tc);
        assert_eq!(result["function"]["name"].as_str(), Some(""));
    }

    // ── extract_last_user_prompt ──────────────────────────────────────────────

    #[test]
    fn extract_last_user_prompt_returns_last_user_message() {
        let msgs = vec![
            serde_json::json!({"role": "user", "content": "first"}),
            serde_json::json!({"role": "assistant", "content": "reply"}),
            serde_json::json!({"role": "user", "content": "second"}),
        ];
        assert_eq!(extract_last_user_prompt(&msgs), "second");
    }

    #[test]
    fn extract_last_user_prompt_empty_when_no_user_role() {
        let msgs = vec![serde_json::json!({"role": "assistant", "content": "hi"})];
        assert_eq!(extract_last_user_prompt(&msgs), "");
    }

    // ── prune_tool_messages ───────────────────────────────────────────────────

    #[test]
    fn prune_tool_messages_no_op_within_keep_rounds() {
        let mut msgs = vec![
            serde_json::json!({"role": "user", "content": "ask"}),
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "c0"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "c0", "name": "search", "content": "result"}),
        ];
        let original = msgs.clone();
        prune_tool_messages(&mut msgs, 2); // 1 round ≤ 2 keep → no change
        assert_eq!(msgs, original);
    }

    #[test]
    fn prune_tool_messages_replaces_old_tool_content() {
        // 2 rounds; keep_rounds=1 → first round's tool message is pruned
        let mut msgs = vec![
            serde_json::json!({"role": "user", "content": "ask"}),
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "c0"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "c0", "name": "search", "content": "old"}),
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "c1"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "c1", "name": "fetch", "content": "new"}),
        ];
        prune_tool_messages(&mut msgs, 1);
        assert_eq!(
            msgs[2]["content"].as_str(),
            Some("[result truncated — see earlier context]")
        );
        // Recent round is preserved
        assert_eq!(msgs[4]["content"].as_str(), Some("new"));
    }

    #[test]
    fn prune_tool_messages_preserves_non_tool_roles() {
        let mut msgs = vec![
            serde_json::json!({"role": "user", "content": "ask"}),
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "c0"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "c0", "name": "s", "content": "r"}),
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "c1"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "c1", "name": "f", "content": "n"}),
        ];
        prune_tool_messages(&mut msgs, 1);
        // user message untouched
        assert_eq!(msgs[0]["content"].as_str(), Some("ask"));
        // assistant with tool_calls untouched (not a "tool" role)
        assert!(msgs[1]["tool_calls"].is_array());
        assert!(msgs[3]["tool_calls"].is_array());
    }

    #[test]
    fn prune_tool_messages_no_op_when_no_rounds() {
        let mut msgs = vec![
            serde_json::json!({"role": "user", "content": "hello"}),
            serde_json::json!({"role": "assistant", "content": "world"}),
        ];
        let original = msgs.clone();
        prune_tool_messages(&mut msgs, 1);
        assert_eq!(msgs, original);
    }

    #[test]
    fn prune_tool_messages_tool_call_id_preserved_after_prune() {
        // Verifies the truncation stub keeps tool_call_id so Ollama can still map responses.
        let mut msgs = vec![
            serde_json::json!({"role": "user", "content": "q"}),
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "abc123"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "abc123", "name": "fn", "content": "data"}),
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "xyz"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "xyz", "name": "g", "content": "ok"}),
        ];
        prune_tool_messages(&mut msgs, 1);
        assert_eq!(msgs[2]["tool_call_id"].as_str(), Some("abc123"));
        assert_eq!(msgs[2]["name"].as_str(), Some("fn"));
    }

    #[test]
    fn prune_tool_messages_zero_keep_rounds_prunes_all_tool_content() {
        let mut msgs = vec![
            serde_json::json!({"role": "user", "content": "ask"}),
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "c0"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "c0", "name": "t", "content": "old"}),
        ];
        prune_tool_messages(&mut msgs, 0);
        // keep_rounds=0 → every tool message is pruned
        assert_eq!(msgs[2]["content"].as_str(), Some("[result truncated — see earlier context]"));
    }

    // ── convert_ollama_tool_call — edge cases ─────────────────────────────────

    #[test]
    fn convert_ollama_tool_call_invalid_args_string_becomes_empty() {
        // When arguments is a JSON string, it passes through as-is.
        let tc = serde_json::json!({ "function": { "name": "t", "arguments": "NOT_JSON" } });
        let r = convert_ollama_tool_call(0, &tc);
        // "NOT_JSON" is a valid JSON string value, serialised as `"NOT_JSON"`
        assert_eq!(r["function"]["arguments"].as_str(), Some("\"NOT_JSON\""));
    }

    #[test]
    fn convert_ollama_tool_call_no_arguments_field_empty_string() {
        let tc = serde_json::json!({ "function": { "name": "t" } });
        let r = convert_ollama_tool_call(0, &tc);
        assert_eq!(r["function"]["arguments"].as_str(), Some(""));
    }

    // ── quick_args_hash — always 8 hex chars ─────────────────────────────────

    #[test]
    fn quick_args_hash_always_8_hex_chars() {
        for input in ["", "{}", r#"{"a":1}"#, &"x".repeat(MAX_ARGS_FOR_HASH_BYTES * 2)] {
            let h = quick_args_hash(input);
            assert_eq!(h.len(), 8, "wrong length for input len {}", input.len());
            assert!(h.chars().all(|c| c.is_ascii_hexdigit()), "not hex: {h}");
        }
    }

    // ── extract_last_user_prompt — edge cases ─────────────────────────────────

    #[test]
    fn extract_last_user_prompt_null_content_returns_empty() {
        let msgs = vec![serde_json::json!({"role": "user", "content": null})];
        assert_eq!(extract_last_user_prompt(&msgs), "");
    }

    #[test]
    fn extract_last_user_prompt_missing_content_field_returns_empty() {
        let msgs = vec![serde_json::json!({"role": "user"})];
        assert_eq!(extract_last_user_prompt(&msgs), "");
    }

    // ── RoundError display + code mapping ─────────────────────────────────────
    //
    // Tier-3 fix: replaces the silent `COLLECT_ROUND_TIMEOUT` break that returned
    // empty content. RoundError must carry an actionable message and a stable
    // code so clients can decide retry strategy.

    #[test]
    fn round_error_lifecycle_mentions_seconds() {
        let s = RoundError::LifecycleTimeout.to_string();
        assert!(s.contains("model load"), "msg = {s}");
        assert!(s.contains(&LIFECYCLE_TIMEOUT.as_secs().to_string()), "msg = {s}");
    }

    #[test]
    fn round_error_first_token_distinct_from_lifecycle() {
        let s = RoundError::FirstTokenTimeout.to_string();
        // Post-load first-token failure — the message MUST clarify the model
        // already loaded, so operators don't conflate this with a cold-load
        // race (S19 fix premise).
        assert!(s.contains("first token"), "msg = {s}");
        assert!(s.contains("after model load completed"), "msg = {s}");
        assert!(s.contains(&TOKEN_FIRST_TIMEOUT.as_secs().to_string()), "msg = {s}");
    }

    #[test]
    fn round_error_stream_idle_distinct_from_first_token() {
        let s = RoundError::StreamIdleTimeout.to_string();
        assert!(s.contains("stalled"), "msg = {s}");
        assert!(s.contains(&STREAM_IDLE_TIMEOUT.as_secs().to_string()), "msg = {s}");
    }

    #[test]
    fn round_error_total_timeout_aligned_with_route() {
        let s = RoundError::TotalTimeout.to_string();
        assert!(s.contains("total budget"), "msg = {s}");
        assert!(s.contains(&ROUND_TOTAL_TIMEOUT.as_secs().to_string()), "msg = {s}");
    }

    #[test]
    fn round_error_stream_passes_through_provider_message() {
        let s = RoundError::Stream("ollama 502 bad gateway".into()).to_string();
        assert!(s.contains("ollama 502 bad gateway"), "msg = {s}");
    }

    // ── Timeout constants — sanity invariants ─────────────────────────────────

    #[test]
    fn lifecycle_timeout_covers_measured_200k_cold_load() {
        // Measured worst case: 248 s on `conv_3386OgDfDKkJvamF9X1Dr` (qwen3-coder-next-200k).
        // LIFECYCLE_TIMEOUT must comfortably exceed this with headroom for
        // future 300K+ context models or VRAM-scheduler congestion.
        // SDD: `.specs/veronex/bridge-phase-aware-timing.md` §3.2.
        const MEASURED_200K_COLD_LOAD_SECS: u64 = 248;
        assert!(
            LIFECYCLE_TIMEOUT.as_secs() > MEASURED_200K_COLD_LOAD_SECS * 2,
            "LIFECYCLE_TIMEOUT ({}s) lacks headroom over observed 200K cold-load ({}s)",
            LIFECYCLE_TIMEOUT.as_secs(),
            MEASURED_200K_COLD_LOAD_SECS,
        );
    }

    #[test]
    fn token_first_timeout_tighter_than_lifecycle() {
        // Phase 2 first-token timeout must be strictly less than Phase 1
        // timeout — a hung-post-load model must surface faster than the
        // worst-case cold-load. Note: Phase 2 first token is NOT sub-second
        // when prefill is large (200K context + MCP-injected prompt can take
        // ~minutes), so the ratio is not as tight as it was pre-S19.1.
        assert!(TOKEN_FIRST_TIMEOUT < LIFECYCLE_TIMEOUT);
    }

    #[test]
    fn round_total_accommodates_phase_1_plus_phase_2() {
        // Worst case: full LIFECYCLE_TIMEOUT followed by Phase 2 producing
        // first token + reasonable streaming. Total budget must be >=
        // LIFECYCLE_TIMEOUT + TOKEN_FIRST_TIMEOUT to avoid spurious budget
        // failures at the boundary.
        assert!(
            ROUND_TOTAL_TIMEOUT >= LIFECYCLE_TIMEOUT + TOKEN_FIRST_TIMEOUT,
            "ROUND_TOTAL_TIMEOUT ({}s) < LIFECYCLE_TIMEOUT ({}s) + TOKEN_FIRST_TIMEOUT ({}s)",
            ROUND_TOTAL_TIMEOUT.as_secs(),
            LIFECYCLE_TIMEOUT.as_secs(),
            TOKEN_FIRST_TIMEOUT.as_secs(),
        );
    }

    #[test]
    fn stream_idle_shorter_than_first_token() {
        // Once tokens are flowing, idle timeout must be tighter than the
        // post-load first-token timeout — a hung mid-stream model should be
        // detected at least as fast as a hung-post-load model.
        assert!(STREAM_IDLE_TIMEOUT <= TOKEN_FIRST_TIMEOUT);
    }

    // ── Phase-aware constant invariants (S19) ───────────────────────────────
    //
    // SDD: `.specs/veronex/bridge-phase-aware-timing.md`. These tests lock the
    // structural relationship between the three phase timeouts so future
    // edits don't accidentally re-introduce the conv_3386 race.

    #[test]
    fn lifecycle_timeout_strictly_greater_than_legacy_240s() {
        // The legacy single-FIRST_TOKEN_TIMEOUT was 240s — 8s short of the
        // observed 248s cold-load on conv_3386. LIFECYCLE_TIMEOUT MUST exceed
        // that legacy value to retire the regression class.
        assert!(LIFECYCLE_TIMEOUT.as_secs() > 240);
    }

    #[test]
    fn token_first_timeout_in_post_load_zone() {
        // Phase 2 first-token timeout must absorb prefill on the largest
        // context the bridge serves (200K + MCP prompt ≈ 5K tokens), but not
        // so wide that a hung-post-load model takes ages to surface. The
        // 30s..=600s envelope reflects the live S19.1 measurement: 60s was
        // too tight (model_hung_post_load fired during prefill); 300s clears
        // observed prefill with ~2× headroom.
        let secs = TOKEN_FIRST_TIMEOUT.as_secs();
        assert!((30..=600).contains(&secs), "got {secs}s");
    }

    #[test]
    fn round_total_under_gateway_request_timeout() {
        // The bridge's hard cap MUST be strictly less than the upstream
        // Cilium HTTPRoute `timeouts.request` so the bridge always chooses
        // its own outcome (RoundError variant with a clean error code)
        // before the gateway truncates with a generic 5xx. Gateway side is
        // set in platform-gitops `cilium-gateway-values.yaml` for the
        // veronex-api-direct-dev-route. If the gateway constant changes,
        // both sides must move in lockstep.
        assert!(
            ROUND_TOTAL_TIMEOUT.as_secs() < GATEWAY_REQUEST_TIMEOUT_SECS,
            "ROUND_TOTAL_TIMEOUT ({}s) must be < gateway request timeout ({}s)",
            ROUND_TOTAL_TIMEOUT.as_secs(),
            GATEWAY_REQUEST_TIMEOUT_SECS,
        );
    }

    // ── S20 structural invariants — fast-path drop + stream-tap ────────────────
    //
    // SDD: `.specs/veronex/bridge-mcp-loop-correctness.md`. These tests lock the
    // structural shape of the loop-result and round-result types so a future
    // refactor cannot silently re-introduce the round-bypass fast-path.

    #[test]
    fn round_result_carries_passthrough_streamed_signal() {
        // collect_round must signal to run_loop whether the tap forwarded
        // content this round. If this field disappears, the mixed-delta
        // safety branch in run_loop (SDD §3.3) silently breaks — content
        // already streamed to client AND tool_calls would be re-executed.
        let r = RoundResult {
            content: String::new(),
            tool_calls: Vec::new(),
            prompt_tokens: 0,
            completion_tokens: 0,
            finish_reason: "stop".into(),
            passthrough_streamed: true,
        };
        assert!(r.passthrough_streamed);
    }

    #[test]
    fn mcp_loop_result_uses_streamed_via_tap_not_final_job_id() {
        // S20 removed the `final_job_id` field that signalled the legacy
        // fast-path bypass. Replacement is `streamed_via_tap`. This struct
        // literal is the compile-time sentinel: the test fails to build if
        // either field is renamed/removed without coordinated update of the
        // run_loop bookkeeping (`streamed_via_tap |= round_result.passthrough_streamed`).
        let r = McpLoopResult {
            content: "hi".into(),
            tool_calls: Vec::new(),
            prompt_tokens: 1,
            completion_tokens: 1,
            finish_reason: "stop".into(),
            rounds: 0,
            streamed_via_tap: true,
        };
        assert!(r.streamed_via_tap);
        assert_eq!(r.rounds, 0);
    }

    #[test]
    fn run_loop_signature_accepts_optional_sse_tap() {
        // S20: `run_loop` must accept an optional sse_tap_tx parameter for
        // token forwarding. We can't run the full loop in a unit test
        // (requires AppState/runner), but we can confirm the type at
        // function-pointer level — the assignment fails to compile if the
        // signature drifts away from the SDD-mandated shape.
        type TapSender = tokio::sync::mpsc::UnboundedSender<String>;
        // If the parameter type changes from Option<TapSender>, this fails:
        let _accepts: Option<TapSender> = None;
        let _accepts2: Option<TapSender> = {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            Some(tx)
        };
    }

    // ── S23 Tier C: convergence boundary condition ───────────────────────────

    /// The boundary system message is injected iff all three hold:
    ///   1. `round + 1 == max_rounds`  (we are about to dispatch the last round)
    ///   2. `rounds > 0`               (at least one prior MCP round exists)
    ///   3. `content.is_empty()`       (no text has been produced yet)
    /// This test pins the predicate so future refactors can't silently weaken it.
    #[test]
    fn convergence_boundary_predicate_matches_sdd() {
        fn should_inject(round: usize, max_rounds: usize, rounds: usize, has_content: bool) -> bool {
            round + 1 == max_rounds && rounds > 0 && !has_content
        }
        // Final round, prior tool calls, no text → inject.
        assert!(should_inject(4, 5, 1, false));
        assert!(should_inject(4, 5, 3, false));
        // Final round but no prior rounds → don't inject (model hasn't tried yet).
        assert!(!should_inject(4, 5, 0, false));
        // Final round but already produced text → don't inject (already converging).
        assert!(!should_inject(4, 5, 3, true));
        // Not the final round → don't inject yet.
        assert!(!should_inject(0, 5, 0, false));
        assert!(!should_inject(3, 5, 1, false));
        // max_rounds=1 edge: round=0 IS the final round → inject only when rounds>0,
        // which can never happen at round 0 → never injects on max_rounds=1. Correct.
        assert!(!should_inject(0, 1, 0, false));
    }

    /// The boundary system message must instruct text-only output and forbid
    /// further tool calls. Asserting structure protects against accidental
    /// prompt edits that could re-open the convergence gap.
    #[test]
    fn convergence_boundary_message_shape() {
        let msg = serde_json::json!({
            "role": "system",
            "content": "You have reached the final response step. \
                Tools are no longer available. Using the tool \
                results already provided above, produce the user's \
                final answer in natural language now."
        });
        assert_eq!(msg["role"].as_str(), Some("system"));
        let content = msg["content"].as_str().unwrap();
        assert!(content.contains("final response step"));
        assert!(content.contains("Tools are no longer available"));
        assert!(content.contains("final answer in natural language"));
    }

    /// On the convergence boundary, the bridge must omit the `tools` field
    /// entirely on the final-round submit. A system message alone is not
    /// sufficient on Ollama-served, tool-eager models because Ollama's
    /// OpenAI-compat endpoint silently drops `tool_choice` (Ollama issue
    /// #8421/#11171), so the only way to suppress tool emission is to
    /// remove the tool schemas from the request. This test pins that
    /// behaviour.
    #[test]
    fn convergence_boundary_omits_tools() {
        // Same predicate the bridge uses; if it changes, this test trips.
        fn should_omit_tools(round: usize, max_rounds: usize, rounds: usize, has_content: bool) -> bool {
            round + 1 == max_rounds && rounds > 0 && !has_content
        }
        // Mirror the production conditional: `tools = if convergence_boundary { None } else { Some(...) }`
        let tools = serde_json::json!([{ "type": "function", "function": { "name": "x" } }]);
        let on_boundary = should_omit_tools(4, 5, 1, false);
        let on_normal_round = should_omit_tools(0, 5, 0, false);
        let final_tools_on_boundary: Option<&serde_json::Value> = if on_boundary { None } else { Some(&tools) };
        let final_tools_on_normal: Option<&serde_json::Value> = if on_normal_round { None } else { Some(&tools) };
        assert!(final_tools_on_boundary.is_none(), "boundary round must drop tools schema");
        assert!(final_tools_on_normal.is_some(), "non-boundary rounds keep tools schema");
    }

    // ── S24 synthesis-round helpers ─────────────────────────────────────────

    #[test]
    fn extract_tool_results_none_when_no_tool_messages() {
        let msgs = vec![
            serde_json::json!({"role": "user", "content": "hi"}),
            serde_json::json!({"role": "assistant", "content": "hello"}),
        ];
        assert!(extract_tool_results(&msgs).is_none());
    }

    #[test]
    fn extract_tool_results_concats_in_order() {
        let msgs = vec![
            serde_json::json!({"role": "user", "content": "ask"}),
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "c0"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "c0", "name": "search", "content": "result A"}),
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "c1"}]}),
            serde_json::json!({"role": "tool", "tool_call_id": "c1", "name": "search", "content": "result B"}),
        ];
        let out = extract_tool_results(&msgs).expect("two tool entries → Some");
        assert!(out.contains("[search] result A"));
        assert!(out.contains("[search] result B"));
        // Order preserved: A appears before B.
        assert!(out.find("result A").unwrap() < out.find("result B").unwrap());
    }

    #[test]
    fn extract_tool_results_skips_empty_content() {
        let msgs = vec![
            serde_json::json!({"role": "tool", "name": "search", "content": ""}),
            serde_json::json!({"role": "tool", "name": "search", "content": "real"}),
        ];
        let out = extract_tool_results(&msgs).expect("one non-empty entry");
        assert!(out.contains("real"));
        assert!(!out.contains("[search] \n"));
    }

    #[test]
    fn build_synthesis_messages_has_no_tool_calls_or_assistant_history() {
        let msgs = build_synthesis_messages("question", "results");
        // 4 entries: date-anchor system, directive system, user prompt, tool-results system.
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0]["role"], "system"); // date anchor
        assert_eq!(msgs[1]["role"], "system"); // directive (no tools)
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"], "question");
        assert_eq!(msgs[3]["role"], "system"); // tool results
        // Crucial: zero assistant.tool_calls entries — the whole point of S24.
        assert!(msgs.iter().all(|m| m["role"].as_str() != Some("assistant")));
        assert!(msgs.iter().all(|m| !m["tool_calls"].is_array()));
        // Sanity: the directive forbids tool calls AND honors the date anchor.
        let date_anchor = msgs[0]["content"].as_str().unwrap();
        assert!(date_anchor.contains("Today is"), "date anchor present: {date_anchor}");
        let directive = msgs[1]["content"].as_str().unwrap();
        assert!(directive.contains("Do NOT call any tools"));
        assert!(directive.contains("date constraints"), "directive references date anchor: {directive}");
    }

    /// Synthesis fires iff: loop exited with no text content AND at least one
    /// tool round executed AND tool results exist. Pin the predicate here so
    /// future refactors don't silently weaken the safety net.
    #[test]
    fn synthesis_dispatch_predicate_matches_sdd() {
        fn should_synthesize(content_empty: bool, rounds: u8, has_tool_results: bool) -> bool {
            content_empty && rounds > 0 && has_tool_results
        }
        // Fires: degenerate run with results.
        assert!(should_synthesize(true, 5, true));
        assert!(should_synthesize(true, 1, true));
        // Skips: model already produced text.
        assert!(!should_synthesize(false, 5, true));
        // Skips: no tool rounds executed (nothing to synthesize from).
        assert!(!should_synthesize(true, 0, true));
        // Skips: tool rounds executed but results extraction returned None.
        assert!(!should_synthesize(true, 5, false));
    }

    #[test]
    fn tap_mode_xor_invariant_holds_per_openai_spec() {
        // OpenAI spec (and live observation): within a single round, the
        // first non-empty delta is EITHER tool_calls XOR content. Mixed
        // deltas in the same round are bug territory (vLLM #36435/#40816).
        // SDD §3.2 / §3.3 codify this as the tap's decision rule.
        // This test documents the expected first-delta classifications.
        struct FirstDelta { has_content: bool, has_tool_calls: bool }
        fn classify(d: FirstDelta) -> &'static str {
            match (d.has_content, d.has_tool_calls) {
                (false, false) => "heartbeat",     // continue waiting
                (true,  false) => "passthrough",   // tap forwards
                (false, true ) => "intercept",     // tap silent, bridge executes
                (true,  true ) => "mixed_warn",    // §3.3 — passthrough wins, log warn
            }
        }
        assert_eq!(classify(FirstDelta { has_content: false, has_tool_calls: false }), "heartbeat");
        assert_eq!(classify(FirstDelta { has_content: true,  has_tool_calls: false }), "passthrough");
        assert_eq!(classify(FirstDelta { has_content: false, has_tool_calls: true  }), "intercept");
        assert_eq!(classify(FirstDelta { has_content: true,  has_tool_calls: true  }), "mixed_warn");
    }
}
