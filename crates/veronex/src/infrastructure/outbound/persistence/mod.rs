/// SQL fragment for soft-delete filtering. Use in WHERE clauses.
pub const SOFT_DELETE: &str = "deleted_at IS NULL";

/// Parse an optional DB string column into a typed enum with Default fallback.
/// Logs a warning if the value is present but unrecognized.
pub fn parse_db_enum<T>(value: Option<String>, column: &str) -> T
where
    T: std::str::FromStr + Default,
{
    value
        .and_then(|s| match s.parse::<T>() {
            Ok(v) => Some(v),
            Err(_) => {
                tracing::warn!(column, raw_value = %s, "unknown enum value, using default");
                None
            }
        })
        .unwrap_or_default()
}

pub mod account_repository;
pub mod caching_model_selection;
pub mod caching_ollama_model_repo;
pub mod caching_provider_registry;
pub mod capacity_settings_repository;
pub mod lab_settings_repository;
pub mod model_capacity_repository;
pub mod api_key_repository;
pub mod provider_model_selection;
pub mod gemini_model_repository;
pub mod gemini_policy_repository;
pub mod gemini_sync_config;
pub mod provider_registry;
pub mod database;
pub mod gpu_server_registry;
pub mod job_repository;
pub mod ollama_model_repository;
pub mod ollama_sync_job_repository;
pub mod session_repository;
pub mod ttl_cache;
