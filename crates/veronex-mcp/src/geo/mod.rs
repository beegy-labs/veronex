//! Offline geocoding — city name → coordinates.
//!
//! Data is embedded at compile time from GeoNames cities1000 dataset.
//! No external API calls. Supports any language via GeoNames alternate names.
//!
//! # Usage
//!
//! ```rust
//! use veronex_mcp::geo::search;
//!
//! let result = search("서울 강남").unwrap();
//! println!("{}, {} → ({}, {})", result.name, result.admin1, result.latitude, result.longitude);
//! ```

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

// ── Data source ───────────────────────────────────────────────────────────────
//
// `geo.bin` (~10 MB postcard+zstd) is read from disk at first access, NOT
// embedded via `include_bytes!`. Reason: `include_bytes!` on a 10 MB blob
// inflates the .rodata section of every binary that links the
// veronex-mcp lib AND adds 100–200 MB to the compiler's RAM usage during
// monomorphization (rust-lang/rust#65818). With the runner pod limited
// to 4 GB RAM, this matters.
//
// Lookup order:
//   1. `GEO_DATA_PATH` env var (production: set by Dockerfile / Helm)
//   2. `GEO_DATA_BUILD_PATH` const (set by build.rs to OUT_DIR/geo.bin —
//      stable for `cargo run` / `cargo test` from the workspace root)

const GEO_DATA_BUILD_PATH: &str = env!("GEO_DATA_BUILD_PATH");

fn read_geo_bytes() -> Result<Vec<u8>, GeoError> {
    let path = std::env::var("GEO_DATA_PATH")
        .unwrap_or_else(|_| GEO_DATA_BUILD_PATH.to_string());
    std::fs::read(&path).map_err(|e| {
        GeoError::Init(format!("read {} ({}); set GEO_DATA_PATH to override", path, e))
    })
}

// ── Internal types ────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct City {
    name: String,
    ascii_name: String,
    latitude: f32,
    longitude: f32,
    country_code: String,
    admin1_name: String,
    population: u32,
    timezone: String,
}

#[derive(Serialize, Deserialize)]
struct GeoData {
    cities: Vec<City>,
    index: Vec<(String, Vec<u32>)>,
}

// ── Public types ──────────────────────────────────────────────────────────────

/// A resolved geographic location.
#[derive(Debug, Clone)]
pub struct GeoResult {
    /// Canonical city name (English).
    pub name: String,
    /// State / province name.
    pub admin1: String,
    /// ISO 3166-1 alpha-2 country code (e.g. "KR").
    pub country_code: String,
    /// WGS84 latitude.
    pub latitude: f64,
    /// WGS84 longitude.
    pub longitude: f64,
    /// City population.
    pub population: u32,
    /// IANA timezone (e.g. "Asia/Seoul").
    pub timezone: String,
}

#[derive(Debug, thiserror::Error)]
pub enum GeoError {
    #[error("City not found: {0}")]
    NotFound(String),
    #[error("Index initialization failed: {0}")]
    Init(String),
}

// ── Index ─────────────────────────────────────────────────────────────────────

struct GeoIndex {
    cities: Vec<City>,
    index: HashMap<String, Vec<u32>>,
}

static INDEX: OnceLock<GeoIndex> = OnceLock::new();

fn get_index() -> &'static GeoIndex {
    INDEX.get_or_init(|| {
        let bytes = read_geo_bytes()
            .unwrap_or_else(|e| panic!("veronex-geo: {e}"));
        let decompressed = zstd::decode_all(bytes.as_slice())
            .expect("veronex-geo: decompression failed");
        let data: GeoData =
            postcard::from_bytes(&decompressed).expect("veronex-geo: deserialization failed");

        let index: HashMap<String, Vec<u32>> = data.index.into_iter().collect();
        GeoIndex { cities: data.cities, index }
    })
}

// ── Normalization ─────────────────────────────────────────────────────────────

fn normalize(s: &str) -> String {
    s.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Search ────────────────────────────────────────────────────────────────────

fn lookup<'a>(idx: &'a GeoIndex, key: &str) -> Option<Vec<&'a City>> {
    idx.index.get(key).map(|indices| {
        indices.iter().map(|&i| &idx.cities[i as usize]).collect()
    })
}

