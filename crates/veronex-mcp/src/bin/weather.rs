//! Weather MCP server — reference implementation for veronex-mcp.
//!
//! Tools exposed:
//!   - `get_coordinates`  — resolve city name → (lat, lng) via geocoding
//!   - `get_weather`      — fetch current weather from open-meteo.com
//!
//! Transport: MCP 2025-03-26 Streamable HTTP (single `/mcp` endpoint).
//! Session management: stateless (no Mcp-Session-Id issued).
//!
//! Run: `RUST_LOG=info cargo run -p veronex-mcp --bin weather-mcp`
//! Default port: 3100 (override with `PORT` env var).

use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

// ── Tool schemas ──────────────────────────────────────────────────────────────

fn tool_list() -> Value {
    json!([
        {
            "name": "get_coordinates",
            "description": "Resolve a city name to geographic coordinates (latitude, longitude) using the Open-Meteo geocoding API.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "city": {
                        "type": "string",
                        "description": "City name, e.g. \"Seoul\", \"Tokyo\", \"London\""
                    },
                    "count": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 1)",
                        "default": 1
                    }
                },
                "required": ["city"]
            },
            "annotations": {
                "readOnlyHint": true,
                "idempotentHint": true,
                "destructiveHint": false,
                "openWorldHint": true
            }
        },
        {
            "name": "get_weather",
            "description": "Fetch current weather conditions for a location using the Open-Meteo API. Returns temperature, wind speed, weather code, and more.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "latitude": {
                        "type": "number",
                        "description": "Geographic latitude in decimal degrees (-90 to 90)"
                    },
                    "longitude": {
                        "type": "number",
                        "description": "Geographic longitude in decimal degrees (-180 to 180)"
                    },
                    "temperature_unit": {
                        "type": "string",
                        "enum": ["celsius", "fahrenheit"],
                        "description": "Temperature unit (default: celsius)",
                        "default": "celsius"
                    },
                    "wind_speed_unit": {
                        "type": "string",
                        "enum": ["kmh", "ms", "mph", "kn"],
                        "description": "Wind speed unit (default: kmh)",
                        "default": "kmh"
                    }
                },
                "required": ["latitude", "longitude"]
            },
            "annotations": {
                "readOnlyHint": true,
                "idempotentHint": false,
                "destructiveHint": false,
                "openWorldHint": true
            }
        }
    ])
}

// ── WMO weather code descriptions ─────────────────────────────────────────────

fn wmo_description(code: u64) -> &'static str {
    match code {
        0 => "Clear sky",
        1 => "Mainly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 => "Fog",
        48 => "Depositing rime fog",
        51 => "Light drizzle",
        53 => "Moderate drizzle",
        55 => "Dense drizzle",
        61 => "Slight rain",
        63 => "Moderate rain",
        65 => "Heavy rain",
        71 => "Slight snow",
        73 => "Moderate snow",
        75 => "Heavy snow",
        77 => "Snow grains",
        80 => "Slight rain showers",
        81 => "Moderate rain showers",
        82 => "Violent rain showers",
        85 => "Slight snow showers",
        86 => "Heavy snow showers",
        95 => "Thunderstorm",
        96 => "Thunderstorm with slight hail",
        99 => "Thunderstorm with heavy hail",
        _ => "Unknown",
    }
}

// ── Tool implementations ───────────────────────────────────────────────────────

async fn handle_get_coordinates(
    client: &reqwest::Client,
    args: &Value,
) -> Result<Value, String> {
    let city = args["city"]
        .as_str()
        .ok_or("Missing required argument: city")?;
    let count = args["count"].as_u64().unwrap_or(1).clamp(1, 10);

    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count={}&language=en&format=json",
        urlencoding::encode(city),
        count
    );

    let resp: Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Geocoding request failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Geocoding response parse error: {e}"))?;

    let results = resp["results"].as_array().ok_or_else(|| {
        format!("No results found for city: {city}")
    })?;

    if results.is_empty() {
        return Err(format!("No results found for city: {city}"));
    }

    let locations: Vec<Value> = results
        .iter()
        .map(|r| {
            json!({
                "name": r["name"],
                "country": r["country"],
                "country_code": r["country_code"],
                "admin1": r["admin1"],
                "latitude": r["latitude"],
                "longitude": r["longitude"],
                "elevation": r["elevation"],
                "timezone": r["timezone"],
                "population": r["population"]
            })
        })
        .collect();

    Ok(json!({
        "city": city,
        "results": locations
    }))
}

