//! `get_weather` tool — high-throughput weather + air quality.
//!
//! ## Cache architecture (1M+ TPS)
//!
//! ```text
//! Request
//!   → snap coords to 0.01° grid (≈1.1 km)
//!   → L1: Moka (in-process, bounded W-TinyLFU, < 1 µs)
//!       fresh  → return immediately
//!       stale  → return immediately + spawn background refresh
//!   → L2: Valkey (shared across instances, < 1 ms)
//!   → Singleflight gate (prevents stampede on cold cache)
//!   → Open-Meteo API (2 parallel calls, full 7-day raw data)
//!       → cache in L2 then L1
//!       → singleflight waiters resolved from L1
//! ```
//!
//! One API fetch caches all periods for a location.
//! Period slicing (morning/evening/week/…) is done in-process — zero extra API calls.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::{Value, json};
use tracing::debug;

use super::Tool;
use crate::geo;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Coordinate grid precision: 0.01° ≈ 1.1 km — all coords within the same
/// cell share one cache entry.
const COORD_PRECISION: f64 = 100.0;

/// L1 (Moka) hard eviction TTL = L2 TTL + grace.
const L1_TTL: Duration = Duration::from_secs(4_200); // 70 min

/// L2 (Valkey) TTL — Open-Meteo's own recommendation.
const L2_TTL_SECS: u64 = 3_600; // 60 min

/// Stale-while-revalidate window after L2 TTL expires.
const GRACE_SECS: u64 = 600; // 10 min

/// Default L1 max entries (each ~50-100 KB of raw JSON).
pub const L1_MAX_ENTRIES_DEFAULT: u64 = 10_000;

/// Open-Meteo: request 8 days so day_offset 0-6 always has data + buffer.
const FORECAST_DAYS: u32 = 8;
/// Timeout for geocoding fallback HTTP requests (Open-Meteo API).
const GEOCODING_API_TIMEOUT: Duration = Duration::from_secs(5);

pub type ValkeyPool = fred::clients::Pool;
type InflightMap = DashMap<String, Arc<tokio::sync::watch::Sender<bool>>>;

// ── Raw cache entry ────────────────────────────────────────────────────────────

/// Full raw API response for one grid cell. All period slices derived from this.
pub(super) struct RawEntry {
    forecast: Value,
    aq: Value,
    fetched_at: Instant,
}

impl RawEntry {
    fn is_fresh(&self) -> bool {
        self.fetched_at.elapsed().as_secs() < L2_TTL_SECS
    }
    fn is_in_grace(&self) -> bool {
        let age = self.fetched_at.elapsed().as_secs();
        age >= L2_TTL_SECS && age < L2_TTL_SECS + GRACE_SECS
    }
}

// ── WeatherState ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct WeatherState {
    pub http: reqwest::Client,
    pub(super) l1: Arc<moka::sync::Cache<String, Arc<RawEntry>>>,
    inflight: Arc<InflightMap>,
    pub valkey: Option<Arc<ValkeyPool>>,
    pub req_counter: Arc<AtomicU64>,
}

// ── WeatherTool ───────────────────────────────────────────────────────────────

pub struct WeatherTool {
    state: WeatherState,
}

impl WeatherTool {
    pub fn new(http: reqwest::Client, valkey: Option<Arc<ValkeyPool>>, l1_max: u64) -> Self {
        let l1 = moka::sync::Cache::builder()
            .max_capacity(l1_max)
            .time_to_live(L1_TTL)
            .build();
        Self {
            state: WeatherState {
                http,
                l1: Arc::new(l1),
                inflight: Arc::new(DashMap::new()),
                valkey,
                req_counter: Arc::new(AtomicU64::new(0)),
            },
        }
    }
}

#[async_trait]
impl Tool for WeatherTool {
    fn spec(&self) -> Value {
        json!({
            "name": "get_weather",
            "description": "Fetch weather conditions and air quality for a location. \
                Supports current conditions and forecasts up to 6 days ahead by time-of-day. \
                Returns temperature, feels-like, humidity, UV index, precipitation, \
                wind speed/direction/gusts, PM2.5, PM10, and AQI. \
                Accepts city names in any language including sub-city districts \
                (e.g. \"여의도\", \"강남\", \"Tokyo Shibuya\") or GPS coordinates (lat/lng). \
                Use period='hourly' to get a rain timeline (rain_spans, rain_ends_at) \
                when asked 'until when does it rain' or 'hourly precipitation'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "city": {
                        "type": "string",
                        "description": "City or district name in any language. Required unless lat/lng provided."
                    },
                    "lat": {
                        "type": "number",
                        "description": "WGS84 latitude (-90 to 90). Use instead of city for GPS coordinates.",
                        "minimum": -90,
                        "maximum": 90
                    },
                    "lng": {
                        "type": "number",
                        "description": "WGS84 longitude (-180 to 180). Use instead of city for GPS coordinates.",
                        "minimum": -180,
                        "maximum": 180
                    },
                    "day_offset": {
                        "type": "integer",
                        "description": "Days from today (0=today … 6=6 days out). Default 0.",
                        "minimum": 0,
                        "maximum": 6,
                        "default": 0
                    },
                    "period": {
                        "type": "string",
                        "enum": ["now", "morning", "afternoon", "evening", "night", "full", "hourly", "week"],
                        "description": "Time period. 'now'=current conditions, 'morning/afternoon/evening/night'=specific time slot, 'full'=daily summary, 'hourly'=24-hour rain timeline with rain_spans and rain_ends_at (best for 'until when does it rain?'), 'week'=7-day array. Default: 'now' for day_offset=0, 'full' otherwise."
                    },
                    "temperature_unit": {
                        "type": "string",
                        "enum": ["celsius", "fahrenheit"],
                        "default": "celsius"
                    },
                    "wind_speed_unit": {
                        "type": "string",
                        "enum": ["kmh", "ms", "mph", "kn"],
                        "default": "kmh"
                    }
                },
                "required": []
            },
            "annotations": {
                "readOnlyHint": true,
                "idempotentHint": false,
                "destructiveHint": false,
                "openWorldHint": true
            }
        })
    }

    async fn call(&self, args: &Value) -> Result<Value, String> {
        handle_get_weather(&self.state, args).await
    }
}

