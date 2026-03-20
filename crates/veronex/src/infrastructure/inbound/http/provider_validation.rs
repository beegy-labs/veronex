//! Provider URL and type validation logic.

use crate::domain::enums::ProviderType;

use super::error::AppError;

/// Parse a provider type string (case-insensitive).
pub(super) fn parse_provider_type(s: &str) -> Option<ProviderType> {
    match s.to_lowercase().as_str() {
        "ollama"  => Some(ProviderType::Ollama),
        "gemini"  => Some(ProviderType::Gemini),
        "whisper" => Some(ProviderType::Whisper),
        _ => None,
    }
}

/// SSRF prevention: block cloud metadata endpoints, IPv6 loopback/link-local,
/// and IPv4-mapped IPv6 addresses. Enforce http(s) scheme.
///
/// Localhost and private-network IPs are intentionally allowed because Ollama
/// providers run on local machines (e.g. `http://192.168.1.10:11434`).
pub(super) fn validate_provider_url(url: &str) -> Result<(), AppError> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::BadRequest(
            "provider URL must use http:// or https://".into(),
        ));
    }

    // Block known metadata hostnames.
    if url.contains("metadata.google.internal") {
        return Err(AppError::BadRequest("metadata endpoints are not allowed".into()));
    }

    // Extract host portion between :// and the next /.
    let after_scheme = url.split("://").nth(1).unwrap_or("");
    let authority = after_scheme.split('/').next().unwrap_or("");
    // Handle IPv6 brackets: [::1]:port -> extract content between [ and ].
    let bare = if let Some(start) = authority.find('[') {
        let end = authority.find(']').unwrap_or(authority.len());
        &authority[start + 1..end]
    } else {
        // IPv4 or hostname: strip port.
        authority.split(':').next().unwrap_or("")
    };

    if let Ok(ip) = bare.parse::<std::net::IpAddr>() {
        // Block link-local (169.254.x.x / fe80::) -- cloud metadata lives here.
        if is_link_local(&ip) {
            return Err(AppError::BadRequest("metadata endpoints are not allowed".into()));
        }
        // Block IPv4-mapped IPv6 link-local (e.g. ::ffff:169.254.169.254).
        if let std::net::IpAddr::V6(v6) = ip
            && let Some(mapped) = v6.to_ipv4_mapped()
                && mapped.is_link_local() {
                    return Err(AppError::BadRequest("metadata endpoints are not allowed".into()));
                }
    }
    Ok(())
}

fn is_link_local(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_link_local(),
        std::net::IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) == 0xfe80,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Concrete examples for unique SSRF edge cases.
    #[test]
    fn validate_provider_url_blocks_metadata_endpoints() {
        assert!(validate_provider_url("http://metadata.google.internal/computeMetadata/v1/").is_err());
        assert!(validate_provider_url("http://[fe80::1]:11434").is_err());
        assert!(validate_provider_url("http://[::ffff:169.254.169.254]/latest/").is_err());
        assert!(validate_provider_url("http://169.254.169.254/latest/meta-data/").is_err());
    }

    /// Concrete example: parse_provider_type case insensitivity.
    #[test]
    fn parse_provider_type_examples() {
        assert_eq!(parse_provider_type("Ollama"), Some(ProviderType::Ollama));
        assert_eq!(parse_provider_type("GEMINI"), Some(ProviderType::Gemini));
        assert_eq!(parse_provider_type("Whisper"), Some(ProviderType::Whisper));
        assert_eq!(parse_provider_type("unknown"), None);
    }

    proptest! {
        /// Any case variation of "ollama" or "gemini" is recognized.
        #[test]
        fn parse_provider_type_case_insensitive(
            mixed_case in prop::sample::select(vec![
                "ollama", "OLLAMA", "Ollama", "oLlAmA",
                "gemini", "GEMINI", "Gemini", "gEmInI",
            ])
        ) {
            let result = parse_provider_type(mixed_case);
            let lower = mixed_case.to_lowercase();
            match lower.as_str() {
                "ollama" => prop_assert_eq!(result, Some(ProviderType::Ollama)),
                "gemini" => prop_assert_eq!(result, Some(ProviderType::Gemini)),
                _ => unreachable!(),
            }
        }

        /// Any non-matching string returns None.
        #[test]
        fn parse_provider_type_unknown_returns_none(
            s in "[a-z]{1,20}"
        ) {
            prop_assume!(s.to_lowercase() != "ollama" && s.to_lowercase() != "gemini" && s.to_lowercase() != "whisper");
            prop_assert_eq!(parse_provider_type(&s), None);
        }

        /// Valid http/https URLs with normal hosts always pass.
        #[test]
        fn validate_url_http_https_accepted(
            host in "[a-z]{3,10}(\\.[a-z]{2,5})?",
            port in 1000u16..65535,
            use_https in proptest::bool::ANY,
        ) {
            let scheme = if use_https { "https" } else { "http" };
            let url = format!("{scheme}://{host}:{port}");
            prop_assert!(validate_provider_url(&url).is_ok());
        }

        /// Non http/https schemes are always rejected.
        #[test]
        fn validate_url_non_http_rejected(
            scheme in "(ftp|ssh|ws|wss|file|tcp|udp)",
            host in "[a-z]{3,10}",
        ) {
            let url = format!("{scheme}://{host}");
            prop_assert!(validate_provider_url(&url).is_err());
        }

        /// IPv4 link-local range 169.254.x.x is always blocked.
        #[test]
        fn validate_url_link_local_ipv4_blocked(
            a in 0u8..=255,
            b in 0u8..=255,
        ) {
            let url = format!("http://169.254.{a}.{b}/");
            prop_assert!(validate_provider_url(&url).is_err());
        }
    }
}
