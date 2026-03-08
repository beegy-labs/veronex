//! Shared helpers for Gemini-related HTTP handlers.

/// Mask an API key for display: show first 4 and last 4 characters.
///
/// Keys shorter than 9 characters are fully redacted.
///
/// # Examples
///
/// ```text
/// "AIzaSyD1234567890abcdefg" → "AIza...defg"
/// "short"                    → "****"
/// ```
pub fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        "****".to_string()
    } else {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    }
}

/// Fetch the list of Gemini models that support `generateContent` using the
/// given `api_key`.
///
/// Calls `https://generativelanguage.googleapis.com/v1beta/models`, filters to
/// models whose `supportedGenerationMethods` includes `"generateContent"`, and
/// strips the `"models/"` prefix from each model name.
pub async fn fetch_gemini_models(client: &reqwest::Client, api_key: &str) -> anyhow::Result<Vec<String>> {
    use crate::infrastructure::outbound::gemini::adapter::GEMINI_BASE_URL;
    let url = format!(
        "{GEMINI_BASE_URL}/v1beta/models?key={api_key}"
    );

    let json: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("cannot reach gemini api: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow::anyhow!("gemini api returned error: {e}"))?
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("failed to parse gemini response: {e}"))?;

    let models = json["models"]
        .as_array()
        .map_or(&[] as &[_], |v| v)
        .iter()
        .filter(|m| {
            m["supportedGenerationMethods"]
                .as_array()
                .map(|methods| {
                    methods
                        .iter()
                        .any(|method| method.as_str() == Some("generateContent"))
                })
                .unwrap_or(false)
        })
        .filter_map(|m| {
            m["name"]
                .as_str()
                .map(|s| s.strip_prefix("models/").unwrap_or(s).to_string())
        })
        .collect();

    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_long_key() {
        assert_eq!(mask_api_key("AIzaSyD1234567890abcdefg"), "AIza...defg");
    }

    #[test]
    fn mask_short_key() {
        assert_eq!(mask_api_key("short"), "****");
    }

    #[test]
    fn mask_exactly_eight_chars() {
        assert_eq!(mask_api_key("12345678"), "****");
    }

    #[test]
    fn mask_nine_chars() {
        assert_eq!(mask_api_key("123456789"), "1234...6789");
    }
}
