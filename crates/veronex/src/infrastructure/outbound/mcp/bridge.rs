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
/// First-token wait. Covers ollama cold load + KV cache pre-allocation + first-token compute.
///
/// Sized for 200K-context models (qwen3-coder-next-200k:latest) on Strix Halo / AI Max+ 395:
/// measured `load_duration` ≈ 163 s for the full 200K KV alloc + warmup. 240 s leaves a 47 s
/// safety buffer for variance. Warm-state requests return in <100 ms regardless.
const FIRST_TOKEN_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(240);

/// Per-token stream idle. Fires only when the model hangs mid-response (true stall).
/// Generation gap on warm models is sub-second; 45 s is a generous safety margin.
const STREAM_IDLE_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(45);

/// Hard cap per round. Aligns with `INFERENCE_ROUTER_TIMEOUT` so the route layer never
/// fires before the bridge has chosen its own outcome.
const ROUND_TOTAL_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(360);
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
pub struct McpLoopResult {
    /// Final assistant text content (populated when `want_stream = false`).
    pub content: String,
    /// Final round tool_calls — non-empty when the model finished with non-MCP tools
    /// or when `want_stream = false`.
    pub tool_calls: Vec<Value>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub finish_reason: String,
    /// How many MCP tool-call rounds were executed.
    pub rounds: u8,
    /// When `want_stream = true` and the final round has no MCP tool_calls, this
    /// contains the final round's JobId so the caller can pipe it through SSE.
    /// The `content` / `tool_calls` fields are empty in this case.
    pub final_job_id: Option<JobId>,
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
    /// When `want_stream = true`, the final round is NOT collected — instead
    /// `McpLoopResult.final_job_id` is returned so the caller can stream it via SSE.
    #[instrument(skip_all, fields(model = %model))]
    #[allow(clippy::too_many_arguments)]
    pub async fn run_loop(
        &self,
        state: &AppState,
        caller: &InferCaller,
        model: String,
        mut messages: Vec<Value>,
        base_tools: Option<Vec<Value>>,
        want_stream: bool,
        conversation_id: Option<uuid::Uuid>,
        stop: Option<Value>,
        seed: Option<u32>,
        response_format: Option<Value>,
        frequency_penalty: Option<f64>,
        presence_penalty: Option<f64>,
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
        let tools_json = if all_tools.is_empty() { None } else { Some(all_tools) };

        // ── Loop ID — groups all rounds into one traceable unit ───────────────
        let mcp_loop_id = Uuid::new_v4();

        // ── Loop state ─────────────────────────────────────────────────────────
        let mut total_prompt_tokens: u32 = 0;
        let mut total_completion_tokens: u32 = 0;
        let mut finish_reason = "stop".to_string();
        let mut content = String::new();
        let mut final_tool_calls: Vec<Value> = Vec::new();
        let mut all_mcp_tool_calls: Vec<Value> = Vec::new();
        let mut rounds: u8 = 0;
        let mut final_job_id: Option<JobId> = None;

        let mut first_job_id: Option<JobId> = None;

        let mut intermediate_job_ids: Vec<Uuid> = Vec::new();

        // Loop-detection: (tool_name, args_hash) → count
        let mut call_sig_counts: HashMap<(String, String), u8> = HashMap::new();

        for round in 0..max_rounds {
            debug!(round, "MCP agentic loop round");

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
                tools: tools_json.clone().map(Value::Array),
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
            } else if want_stream && rounds > 0 {
                // Streaming fast-path: at least one MCP tool-call round completed,
                // so this next job is almost certainly the final text response.
                // Skip synchronous collection and hand the job_id to the SSE path
                // so the client starts receiving tokens immediately.
                final_job_id = Some(job_id);
                break;
            } else {
                intermediate_job_ids.push(job_id.0);
            }

            // ── Collect response (or defer to streaming on the final round) ────
            //
            // Streaming optimisation: on the FIRST round with no MCP tool_calls
            // (i.e. the model will return text or non-MCP tools), skip collection
            // and return the job_id so the caller can pipe it through SSE.
            // All intermediate rounds (with MCP tool_calls) are always collected.
            //
            // We don't know whether this round will have MCP calls until we have
            // collected it, so we always collect — EXCEPT when `want_stream=true`
            // AND we have already processed at least one tool-call round (rounds>0),
            // in which case this is almost certainly the final text round.
            // For the first-round streaming case (no tool rounds yet), we can't
            // skip collection because we don't know if MCP tools will be called.
            // MCP loop always collects all rounds (including final text round).
            // Streaming is handled after the loop completes by re-submitting
            // the final content through the SSE path if want_stream=true.

            // `collect_round` owns the phased timeout (FIRST_TOKEN/STREAM_IDLE/ROUND_TOTAL).
            // The outer `tokio::time::timeout` was removed — a single 45 s wrapper used
            // to mean cold-load on a 200K-context model (measured 163 s on Strix Halo)
            // would silently break and return empty content to the client.
            let round_result = match collect_round(state, &job_id).await {
                Ok(r) => r,
                Err(e) => {
                    warn!(round = rounds, model = %model, error = %e, "MCP round failed");
                    let code = match &e {
                        RoundError::FirstTokenTimeout => "model_loading",
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
                        final_job_id: None,
                    });
                }
            };
            total_prompt_tokens = total_prompt_tokens.saturating_add(round_result.prompt_tokens);
            total_completion_tokens = total_completion_tokens.saturating_add(round_result.completion_tokens);
            finish_reason = round_result.finish_reason.clone();
            content = round_result.content.clone();
            final_tool_calls = round_result.tool_calls.clone();

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
                // No MCP tools requested — done.
                // If streaming was requested on round 0 (no tool calls at all),
                // we've already collected — set final_job_id only if streaming
                // AND there were 0 tool rounds (model answered directly).
                // In that case content is already collected, so leave final_job_id None.
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

