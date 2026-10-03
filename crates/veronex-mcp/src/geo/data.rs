use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

// Binary schema and name normalization shared by the index writer and reader.

#[derive(Serialize, Deserialize)]
pub(super) struct City {
    pub(super) name: String,
    pub(super) ascii_name: String,
    pub(super) latitude: f32,
    pub(super) longitude: f32,
    pub(super) country_code: String,
    pub(super) admin1_name: String,
    pub(super) population: u32,
    pub(super) timezone: String,
}

/// Serialized index: cities vec + name → indices mapping (sorted by population desc).
#[derive(Serialize, Deserialize)]
pub(super) struct GeoData {
    pub(super) cities: Vec<City>,
    /// Sorted Vec for deterministic serialization; rebuilt as HashMap at runtime.
    pub(super) index: Vec<(String, Vec<u32>)>,
}

// ── Normalization ─────────────────────────────────────────────────────────────

pub(super) fn normalize(s: &str) -> String {
    s.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

