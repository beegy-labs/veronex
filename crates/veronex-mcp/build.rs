//! Build script — downloads GeoNames data once, processes into a compact binary index,
//! and writes it to OUT_DIR/geo.bin for include_bytes! embedding.
//!
//! Downloads:
//!   - cities1000.zip  (~9.7 MB) — cities with population ≥ 1000
//!   - admin1CodesASCII.txt (~150 KB) — state/province name lookup
//!
//! Data is cached in `data/` (gitignored). Re-download only if files are missing.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

// ── Serializable types (shared with lib.rs via OUT_DIR) ───────────────────────

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

/// Serialized index: cities vec + name → indices mapping (sorted by population desc).
#[derive(Serialize, Deserialize)]
struct GeoData {
    cities: Vec<City>,
    /// Sorted Vec for deterministic serialization; rebuilt as HashMap at runtime.
    index: Vec<(String, Vec<u32>)>,
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

// ── Download helpers ──────────────────────────────────────────────────────────

fn download_file(url: &str, dest: &Path) {
    println!("cargo:warning=Downloading {url}");
    let resp = ureq::get(url)
        .call()
        .unwrap_or_else(|e| panic!("Failed to download {url}: {e}"));
    let mut buf = Vec::new();
    resp.into_reader().read_to_end(&mut buf).unwrap();
    std::fs::write(dest, &buf).unwrap();
}

fn download_and_extract_zip(url: &str, file_name: &str, dest: &Path) {
    let zip_path = dest.parent().unwrap().join(format!("{file_name}.zip"));
    download_file(url, &zip_path);

    let zip_data = std::fs::read(&zip_path).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_data)).unwrap();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        if entry.name() == file_name {
            let mut out = std::fs::File::create(dest).unwrap();
            std::io::copy(&mut entry, &mut out).unwrap();
            break;
        }
    }

    std::fs::remove_file(&zip_path).ok();
}

// ── Parsing ───────────────────────────────────────────────────────────────────

/// Returns: HashMap<"country_code.admin1_code" → admin1_name>
fn parse_admin1(path: &Path) -> HashMap<String, String> {
    let file = std::fs::File::open(path).unwrap();
    let mut map = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line = line.unwrap();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.splitn(4, '\t').collect();
        if cols.len() < 2 {
            continue;
        }
        // cols[0] = "KR.11", cols[1] = "Seoul"
        map.insert(cols[0].to_string(), cols[1].to_string());
    }
    map
}

/// Parses cities1000.txt (tab-separated, 19 columns).
/// Returns (cities, name_index).
fn parse_cities(
    path: &Path,
    admin1_map: &HashMap<String, String>,
) -> (Vec<City>, HashMap<String, Vec<u32>>) {
    let file = std::fs::File::open(path).unwrap();
    let mut cities: Vec<City> = Vec::with_capacity(130_000);
    let mut index: HashMap<String, Vec<u32>> = HashMap::new();

    for line in BufReader::new(file).lines() {
        let line = line.unwrap();
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 18 {
            continue;
        }

        let name = cols[1].to_string();
        let ascii_name = cols[2].to_string();
        let alternate_names = cols[3]; // comma-separated
        let lat: f32 = cols[4].parse().unwrap_or(0.0);
        let lng: f32 = cols[5].parse().unwrap_or(0.0);
        let country_code = cols[8].to_string();
        let admin1_code = cols[10];
        let population: u32 = cols[14].parse().unwrap_or(0);
        let timezone = cols[17].to_string();

        let admin1_key = format!("{country_code}.{admin1_code}");
        let admin1_name = admin1_map.get(&admin1_key).cloned().unwrap_or_default();

        let idx = cities.len() as u32;
        let city = City { name, ascii_name, latitude: lat, longitude: lng, country_code, admin1_name, population, timezone };

        // ── Index all name variants ──────────────────────────────────────────
        let mut add = |key: &str| {
            let k = normalize(key);
            if !k.is_empty() {
                index.entry(k).or_default().push(idx);
            }
        };

        add(&city.name);
        add(&city.ascii_name);

        for alt in alternate_names.split(',') {
            add(alt.trim());
        }

        // Admin-qualified variants: "admin1 name" and "admin1 ascii_name"
        if !city.admin1_name.is_empty() {
            let q1 = format!("{} {}", city.admin1_name, city.name);
            let q2 = format!("{} {}", city.admin1_name, city.ascii_name);
            add(&q1);
            add(&q2);
        }

        cities.push(city);
    }

    // Sort each bucket by population descending
    for indices in index.values_mut() {
        indices.sort_unstable_by(|&a, &b| {
            cities[b as usize].population.cmp(&cities[a as usize].population)
        });
        indices.dedup();
    }

    println!("cargo:warning=veronex-geo: {} cities, {} index entries", cities.len(), index.len());

    (cities, index)
}

// ── Main ──────────────────────────────────────────────────────────────────────

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let data_dir = PathBuf::from(&manifest_dir).join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_path = PathBuf::from(&out_dir).join("geo.bin");

    println!("cargo:rerun-if-changed=data/cities1000.txt");
    println!("cargo:rerun-if-changed=data/admin1CodesASCII.txt");

    // ── Download (once) ──────────────────────────────────────────────────────
    let cities_path = data_dir.join("cities1000.txt");
    if !cities_path.exists() {
        download_and_extract_zip(
            "https://download.geonames.org/export/dump/cities1000.zip",
            "cities1000.txt",
            &cities_path,
        );
    }

    let admin1_path = data_dir.join("admin1CodesASCII.txt");
    if !admin1_path.exists() {
        download_file(
            "https://download.geonames.org/export/dump/admin1CodesASCII.txt",
            &admin1_path,
        );
    }

    // ── Parse ────────────────────────────────────────────────────────────────
    let admin1_map = parse_admin1(&admin1_path);
    let (cities, index) = parse_cities(&cities_path, &admin1_map);

    // ── Serialize + compress ─────────────────────────────────────────────────
    let mut index_vec: Vec<(String, Vec<u32>)> = index.into_iter().collect();
    index_vec.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    let geo_data = GeoData { cities, index: index_vec };
    let mut encoded = Vec::new();
    postcard::to_io(&geo_data, &mut encoded).expect("postcard serialization failed");
    let compressed = zstd::encode_all(encoded.as_slice(), 9).expect("zstd compression failed");

    println!("cargo:warning=veronex-geo: geo.bin = {} KB (compressed)", compressed.len() / 1024);

    std::fs::write(&out_path, compressed).unwrap();

    // Tier 3 fix (.specs/veronex/ci-build-optimization.md): expose the
    // OUT_DIR path as a compile-time const so the runtime reads geo.bin
    // from disk instead of `include_bytes!`-embedding 10 MB into every
    // binary that links the veronex-mcp lib (rust-lang/rust#65818).
    // Production overrides this with the `GEO_DATA_PATH` env var.
    println!("cargo:rustc-env=GEO_DATA_BUILD_PATH={}", out_path.display());
}