// ── Coordinate helpers ────────────────────────────────────────────────────────

#[inline]
fn snap(v: f64) -> f64 {
    (v * COORD_PRECISION).round() / COORD_PRECISION
}

// ── Date helpers ──────────────────────────────────────────────────────────────

fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 29 } else { 28 },
        _ => 30,
    }
}

fn add_days(date: &str, days: u8) -> String {
    if days == 0 { return date.to_string(); }
    let y: i32 = date[..4].parse().unwrap_or(2026);
    let m: i32 = date[5..7].parse().unwrap_or(1);
    let d: i32 = date[8..10].parse().unwrap_or(1);
    let mut dd = d + days as i32;
    let mut mm = m; let mut yy = y;
    loop {
        let dim = days_in_month(yy, mm);
        if dd <= dim { break; }
        dd -= dim; mm += 1;
        if mm > 12 { mm = 1; yy += 1; }
    }
    format!("{yy:04}-{mm:02}-{dd:02}")
}

fn period_center_hour(period: &str) -> u8 {
    match period {
        "morning"   => 9,
        "afternoon" => 15,
        "evening"   => 20,
        "night"     => 23,
        _           => 12,
    }
}

// ── Unit conversion ────────────────────────────────────────────────────────────

fn convert_temp(celsius: f64, unit: &str) -> (f64, &'static str) {
    match unit {
        "fahrenheit" => (celsius * 9.0 / 5.0 + 32.0, "°F"),
        _            => (celsius, "°C"),
    }
}

fn convert_wind(kmh: f64, unit: &str) -> (f64, &'static str) {
    match unit {
        "ms"  => (kmh / 3.6,       "m/s"),
        "mph" => (kmh * 0.621371,  "mph"),
        "kn"  => (kmh * 0.539957,  "kn"),
        _     => (kmh,             "km/h"),
    }
}

fn temp_val(v: &Value, unit: &str) -> Value {
    let c = v.as_f64().unwrap_or(0.0);
    let (t, _) = convert_temp(c, unit);
    json!((t * 10.0).round() / 10.0)
}

fn wind_val(v: &Value, unit: &str) -> Value {
    let k = v.as_f64().unwrap_or(0.0);
    let (w, _) = convert_wind(k, unit);
    json!((w * 10.0).round() / 10.0)
}

fn temp_unit_label(unit: &str) -> &'static str {
    if unit == "fahrenheit" { "°F" } else { "°C" }
}

fn wind_unit_label(unit: &str) -> &'static str {
    match unit { "ms" => "m/s", "mph" => "mph", "kn" => "kn", _ => "km/h" }
}

// ── WMO weather codes ─────────────────────────────────────────────────────────

fn wmo_description(code: u64) -> &'static str {
    match code {
        0  => "Clear sky",       1  => "Mainly clear",     2  => "Partly cloudy",
        3  => "Overcast",        45 => "Fog",               48 => "Rime fog",
        51 => "Light drizzle",   53 => "Drizzle",           55 => "Dense drizzle",
        61 => "Slight rain",     63 => "Rain",              65 => "Heavy rain",
        71 => "Slight snow",     73 => "Snow",              75 => "Heavy snow",
        77 => "Snow grains",     80 => "Rain showers",      81 => "Showers",
        82 => "Violent showers", 85 => "Snow showers",      86 => "Heavy snow showers",
        95 => "Thunderstorm",    96 => "Thunderstorm+hail", 99 => "Thunderstorm+heavy hail",
        _  => "Unknown",
    }
}

// ── AQI helpers ───────────────────────────────────────────────────────────────

fn eu_aqi_category(v: u64) -> &'static str {
    match v { 0..=20 => "Good", 21..=40 => "Fair", 41..=60 => "Moderate",
              61..=80 => "Poor", 81..=100 => "Very Poor", _ => "Extremely Poor" }
}

fn us_aqi_category(v: u64) -> &'static str {
    match v { 0..=50 => "Good", 51..=100 => "Moderate",
              101..=150 => "Unhealthy for Sensitive Groups", 151..=200 => "Unhealthy",
              201..=300 => "Very Unhealthy", _ => "Hazardous" }
}

