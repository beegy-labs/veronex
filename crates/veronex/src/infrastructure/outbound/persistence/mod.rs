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
pub mod caching_api_key_repo;
pub mod caching_lab_settings_repo;
pub mod caching_model_selection;
pub mod caching_ollama_model_repo;
pub mod caching_provider_registry;
pub mod capacity_settings_repository;
pub mod lab_settings_repository;
pub mod mcp_settings_repository;
pub mod model_capacity_repository;
pub mod api_key_repository;
pub mod provider_model_selection;
pub mod global_model_settings;
pub mod api_key_provider_access;
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
pub mod provider_vram_budget_repository;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_db_enum_valid_value() {
        let result: i32 = parse_db_enum(Some("42".to_string()), "test_col");
        assert_eq!(result, 42);
    }

    #[test]
    fn parse_db_enum_none_returns_default() {
        let result: i32 = parse_db_enum(None, "test_col");
        assert_eq!(result, 0); // i32::default()
    }

    #[test]
    fn parse_db_enum_invalid_falls_back_to_default() {
        let result: i32 = parse_db_enum(Some("not_a_number".to_string()), "test_col");
        assert_eq!(result, 0);
    }
}
