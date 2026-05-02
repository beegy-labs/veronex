/// Application configuration parsed from environment variables.
pub struct AppConfig {
    pub database_url: String,
    pub valkey_url: Option<String>,
    pub analytics_url: Option<String>,
    pub analytics_secret: Option<String>,
    /// OTLP HTTP endpoint for the OTel Collector (e.g. `http://otel-collector:4318`).
    /// When set, inference and audit events are emitted directly via OTLP,
    /// bypassing the veronex-analytics HTTP write path.
    pub otel_http_endpoint: Option<String>,
    pub jwt_secret: String,
    pub bootstrap_super_user: Option<String>,
    pub bootstrap_super_pass: Option<String>,
    pub port: u16,
    pub cors_origins: Vec<axum::http::HeaderValue>,
    pub analyzer_url: String,
    pub session_grouping_interval_secs: u64,
    pub gemini_encryption_key: [u8; 32],
    /// Redpanda/Kafka broker address (e.g. `redpanda:9092`). Used for pipeline metrics.
    pub kafka_broker: Option<String>,
    /// ClickHouse HTTP URL (e.g. `http://clickhouse:8123`). Used for pipeline metrics.
    pub clickhouse_http_url: Option<String>,
    /// ClickHouse credentials.
    pub clickhouse_user: Option<String>,
    pub clickhouse_password: Option<String>,
    pub clickhouse_db: Option<String>,
    /// Vespa environment isolation key — injected via `VESPA_ENVIRONMENT`.
    /// Partitions a shared Vespa instance per environment (prod, dev, local-dev, ...).
    /// Defaults to `"local-dev"` when unset.
    pub vespa_environment: String,
    /// Vespa tenant isolation key — injected via `VESPA_TENANT_ID`.
    /// Sub-partitions a deployment's documents by logical tenant (e.g. org, team, workspace).
    /// Defaults to `"default"` when unset.
    pub vespa_tenant_id: String,
    /// Optional Valkey key prefix — injected via `VALKEY_KEY_PREFIX`.
    /// Namespaces all Valkey keys for multi-tenant / multi-deployment shared instances.
    /// Defaults to `""` (no prefix) when unset.
    pub valkey_key_prefix: String,
    /// Postgres connection pool max size — injected via `PG_POOL_MAX`.
    /// Defaults to `10`. Tune up on hot-path-heavy deployments.
    pub pg_pool_max: u32,
    /// Per-IP login attempts allowed in `LOGIN_ATTEMPTS_WINDOW_SECS` (5 min)
    /// before the rate-limiter trips a 429 — injected via `LOGIN_RATE_LIMIT`.
    /// Defaults to `10`.
    pub login_rate_limit: u32,
    /// Vision model used when an image-analysis request leaves the model
    /// unspecified — injected via `VISION_FALLBACK_MODEL`. Defaults to
    /// `qwen3-vl:8b`. Caller may override per-call (lab settings).
    pub vision_fallback_model: String,
    /// Valkey client pool size — injected via `VALKEY_POOL_SIZE`. Defaults to `6`.
    pub valkey_pool_size: usize,
    /// API instance identity — injected via `VERONEX_INSTANCE_ID`. Defaults
    /// to a fresh UUIDv7 generated at startup so a missing env var still
    /// produces a unique heartbeat key.
    pub instance_id: String,
    /// Vespa endpoint for MCP tool vector retrieval — injected via `VESPA_URL`.
    /// `None` disables vector selection (fallback: `get_all`).
    pub vespa_url: Option<String>,
    /// Embedding service endpoint — injected via `EMBED_URL`. Required when
    /// `vespa_url` is set; otherwise vector selection stays off.
    pub embed_url: Option<String>,
    /// Number of top-K MCP tools returned by the vector selector — injected
    /// via `MCP_VECTOR_TOP_K`. Defaults to `16`.
    pub mcp_vector_top_k: usize,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL env var is required");

        let valkey_url = std::env::var("VALKEY_URL").ok();

        let analytics_url = std::env::var("ANALYTICS_URL").ok();
        let analytics_secret = std::env::var("ANALYTICS_SECRET").ok();
        let otel_http_endpoint = std::env::var("OTEL_HTTP_ENDPOINT").ok();

        let ollama_url = std::env::var("OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());

        // consumed but unused — kept to avoid silent ignore of the env var
        let _gemini_api_key = std::env::var("GEMINI_API_KEY").ok();

        let jwt_secret = std::env::var("JWT_SECRET")
            .expect("JWT_SECRET env var is required — generate with: openssl rand -hex 32");
        assert!(
            jwt_secret.len() >= 32,
            "JWT_SECRET must be at least 32 characters long (got {})",
            jwt_secret.len()
        );

        let bootstrap_super_user = std::env::var("BOOTSTRAP_SUPER_USER")
            .ok()
            .filter(|s| !s.is_empty());
        let bootstrap_super_pass = std::env::var("BOOTSTRAP_SUPER_PASS")
            .ok()
            .filter(|s| !s.is_empty());

        let port: u16 = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000);

