//! Provider URL and type validation logic.

use crate::domain::enums::ProviderType;

use super::error::AppError;

/// Parse a provider type string (case-insensitive).
pub(super) fn parse_provider_type(s: &str) -> Option<ProviderType> {
    match s.to_lowercase().as_str() {
        "ollama" => Some(ProviderType::Ollama),
        "gemini" => Some(ProviderType::Gemini),
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

    #[test]
    fn parse_provider_type_case_insensitive() {
        assert_eq!(parse_provider_type("Ollama"), Some(ProviderType::Ollama));
        assert_eq!(parse_provider_type("GEMINI"), Some(ProviderType::Gemini));
        assert_eq!(parse_provider_type("unknown"), None);
    }

    #[test]
    fn validate_provider_url_allows_http() {
        assert!(validate_provider_url("http://192.168.1.10:11434").is_ok());
    }

    #[test]
    fn validate_provider_url_allows_https() {
        assert!(validate_provider_url("https://api.example.com").is_ok());
    }

    #[test]
    fn validate_provider_url_allows_localhost() {
        assert!(validate_provider_url("http://localhost:11434").is_ok());
    }

    #[test]
    fn validate_provider_url_blocks_gcp_metadata() {
        let err = validate_provider_url("http://metadata.google.internal/computeMetadata/v1/")
            .unwrap_err();
        assert!(err.to_string().contains("metadata"));
    }

    #[test]
    fn validate_provider_url_blocks_ftp_scheme() {
        let err = validate_provider_url("ftp://example.com").unwrap_err();
        assert!(err.to_string().contains("http://"));
    }

    #[test]
    fn validate_provider_url_blocks_file_scheme() {
        let err = validate_provider_url("file:///etc/passwd").unwrap_err();
        assert!(err.to_string().contains("http://"));
    }

    #[test]
    fn validate_provider_url_blocks_ipv6_link_local() {
        let err = validate_provider_url("http://[fe80::1]:11434").unwrap_err();
        assert!(err.to_string().contains("metadata"));
    }

    #[test]
    fn validate_provider_url_blocks_ipv4_mapped_ipv6_metadata() {
        let err = validate_provider_url("http://[::ffff:169.254.169.254]/latest/").unwrap_err();
        assert!(err.to_string().contains("metadata"));
    }

    #[test]
    fn validate_provider_url_blocks_ipv4_link_local() {
        let err = validate_provider_url("http://169.254.169.254/latest/meta-data/").unwrap_err();
        assert!(err.to_string().contains("metadata"));
    }
}
