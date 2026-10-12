//! Duplicate suppression — `(src, packet_id)` seen-window cache.

use std::collections::HashMap;
use ucr_protocol::{NodeId, PacketId};

/// Bounded dedup cache. Entries expire after `ttl_ms`.
pub struct DedupCache {
    seen: HashMap<(NodeId, PacketId), u64>,
    max: usize,
    ttl_ms: u64,
}

impl DedupCache {
    pub fn new(max: usize, ttl_ms: u64) -> Self {
        Self {
            seen: HashMap::new(),
            max,
            ttl_ms,
        }
    }

    /// Returns `true` if this packet was already seen (and records it if new).
    pub fn check_and_insert(&mut self, src: &NodeId, id: PacketId, now: u64) -> bool {
        if self.seen.contains_key(&(*src, id)) {
            return true;
        }
        if self.seen.len() >= self.max {
            // evict expired first; if still full, evict oldest
            self.seen
                .retain(|_, t| now.saturating_sub(*t) < self.ttl_ms);
            if self.seen.len() >= self.max {
                if let Some(k) = self.seen.iter().min_by_key(|(_, t)| **t).map(|(k, _)| *k) {
                    self.seen.remove(&k);
                }
            }
        }
        self.seen.insert((*src, id), now);
        false
    }

    /// Check only (no insert) — for locally originated packets we record
    /// separately.
    pub fn contains(&self, src: &NodeId, id: PacketId) -> bool {
        self.seen.contains_key(&(*src, id))
    }

    pub fn insert(&mut self, src: &NodeId, id: PacketId, now: u64) {
        self.check_and_insert(src, id, now);
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_works() {
        let mut d = DedupCache::new(4, 1000);
        let a = NodeId([1; 16]);
        assert!(!d.check_and_insert(&a, 1, 0));
        assert!(d.check_and_insert(&a, 1, 0));
        assert!(!d.check_and_insert(&a, 2, 0));
    }

    #[test]
    fn evicts_when_full() {
        let mut d = DedupCache::new(2, 10_000);
        let a = NodeId([1; 16]);
        d.insert(&a, 1, 0);
        d.insert(&a, 2, 1);
        d.insert(&a, 3, 2); // exceeds max → evicts oldest
        assert!(d.len() <= 2);
    }
}
