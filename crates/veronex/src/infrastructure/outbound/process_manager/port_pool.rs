//! Per-node TCP port allocator for managed `llama-server` instances.
//!
//! Each spawn picks an unused port from a configured range; when the
//! process stops, the port returns to the pool. Implemented with a
//! [`DashSet`] of in-use ports for O(1) allocate / release.
//!
//! Ports are scoped per-`PortPool` instance — production ships one pool
//! per `LlmNode` so two nodes can independently reuse the same range.

use std::ops::RangeInclusive;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use dashmap::DashSet;

/// Default range — `11430..=11530` gives 101 slots, well above the
/// realistic concurrent-process count per node (Mac mini has 16-32 GB
/// unified memory, so ~2-4 simultaneous models). The range starts above
/// 11434 so a co-resident upstream service doesn't conflict.
pub const DEFAULT_RANGE: RangeInclusive<u16> = 11430..=11530;

/// Allocator state. Cheap to clone (`Arc`-of-state).
#[derive(Clone)]
pub struct PortPool {
    inner: Arc<Inner>,
}

struct Inner {
    range: RangeInclusive<u16>,
    in_use: DashSet<u16>,
}

impl PortPool {
    /// Construct with the given range. Caller is responsible for choosing
    /// a range that doesn't collide with anything else on the host.
    pub fn new(range: RangeInclusive<u16>) -> Self {
        Self {
            inner: Arc::new(Inner {
                range,
                in_use: DashSet::new(),
            }),
        }
    }

    /// Use [`DEFAULT_RANGE`].
    pub fn default_range() -> Self {
        Self::new(DEFAULT_RANGE)
    }

    /// Total slots in the range.
    pub fn capacity(&self) -> usize {
        let r = &self.inner.range;
        (*r.end() as usize).saturating_sub(*r.start() as usize) + 1
    }

    /// Slots currently in use.
    pub fn in_use(&self) -> usize {
        self.inner.in_use.len()
    }

    /// Allocate the lowest available port. Errors when the range is
    /// exhausted; callers should treat this as a soft cap on concurrent
    /// managed processes per node.
    ///
    /// Returns a [`PortLease`] that releases automatically on drop —
    /// ensures the slot returns to the pool even if the orchestrator
    /// path early-returns due to a spawn failure.
    pub fn allocate(&self) -> Result<PortLease> {
        for port in self.inner.range.clone() {
            // `insert` returns true only when the value was newly added.
            // Two concurrent `allocate()` callers on the same pool race here
            // and the loser tries the next port — DashSet's per-bucket lock
            // makes this safe.
            if self.inner.in_use.insert(port) {
                return Ok(PortLease {
                    port,
                    pool: self.inner.clone(),
                });
            }
        }
        Err(anyhow!(
            "port pool exhausted ({}..={})",
            self.inner.range.start(),
            self.inner.range.end()
        ))
    }

    /// Mark a specific port as in-use without going through `allocate`.
    /// Used at boot when reconnecting to processes that survived a restart.
    /// Returns false when the port was already taken.
    pub fn reserve(&self, port: u16) -> bool {
        if !self.inner.range.contains(&port) {
            return false;
        }
        self.inner.in_use.insert(port)
    }

    /// Manual release for the rare case where the caller can't hold a
    /// [`PortLease`] (e.g. recovering from external state).
    pub fn release(&self, port: u16) {
        self.inner.in_use.remove(&port);
    }
}

/// RAII handle. Drop returns the port to the pool. Hold one for the
/// lifetime of the spawned process.
pub struct PortLease {
    port: u16,
    pool: Arc<Inner>,
}

impl PortLease {
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for PortLease {
    fn drop(&mut self) {
        self.pool.in_use.remove(&self.port);
    }
}

impl std::fmt::Debug for PortLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortLease").field("port", &self.port).finish()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn allocate_returns_lowest_port() {
        let pool = PortPool::new(20000..=20002);
        // Bind to keep the lease alive — dropping it releases the port and
        // the next call would reuse the same number.
        let a = pool.allocate().unwrap();
        let b = pool.allocate().unwrap();
        assert_eq!(a.port(), 20000);
        assert_eq!(b.port(), 20001);
    }

    #[test]
    fn drop_releases_port() {
        let pool = PortPool::new(20000..=20001);
        {
            let _l = pool.allocate().unwrap();
            assert_eq!(pool.in_use(), 1);
        }
        assert_eq!(pool.in_use(), 0);
        // The next allocate gets the same port back.
        assert_eq!(pool.allocate().unwrap().port(), 20000);
    }

    #[test]
    fn allocate_fails_when_exhausted() {
        let pool = PortPool::new(20000..=20001);
        let _a = pool.allocate().unwrap();
        let _b = pool.allocate().unwrap();
        assert!(pool.allocate().is_err());
    }

    #[test]
    fn reserve_blocks_a_specific_port() {
        let pool = PortPool::new(20000..=20002);
        assert!(pool.reserve(20001));
        // First allocate skips 20001.
        let p1 = pool.allocate().unwrap();
        let p2 = pool.allocate().unwrap();
        let mut got = vec![p1.port(), p2.port()];
        got.sort_unstable();
        assert_eq!(got, vec![20000, 20002]);
        // Re-reserving a reserved port returns false.
        assert!(!pool.reserve(20001));
    }

    #[test]
    fn reserve_rejects_out_of_range() {
        let pool = PortPool::new(20000..=20001);
        assert!(!pool.reserve(19999));
        assert!(!pool.reserve(20002));
    }

    #[test]
    fn capacity_reflects_range_size() {
        assert_eq!(PortPool::new(20000..=20000).capacity(), 1);
        assert_eq!(PortPool::new(20000..=20002).capacity(), 3);
    }
}
