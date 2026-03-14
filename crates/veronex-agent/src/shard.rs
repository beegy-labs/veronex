/// Lock-free modulus sharding for multi-replica target distribution.
///
/// Each agent pod owns targets where `hash(server_id) % replicas == ordinal`.
/// Ordinal is extracted from the StatefulSet pod hostname (e.g. `agent-2` → 2).
use std::hash::{DefaultHasher, Hash, Hasher};

/// Returns true if this agent ordinal owns the given server_id.
pub fn owns(server_id: &str, ordinal: u32, replicas: u32) -> bool {
    if replicas <= 1 {
        return true;
    }
    let mut hasher = DefaultHasher::new();
    server_id.hash(&mut hasher);
    (hasher.finish() % replicas as u64) as u32 == ordinal
}

/// Extract ordinal from StatefulSet HOSTNAME (e.g. "veronex-agent-2" → 2).
/// Falls back to 0 for non-StatefulSet deployments.
pub fn ordinal_from_hostname() -> u32 {
    std::env::var("HOSTNAME")
        .ok()
        .and_then(|h| h.rsplit('-').next()?.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Concrete edge cases kept as documentation.
    #[test]
    fn single_replica_owns_all() {
        assert!(owns("any-server", 0, 1));
        assert!(owns("another", 0, 0)); // edge case: replicas=0
    }

    #[test]
    fn ordinal_parsing() {
        assert_eq!("veronex-agent-2".rsplit('-').next().and_then(|s| s.parse::<u32>().ok()), Some(2));
        assert_eq!("agent-0".rsplit('-').next().and_then(|s| s.parse::<u32>().ok()), Some(0));
        assert_eq!("no-number-here".rsplit('-').next().and_then(|s| s.parse::<u32>().ok()), None);
    }

    proptest! {
        /// Each server_id is assigned to exactly one replica (no duplicates, no orphans).
        #[test]
        fn every_id_assigned_to_exactly_one_replica(
            id in "[a-z]{3,20}",
            replicas in 2u32..=16,
        ) {
            let owners: Vec<u32> = (0..replicas).filter(|&o| owns(&id, o, replicas)).collect();
            prop_assert_eq!(owners.len(), 1, "{} assigned to {:?} (expected exactly 1)", id, owners);
        }

        /// Single replica always owns everything.
        #[test]
        fn single_replica_always_owns(id in "[a-z]{1,30}") {
            prop_assert!(owns(&id, 0, 1));
        }

        /// Distribution across replicas is roughly even (chi-squared-like check).
        #[test]
        fn distribution_roughly_even(replicas in 2u32..=8) {
            let n = 1000u32;
            let mut counts = vec![0u32; replicas as usize];
            for i in 0..n {
                let id = format!("server-{i}");
                let owner = (0..replicas).find(|&o| owns(&id, o, replicas)).unwrap();
                counts[owner as usize] += 1;
            }
            let expected = n / replicas;
            let tolerance = expected / 3; // ~33% tolerance
            for (i, &c) in counts.iter().enumerate() {
                prop_assert!(
                    c > expected - tolerance && c < expected + tolerance,
                    "shard {i} got {c}/{n}, expected ~{expected} ± {tolerance}"
                );
            }
        }
    }
}
