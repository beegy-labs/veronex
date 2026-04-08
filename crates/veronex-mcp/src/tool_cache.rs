//! `McpToolCache` — two-level tool schema cache.
//!
//! L1: `DashMap` (in-process, O(1) read).
//! L2: Valkey (shared across replicas, TTL 35 s).
//!
//! Only a single replica refreshes the Valkey entry per window (SET NX lock).
//! Other replicas populate their DashMap from Valkey on L1 miss.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use dashmap::DashMap;
use fred::prelude::*;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::session::McpSessionManager;
use crate::types::McpTool;

// ── Valkey key helpers ────────────────────────────────────────────────────────

fn tool_key(server_id: Uuid) -> String {
    format!("veronex:mcp:tools:{server_id}")
}

fn lock_key(server_id: Uuid) -> String {
    format!("veronex:mcp:tools:lock:{server_id}")
}

fn heartbeat_key(server_id: Uuid) -> String {
    format!("veronex:mcp:heartbeat:{server_id}")
}

#[cfg(test)]
mod key_format_tests {
    use super::*;

    /// Guard against silent key renames that would break cross-crate Valkey access.
    #[test]
    fn tool_key_format() {
        let id = Uuid::nil();
        let key = tool_key(id);
        assert!(key.starts_with("veronex:mcp:tools:"), "unexpected prefix: {key}");
        assert!(key.contains(&id.to_string()), "uuid not embedded: {key}");
    }

    #[test]
    fn lock_key_format() {
        let id = Uuid::nil();
        let key = lock_key(id);
        assert!(key.starts_with("veronex:mcp:tools:lock:"), "unexpected prefix: {key}");
        assert!(key.contains(&id.to_string()), "uuid not embedded: {key}");
    }

    #[test]
    fn heartbeat_key_format() {
        let id = Uuid::nil();
        let key = heartbeat_key(id);
        assert!(key.starts_with("veronex:mcp:heartbeat:"), "unexpected prefix: {key}");
        assert!(key.contains(&id.to_string()), "uuid not embedded: {key}");
    }

    /// All three keys for the same server must be distinct (no collision).
    #[test]
    fn tool_lock_heartbeat_keys_are_distinct() {
        let id = Uuid::nil();
        let keys = [tool_key(id), lock_key(id), heartbeat_key(id)];
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), 3, "key collision detected");
    }
}

// ── Cache entry ───────────────────────────────────────────────────────────────

#[derive(Clone)]
struct CachedTools {
    tools: Vec<McpTool>,
    fetched_at: Instant,
}

// ── Tool cache ────────────────────────────────────────────────────────────────

/// TTL for the in-process DashMap entries.
const L1_TTL: Duration = Duration::from_secs(30);
/// TTL for the Valkey entries.
const L2_TTL_SECS: i64 = 35;
/// Lock TTL — slightly longer than refresh interval to avoid races.
const LOCK_TTL_SECS: i64 = 33;

pub struct McpToolCache {
    l1: DashMap<Uuid, CachedTools>,
    /// Shared reverse map across all servers.
    name_to_server: Arc<DashMap<String, Uuid>>,
    /// Slug (server_name) per server_id — survives Valkey round-trips where
    /// `McpTool.server_name` is not serialized (`#[serde(skip)]`).
    server_slugs: DashMap<Uuid, String>,
    valkey: Arc<Pool>,
    /// Max tools returned in `get_all` (context window protection).
    max_tools: usize,
}

impl McpToolCache {
    pub fn new(valkey: Arc<Pool>, max_tools: usize) -> Self {
        Self {
            l1: DashMap::new(),
            name_to_server: Arc::new(DashMap::new()),
            server_slugs: DashMap::new(),
            valkey,
            max_tools,
        }
    }

    // ── Read ──────────────────────────────────────────────────────────────────