async fn handle_get_weather(
    client: &reqwest::Client,
    args: &Value,
) -> Result<Value, String> {
    let lat = args["latitude"]
        .as_f64()
        .ok_or("Missing required argument: latitude")?;
    let lng = args["longitude"]
        .as_f64()
        .ok_or("Missing required argument: longitude")?;

    if !(-90.0..=90.0).contains(&lat) {
        return Err(format!("latitude {lat} out of range [-90, 90]"));
    }
    if !(-180.0..=180.0).contains(&lng) {
        return Err(format!("longitude {lng} out of range [-180, 180]"));
    }

    let temp_unit = args["temperature_unit"].as_str().unwrap_or("celsius");
    let wind_unit = args["wind_speed_unit"].as_str().unwrap_or("kmh");

    let url = format!(
        "https://api.open-meteo.com/v1/forecast\
         ?latitude={lat}&longitude={lng}\
         &current=temperature_2m,relative_humidity_2m,apparent_temperature,\
         precipitation,weather_code,cloud_cover,wind_speed_10m,wind_direction_10m,\
         wind_gusts_10m,surface_pressure,is_day\
         &temperature_unit={temp_unit}&wind_speed_unit={wind_unit}&timezone=auto"
    );

    let resp: Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Open-Meteo request failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Open-Meteo response parse error: {e}"))?;

    if let Some(err) = resp["error"].as_bool() {
        if err {
            let reason = resp["reason"].as_str().unwrap_or("unknown");
            return Err(format!("Open-Meteo API error: {reason}"));
        }
    }

    let current = &resp["current"];
    let units = &resp["current_units"];

    let weather_code = current["weather_code"].as_u64().unwrap_or(0);

    Ok(json!({
        "location": {
            "latitude": lat,
            "longitude": lng,
            "timezone": resp["timezone"],
            "timezone_abbreviation": resp["timezone_abbreviation"],
            "elevation": resp["elevation"]
        },
        "current": {
            "time": current["time"],
            "is_day": current["is_day"].as_u64().map(|v| v == 1),
            "weather_code": weather_code,
            "weather_description": wmo_description(weather_code),
            "temperature": {
                "value": current["temperature_2m"],
                "unit": units["temperature_2m"]
            },
            "apparent_temperature": {
                "value": current["apparent_temperature"],
                "unit": units["apparent_temperature"]
            },
            "relative_humidity_percent": current["relative_humidity_2m"],
            "precipitation": {
                "value": current["precipitation"],
                "unit": units["precipitation"]
            },
            "cloud_cover_percent": current["cloud_cover"],
            "surface_pressure_hpa": current["surface_pressure"],
            "wind": {
                "speed": {
                    "value": current["wind_speed_10m"],
                    "unit": units["wind_speed_10m"]
                },
                "direction_degrees": current["wind_direction_10m"],
                "gusts": {
                    "value": current["wind_gusts_10m"],
                    "unit": units["wind_gusts_10m"]
                }
            }
        }
    }))
}

// ── JSON-RPC dispatch ─────────────────────────────────────────────────────────

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
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(json!({ "code": code, "message": message.into() })),
        }
    }
}

// ── App state ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    http: reqwest::Client,
    req_counter: std::sync::Arc<AtomicU64>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

async fn mcp_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<JsonRpcRequest>,
) -> Response {
    if req.jsonrpc != "2.0" {
        return (
            StatusCode::BAD_REQUEST,
            Json(JsonRpcResponse::err(
                Value::Null,
                -32600,
                "Invalid Request: jsonrpc must be \"2.0\"",
            )),
        )
            .into_response();
    }

    let id = req.id.unwrap_or(Value::Null);
    let session_id = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");

    let count = state.req_counter.fetch_add(1, Ordering::Relaxed);
    debug!(method = %req.method, session = %session_id, req = count, "MCP request");

    let resp = match req.method.as_str() {
        // ── Lifecycle ────────────────────────────────────────────────────────
        "initialize" => {
            let client_name = req.params["clientInfo"]["name"]
                .as_str()
                .unwrap_or("unknown");
            info!(client = %client_name, "MCP initialize");

            JsonRpcResponse::ok(
                id,
                json!({
                    "protocolVersion": "2025-03-26",
                    "capabilities": {
                        "tools": { "listChanged": false }
                    },
                    "serverInfo": {
                        "name": "weather-mcp",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )
        }

        "notifications/initialized" => {
            // Fire-and-forget notification — return 202 with empty body.
            return StatusCode::ACCEPTED.into_response();
        }

        "ping" => JsonRpcResponse::ok(id, json!({})),

        // ── Tools ────────────────────────────────────────────────────────────
        "tools/list" => JsonRpcResponse::ok(id, json!({ "tools": tool_list() })),

        "tools/call" => {
            let tool_name = req.params["name"].as_str().unwrap_or("");
            let args = &req.params["arguments"];

            debug!(tool = %tool_name, "tools/call");

            let result = match tool_name {
                "get_coordinates" => handle_get_coordinates(&state.http, args).await,
                "get_weather" => handle_get_weather(&state.http, args).await,
                other => Err(format!("Unknown tool: {other}")),
            };

            match result {
                Ok(data) => JsonRpcResponse::ok(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": data.to_string() }],
                        "isError": false
                    }),
                ),
                Err(e) => {
                    warn!(tool = %tool_name, error = %e, "tools/call error");
                    JsonRpcResponse::ok(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": e }],
                            "isError": true
                        }),
                    )
                }
            }
        }

        // ── Unknown ──────────────────────────────────────────────────────────
        method => {
            warn!(method = %method, "Unknown MCP method");
            JsonRpcResponse::err(id, -32601, format!("Method not found: {method}"))
        }
    };

    (StatusCode::OK, Json(resp)).into_response()
}

// ── Health check ──────────────────────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "weather-mcp" }))
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3100);

    let state = AppState {
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("veronex-weather-mcp/1.0")
            .build()
            .expect("Failed to build HTTP client"),
        req_counter: std::sync::Arc::new(AtomicU64::new(0)),
    };

    let app = Router::new()
        .route("/mcp", post(mcp_handler))
        .route("/health", axum::routing::get(health))
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    info!(addr = %addr, "weather-mcp listening");
    axum::serve(listener, app).await.expect("Server error");
}
