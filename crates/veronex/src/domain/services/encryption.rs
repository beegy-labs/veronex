use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, AeadCore,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

/// Encrypt plaintext with AES-256-GCM.
/// Returns base64-encoded `nonce || ciphertext` string.
pub fn encrypt(plaintext: &str, master_key: &[u8; 32]) -> anyhow::Result<String> {
    let cipher = Aes256Gcm::new(master_key.into());
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;
    let mut combined = nonce.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(&combined))
}

/// Decrypt base64-encoded `nonce || ciphertext` with AES-256-GCM.
pub fn decrypt(encoded: &str, master_key: &[u8; 32]) -> anyhow::Result<String> {
    let combined = BASE64
        .decode(encoded)
        .map_err(|e| anyhow::anyhow!("base64 decode failed: {e}"))?;
    if combined.len() < 12 {
        anyhow::bail!("ciphertext too short");
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = aes_gcm::Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new(master_key.into());
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))?;
    String::from_utf8(plaintext).map_err(|e| anyhow::anyhow!("invalid UTF-8: {e}"))
}

/// Minimum byte length of a valid AES-256-GCM blob: 12-byte nonce + 16-byte auth tag.
const MIN_CIPHERTEXT_BYTES: usize = 28;

/// Try to decrypt; if it fails, distinguish corrupted ciphertext from legacy plaintext.
///
/// Returns `(plaintext, needs_re_encrypt)`.
/// - Decryption succeeds → `(plaintext, false)`
/// - Value looks like a ciphertext blob (valid base64, ≥28 bytes) but decryption fails
///   → **corrupted data** — logs an error and returns the raw value with `needs_re_encrypt = false`
///   so it is NOT silently overwritten.
/// - Value does NOT look like a ciphertext blob → **legacy plaintext** — returns it with
///   `needs_re_encrypt = true`.
pub fn decrypt_or_legacy(encoded: &str, master_key: &[u8; 32]) -> (String, bool) {
    match decrypt(encoded, master_key) {
        Ok(plaintext) => (plaintext, false),
        Err(e) => {
            // Heuristic: if base64-decodable with enough bytes for nonce+tag, it was
            // encrypted with a different key or the data is corrupted — not legacy plain.
            let looks_like_ciphertext = BASE64
                .decode(encoded)
                .map(|b| b.len() >= MIN_CIPHERTEXT_BYTES)
                .unwrap_or(false);

            if looks_like_ciphertext {
                tracing::error!(
                    error = %e,
                    "failed to decrypt value that appears to be ciphertext \
                     (base64-valid, ≥{MIN_CIPHERTEXT_BYTES}B) — possible key mismatch or corruption"
                );
                // Return raw value but do NOT flag for re-encryption to avoid data loss.
                (encoded.to_string(), false)
            } else {
                tracing::warn!("treating value as legacy plaintext — will re-encrypt on next write");
                (encoded.to_string(), true)
            }
        }
    }
}

/// Derive a 32-byte key from a variable-length secret using SHA-256.
pub fn derive_key(secret: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.finalize().into()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = derive_key("test-master-key-for-unit-tests");
        let plaintext = "AIzaSyD-test-api-key-12345";
        let encrypted = encrypt(plaintext, &key).unwrap();
        assert_ne!(encrypted, plaintext);
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn different_nonces_produce_different_ciphertexts() {
        let key = derive_key("test-master-key");
        let plaintext = "same-input";
        let a = encrypt(plaintext, &key).unwrap();
        let b = encrypt(plaintext, &key).unwrap();
        assert_ne!(a, b); // random nonce
    }

    #[test]
    fn wrong_key_fails_decryption() {
        let key1 = derive_key("key-one");
        let key2 = derive_key("key-two");
        let encrypted = encrypt("secret", &key1).unwrap();
        assert!(decrypt(&encrypted, &key2).is_err());
    }

    #[test]
    fn empty_string_roundtrip() {
        let key = derive_key("key");
        let encrypted = encrypt("", &key).unwrap();
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(decrypted, "");
    }

    #[test]
    fn legacy_plaintext_detected_as_needing_re_encrypt() {
        let key = derive_key("key");
        // A typical API key that is NOT valid base64 ciphertext
        let (plaintext, needs) = decrypt_or_legacy("AIzaSyD-test-api-key", &key);
        assert_eq!(plaintext, "AIzaSyD-test-api-key");
        assert!(needs, "legacy plaintext should flag needs_re_encrypt");
    }

    #[test]
    fn corrupted_ciphertext_not_treated_as_legacy() {
        let key1 = derive_key("key-one");
        let key2 = derive_key("key-two");
        // Encrypt with key1 → try to decrypt with key2 → looks like ciphertext
        let encrypted = encrypt("secret-value", &key1).unwrap();
        let (value, needs) = decrypt_or_legacy(&encrypted, &key2);
        // Should return raw ciphertext and NOT flag for re-encryption
        assert_eq!(value, encrypted);
        assert!(!needs, "corrupted ciphertext must not be flagged for re-encryption");
    }
}