fn to_result(city: &City) -> GeoResult {
    GeoResult {
        name: city.name.clone(),
        admin1: city.admin1_name.clone(),
        country_code: city.country_code.clone(),
        latitude: city.latitude as f64,
        longitude: city.longitude as f64,
        population: city.population,
        timezone: city.timezone.clone(),
    }
}

/// Search for a city by name. Returns the best match or `GeoError::NotFound`.
///
/// Input language is irrelevant — any GeoNames alternate name works.
///
/// Search strategy (in order):
/// 1. Exact full-string match
/// 2. Right-to-left token subsets (most specific token first)
/// 3. Left-to-right token subsets
pub fn search(query: &str) -> Result<GeoResult, GeoError> {
    search_many(query, 1)
        .into_iter()
        .next()
        .ok_or_else(|| GeoError::NotFound(query.to_string()))
}

/// Search for a city by name. Returns up to `count` matches sorted by population.
pub fn search_many(query: &str, count: usize) -> Vec<GeoResult> {
    let idx = get_index();
    let q = normalize(query);

    if q.is_empty() {
        return vec![];
    }

    // Strategy 1: exact full match
    if let Some(hits) = lookup(idx, &q) {
        return hits.into_iter().take(count).map(to_result).collect();
    }

    let tokens: Vec<&str> = q.split_whitespace().collect();
    if tokens.len() > 1 {
        // Strategy 2: right-to-left subsets ("서울 강남" → try "강남" first)
        for start in (0..tokens.len()).rev() {
            let sub = tokens[start..].join(" ");
            if sub == q {
                continue;
            }
            if let Some(hits) = lookup(idx, &sub) {
                return hits.into_iter().take(count).map(to_result).collect();
            }
        }

        // Strategy 3: left-to-right subsets
        for end in (1..tokens.len()).rev() {
            let sub = tokens[..end].join(" ");
            if let Some(hits) = lookup(idx, &sub) {
                return hits.into_iter().take(count).map(to_result).collect();
            }
        }
    }

    vec![]
}

/// Find the nearest city to the given WGS84 coordinates.
///
/// Uses squared Euclidean distance on lat/lng — sufficient for nearest-city
/// lookup (error negligible within a few hundred km, and we only need the name).
/// O(n) over ~167 K cities; completes in < 2 ms on typical hardware.
pub fn nearest(lat: f64, lng: f64) -> GeoResult {
    let idx = get_index();
    let best = idx.cities.iter().min_by(|a, b| {
        let da = (a.latitude as f64 - lat).powi(2) + (a.longitude as f64 - lng).powi(2);
        let db = (b.latitude as f64 - lat).powi(2) + (b.longitude as f64 - lng).powi(2);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });
    // Fallback to empty result is unreachable in practice (dataset always non-empty).
    best.map(to_result).unwrap_or_else(|| GeoResult {
        name: "Unknown".into(),
        admin1: String::new(),
        country_code: String::new(),
        latitude: lat,
        longitude: lng,
        population: 0,
        timezone: "UTC".into(),
    })
}

/// Preload the index into memory. Optional — index loads lazily on first search.
/// Call at service startup to avoid first-request latency.
pub fn preload() {
    let _ = get_index();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_english() {
        let r = search("Seoul").unwrap();
        assert_eq!(r.country_code, "KR");
    }

    #[test]
    fn test_korean() {
        let r = search("서울").unwrap();
        assert_eq!(r.country_code, "KR");
    }

    #[test]
    fn test_district() {
        let r = search("서울 강남").unwrap();
        assert!(r.latitude > 37.0 && r.latitude < 38.0);
    }

    #[test]
    fn test_japanese() {
        let r = search("東京").unwrap();
        assert_eq!(r.country_code, "JP");
    }

    #[test]
    fn test_not_found() {
        assert!(search("xyzzy_nonexistent_city_12345").is_err());
    }

    #[test]
    fn normalize_lowercases_ascii() {
        assert_eq!(normalize("Seoul"), "seoul");
        assert_eq!(normalize("NEW YORK"), "new york");
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize("  new  york  "), "new york");
        assert_eq!(normalize("los  angeles"), "los angeles");
    }

    #[test]
    fn normalize_strips_diacritics() {
        // é → e, ü → u
        assert_eq!(normalize("München"), "munchen");
        assert_eq!(normalize("Séoul"), "seoul");
    }

    #[test]
    fn normalize_empty_stays_empty() {
        assert_eq!(normalize(""), "");
        assert_eq!(normalize("   "), "");
    }
}
