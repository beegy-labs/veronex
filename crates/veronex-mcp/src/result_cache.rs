//! `McpResultCache` — Valkey-backed tool result cache.
//!
//! Only caches results for tools where `readOnlyHint AND idempotentHint` are
//! both `true`. Cache key is `SHA-256(tool_name + sorted canonical JSON args)`.

use std::sync::Arc;

use anyhow::Result;
use fred::prelude::*;
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::types::{McpContent, McpTool, McpToolResult};

// ── Key ───────────────────────────────────────────────────────────────────────

/// Max recursion depth for canonical_json. Prevents stack overflow from malicious servers.
const MAX_CANONICAL_DEPTH: u8 = 16;

/// Canonical args hash: SHA-256 of `sort_keys(JSON(args))`, hex-encoded, first 16 chars.
fn args_hash(tool_name: &str, args: &serde_json::Value) -> String {
    let canonical = canonical_json(args, 0);
    let mut hasher = Sha256::new();
    hasher.update(tool_name.as_bytes());
    hasher.update(b":");
    hasher.update(canonical.as_bytes());
    let hash = hasher.finalize();
    hex::encode(&hash[..8]) // 16 hex chars = 64-bit uniqueness
}

/// Recursively sort object keys so `{"b":2,"a":1}` == `{"a":1,"b":2}`.
/// Returns `"..."` when depth exceeds MAX_CANONICAL_DEPTH to prevent stack overflow.
fn canonical_json(v: &serde_json::Value, depth: u8) -> String {
    if depth >= MAX_CANONICAL_DEPTH {
        return "\"...\"".to_owned();
    }
    match v {
        serde_json::Value::Object(map) => {
            let mut pairs: Vec<_> = map.iter().collect();
            pairs.sort_by_key(|(k, _)| *k);
            let inner: Vec<String> = pairs
                .into_iter()
                .map(|(k, v)| format!("{:?}:{}", k, canonical_json(v, depth + 1)))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(|v| canonical_json(v, depth + 1)).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
    }
}

fn cache_key(tool_name: &str, args: &serde_json::Value) -> String {
    format!("veronex:mcp:result:{}:{}", tool_name, args_hash(tool_name, args))
}

// ── Cache ─────────────────────────────────────────────────────────────────────

pub struct McpResultCache {
    valkey: Arc<Pool>,
}

impl McpResultCache {
    pub fn new(valkey: Arc<Pool>) -> Self {
        Self { valkey }
    }

    /// Try to get a cached result. Returns `None` on miss or if tool is not cacheable.
    pub async fn get(
        &self,
        tool: &McpTool,
        args: &serde_json::Value,
    ) -> Option<McpToolResult> {
        if !tool.can_cache() {
            return None;
        }

        let key = cache_key(&tool.name, args);
        let conn: fred::clients::Client = self.valkey.next().clone();
        let raw: String = conn.get::<Option<String>, _>(&key).await.ok().flatten()?;

        let content: Vec<McpContent> = serde_json::from_str(&raw).ok()?;
        debug!(key = %key, "McpResultCache: hit");
        Some(McpToolResult::cached(content))
    }

    /// Store a successful result if the tool is cacheable.
    pub async fn set(
        &self,
        tool: &McpTool,
        args: &serde_json::Value,
        result: &McpToolResult,
        ttl_secs: i64,
    ) {
        if !tool.can_cache() || result.is_error || ttl_secs <= 0 {
            return;
        }

        let key = cache_key(&tool.name, args);
        let Ok(json) = serde_json::to_string(&result.content) else {
            return;
        };

        let conn: fred::clients::Client = self.valkey.next().clone();
        let _: Result<(), _> = conn
            .set(
                &key,
                json,
                Some(Expiration::EX(ttl_secs)),
                None,
                false,
            )
            .await;

        debug!(key = %key, ttl = ttl_secs, "McpResultCache: stored");
    }

    /// Compute the args hash for analytics (so the hash is consistent).
    pub fn compute_hash(tool_name: &str, args: &serde_json::Value) -> String {
        args_hash(tool_name, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard against silent key renames that would break cross-crate Valkey access.
    #[test]
    fn cache_key_format() {
        let args = serde_json::json!({"lat": 37.5});
        let key = cache_key("get_weather", &args);
        assert!(key.starts_with("veronex:mcp:result:"), "unexpected prefix: {key}");
        assert!(key.contains("get_weather"), "tool name not embedded: {key}");
        // hash segment is 16 hex chars
        let hash_part = key.split(':').last().unwrap_or("");
        assert_eq!(hash_part.len(), 16, "hash segment wrong length: {key}");
        assert!(hash_part.chars().all(|c| c.is_ascii_hexdigit()), "hash not hex: {key}");
    }

    /// Same args in different order must produce the same cache key.
    #[test]
    fn cache_key_order_independent() {
        let a = serde_json::json!({"b": 2, "a": 1});
        let b = serde_json::json!({"a": 1, "b": 2});
        assert_eq!(cache_key("tool", &a), cache_key("tool", &b));
    }

    /// Different tool names must produce different cache keys even for identical args.
    #[test]
    fn cache_key_tool_name_scoped() {
        let args = serde_json::json!({});
        assert_ne!(cache_key("tool_a", &args), cache_key("tool_b", &args));
    }

    // ── canonical_json — primitive types ─────────────────────────────────────
    // (canonical_json_sorts_keys and args_hash_stable removed: covered by
    // cache_key_order_independent, canonical_json_nested_objects_sorted,
    // and args_hash_always_16_hex_chars)

    #[test]
    fn canonical_json_primitives() {
        assert_eq!(canonical_json(&serde_json::json!(null), 0), "null");
        assert_eq!(canonical_json(&serde_json::json!(true), 0), "true");
        assert_eq!(canonical_json(&serde_json::json!(false), 0), "false");
        assert_eq!(canonical_json(&serde_json::json!(42), 0), "42");
        assert_eq!(canonical_json(&serde_json::json!("hello"), 0), "\"hello\"");
    }

    #[test]
    fn canonical_json_array_preserves_order() {
        // Arrays are order-sensitive (not sorted)
        let a = canonical_json(&serde_json::json!([1, 2, 3]), 0);
        let b = canonical_json(&serde_json::json!([3, 2, 1]), 0);
        assert_ne!(a, b);
        assert_eq!(a, "[1,2,3]");
    }

    #[test]
    fn canonical_json_nested_objects_sorted() {
        let v = serde_json::json!({"z": {"b": 2, "a": 1}, "a": 0});
        let c = canonical_json(&v, 0);
        // outer: a before z; inner: a before b
        assert!(c.starts_with("{\"a\":0,\"z\":"));
        assert!(c.contains("\"a\":1,\"b\":2"));
    }

    #[test]
    fn canonical_json_depth_cutoff() {
        // Build an object nested exactly at MAX_CANONICAL_DEPTH — must produce "..."
        let mut v = serde_json::json!("leaf");
        for _ in 0..MAX_CANONICAL_DEPTH {
            v = serde_json::json!({ "k": v });
        }
        let result = canonical_json(&v, 0);
        assert!(result.contains("\"...\""), "expected depth cutoff sentinel: {result}");
    }

    // ── args_hash — always 16 hex chars ──────────────────────────────────────

    #[test]
    fn args_hash_always_16_hex_chars() {
        for args in [
            serde_json::json!({}),
            serde_json::json!(null),
            serde_json::json!({"a": "very long string ".repeat(1000)}),
        ] {
            let h = args_hash("tool", &args);
            assert_eq!(h.len(), 16, "hash wrong length for args: {args}");
            assert!(h.chars().all(|c| c.is_ascii_hexdigit()), "not hex: {h}");
        }
    }

    // ── cache_key — full structure ────────────────────────────────────────────

    #[test]
    fn cache_key_structure() {
        let key = cache_key("my_tool", &serde_json::json!({"x": 1}));
        let parts: Vec<&str> = key.splitn(5, ':').collect();
        // "veronex:mcp:result:{tool_name}:{hash}"
        assert_eq!(parts[0], "veronex");
        assert_eq!(parts[1], "mcp");
        assert_eq!(parts[2], "result");
        assert_eq!(parts[3], "my_tool");
        assert_eq!(parts[4].len(), 16);
    }
}