fn build_aq(aq_h: &Value, ai: usize) -> Value {
    let eu = aq_h["european_aqi"][ai].as_u64().unwrap_or(0);
    let us = aq_h["us_aqi"][ai].as_u64().unwrap_or(0);
    json!({
        "pm2_5": { "value": aq_h["pm2_5"][ai], "unit": "µg/m³" },
        "pm10":  { "value": aq_h["pm10"][ai],  "unit": "µg/m³" },
        "european_aqi": { "value": eu, "category": eu_aqi_category(eu) },
        "us_aqi":       { "value": us, "category": us_aqi_category(us) }
    })
}

fn find_time_idx(times: &Value, target: &str) -> Option<usize> {
    times.as_array()?.iter().position(|t| t.as_str() == Some(target))
}

fn check_api_error(resp: &Value, name: &str) -> Result<(), String> {
    if resp["error"].as_bool().unwrap_or(false) {
        Err(format!("{name}: {}", resp["reason"].as_str().unwrap_or("unknown")))
    } else {
        Ok(())
    }
}

// ── Raw API fetch ─────────────────────────────────────────────────────────────

async fn fetch_raw(client: &reqwest::Client, lat: f64, lng: f64) -> Result<RawEntry, String> {
    let forecast_url = format!(
        "https://api.open-meteo.com/v1/forecast\
         ?latitude={lat}&longitude={lng}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,\
         precipitation,precipitation_probability,weather_code,cloud_cover,\
         uv_index,wind_speed_10m,wind_direction_10m,wind_gusts_10m,is_day\
         &hourly=temperature_2m,apparent_temperature,relative_humidity_2m,\
         precipitation_probability,precipitation,weather_code,cloud_cover,\
         uv_index,wind_speed_10m,wind_direction_10m,wind_gusts_10m,is_day\
         &daily=temperature_2m_max,temperature_2m_min,apparent_temperature_max,\
         apparent_temperature_min,precipitation_sum,precipitation_probability_max,\
         weather_code,wind_speed_10m_max,wind_gusts_10m_max,wind_direction_10m_dominant,\
         uv_index_max,sunrise,sunset\
         &forecast_days={FORECAST_DAYS}&timezone=auto"
    );
    let aq_url = format!(
        "https://air-quality-api.open-meteo.com/v1/air-quality\
         ?latitude={lat}&longitude={lng}\
         &current=pm2_5,pm10,european_aqi,us_aqi\
         &hourly=pm2_5,pm10,european_aqi,us_aqi\
         &forecast_days=7&timezone=auto"
    );

    let (fr, aqr) = tokio::join!(
        client.get(&forecast_url).send(),
        client.get(&aq_url).send()
    );
    let forecast: Value = fr.map_err(|e| format!("Forecast request failed: {e}"))?.json().await
        .map_err(|e| format!("Forecast parse error: {e}"))?;
    let aq: Value = aqr.map_err(|e| format!("Air quality request failed: {e}"))?.json().await
        .map_err(|e| format!("Air quality parse error: {e}"))?;
    check_api_error(&forecast, "Open-Meteo")?;
    check_api_error(&aq, "Air Quality API")?;

    Ok(RawEntry { forecast, aq, fetched_at: Instant::now() })
}

// ── Valkey L2 helpers ─────────────────────────────────────────────────────────

async fn valkey_get_raw(pool: &ValkeyPool, key: &str) -> Option<RawEntry> {
    use fred::prelude::*;
    let raw: Option<String> = pool.get(key).await.ok()?;
    let val: serde_json::Value = serde_json::from_str(&raw?).ok()?;
    Some(RawEntry {
        forecast: val["forecast"].clone(),
        aq: val["aq"].clone(),
        fetched_at: Instant::now(),
    })
}

async fn valkey_set_raw(pool: &ValkeyPool, key: &str, entry: &RawEntry) {
    use fred::prelude::*;
    let payload = match serde_json::to_string(&json!({
        "forecast": &entry.forecast,
        "aq": &entry.aq,
    })) {
        Ok(s) => s,
        Err(_) => return,
    };
    let _ = pool.set::<(), _, _>(
        key, payload,
        Some(fred::types::Expiration::EX(L2_TTL_SECS as i64)),
        None, false,
    ).await;
}

async fn check_rate_limit(pool: &ValkeyPool) -> bool {
    use fred::prelude::*;
    let limit: u64 = std::env::var("WEATHER_API_RATE_LIMIT")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let key = format!("weather:rate:{}", chrono::Utc::now().format("%Y%m%d%H"));
    let count: u64 = pool.incr(&key).await.unwrap_or(0);
    if count == 1 { let _ = pool.expire::<(), _>(&key, 3600, None).await; }
    count <= limit
}

// ── Cache resolution: L1 → L2 → singleflight → API ──────────────────────────

