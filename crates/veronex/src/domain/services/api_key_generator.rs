use blake2::{Blake2b, Digest, digest::consts::U32};
use rand::RngCore as _;
use uuid::Uuid;

use crate::domain::constants::API_KEY_PREFIX;

/// BLAKE2b with 256-bit output.
type Blake2b256 = Blake2b<U32>;

/// Generate a new API key with UUIDv7 ID, cryptographically random plaintext, and BLAKE2b hash.
///
/// The database primary key (`id`) is UUIDv7 for time-ordered indexing.
/// The key *payload* uses 128 bits of cryptographically secure random bytes so that
/// the plaintext cannot be guessed from the approximate creation timestamp.
///
/// Returns `(id, plaintext, key_hash, key_prefix)`.
pub fn generate_api_key() -> (Uuid, String, String, String) {
    let id = Uuid::now_v7();
    // Independent random bytes — prevents timestamp-based key prediction.
    let mut random_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut random_bytes);
    let random_u128 = u128::from_be_bytes(random_bytes);
    let encoded = base62::encode(random_u128);
    let plaintext = format!("{API_KEY_PREFIX}{encoded}");

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
    use proptest::prelude::*;

    /// Concrete example: generated key has all expected structural properties.
    #[test]
    fn generate_api_key_structure_example() {
        let (id, plaintext, key_hash, key_prefix) = generate_api_key();
        assert_eq!(id.get_version_num(), 7);
        assert!(plaintext.starts_with("iq_"));
        assert_eq!(key_prefix.len(), 12);
        assert_eq!(&plaintext[..12], key_prefix);
        assert_eq!(key_hash, hash_api_key(&plaintext));
    }

    proptest! {
        /// Every generated key satisfies all structural invariants.
        #[test]
        fn generate_api_key_invariants(_ in 0u8..50) {
            let (id, plaintext, key_hash, key_prefix) = generate_api_key();
            // UUIDv7
            prop_assert_eq!(id.get_version_num(), 7);
            // Prefix
            prop_assert!(plaintext.starts_with("iq_"));
            prop_assert_eq!(key_prefix.len(), 12);
            prop_assert_eq!(&plaintext[..12], key_prefix.as_str());
            // Hash: Blake2b-256 = 64 hex chars
            prop_assert_eq!(key_hash.len(), 64);
            prop_assert!(key_hash.chars().all(|c| c.is_ascii_hexdigit()));
            // Hash matches plaintext
            prop_assert_eq!(&key_hash, &hash_api_key(&plaintext));
        }

        /// hash_api_key is deterministic for any input.
        #[test]
        fn hash_api_key_deterministic(input in "\\PC{1,100}") {
            let h1 = hash_api_key(&input);
            let h2 = hash_api_key(&input);
            prop_assert_eq!(&h1, &h2);
            prop_assert_eq!(h1.len(), 64);
            prop_assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
        }

        /// Different inputs produce different hashes (collision resistance).
        #[test]
        fn hash_api_key_collision_resistance(
            a in "[a-z]{5,20}",
            b in "[a-z]{5,20}",
        ) {
            prop_assume!(a != b);
            prop_assert_ne!(hash_api_key(&a), hash_api_key(&b));
        }
    }
}
