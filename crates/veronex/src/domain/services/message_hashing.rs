use std::io;

use blake2::{Blake2b, Digest, digest::consts::U32};

type Blake2b256 = Blake2b<U32>;

/// `io::Write` adapter that forwards bytes directly into a `Digest::update()`,
/// avoiding intermediate `String` allocations when hashing JSON.
struct HashWriter<D: Digest>(D);

impl<D: Digest> io::Write for HashWriter<D> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Compute Blake2b-256 of the full messages array and its prefix (all-but-last).
///
/// Returns `(messages_hash, messages_prefix_hash)`.
/// `messages_prefix_hash` is an empty string for single-turn jobs (first message only).
/// Returns `None` when `messages` is None or not a JSON array.
pub fn compute_message_hashes(messages: &serde_json::Value) -> Option<(String, String)> {
    let arr = messages.as_array()?;
    if arr.is_empty() {
        return None;
    }

    // Full hash — streams JSON directly into the hasher (no intermediate String).
    let mut w = HashWriter(Blake2b256::new());
    serde_json::to_writer(&mut w, arr).ok()?;
    let messages_hash = hex::encode(w.0.finalize());

    // Prefix hash — all turns except the last user message.
    // Empty string signals "first turn" so the grouping loop can skip parent lookup.
    let messages_prefix_hash = if arr.len() <= 1 {
        String::new()
    } else {
        let prefix = &arr[..arr.len() - 1];
        let mut w = HashWriter(Blake2b256::new());
        serde_json::to_writer(&mut w, prefix).ok()?;
        hex::encode(w.0.finalize())
    };

    Some((messages_hash, messages_prefix_hash))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    /// Concrete examples: edge cases that return None.
    #[test]
    fn returns_none_for_invalid_inputs() {
        assert!(compute_message_hashes(&json!([])).is_none());
        assert!(compute_message_hashes(&json!({"role": "user"})).is_none());
        assert!(compute_message_hashes(&json!(null)).is_none());
    }

    /// Concrete example: prefix hash matches subset full hash (chain linkage).
    #[test]
    fn prefix_hash_matches_subset_full_hash() {
        let msgs = json!([
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": "hi"},
            {"role": "user", "content": "bye"}
        ]);
        let (_, prefix) = compute_message_hashes(&msgs).unwrap();
        let first_two = json!([
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": "hi"}
        ]);
        let (full_of_two, _) = compute_message_hashes(&first_two).unwrap();
        assert_eq!(prefix, full_of_two);
    }

    proptest! {
        /// Hash is deterministic, 64-char hex for any single-message array.
        #[test]
        fn single_message_hash_properties(content in "\\PC{1,100}") {
            let msgs = json!([{"role": "user", "content": content}]);
            let (hash, prefix) = compute_message_hashes(&msgs).unwrap();
            // Blake2b-256 = 64 hex chars
            prop_assert_eq!(hash.len(), 64);
            prop_assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
            // Single message → empty prefix
            prop_assert!(prefix.is_empty());
            // Deterministic
            let (h2, p2) = compute_message_hashes(&msgs).unwrap();
            prop_assert_eq!(&hash, &h2);
            prop_assert_eq!(&prefix, &p2);
        }

        /// Different content always produces different hashes.
        #[test]
        fn different_content_different_hashes(
            a in "[a-z]{5,50}",
            b in "[a-z]{5,50}",
        ) {
            prop_assume!(a != b);
            let ma = json!([{"role": "user", "content": a}]);
            let mb = json!([{"role": "user", "content": b}]);
            let (ha, _) = compute_message_hashes(&ma).unwrap();
            let (hb, _) = compute_message_hashes(&mb).unwrap();
            prop_assert_ne!(ha, hb);
        }

        /// Multi-turn: both hashes are 64-char hex and different from each other.
        #[test]
        fn multi_turn_hash_properties(
            c1 in "[a-z]{1,30}",
            c2 in "[a-z]{1,30}",
            c3 in "[a-z]{1,30}",
        ) {
            let msgs = json!([
                {"role": "user", "content": c1},
                {"role": "assistant", "content": c2},
                {"role": "user", "content": c3}
            ]);
            let (hash, prefix) = compute_message_hashes(&msgs).unwrap();
            prop_assert_eq!(hash.len(), 64);
            prop_assert_eq!(prefix.len(), 64);
            prop_assert_ne!(&hash, &prefix);
        }
    }
}
