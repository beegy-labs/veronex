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
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn mask_short_key_fully_redacted(key in "[a-zA-Z0-9]{0,8}") {
            prop_assert_eq!(mask_api_key(&key), "****");
        }

        #[test]
        fn mask_long_key_preserves_first4_last4(key in "[a-zA-Z0-9]{9,64}") {
            let masked = mask_api_key(&key);
            prop_assert!(masked.starts_with(&key[..4]));
            prop_assert!(masked.ends_with(&key[key.len()-4..]));
            prop_assert!(masked.contains("..."));
            // Format is always "XXXX...YYYY" = 11 chars, hides middle portion
            prop_assert_eq!(masked.len(), 11);
        }
    }
}
