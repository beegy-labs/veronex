//! veronex-mcp — unified MCP server.
//!
//! Single deployment, multiple tools. Add tools in `src/tools/` and register below.
//!
//! Run: `RUST_LOG=info cargo run -p veronex-mcp --bin veronex-mcp`
//! Default port: 3100 (override with `PORT` env var).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{debug, info, warn};

use veronex_mcp::tools::{Tool, analyze_image::AnalyzeImageTool, weather::WeatherTool, web_search::WebSearchTool};

// ── App state ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    /// Precomputed specs for `tools/list` — avoids calling `spec()` per request.
    specs: Vec<Value>,
    /// O(1) name → tool lookup for `tools/call`.
    tool_map: HashMap<String, Arc<dyn Tool>>,
    req_counter: Arc<AtomicU64>,
}

impl AppState {
    fn new(tools: Vec<Arc<dyn Tool>>) -> Self {
        let mut specs = Vec::with_capacity(tools.len());
        let mut tool_map = HashMap::with_capacity(tools.len());
        for t in &tools {
            let spec = t.spec();
            if let Some(name) = spec["name"].as_str() {
                tool_map.insert(name.to_string(), Arc::clone(t));
            }
            specs.push(spec);
        }
        Self { specs, tool_map, req_counter: Arc::new(AtomicU64::new(0)) }
    }

    fn find_tool(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tool_map.get(name)
    }
}

// ── JSON-RPC ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

impl JsonRpcResponse {
    fn ok(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
    }
    fn err(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self { jsonrpc: "2.0".into(), id, result: None,
               error: Some(json!({ "code": code, "message": message.into() })) }
    }
}

// ── HTTP handler ──────────────────────────────────────────────────────────────

async fn mcp_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<JsonRpcRequest>,
) -> Response {
    if req.jsonrpc != "2.0" {
        return (StatusCode::BAD_REQUEST,
                Json(JsonRpcResponse::err(Value::Null, -32600, "jsonrpc must be \"2.0\""))).into_response();
    }
    let id = req.id.unwrap_or(Value::Null);
    let session_id = headers.get("mcp-session-id").and_then(|v| v.to_str().ok()).unwrap_or("-");
    let count = state.req_counter.fetch_add(1, Ordering::Relaxed);
    debug!(method = %req.method, session = %session_id, req = count, "MCP request");

    let resp = match req.method.as_str() {
        "initialize" => {
            info!(client = %req.params["clientInfo"]["name"].as_str().unwrap_or("unknown"), "MCP initialize");
            JsonRpcResponse::ok(id, json!({
                "protocolVersion": "2025-03-26",
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "veronex-mcp", "version": env!("CARGO_PKG_VERSION") }
            }))
        }
        "notifications/initialized" => return StatusCode::ACCEPTED.into_response(),
        "ping" => JsonRpcResponse::ok(id, json!({})),
        "tools/list" => {
            JsonRpcResponse::ok(id, json!({ "tools": state.specs }))
        }
        "tools/call" => {
            let tool_name = req.params["name"].as_str().unwrap_or("");
            let args = &req.params["arguments"];
            debug!(tool = %tool_name, "tools/call");
            match state.find_tool(tool_name) {
                Some(tool) => match tool.call(args).await {
                    Ok(data) => JsonRpcResponse::ok(id, json!({
                        "content": [{ "type": "text", "text": data.to_string() }],
                        "isError": false
                    })),
                    Err(e) => {
                        warn!(tool = %tool_name, error = %e, "tools/call error");
                        JsonRpcResponse::ok(id, json!({
                            "content": [{ "type": "text", "text": e }],
                            "isError": true
                        }))
                    }
                },
                None => JsonRpcResponse::err(id, -32601, format!("Unknown tool: {tool_name}")),
            }
        }
        method => {
            warn!(method = %method, "Unknown MCP method");
            JsonRpcResponse::err(id, -32601, format!("Method not found: {method}"))
        }
    };
    (StatusCode::OK, Json(resp)).into_response()
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "veronex-mcp" }))
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info".into()))
        .init();

    info!("Loading geo index...");
    veronex_mcp::geo::preload();
    info!("Geo index ready");

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3100);

    let valkey = if let Ok(url) = std::env::var("VALKEY_URL") {
        use fred::prelude::*;
        let pool_size: usize = std::env::var("WEATHER_VALKEY_POOL")
            .ok().and_then(|v| v.parse().ok()).unwrap_or(4);
        let cfg = Config::from_url(&url).expect("invalid VALKEY_URL");
        let pool = Pool::new(cfg, None, None, None, pool_size).expect("valkey pool init failed");
        pool.init().await.expect("valkey connect failed");
        info!("Valkey cache connected (pool_size={pool_size})");
        Some(Arc::new(pool))
    } else {
        info!("VALKEY_URL not set — using in-memory L1 cache only");
        None
    };

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("veronex-mcp/1.0")
        .build()
        .expect("Failed to build HTTP client");

    let l1_max: u64 = std::env::var("WEATHER_L1_MAX_ENTRIES")
        .ok().and_then(|v| v.parse().ok())
        .unwrap_or(veronex_mcp::tools::weather::L1_MAX_ENTRIES_DEFAULT);

    // ── Tool registration ─────────────────────────────────────────────────────
    // To add a tool: create tools/{name}.rs, implement Tool, add Arc::new(...) here.
    let tools: Vec<Arc<dyn Tool>> = vec![
        Arc::new(AnalyzeImageTool::new(http.clone())),
        Arc::new(WeatherTool::new(http.clone(), valkey, l1_max)),
        Arc::new(WebSearchTool::new(http)),
    ];

    let state = AppState::new(tools);
    let names: Vec<String> = state.specs.iter()
        .filter_map(|s| s["name"].as_str().map(str::to_string))
        .collect();
    info!(tool_count = state.specs.len(), "Tools registered: {}", names.join(", "));

    let app = Router::new()
        .route("/", post(mcp_handler))
        .route("/health", axum::routing::get(health))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("Failed to bind");
    info!(addr = %addr, "veronex-mcp listening");
    axum::serve(listener, app).await.expect("Server error");
}