            // ── Collect tool_calls for S3 record ──────────────────────────────
            all_mcp_tool_calls.extend(mcp_calls.iter().cloned());

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

            // Batch-insert all tool call records for this round in one query.
            let db_rows: Vec<&ToolCallRecord> = exec_results.iter().map(|(_, r)| r).collect();
            batch_insert_tool_calls(&state.pg_pool, mcp_loop_id, job_id.0, &db_rows).await;

            for (tc, (result_text, _)) in mcp_calls.iter().zip(exec_results.into_iter()) {
                let call_id = tc["id"].as_str().unwrap_or("call_0");
                let tool_name = tc["function"]["name"].as_str().unwrap_or("");
                messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "name": tool_name,
                    "content": result_text
                }));
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

        // Persist final result to S3 + cleanup intermediate jobs
        if let Some(ref fid) = first_job_id {
            let pg = &state.pg_pool;

            // Write single complete turn to S3: tool_calls from all rounds + final result.
            // Runner skips S3 for mcp_loop jobs, so this is the only S3 write.
            //
            // Tier-B (SDD `.specs/veronex/history/inference-mcp-streaming-first.md` §6):
            // also write when only `all_mcp_tool_calls` were captured (no final
            // text yet). Pre-Tier-B gate `if !content.is_empty()` silently
            // dropped the entire conversation when the loop was cancelled
            // mid-round (client disconnect via Cloudflare 524 → CancelOnDrop)
            // — UI surfaced this as "저장된 결과 없음".
            if !content.is_empty() || !all_mcp_tool_calls.is_empty() {
                if let Some(ref store) = state.message_store {
                    let owner_id = caller.account_id()
                        .or(caller.api_key_id())
                        .unwrap_or(fid.0);
                    let date = chrono::Utc::now().date_naive();
                    let s3_key = conversation_id.unwrap_or(fid.0);

                    let mut record = store.get_conversation(owner_id, date, s3_key).await
                        .ok().flatten()
                        .unwrap_or_else(crate::application::ports::outbound::message_store::ConversationRecord::new);

                    let tool_calls_val = if all_mcp_tool_calls.is_empty() {
                        None
                    } else {
                        Some(serde_json::Value::Array(all_mcp_tool_calls))
                    };

                    let result_val = if content.is_empty() { None } else { Some(content.clone()) };

                    record.turns.push(crate::application::ports::outbound::message_store::ConversationTurn::Regular(
                        crate::application::ports::outbound::message_store::TurnRecord {
                            job_id: fid.0,
                            prompt: extract_last_user_prompt(&messages),
                            messages: Some(serde_json::Value::Array(messages.clone())),
                            tool_calls: tool_calls_val,
                            result: result_val,
                            model_name: Some(model.clone()),
                            created_at: chrono::Utc::now().to_rfc3339(),
                            compressed: None,
                            vision_analysis: None,
                        }
                    ));

                    if let Err(e) = store.put_conversation(owner_id, date, s3_key, &record).await {
                        warn!(job_id = %fid.0, error = %e, "MCP: S3 conversation write failed");
                    } else if let Some(conv_id) = conversation_id {
                        // Invalidate cached conversation detail
                        if let Some(ref pool) = state.valkey_pool {
                            use fred::prelude::*;
                            let cache_key = format!("conv_s3:{}", conv_id);
                            if let Err(e) = pool.del::<i64, _>(cache_key).await {
                                tracing::warn!(error = %e, "Valkey DEL conversation cache failed");
                            }
                        }
                    }
                }

                // Update DB: tokens only — result lives in S3, fetched on demand
                let _ = sqlx::query(
                    "UPDATE inference_jobs SET prompt_tokens = $1, completion_tokens = $2 WHERE id = $3"
                )
                .bind(total_prompt_tokens.min(i32::MAX as u32) as i32)
                .bind(total_completion_tokens.min(i32::MAX as u32) as i32)
                .bind(fid.0)
                .execute(pg)
                .await
                .map_err(|e| warn!(job_id = %fid.0, error = %e, "MCP: failed to update job tokens"));
            }

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
        }

        Some(McpLoopResult {
            content,
            tool_calls: final_tool_calls,
            prompt_tokens: total_prompt_tokens,
            completion_tokens: total_completion_tokens,
            finish_reason,
            rounds,
            final_job_id,
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
        mcp_loop_id: Uuid,
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
                let rec = ToolCallRecord::error(mcp_loop_id, triggering_job_id, loop_round, Uuid::nil(), "unknown", namespaced, &args, "unknown_tool");
                return (text, rec);
            }
        };

        // ── ACL check ──────────────────────────────────────────────────────────
        if allowed_servers.is_some_and(|a| !a.contains(&server_id)) {
            warn!(tool = %namespaced, server = %server_id, "MCP ACL: access denied for this key");
            let rec = ToolCallRecord::error(mcp_loop_id, triggering_job_id, loop_round, server_id, namespaced, namespaced, &args, "acl_denied");
            return ("{\"error\": \"MCP server access denied\"}".into(), rec);
        }

        // ── Circuit breaker ────────────────────────────────────────────────────
        if self.circuit_breaker.is_open(server_id) {
            warn!(tool = %namespaced, server = %server_id, "MCP circuit open — skipping");
            let slug = server_slug_from_namespaced(namespaced);
            let rname = raw_tool_name(namespaced);
            self.fire_mcp_ingest(triggering_job_id, api_key_id, tenant_id.clone(), server_id, slug.to_string(), rname.to_string(), namespaced.to_string(), "circuit_open", false, 0, 0, 0, loop_round);
            let rec = ToolCallRecord::error(mcp_loop_id, triggering_job_id, loop_round, server_id, rname, namespaced, &args, "circuit_open");
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
                    mcp_loop_id,
                    job_id: triggering_job_id,
                    loop_round,
                    server_id,
                    tool_name: raw_name.to_string(),
                    namespaced_name: namespaced.to_string(),
                    args_json: args.clone(),
                    result_text: Some(text.clone()),
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

        let (text, outcome, result_for_db) = match result {
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
                let db_text = if is_err { None } else { Some(text.clone()) };
                (text, outcome, db_text)
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
                ("{\"error\": \"MCP tool call failed\"}".into(), "error", None)
            }
            Err(_elapsed) => {
                self.circuit_breaker.record_failure(server_id);
                warn!(tool = %namespaced, "MCP tool call timed out");
                ("{\"error\": \"MCP tool call timed out\"}".into(), "timeout", None)
            }
        };

        let bytes = text.len() as u32;
        self.fire_mcp_ingest(triggering_job_id, api_key_id, tenant_id, server_id, server_slug.to_string(), raw_name.to_string(), namespaced.to_string(), outcome, false, latency_ms, bytes, 1, loop_round);

        let rec = ToolCallRecord {
            mcp_loop_id,
            job_id: triggering_job_id,
            loop_round,
            server_id,
            tool_name: raw_name.to_string(),
            namespaced_name: namespaced.to_string(),
            args_json: args.clone(),
            result_text: result_for_db,
            outcome: outcome.to_string(),
            cache_hit: false,
            latency_ms: latency_ms as i32,
            result_bytes: bytes as i32,
        };

        (text, rec)
    }
}

