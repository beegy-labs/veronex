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
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn consistent_output_for_same_input() {
        let msgs = json!([{"role": "user", "content": "hello"}]);
        let (h1, p1) = compute_message_hashes(&msgs).unwrap();
        let (h2, p2) = compute_message_hashes(&msgs).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(p1, p2);
    }

    #[test]
    fn different_inputs_produce_different_hashes() {
        let a = json!([{"role": "user", "content": "hello"}]);
        let b = json!([{"role": "user", "content": "world"}]);
        let (ha, _) = compute_message_hashes(&a).unwrap();
        let (hb, _) = compute_message_hashes(&b).unwrap();
        assert_ne!(ha, hb);
    }

    #[test]
    fn empty_array_returns_none() {
        let msgs = json!([]);
        assert!(compute_message_hashes(&msgs).is_none());
    }

    #[test]
    fn non_array_returns_none() {
        let msgs = json!({"role": "user", "content": "hello"});
        assert!(compute_message_hashes(&msgs).is_none());
    }

    #[test]
    fn null_returns_none() {
        let msgs = json!(null);
        assert!(compute_message_hashes(&msgs).is_none());
    }

    #[test]
    fn hash_is_64_char_hex() {
        let msgs = json!([{"role": "user", "content": "test"}]);
        let (hash, _) = compute_message_hashes(&msgs).unwrap();
        // Blake2b-256 => 32 bytes => 64 hex chars
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn single_message_has_empty_prefix_hash() {
        let msgs = json!([{"role": "user", "content": "hello"}]);
        let (hash, prefix) = compute_message_hashes(&msgs).unwrap();
        assert!(!hash.is_empty());
        assert!(prefix.is_empty(), "single-turn should have empty prefix hash");
    }

    #[test]
    fn multi_turn_prefix_hash_is_nonempty() {
        let msgs = json!([
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": "hi"},
            {"role": "user", "content": "how are you"}
        ]);
        let (hash, prefix) = compute_message_hashes(&msgs).unwrap();
        assert_eq!(hash.len(), 64);
        assert_eq!(prefix.len(), 64);
        assert_ne!(hash, prefix);
    }

    #[test]
    fn prefix_hash_matches_subset_full_hash() {
        let msgs = json!([
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": "hi"},
            {"role": "user", "content": "bye"}
        ]);
        let (_, prefix) = compute_message_hashes(&msgs).unwrap();

        // The prefix hash should equal the full hash of just the first two messages
        let first_two = json!([
            {"role": "user", "content": "hello"},
            {"role": "assistant", "content": "hi"}
        ]);
        let (full_of_two, _) = compute_message_hashes(&first_two).unwrap();
        assert_eq!(prefix, full_of_two);
    }
}