async fn get_raw(state: &WeatherState, cache_key: &str, lat: f64, lng: f64) -> Result<Arc<RawEntry>, String> {
    if let Some(entry) = state.l1.get(cache_key) {
        if entry.is_fresh() {
            return Ok(entry);
        }
        if entry.is_in_grace() {
            let state2 = state.clone();
            let key2 = cache_key.to_string();
            tokio::spawn(async move {
                let _ = do_fetch(&state2, &key2, lat, lng).await;
            });
            return Ok(entry);
        }
    }

    if let Some(pool) = &state.valkey {
        if let Some(raw) = valkey_get_raw(pool, cache_key).await {
            let entry = Arc::new(raw);
            state.l1.insert(cache_key.to_string(), entry.clone());
            return Ok(entry);
        }
    }

    let (tx, is_leader) = {
        use dashmap::Entry;
        match state.inflight.entry(cache_key.to_string()) {
            Entry::Vacant(e) => {
                let (tx, _) = tokio::sync::watch::channel(false);
                let tx = Arc::new(tx);
                e.insert(tx.clone());
                (tx, true)
            }
            Entry::Occupied(e) => (e.get().clone(), false),
        }
    };

    if !is_leader {
        let mut rx = tx.subscribe();
        rx.wait_for(|v| *v).await.ok();
        return state.l1.get(cache_key)
            .map(Ok)
            .unwrap_or_else(|| Err("singleflight fetch failed".into()));
    }

    let result = do_fetch(state, cache_key, lat, lng).await;
    let _ = tx.send(true);
    state.inflight.remove(cache_key);
    result
}

async fn do_fetch(state: &WeatherState, cache_key: &str, lat: f64, lng: f64) -> Result<Arc<RawEntry>, String> {
    if let Some(pool) = &state.valkey {
        if !check_rate_limit(pool).await {
            return Err("Weather API rate limit exceeded. Try again later.".into());
        }
    }
    let raw = fetch_raw(&state.http, lat, lng).await?;
    let entry = Arc::new(raw);
    if let Some(pool) = &state.valkey {
        valkey_set_raw(pool, cache_key, &entry).await;
    }
    state.l1.insert(cache_key.to_string(), entry.clone());
    Ok(entry)
}

// ── Period slicers ────────────────────────────────────────────────────────────

fn slice_current(entry: &RawEntry, temp_unit: &str, wind_unit: &str) -> Result<Value, String> {
    let cur = &entry.forecast["current"];
    let aq_cur = &entry.aq["current"];
    let weather_code = cur["weather_code"].as_u64().unwrap_or(0);
    let eu_aqi = aq_cur["european_aqi"].as_u64().unwrap_or(0);
    let us_aqi  = aq_cur["us_aqi"].as_u64().unwrap_or(0);
    Ok(json!({
        "time": cur["time"],
        "period": "now",
        "is_day": cur["is_day"].as_u64().map(|v| v == 1),
        "weather_code": weather_code,
        "weather": wmo_description(weather_code),
        "temperature": {
            "value": temp_val(&cur["temperature_2m"], temp_unit),
            "feels_like": temp_val(&cur["apparent_temperature"], temp_unit),
            "unit": temp_unit_label(temp_unit)
        },
        "humidity_percent": cur["relative_humidity_2m"],
        "uv_index": cur["uv_index"],
        "precipitation": {
            "value": cur["precipitation"],
            "probability_percent": cur["precipitation_probability"],
            "unit": "mm"
        },
        "cloud_cover_percent": cur["cloud_cover"],
        "wind": {
            "speed": { "value": wind_val(&cur["wind_speed_10m"], wind_unit), "unit": wind_unit_label(wind_unit) },
            "direction_degrees": cur["wind_direction_10m"],
            "gusts": { "value": wind_val(&cur["wind_gusts_10m"], wind_unit), "unit": wind_unit_label(wind_unit) }
        },
        "air_quality": {
            "pm2_5": { "value": aq_cur["pm2_5"], "unit": "µg/m³" },
            "pm10":  { "value": aq_cur["pm10"],  "unit": "µg/m³" },
            "european_aqi": { "value": eu_aqi, "category": eu_aqi_category(eu_aqi) },
            "us_aqi":       { "value": us_aqi, "category": us_aqi_category(us_aqi) }
        }
    }))
}

fn slice_hourly(entry: &RawEntry, day_offset: u8, period: &str, temp_unit: &str, wind_unit: &str) -> Result<Value, String> {
    let target_hour = period_center_hour(period);
    let times = &entry.forecast["hourly"]["time"];
    let base_date = times[0].as_str().and_then(|t| t.get(..10))
        .ok_or("Cannot parse base date")?;
    let target_date = add_days(base_date, day_offset);
    let target_ts = format!("{target_date}T{target_hour:02}:00");
    let idx = find_time_idx(times, &target_ts)
        .ok_or_else(|| format!("No hourly data for {target_ts}"))?;
    let h = &entry.forecast["hourly"];
    let weather_code = h["weather_code"][idx].as_u64().unwrap_or(0);
    let air_quality = find_time_idx(&entry.aq["hourly"]["time"], &target_ts)
        .map(|ai| build_aq(&entry.aq["hourly"], ai))
        .unwrap_or(Value::Null);
    Ok(json!({
        "time": target_ts,
        "period": period,
        "is_day": h["is_day"][idx].as_u64().map(|v| v == 1),
        "weather_code": weather_code,
        "weather": wmo_description(weather_code),
        "temperature": {
            "value": temp_val(&h["temperature_2m"][idx], temp_unit),
            "feels_like": temp_val(&h["apparent_temperature"][idx], temp_unit),
            "unit": temp_unit_label(temp_unit)
        },
        "humidity_percent": h["relative_humidity_2m"][idx],
        "uv_index": h["uv_index"][idx],
        "precipitation": {
            "value": h["precipitation"][idx],
            "probability_percent": h["precipitation_probability"][idx],
            "unit": "mm"
        },
        "cloud_cover_percent": h["cloud_cover"][idx],
        "wind": {
            "speed": { "value": wind_val(&h["wind_speed_10m"][idx], wind_unit), "unit": wind_unit_label(wind_unit) },
            "direction_degrees": h["wind_direction_10m"][idx],
            "gusts": { "value": wind_val(&h["wind_gusts_10m"][idx], wind_unit), "unit": wind_unit_label(wind_unit) }
        },
        "air_quality": air_quality
    }))
}