// ── Internal helpers ───────────────────────────────────────────────────────────

/// Data for one tool call row — collected during execution, batch-inserted after the round.
struct ToolCallRecord {
    mcp_loop_id: Uuid,
    job_id: Uuid,
    loop_round: u8,
    server_id: Uuid,
    tool_name: String,
    namespaced_name: String,
    args_json: Value,
    result_text: Option<String>,
    outcome: String,
    cache_hit: bool,
    latency_ms: i32,
    result_bytes: i32,
}

impl ToolCallRecord {
    #[allow(clippy::too_many_arguments)]
    fn error(
        mcp_loop_id: Uuid,
        job_id: Uuid,
        loop_round: u8,
        server_id: Uuid,
        tool_name: &str,
        namespaced_name: &str,
        args: &Value,
        outcome: &str,
    ) -> Self {
        Self {
            mcp_loop_id,
            job_id,
            loop_round,
            server_id,
            tool_name: tool_name.to_string(),
            namespaced_name: namespaced_name.to_string(),
            args_json: args.clone(),
            result_text: None,
            outcome: outcome.to_string(),
            cache_hit: false,
            latency_ms: 0,
            result_bytes: 0,
        }
    }
}

/// Batch INSERT all tool call records for a round in a single multi-row statement.
/// Falls back to a no-op on empty input. Errors are logged, not propagated.
async fn batch_insert_tool_calls(pg_pool: &sqlx::PgPool, mcp_loop_id: Uuid, job_id: Uuid, rows: &[&ToolCallRecord]) {
    if rows.is_empty() {
        return;
    }

    // Build parallel arrays for unnest — avoids dynamic SQL generation and
    // is safe against injection (all values are typed, not interpolated).
    let mut loop_ids: Vec<Uuid> = Vec::with_capacity(rows.len());
    let mut job_ids: Vec<Uuid> = Vec::with_capacity(rows.len());
    let mut rounds: Vec<i16> = Vec::with_capacity(rows.len());
    let mut server_ids: Vec<Uuid> = Vec::with_capacity(rows.len());
    let mut tool_names: Vec<&str> = Vec::with_capacity(rows.len());
    let mut ns_names: Vec<&str> = Vec::with_capacity(rows.len());
    let mut args_jsons: Vec<&Value> = Vec::with_capacity(rows.len());
    let mut result_texts: Vec<Option<&str>> = Vec::with_capacity(rows.len());
    let mut outcomes: Vec<&str> = Vec::with_capacity(rows.len());
    let mut cache_hits: Vec<bool> = Vec::with_capacity(rows.len());
    let mut latencies: Vec<i32> = Vec::with_capacity(rows.len());
    let mut result_bytes: Vec<i32> = Vec::with_capacity(rows.len());

    for r in rows {
        loop_ids.push(r.mcp_loop_id);
        job_ids.push(r.job_id);
        rounds.push(r.loop_round as i16);
        server_ids.push(r.server_id);
        tool_names.push(&r.tool_name);
        ns_names.push(&r.namespaced_name);
        args_jsons.push(&r.args_json);
        result_texts.push(r.result_text.as_deref());
        outcomes.push(&r.outcome);
        cache_hits.push(r.cache_hit);
        latencies.push(r.latency_ms);
        result_bytes.push(r.result_bytes);
    }

    // Unused params mcp_loop_id/job_id kept in signature for callsite clarity.
    let _ = (mcp_loop_id, job_id);

    let _ = sqlx::query(
        "INSERT INTO mcp_loop_tool_calls \
         (mcp_loop_id, job_id, loop_round, server_id, tool_name, namespaced_name, \
          args_json, result_text, outcome, cache_hit, latency_ms, result_bytes) \
         SELECT * FROM unnest($1::uuid[], $2::uuid[], $3::smallint[], $4::uuid[], \
          $5::text[], $6::text[], $7::jsonb[], $8::text[], $9::text[], \
          $10::bool[], $11::int[], $12::int[])"
    )
    .bind(&loop_ids)
    .bind(&job_ids)
    .bind(&rounds)
    .bind(&server_ids)
    .bind(&tool_names)
    .bind(&ns_names)
    .bind(&args_jsons)
    .bind(&result_texts)
    .bind(&outcomes)
    .bind(&cache_hits)
    .bind(&latencies)
    .bind(&result_bytes)
    .execute(pg_pool)
    .await
    .map_err(|e| warn!(error = %e, n = rows.len(), "mcp_loop_tool_calls batch insert failed"));
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
}

