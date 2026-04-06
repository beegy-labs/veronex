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