fn slice_daily(entry: &RawEntry, day_offset: u8, temp_unit: &str, wind_unit: &str) -> Result<Value, String> {
    let d = &entry.forecast["daily"];
    let idx = day_offset as usize;
    let target_date = d["time"][idx].as_str().unwrap_or("unknown");
    let weather_code = d["weather_code"][idx].as_u64().unwrap_or(0);
    let noon_ts = format!("{target_date}T12:00");
    let air_quality = find_time_idx(&entry.aq["hourly"]["time"], &noon_ts)
        .map(|ai| build_aq(&entry.aq["hourly"], ai))
        .unwrap_or(Value::Null);
    Ok(json!({
        "date": target_date,
        "period": "full",
        "weather_code": weather_code,
        "weather": wmo_description(weather_code),
        "temperature": {
            "max": temp_val(&d["temperature_2m_max"][idx], temp_unit),
            "min": temp_val(&d["temperature_2m_min"][idx], temp_unit),
            "feels_like_max": temp_val(&d["apparent_temperature_max"][idx], temp_unit),
            "feels_like_min": temp_val(&d["apparent_temperature_min"][idx], temp_unit),
            "unit": temp_unit_label(temp_unit)
        },
        "uv_index_max": d["uv_index_max"][idx],
        "precipitation": {
            "sum": d["precipitation_sum"][idx],
            "probability_max": d["precipitation_probability_max"][idx],
            "unit": "mm"
        },
        "wind": {
            "speed_max": { "value": wind_val(&d["wind_speed_10m_max"][idx], wind_unit), "unit": wind_unit_label(wind_unit) },
            "direction_dominant": d["wind_direction_10m_dominant"][idx],
            "gusts_max": { "value": wind_val(&d["wind_gusts_10m_max"][idx], wind_unit), "unit": wind_unit_label(wind_unit) }
        },
        "sunrise": d["sunrise"][idx],
        "sunset": d["sunset"][idx],
        "air_quality": air_quality
    }))
}

fn slice_weekly(entry: &RawEntry, start_offset: u8, temp_unit: &str, wind_unit: &str) -> Result<Value, String> {
    let d = &entry.forecast["daily"];
    let days: Vec<Value> = (0..7u8).filter_map(|i| {
        let idx = (start_offset + i) as usize;
        let target_date = d["time"][idx].as_str()?;
        let weather_code = d["weather_code"][idx].as_u64().unwrap_or(0);
        let noon_ts = format!("{target_date}T12:00");
        let air_quality = find_time_idx(&entry.aq["hourly"]["time"], &noon_ts)
            .map(|ai| build_aq(&entry.aq["hourly"], ai))
            .unwrap_or(Value::Null);
        Some(json!({
            "date": target_date,
            "weather_code": weather_code,
            "weather": wmo_description(weather_code),
            "temperature": {
                "max": temp_val(&d["temperature_2m_max"][idx], temp_unit),
                "min": temp_val(&d["temperature_2m_min"][idx], temp_unit),
                "feels_like_max": temp_val(&d["apparent_temperature_max"][idx], temp_unit),
                "feels_like_min": temp_val(&d["apparent_temperature_min"][idx], temp_unit),
                "unit": temp_unit_label(temp_unit)
            },
            "uv_index_max": d["uv_index_max"][idx],
            "precipitation": {
                "sum": d["precipitation_sum"][idx],
                "probability_max": d["precipitation_probability_max"][idx],
                "unit": "mm"
            },
            "wind": {
                "speed_max": { "value": wind_val(&d["wind_speed_10m_max"][idx], wind_unit), "unit": wind_unit_label(wind_unit) },
                "direction_dominant": d["wind_direction_10m_dominant"][idx],
                "gusts_max": { "value": wind_val(&d["wind_gusts_10m_max"][idx], wind_unit), "unit": wind_unit_label(wind_unit) }
            },
            "sunrise": d["sunrise"][idx],
            "sunset": d["sunset"][idx],
            "air_quality": air_quality
        }))
    }).collect();
    Ok(json!({ "period": "week", "start_offset": start_offset, "days": days }))
}

// ── Geocoding fallback (Open-Meteo API) ───────────────────────────────────────