/// Collect all tokens from a submitted job into a `RoundResult`.
/// Failure modes from `collect_round`. Each variant maps to a distinct user-facing
/// error so the client can decide its retry strategy (cold-load vs hung vs network).
#[derive(Debug)]
enum RoundError {
    /// No first token received within `FIRST_TOKEN_TIMEOUT` — model is still cold-loading
    /// or unreachable. Client retry usually succeeds because the load completes in
    /// the background and the next request hits a warm model.
    FirstTokenTimeout,
    /// Tokens started but the gap between tokens exceeded `STREAM_IDLE_TIMEOUT` —
    /// the model is genuinely hung. Retry will likely re-trigger the same hang.
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
            Self::FirstTokenTimeout => write!(
                f,
                "model is still loading (first-token timeout {}s exceeded). Retry in a moment.",
                FIRST_TOKEN_TIMEOUT.as_secs()
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

async fn collect_round(state: &AppState, job_id: &JobId) -> Result<RoundResult, RoundError> {
    let mut token_stream = state.use_case.stream(job_id);
    let mut content = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut prompt_tokens: u32 = 0;
    let mut completion_tokens: u32 = 0;
    let mut finish_reason = "stop".to_string();
    let mut received_any_token = false;
    let round_start = Instant::now();

    loop {
        // Hard cap defends against unbounded streams.
        if round_start.elapsed() >= ROUND_TOTAL_TIMEOUT {
            return Err(RoundError::TotalTimeout);
        }
        // Phased timeout: long for the first token (covers cold load), tight after.
        let phase_timeout = if received_any_token {
            STREAM_IDLE_TIMEOUT
        } else {
            FIRST_TOKEN_TIMEOUT
        };

        match tokio::time::timeout(phase_timeout, token_stream.next()).await {
            Ok(Some(Ok(token))) => {
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
                if token.tool_calls.is_some() {
                    if let Some(calls) = token.tool_calls.as_ref().and_then(|v| v.as_array()) {
                        for (i, c) in calls.iter().enumerate() {
                            if validate_tool_call(c) {
                                tool_calls.push(convert_ollama_tool_call(i, c));
                            }
                        }
                    }
                } else if !token.value.is_empty() {
                    content.push_str(&token.value);
                }
            }
            Ok(Some(Err(e))) => return Err(RoundError::Stream(e.to_string())),
            Ok(None) => break, // stream ended cleanly
            Err(_) if !received_any_token => return Err(RoundError::FirstTokenTimeout),
            Err(_) => return Err(RoundError::StreamIdleTimeout),
        }
    }

    Ok(RoundResult { content, tool_calls, prompt_tokens, completion_tokens, finish_reason })
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
            .set::<(), _, _>(&vk_key, json, Some(Expiration::EX(60)), None, false)
            .await
        {
            tracing::warn!(key = %vk_key, error = %e, "mcp: failed to populate acl cache");
        }
    }

    ids
}

/// Fetch mcp_cap_points for the given API key.
///
/// L1: `veronex:mcp:cap:{key_id}` (Valkey, value as decimal string, TTL=60s).
/// L2: DB fallback, result cached for next call.
/// Returns `None` if key absent or cap is NULL (JWT session → use MAX_ROUNDS default).
async fn fetch_mcp_cap_points(state: &AppState, key_id: Uuid) -> Option<u8> {
    use fred::prelude::*;
    let vk_key = crate::infrastructure::outbound::valkey_keys::mcp_key_cap_points(key_id);

    // ── L1: Valkey ─────────────────────────────────────────────────────────────
    if let Some(ref pool) = state.valkey_pool
        && let Ok(Some(cached)) = pool.get::<Option<String>, _>(&vk_key).await
    {
        // "null" sentinel = key exists but cap is NULL (no limit)
        if cached == "null" {
            return None;
        }
        if let Ok(v) = cached.parse::<u8>() {
            return Some(v);
        }
    }

    // ── L2: DB ─────────────────────────────────────────────────────────────────
    let result: Option<i16> = sqlx::query_scalar(
        "SELECT mcp_cap_points FROM api_keys WHERE id = $1"
    )
    .bind(key_id)
    .fetch_optional(&state.pg_pool)
    .await
    .ok()
    .flatten();

    // Populate cache — "null" sentinel for absent/NULL cap.
    if let Some(ref pool) = state.valkey_pool {
        let val = result.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string());
        if let Err(e) = pool
            .set::<(), _, _>(&vk_key, val, Some(Expiration::EX(60)), None, false)
            .await
        {
            tracing::warn!(key = %vk_key, error = %e, "mcp: failed to populate cap_points cache");
        }
    }

