use blake2::{Blake2b, Digest, digest::consts::U32};
use uuid::Uuid;

/// BLAKE2b with 256-bit output.
type Blake2b256 = Blake2b<U32>;

const PREFIX: &str = "iq_";

/// Generate a new API key with UUIDv7 ID, base62-encoded plaintext, and BLAKE2b hash.
///
/// Returns `(id, plaintext, key_hash, key_prefix)`.
pub fn generate_api_key() -> (Uuid, String, String, String) {
    let id = Uuid::now_v7();
    let id_u128 = u128::from_be_bytes(*id.as_bytes());
    let encoded = base62::encode(id_u128);
    let plaintext = format!("{PREFIX}{encoded}");

    let mut hasher = Blake2b256::new();
    hasher.update(plaintext.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    let key_prefix = plaintext[..12].to_string();

    (id, plaintext, key_hash, key_prefix)
}

/// Hash a raw API key string using BLAKE2b-256, returning a hex-encoded hash.
pub fn hash_api_key(raw_key: &str) -> String {
    let mut hasher = Blake2b256::new();
    hasher.update(raw_key.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_api_key_returns_valid_uuidv7() {
        let (id, _, _, _) = generate_api_key();
        assert_eq!(id.get_version_num(), 7);
    }

    #[test]
    fn generate_api_key_plaintext_starts_with_prefix() {
        let (_, plaintext, _, _) = generate_api_key();
        assert!(plaintext.starts_with("iq_"));
    }

    #[test]
    fn generate_api_key_hash_is_64_hex_chars() {
        let (_, _, key_hash, _) = generate_api_key();
        assert_eq!(key_hash.len(), 64);
        assert!(key_hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_api_key_prefix_is_first_12_chars() {
        let (_, plaintext, _, key_prefix) = generate_api_key();
        assert_eq!(key_prefix.len(), 12);
        assert_eq!(&plaintext[..12], key_prefix);
    }

    #[test]
    fn generate_api_key_hash_matches_plaintext() {
        let (_, plaintext, key_hash, _) = generate_api_key();
        let recomputed = hash_api_key(&plaintext);
        assert_eq!(key_hash, recomputed);
    }

    #[test]
    fn generate_api_key_unique_keys() {
        let (id1, key1, hash1, _) = generate_api_key();
        let (id2, key2, hash2, _) = generate_api_key();
        assert_ne!(id1, id2);
        assert_ne!(key1, key2);
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn hash_api_key_deterministic() {
        let hash1 = hash_api_key("iq_test123");
        let hash2 = hash_api_key("iq_test123");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn hash_api_key_different_inputs_different_hashes() {
        let hash1 = hash_api_key("iq_key1");
        let hash2 = hash_api_key("iq_key2");
        assert_ne!(hash1, hash2);
    }
}