/// Called when the offline GeoNames index can't find a location (e.g. sub-city
/// districts like "여의도" that aren't in cities1000). Falls back to Open-Meteo's
/// full geocoding API which indexes all GeoNames feature classes.
async fn geocode_fallback(client: &reqwest::Client, query: &str) -> Result<geo::GeoResult, String> {
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=ko&format=json",
        urlencoding::encode(query)
    );
    let resp: Value = client.get(&url)
        .timeout(GEOCODING_API_TIMEOUT)
        .send().await
        .map_err(|e| format!("geocoding request failed: {e}"))?
        .json().await
        .map_err(|e| format!("geocoding parse failed: {e}"))?;

    let r = resp["results"].as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| format!("not found: {query}"))?;

    // Build a descriptive name from admin fields (the raw `name` may be a park/POI)
    let admin3 = r["admin3"].as_str().unwrap_or("");
    let admin2 = r["admin2"].as_str().unwrap_or("");
    let admin1 = r["admin1"].as_str().unwrap_or("");
    let display = [admin3, admin2, admin1].iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");

    Ok(geo::GeoResult {
        name: if display.is_empty() { r["name"].as_str().unwrap_or(query).to_string() } else { display },
        admin1: admin1.to_string(),
        country_code: r["country_code"].as_str().unwrap_or("").to_string(),
        latitude: r["latitude"].as_f64().ok_or("no latitude")?,
        longitude: r["longitude"].as_f64().ok_or("no longitude")?,
        population: r["population"].as_u64().unwrap_or(0) as u32,
        timezone: r["timezone"].as_str().unwrap_or("UTC").to_string(),
    })
}

// ── Hourly rain timeline ───────────────────────────────────────────────────────

fn is_rainy_code(code: u64) -> bool {
    matches!(code, 51..=67 | 80..=82)
}

/// Returns today's hourly data with a condensed rain timeline.
/// Consecutive rainy hours are merged into spans.
fn slice_hourly_rain(entry: &RawEntry, day_offset: u8, temp_unit: &str) -> Result<Value, String> {
    let h = &entry.forecast["hourly"];
    let times = &h["time"];
    let base_date = times[0].as_str().and_then(|t| t.get(..10))
        .ok_or("Cannot parse base date")?;
    let target_date = add_days(base_date, day_offset);

    // Collect all hours for the target date
    let mut hours: Vec<Value> = Vec::new();
    let mut rain_spans: Vec<Value> = Vec::new();
    let mut span_start: Option<String> = None;
    let mut span_max_mm: f64 = 0.0;
    let mut total_mm: f64 = 0.0;
    let mut last_rain_hour: Option<String> = None;

    for idx in 0..times.as_array().map_or(0, |a| a.len()) {
        let ts = times[idx].as_str().unwrap_or("");
        if !ts.starts_with(&target_date) { continue; }

        let hour_label = ts.get(11..16).unwrap_or("").to_string();
        let precip_mm = h["precipitation"][idx].as_f64().unwrap_or(0.0);
        let prob = h["precipitation_probability"][idx].as_u64().unwrap_or(0);
        let code = h["weather_code"][idx].as_u64().unwrap_or(0);
        let temp_c = h["temperature_2m"][idx].as_f64().unwrap_or(0.0);
        let (t, _) = convert_temp(temp_c, temp_unit);

        let raining = is_rainy_code(code) || precip_mm > 0.1;
        total_mm += precip_mm;

        hours.push(json!({
            "time": hour_label,
            "weather_code": code,
            "weather": wmo_description(code),
            "precipitation_mm": (precip_mm * 10.0).round() / 10.0,
            "precipitation_probability": prob,
            "temperature": (t * 10.0).round() / 10.0,
        }));

        if raining {
            last_rain_hour = Some(hour_label.clone());
            span_max_mm = span_max_mm.max(precip_mm);
            if span_start.is_none() {
                span_start = Some(hour_label);
            }
        } else if let Some(start) = span_start.take() {
            let intensity = match span_max_mm {
                x if x >= 7.6 => "heavy",
                x if x >= 2.5 => "moderate",
                _ => "light",
            };
            rain_spans.push(json!({
                "start": start,
                "end": hour_label,
                "intensity": intensity,
                "max_mm_per_hour": (span_max_mm * 10.0).round() / 10.0,
            }));
            span_max_mm = 0.0;
        }
    }

    // Close any open span
    if let Some(start) = span_start {
        let intensity = match span_max_mm {
            x if x >= 7.6 => "heavy",
            x if x >= 2.5 => "moderate",
            _ => "light",
        };
        rain_spans.push(json!({
            "start": start,
            "end": "24:00",
            "intensity": intensity,
            "max_mm_per_hour": (span_max_mm * 10.0).round() / 10.0,
        }));
    }

    Ok(json!({
        "date": target_date,
        "period": "hourly",
        "rain_spans": rain_spans,
        "rain_ends_at": last_rain_hour,
        "total_precipitation_mm": (total_mm * 10.0).round() / 10.0,
        "hours": hours,
    }))
}

// ── Main handler ──────────────────────────────────────────────────────────────