    result.map(|v| v as u8)
}

/// Fetch the minimum top_k across granted MCP access rows for a key.
///
/// L1: `veronex:mcp:topk:{key_id}` (Valkey, value as decimal string, TTL=60s).
/// L2: DB fallback, result cached.
/// Returns `None` if all rows have NULL top_k (use global default).
async fn fetch_mcp_top_k(state: &AppState, key_id: Uuid) -> Option<usize> {
    use fred::prelude::*;
    let vk_key = crate::infrastructure::outbound::valkey_keys::mcp_key_top_k(key_id);

    // ── L1: Valkey ─────────────────────────────────────────────────────────────
    if let Some(ref pool) = state.valkey_pool
        && let Ok(Some(cached)) = pool.get::<Option<String>, _>(&vk_key).await
    {
        if cached == "null" {
            return None;
        }
        if let Ok(v) = cached.parse::<usize>() {
            return Some(v);
        }
    }

    // ── L2: DB ─────────────────────────────────────────────────────────────────
    let result: Option<i16> = sqlx::query_scalar(
        "SELECT MIN(top_k) FROM mcp_key_access WHERE api_key_id = $1 AND is_allowed = true AND top_k IS NOT NULL"
    )
    .bind(key_id)
    .fetch_optional(&state.pg_pool)
    .await
    .ok()
    .flatten();

    // Populate cache.
    if let Some(ref pool) = state.valkey_pool {
        let val = result.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string());
        if let Err(e) = pool
            .set::<(), _, _>(&vk_key, val, Some(Expiration::EX(60)), None, false)
            .await
        {
            tracing::warn!(key = %vk_key, error = %e, "mcp: failed to populate top_k cache");
        }
    }

    result.map(|v| v as usize)
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
    fn round_error_first_token_mentions_seconds() {
        let s = RoundError::FirstTokenTimeout.to_string();
        assert!(s.contains("first-token timeout"), "msg = {s}");
        assert!(s.contains(&FIRST_TOKEN_TIMEOUT.as_secs().to_string()), "msg = {s}");
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
    fn first_token_covers_measured_200k_cold_load() {
        // Measured: ollama load_duration ≈ 163 s for qwen3-coder-next-200k:latest
        // (Strix Halo / AI Max+ 395, 200K KV cache q8_0). FIRST_TOKEN_TIMEOUT must
        // exceed this with safety buffer.
        const MEASURED_200K_COLD_LOAD_SECS: u64 = 163;
        assert!(
            FIRST_TOKEN_TIMEOUT.as_secs() > MEASURED_200K_COLD_LOAD_SECS,
            "FIRST_TOKEN_TIMEOUT ({}s) does not cover measured 200K cold load ({}s)",
            FIRST_TOKEN_TIMEOUT.as_secs(),
            MEASURED_200K_COLD_LOAD_SECS,
        );
    }

    #[test]
    fn round_total_does_not_exceed_route_layer() {
        // INFERENCE_ROUTER_TIMEOUT in inbound::http::constants is 360 s. Bridge must
        // surface its own outcome before the tower-http layer fires a 408.
        assert!(ROUND_TOTAL_TIMEOUT.as_secs() <= 360);
    }

    #[test]
    fn stream_idle_shorter_than_first_token() {
        // Once tokens are flowing, the appropriate timeout is much tighter — a hung
        // model should be detected fast.
        assert!(STREAM_IDLE_TIMEOUT < FIRST_TOKEN_TIMEOUT);
    }
}