        let cors_raw = std::env::var("CORS_ALLOWED_ORIGINS").expect(
            "CORS_ALLOWED_ORIGINS env var is required (use comma-separated origins or 'none')",
        );
        let cors_origins = parse_cors_origins(&cors_raw);

        let analyzer_url = std::env::var("CAPACITY_ANALYZER_OLLAMA_URL")
            .unwrap_or_else(|_| ollama_url.clone());

        let session_grouping_interval_secs: u64 = std::env::var("SESSION_GROUPING_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(86_400); // 24h default

        let gemini_encryption_key = {
            let raw = std::env::var("GEMINI_ENCRYPTION_KEY").expect(
                "GEMINI_ENCRYPTION_KEY env var is required — generate with: openssl rand -hex 32",
            );
            assert!(
                raw.len() >= 32,
                "GEMINI_ENCRYPTION_KEY must be at least 32 characters long (got {})",
                raw.len()
            );
            veronex::domain::services::encryption::derive_key(&raw)
        };

        let kafka_broker = std::env::var("KAFKA_BROKER").ok();
        let clickhouse_http_url = std::env::var("CLICKHOUSE_HTTP_URL").ok();
        let clickhouse_user = std::env::var("CLICKHOUSE_USER").ok();
        let clickhouse_password = std::env::var("CLICKHOUSE_PASSWORD").ok();
        let clickhouse_db = std::env::var("CLICKHOUSE_DB").ok();
        let vespa_environment = std::env::var("VESPA_ENVIRONMENT")
            .unwrap_or_else(|_| "local-dev".to_string());
        let vespa_tenant_id = std::env::var("VESPA_TENANT_ID")
            .unwrap_or_else(|_| "default".to_string());
        let valkey_key_prefix = std::env::var("VALKEY_KEY_PREFIX")
            .unwrap_or_default();
        let pg_pool_max: u32 = std::env::var("PG_POOL_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let login_rate_limit: u32 = std::env::var("LOGIN_RATE_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let vision_fallback_model = std::env::var("VISION_FALLBACK_MODEL")
            .unwrap_or_else(|_| "qwen3-vl:8b".to_string());
        let valkey_pool_size: usize = std::env::var("VALKEY_POOL_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(6);
        let instance_id = std::env::var("VERONEX_INSTANCE_ID")
            .unwrap_or_else(|_| uuid::Uuid::now_v7().to_string());
        let vespa_url = std::env::var("VESPA_URL").ok();
        let embed_url = std::env::var("EMBED_URL").ok();
        let mcp_vector_top_k: usize = std::env::var("MCP_VECTOR_TOP_K")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(16);

        Self {
            database_url,
            valkey_url,
            analytics_url,
            analytics_secret,
            otel_http_endpoint,
            jwt_secret,
            bootstrap_super_user,
            bootstrap_super_pass,
            port,
            cors_origins,
            analyzer_url,
            session_grouping_interval_secs,
            gemini_encryption_key,
            kafka_broker,
            clickhouse_http_url,
            clickhouse_user,
            clickhouse_password,
            clickhouse_db,
            vespa_environment,
            vespa_tenant_id,
            valkey_key_prefix,
            pg_pool_max,
            login_rate_limit,
            vision_fallback_model,
            valkey_pool_size,
            instance_id,
            vespa_url,
            embed_url,
            mcp_vector_top_k,
        }
    }
}

/// Parse `CORS_ALLOWED_ORIGINS` into a list of `HeaderValue`s.
///
/// `"*"` (default) → empty Vec → `AllowOrigin::any()` in the router.
/// `"https://a.com,https://b.com"` → list of two origins.
fn parse_cors_origins(raw: &str) -> Vec<axum::http::HeaderValue> {
    if raw.trim() == "*" {
        return Vec::new();
    }
    raw.split(',')
        .filter_map(|s| {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            axum::http::HeaderValue::from_str(s).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cors_origins_wildcard_returns_empty() {
        assert!(parse_cors_origins("*").is_empty());
        assert!(parse_cors_origins("  *  ").is_empty());
    }

    #[test]
    fn parse_cors_origins_single_origin() {
        let result = parse_cors_origins("http://localhost:3000");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "http://localhost:3000");
    }

    #[test]
    fn parse_cors_origins_multiple_origins() {
        let result = parse_cors_origins("http://localhost:3000,https://app.example.com");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn parse_cors_origins_skips_empty_segments() {
        let result = parse_cors_origins("http://localhost:3000,,https://app.example.com");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn parse_cors_origins_empty_string_returns_empty() {
        assert!(parse_cors_origins("").is_empty());
    }
}