async fn handle_get_weather(state: &WeatherState, args: &Value) -> Result<Value, String> {
    let temp_unit = args["temperature_unit"].as_str().unwrap_or("celsius");
    let wind_unit = args["wind_speed_unit"].as_str().unwrap_or("kmh");
    let day_offset = args["day_offset"].as_u64().unwrap_or(0).min(6) as u8;
    let period = args["period"].as_str()
        .unwrap_or(if day_offset == 0 { "now" } else { "full" });

    let loc = if let (Some(lat), Some(lng)) = (args["lat"].as_f64(), args["lng"].as_f64()) {
        if lat < -90.0 || lat > 90.0 || lng < -180.0 || lng > 180.0 {
            return Err("lat must be -90..90 and lng must be -180..180".into());
        }
        let nearest = geo::nearest(lat, lng);
        geo::GeoResult { latitude: lat, longitude: lng, ..nearest }
    } else {
        let city = args["city"].as_str().ok_or("Provide 'city' or 'lat'+'lng'")?;
        match geo::search(city) {
            Ok(r) => r,
            Err(_) => geocode_fallback(&state.http, city).await
                .map_err(|e| format!("Location not found: {city} ({e})"))?
        }
    };

    let lat_s = snap(loc.latitude);
    let lng_s = snap(loc.longitude);
    let cache_key = format!("weather:raw:{lat_s:.2}_{lng_s:.2}");
    debug!(lat = lat_s, lng = lng_s, cache_key, "weather lookup");

    state.req_counter.fetch_add(1, Ordering::Relaxed);
    let entry = get_raw(state, &cache_key, lat_s, lng_s).await?;

    let conditions = match period {
        "now" if day_offset == 0      => slice_current(&entry, temp_unit, wind_unit)?,
        "morning" | "afternoon" | "evening" | "night"
                                       => slice_hourly(&entry, day_offset, period, temp_unit, wind_unit)?,
        "hourly"                       => slice_hourly_rain(&entry, day_offset, temp_unit)?,
        "week"                         => slice_weekly(&entry, day_offset, temp_unit, wind_unit)?,
        _                              => slice_daily(&entry, day_offset, temp_unit, wind_unit)?,
    };

    Ok(json!({
        "location": {
            "name": loc.name,
            "admin1": loc.admin1,
            "country_code": loc.country_code,
            "latitude": lat_s,
            "longitude": lng_s,
        },
        "conditions": conditions
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── snap ──────────────────────────────────────────────────────────────────

    #[test]
    fn snap_rounds_to_0_01_grid() {
        assert!((snap(37.5678) - 37.57).abs() < 1e-10);
    }

    #[test]
    fn snap_already_on_grid() {
        assert!((snap(37.50) - 37.50).abs() < 1e-10);
    }

    #[test]
    fn snap_negative_coord() {
        assert!((snap(-122.4567) - -122.46).abs() < 1e-10);
    }

    // ── days_in_month ─────────────────────────────────────────────────────────

    #[test]
    fn days_in_month_31_day_months() {
        for m in [1u32, 3, 5, 7, 8, 10, 12] {
            assert_eq!(days_in_month(2026, m as i32), 31, "month {m}");
        }
    }

    #[test]
    fn days_in_month_30_day_months() {
        for m in [4u32, 6, 9, 11] {
            assert_eq!(days_in_month(2026, m as i32), 30, "month {m}");
        }
    }

    #[test]
    fn days_in_month_february_non_leap() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(1900, 2), 28); // divisible by 100, not 400
    }

    #[test]
    fn days_in_month_february_leap() {
        assert_eq!(days_in_month(2024, 2), 29); // divisible by 4
        assert_eq!(days_in_month(2000, 2), 29); // divisible by 400
    }

    // ── add_days ──────────────────────────────────────────────────────────────

    #[test]
    fn add_days_zero_returns_same() {
        assert_eq!(add_days("2026-04-07", 0), "2026-04-07");
    }

    #[test]
    fn add_days_within_month() {
        assert_eq!(add_days("2026-04-07", 3), "2026-04-10");
    }

    #[test]
    fn add_days_crosses_month_boundary() {
        assert_eq!(add_days("2026-04-30", 1), "2026-05-01");
    }

    #[test]
    fn add_days_crosses_year_boundary() {
        assert_eq!(add_days("2026-12-31", 1), "2027-01-01");
    }

    #[test]
    fn add_days_february_non_leap() {
        assert_eq!(add_days("2026-02-28", 1), "2026-03-01");
    }

    #[test]
    fn add_days_february_leap() {
        assert_eq!(add_days("2024-02-28", 1), "2024-02-29");
        assert_eq!(add_days("2024-02-29", 1), "2024-03-01");
    }

    // ── period_center_hour ────────────────────────────────────────────────────

    #[test]
    fn period_center_hour_known() {
        assert_eq!(period_center_hour("morning"), 9);
        assert_eq!(period_center_hour("afternoon"), 15);
        assert_eq!(period_center_hour("evening"), 20);
        assert_eq!(period_center_hour("night"), 23);
    }

    #[test]
    fn period_center_hour_unknown_defaults_noon() {
        assert_eq!(period_center_hour("midday"), 12);
        assert_eq!(period_center_hour(""), 12);
    }

    // ── convert_temp ──────────────────────────────────────────────────────────

    #[test]
    fn convert_temp_celsius_passthrough() {
        let (t, unit) = convert_temp(20.0, "celsius");
        assert_eq!(unit, "°C");
        assert!((t - 20.0).abs() < 1e-10);
    }

    #[test]
    fn convert_temp_0c_to_fahrenheit() {
        let (t, unit) = convert_temp(0.0, "fahrenheit");
        assert_eq!(unit, "°F");
        assert!((t - 32.0).abs() < 1e-10);
    }

    #[test]
    fn convert_temp_100c_to_fahrenheit() {
        let (t, _) = convert_temp(100.0, "fahrenheit");
        assert!((t - 212.0).abs() < 1e-10);
    }

    // ── convert_wind ──────────────────────────────────────────────────────────

    #[test]
    fn convert_wind_default_kmh() {
        let (w, unit) = convert_wind(100.0, "kmh");
        assert_eq!(unit, "km/h");
        assert!((w - 100.0).abs() < 1e-10);
    }

    #[test]
    fn convert_wind_to_ms() {
        let (w, unit) = convert_wind(36.0, "ms");
        assert_eq!(unit, "m/s");
        assert!((w - 10.0).abs() < 1e-6);
    }

    #[test]
    fn convert_wind_to_mph_and_kn_have_correct_units() {
        let (_, mph_unit) = convert_wind(100.0, "mph");
        let (_, kn_unit) = convert_wind(100.0, "kn");
        assert_eq!(mph_unit, "mph");
        assert_eq!(kn_unit, "kn");
    }

    // ── wmo_description ───────────────────────────────────────────────────────

    #[test]
    fn wmo_description_known_codes() {
        assert_eq!(wmo_description(0), "Clear sky");
        assert_eq!(wmo_description(3), "Overcast");
        assert_eq!(wmo_description(61), "Slight rain");
        assert_eq!(wmo_description(95), "Thunderstorm");
        assert_eq!(wmo_description(99), "Thunderstorm+heavy hail");
    }

    #[test]
    fn wmo_description_unknown_code() {
        assert_eq!(wmo_description(42), "Unknown");
        assert_eq!(wmo_description(999), "Unknown");
    }

    // ── eu_aqi_category ───────────────────────────────────────────────────────

    #[test]
    fn eu_aqi_category_boundaries() {
        assert_eq!(eu_aqi_category(0),   "Good");
        assert_eq!(eu_aqi_category(20),  "Good");
        assert_eq!(eu_aqi_category(21),  "Fair");
        assert_eq!(eu_aqi_category(40),  "Fair");
        assert_eq!(eu_aqi_category(41),  "Moderate");
        assert_eq!(eu_aqi_category(60),  "Moderate");
        assert_eq!(eu_aqi_category(61),  "Poor");
        assert_eq!(eu_aqi_category(80),  "Poor");
        assert_eq!(eu_aqi_category(81),  "Very Poor");
        assert_eq!(eu_aqi_category(100), "Very Poor");
        assert_eq!(eu_aqi_category(101), "Extremely Poor");
    }

    // ── us_aqi_category ───────────────────────────────────────────────────────

    #[test]
    fn us_aqi_category_boundaries() {
        assert_eq!(us_aqi_category(0),   "Good");
        assert_eq!(us_aqi_category(50),  "Good");
        assert_eq!(us_aqi_category(51),  "Moderate");
        assert_eq!(us_aqi_category(100), "Moderate");
        assert_eq!(us_aqi_category(101), "Unhealthy for Sensitive Groups");
        assert_eq!(us_aqi_category(150), "Unhealthy for Sensitive Groups");
        assert_eq!(us_aqi_category(151), "Unhealthy");
        assert_eq!(us_aqi_category(200), "Unhealthy");
        assert_eq!(us_aqi_category(201), "Very Unhealthy");
        assert_eq!(us_aqi_category(300), "Very Unhealthy");
        assert_eq!(us_aqi_category(301), "Hazardous");
    }

    // ── is_rainy_code ─────────────────────────────────────────────────────────

    #[test]
    fn is_rainy_code_drizzle_and_rain_ranges() {
        for code in [51u64, 55, 61, 63, 65, 66, 67, 80, 81, 82] {
            assert!(is_rainy_code(code), "code {code} should be rainy");
        }
    }

    #[test]
    fn is_rainy_code_non_rainy() {
        for code in [0u64, 3, 45, 71, 73, 75, 95, 99] {
            assert!(!is_rainy_code(code), "code {code} should not be rainy");
        }
    }

    // ── find_time_idx ─────────────────────────────────────────────────────────

    #[test]
    fn find_time_idx_found() {
        let times = serde_json::json!(["2026-04-07T08:00", "2026-04-07T09:00", "2026-04-07T10:00"]);
        assert_eq!(find_time_idx(&times, "2026-04-07T09:00"), Some(1));
    }

    #[test]
    fn find_time_idx_not_found() {
        let times = serde_json::json!(["2026-04-07T08:00"]);
        assert_eq!(find_time_idx(&times, "2026-04-07T12:00"), None);
    }

    #[test]
    fn find_time_idx_empty_array() {
        assert_eq!(find_time_idx(&serde_json::json!([]), "2026-04-07T09:00"), None);
    }

    // ── check_api_error ───────────────────────────────────────────────────────

    #[test]
    fn check_api_error_ok_response() {
        assert!(check_api_error(&serde_json::json!({"temperature": 25.0}), "forecast").is_ok());
    }

    #[test]
    fn check_api_error_with_reason() {
        let resp = serde_json::json!({"error": true, "reason": "Invalid location"});
        let err = check_api_error(&resp, "forecast").unwrap_err();
        assert!(err.contains("forecast"));
        assert!(err.contains("Invalid location"));
    }

    #[test]
    fn check_api_error_no_reason_uses_unknown() {
        let resp = serde_json::json!({"error": true});
        let err = check_api_error(&resp, "aq").unwrap_err();
        assert!(err.contains("unknown"));
    }
}
