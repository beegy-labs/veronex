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
    use proptest::prelude::*;

    /// Concrete example kept as documentation.
    #[test]
    fn encrypt_decrypt_roundtrip_example() {
        let key = derive_key("test-master-key-for-unit-tests");
        let plaintext = "AIzaSyD-test-api-key-12345";
        let encrypted = encrypt(plaintext, &key).unwrap();
        assert_ne!(encrypted, plaintext);
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    /// Unique edge case: legacy plaintext detection.
    #[test]
    fn legacy_plaintext_detected_as_needing_re_encrypt() {
        let key = derive_key("key");
        let (plaintext, needs) = decrypt_or_legacy("AIzaSyD-test-api-key", &key);
        assert_eq!(plaintext, "AIzaSyD-test-api-key");
        assert!(needs, "legacy plaintext should flag needs_re_encrypt");
    }

    /// Unique edge case: corrupted ciphertext not treated as legacy.
    #[test]
    fn corrupted_ciphertext_not_treated_as_legacy() {
        let key1 = derive_key("key-one");
        let key2 = derive_key("key-two");
        let encrypted = encrypt("secret-value", &key1).unwrap();
        let (value, needs) = decrypt_or_legacy(&encrypted, &key2);
        assert_eq!(value, encrypted);
        assert!(!needs, "corrupted ciphertext must not be flagged for re-encryption");
    }

    proptest! {
        /// encrypt → decrypt roundtrip holds for arbitrary plaintext and key.
        #[test]
        fn encrypt_decrypt_roundtrip(
            plaintext in "\\PC{0,200}",
            key_secret in "[a-z]{5,30}",
        ) {
            let key = derive_key(&key_secret);
            let encrypted = encrypt(&plaintext, &key).unwrap();
            prop_assert_ne!(&encrypted, &plaintext, "ciphertext must differ from plaintext");
            let decrypted = decrypt(&encrypted, &key).unwrap();
            prop_assert_eq!(&decrypted, &plaintext);
        }

        /// Same plaintext encrypted twice produces different ciphertext (random nonce).
        #[test]
        fn different_nonces_produce_different_ciphertexts(
            plaintext in "\\PC{1,100}",
            key_secret in "[a-z]{5,20}",
        ) {
            let key = derive_key(&key_secret);
            let a = encrypt(&plaintext, &key).unwrap();
            let b = encrypt(&plaintext, &key).unwrap();
            prop_assert_ne!(a, b);
        }

        /// Wrong key always fails decryption.
        #[test]
        fn wrong_key_fails_decryption(
            plaintext in "\\PC{1,100}",
            key1_secret in "[a-z]{5,20}",
            key2_secret in "[A-Z]{5,20}",
        ) {
            let key1 = derive_key(&key1_secret);
            let key2 = derive_key(&key2_secret);
            prop_assume!(key1 != key2);
            let encrypted = encrypt(&plaintext, &key1).unwrap();
            prop_assert!(decrypt(&encrypted, &key2).is_err());
        }

        /// derive_key is deterministic.
        #[test]
        fn derive_key_deterministic(secret in "\\PC{1,100}") {
            let k1 = derive_key(&secret);
            let k2 = derive_key(&secret);
            prop_assert_eq!(k1, k2);
            prop_assert_eq!(k1.len(), 32);
        }
    }
}