    /// Return all cached tools (across servers) for LLM injection.
    ///
    /// `allowed`: if `Some`, only tools whose server_id is in the set are returned.
    /// `None` = no restriction (default allow all servers).
    pub async fn get_all(&self, allowed: Option<&std::collections::HashSet<Uuid>>) -> Vec<serde_json::Value> {
        // Snapshot the L1 map synchronously — no awaits while holding DashMap Refs.
        // Holding a Ref across .await locks the shard, blocking all concurrent writes.
        let snapshot: Vec<(Uuid, CachedTools)> = self
            .l1
            .iter()
            .map(|e| (*e.key(), e.value().clone()))
            .collect();

        if snapshot.is_empty() {
            return Vec::new();
        }

        // Batch liveness check — one MGET instead of N sequential EXISTS round-trips.
        // At 10K MCP servers, N×RTT on a hot path is unacceptable.
        let conn: fred::clients::Client = self.valkey.next().clone();
        let hb_keys: Vec<String> = snapshot.iter().map(|(id, _)| heartbeat_key(*id)).collect();
        let liveness: Vec<Option<String>> = conn.mget(hb_keys).await.unwrap_or_default();
        let online: std::collections::HashSet<Uuid> = snapshot
            .iter()
            .zip(liveness.into_iter())
            .filter_map(|((id, _), v)| if v.is_some() { Some(*id) } else { None })
            .collect();

        let mut tools = Vec::new();

        for (server_id, cached) in snapshot {
            if !online.contains(&server_id) {
                continue;
            }
            // ACL filter — skip servers the caller is not allowed to use
            if let Some(ids) = allowed {
                if !ids.contains(&server_id) {
                    continue;
                }
            }

            // Refresh stale L1 entry from Valkey (no DashMap ref held at this point)
            let cached = if cached.fetched_at.elapsed() > L1_TTL {
                match self.load_from_valkey(server_id).await {
                    Some(c) => c,
                    None => continue,
                }
            } else {
                cached
            };

            for tool in &cached.tools {
                if tools.len() >= self.max_tools {
                    break;
                }
                tools.push(tool.to_openai_function());
            }

            if tools.len() >= self.max_tools {
                break;
            }
        }

        tools
    }

    /// Resolve a namespaced tool name to its server_id.
    pub fn server_id_of(&self, namespaced_name: &str) -> Option<Uuid> {
        self.name_to_server.get(namespaced_name).as_deref().copied()
    }

    /// Returns all currently known namespaced tool names (from the reverse map).
    /// Fast, sync — does NOT check liveness.
    pub fn all_namespaced_names(&self) -> Vec<String> {
        self.name_to_server.iter().map(|e| e.key().clone()).collect()
    }

    /// Retrieve a single tool definition by namespaced name (L1 only, sync).
    /// Returns `None` on cache miss or if the server entry is stale.
    pub fn get_tool_raw(&self, namespaced_name: &str) -> Option<McpTool> {
        let server_id = self.server_id_of(namespaced_name)?;
        let entry = self.l1.get(&server_id)?;
        entry
            .tools
            .iter()
            .find(|t| t.namespaced_name() == namespaced_name)
            .cloned()
    }

    /// Invalidate a server's L1 entry (called on `notifications/tools/list_changed`).
    pub fn invalidate(&self, server_id: Uuid) {
        if let Some((_, entry)) = self.l1.remove(&server_id) {
            for tool in &entry.tools {
                self.name_to_server.remove(&tool.namespaced_name());
            }
        }
        debug!(server_id = %server_id, "McpToolCache: L1 invalidated (list_changed)");
    }

    /// Remove a server entirely (called when server goes offline).
    pub fn remove_server(&self, server_id: Uuid) {
        if let Some((_, entry)) = self.l1.remove(&server_id) {
            for tool in &entry.tools {
                self.name_to_server.remove(&tool.namespaced_name());
            }
        }
        self.server_slugs.remove(&server_id);
    }

    // ── Direct cache warm ────────────────────────────────────────────────────

