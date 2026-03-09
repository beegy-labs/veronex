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

    #[test]
    fn single_replica_owns_all() {
        assert!(owns("any-server", 0, 1));
        assert!(owns("another", 0, 0)); // edge case: replicas=0
    }

    #[test]
    fn deterministic_assignment() {
        let id = "server-abc-123";
        let owner = (0..3).find(|&o| owns(id, o, 3)).unwrap();
        // Same input always maps to same owner
        for _ in 0..100 {
            assert!(owns(id, owner, 3));
        }
    }

    #[test]
    fn no_duplicates() {
        let ids = ["srv-1", "srv-2", "srv-3", "srv-4", "srv-5"];
        let replicas = 3;
        for id in &ids {
            let owners: Vec<u32> = (0..replicas).filter(|&o| owns(id, o, replicas)).collect();
            assert_eq!(owners.len(), 1, "{id} assigned to multiple owners: {owners:?}");
        }
    }

    #[test]
    fn even_distribution() {
        let replicas = 4;
        let mut counts = [0u32; 4];
        for i in 0..1000 {
            let id = format!("server-{i}");
            let owner = (0..replicas).find(|&o| owns(&id, o, replicas)).unwrap();
            counts[owner as usize] += 1;
        }
        // Each shard should get roughly 250 ± 50
        for (i, &c) in counts.iter().enumerate() {
            assert!(
                (200..=300).contains(&c),
                "shard {i} got {c}/1000 targets — uneven distribution"
            );
        }
    }

    #[test]
    fn ordinal_parsing() {
        // Can't set env in parallel tests safely, so test the parse logic
        assert_eq!("veronex-agent-2".rsplit('-').next().and_then(|s| s.parse::<u32>().ok()), Some(2));
        assert_eq!("agent-0".rsplit('-').next().and_then(|s| s.parse::<u32>().ok()), Some(0));
        assert_eq!("no-number-here".rsplit('-').next().and_then(|s| s.parse::<u32>().ok()), None);
    }
}