    /// Store a pre-fetched tool list directly into L1 and L2 without acquiring
    /// the refresh lock or making an HTTP call to the MCP server.
    ///
    /// Use this after you have already fetched tools (e.g. for DB persistence)
    /// to warm the cache without a redundant HTTP round-trip.
    pub async fn cache_fetched_tools(&self, server_id: Uuid, tools: Vec<McpTool>) {
        if tools.is_empty() {
            return;
        }
        if let Some(first) = tools.first() {
            self.server_slugs.insert(server_id, first.server_name.clone());
        }
        for tool in &tools {
            self.name_to_server.insert(tool.namespaced_name(), server_id);
        }
        let conn: fred::clients::Client = self.valkey.next().clone();
        if let Ok(json) = serde_json::to_string(&tools) {
            let _: std::result::Result<(), _> = conn
                .set(
                    tool_key(server_id),
                    json,
                    Some(Expiration::EX(L2_TTL_SECS)),
                    None,
                    false,
                )
                .await;
        }
        self.l1.insert(server_id, CachedTools { tools, fetched_at: Instant::now() });
        debug!(server_id = %server_id, "McpToolCache: warmed from pre-fetched tools");
    }

    // ── Refresh ───────────────────────────────────────────────────────────────

    /// Refresh tools for one server. Uses SET NX so only one replica fetches.
    ///
    /// Returns `Some(tools)` when this replica fetched fresh data from the MCP server,
    /// so the caller can trigger Vespa re-indexing. Returns `None` when another replica
    /// held the lock (data loaded from Valkey only).
    pub async fn refresh(
        &self,
        server_id: Uuid,
        session_mgr: &McpSessionManager,
    ) -> Option<Vec<McpTool>> {
        // Try to acquire the refresh lock
        let conn: fred::clients::Client = self.valkey.next().clone();
        let lock = lock_key(server_id);
        let acquired: Option<String> = conn
            .set(
                &lock,
                "1",
                Some(Expiration::EX(LOCK_TTL_SECS)),
                Some(SetOptions::NX),
                false,
            )
            .await
            .unwrap_or(None);

        if acquired.is_none() {
            // Another replica holds the lock — just refresh from Valkey
            self.load_from_valkey(server_id).await;
            return None;
        }

        // Fetch from MCP server — reuse the shared client from with_session (connection pool).
        let fetch_result = session_mgr
            .with_session(server_id, |client, session| {
                async move { client.list_tools(&session).await }
            })
            .await;

        match fetch_result {
            Ok(tools) => {
                // Cache server slug (server_name) so load_from_valkey can restore it.
                // McpTool.server_name is #[serde(skip)], so it's absent from Valkey JSON.
                if let Some(first) = tools.first() {
                    self.server_slugs.insert(server_id, first.server_name.clone());
                }

                // Update reverse map
                for tool in &tools {
                    self.name_to_server
                        .insert(tool.namespaced_name(), server_id);
                }

                // Serialize to Valkey
                if let Ok(json) = serde_json::to_string(&tools) {
                    let _: Result<(), _> = conn
                        .set(
                            tool_key(server_id),
                            json,
                            Some(Expiration::EX(L2_TTL_SECS)),
                            None,
                            false,
                        )
                        .await;
                }

                // Update L1
                self.l1.insert(
                    server_id,
                    CachedTools { tools: tools.clone(), fetched_at: Instant::now() },
                );

                Some(tools)
            }
            Err(e) => {
                warn!(server_id = %server_id, error = %e, "McpToolCache: refresh failed");
                None
            }
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    async fn load_from_valkey(&self, server_id: Uuid) -> Option<CachedTools> {
        let conn: fred::clients::Client = self.valkey.next().clone();
        let raw: Option<String> = conn.get(tool_key(server_id)).await.unwrap_or(None);

        let mut tools: Vec<McpTool> = raw
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        if tools.is_empty() {
            return None;
        }

        // Restore server_name from the in-process slug cache.
        // McpTool.server_name is #[serde(skip)] so it's absent from the Valkey JSON.
        let slug = self.server_slugs.get(&server_id).as_deref().cloned().unwrap_or_default();
        for tool in &mut tools {
            tool.server_id = server_id;
            tool.server_name = slug.clone();
        }

        // Rebuild reverse map from Valkey data
        for tool in &tools {
            self.name_to_server
                .insert(tool.namespaced_name(), server_id);
        }

        let entry = CachedTools { tools, fetched_at: Instant::now() };
        self.l1.insert(server_id, entry.clone());
        Some(entry)
    }
}
