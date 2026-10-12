//! Route table — next-hop routes to non-neighbour destinations.

use std::collections::HashMap;
use ucr_protocol::NodeId;

/// How a route was learned — affects trust and lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteOrigin {
    /// Destination is a direct neighbour.
    Direct,
    /// Learned by overhearing traffic (`relay` was observed forwarding
    /// `src`'s packets). Cheapest to maintain, lower confidence.
    Passive,
    /// Learned from an RREP for a request we originated.
    Rrep,
}

#[derive(Clone, Debug)]
pub struct RouteEntry {
    pub dest: NodeId,
    pub next_hop: NodeId,
    /// End-to-end accumulated link metric (lower = better).
    pub metric: u32,
    pub hops: u8,
    pub origin: RouteOrigin,
    pub learned_ms: u64,
    pub expires_ms: u64,
}

impl RouteEntry {
    pub fn is_fresh(&self, now: u64) -> bool {
        now < self.expires_ms
    }
}

/// Best-metric route table, bounded.
#[derive(Debug)]
pub struct RouteTable {
    map: HashMap<NodeId, RouteEntry>,
    max: usize,
}

impl Default for RouteTable {
    fn default() -> Self {
        Self::new(crate::ROUTE_TABLE_MAX)
    }
}

impl RouteTable {
    pub fn new(max: usize) -> Self {
        Self {
            map: HashMap::new(),
            max,
        }
    }

    /// Learn or refresh a route. Keeps the better metric; RREP-learned routes
    /// preempt passive ones at equal metric (they were explicitly verified).
    pub fn learn(&mut self, entry: RouteEntry) {
        if let Some(existing) = self.map.get(&entry.dest) {
            let better = entry.metric < existing.metric
                || (entry.metric == existing.metric
                    && entry.origin == RouteOrigin::Rrep
                    && existing.origin == RouteOrigin::Passive);
            let fresher_origin = existing.origin == RouteOrigin::Direct;
            if !better || fresher_origin {
                return;
            }
        }
        if !self.map.contains_key(&entry.dest) && self.map.len() >= self.max {
            // evict expired-then-worst
            let victim = self
                .map
                .iter()
                .max_by_key(|(_, e)| (e.is_fresh(entry.learned_ms) as u8, e.metric))
                .map(|(k, _)| *k);
            if let Some(v) = victim {
                self.map.remove(&v);
            }
        }
        self.map.insert(entry.dest, entry);
    }

    /// Best fresh route to `dest`.
    pub fn route_to(&self, dest: &NodeId, now: u64) -> Option<&RouteEntry> {
        self.map.get(dest).filter(|e| e.is_fresh(now))
    }

    /// Remove all routes that traverse `next_hop` — called when a neighbour
    /// goes stale. Returns the orphaned destinations.
    pub fn invalidate_via(&mut self, next_hop: &NodeId) -> Vec<NodeId> {
        let lost: Vec<NodeId> = self
            .map
            .iter()
            .filter(|(_, e)| &e.next_hop == next_hop)
            .map(|(d, _)| *d)
            .collect();
        for d in &lost {
            self.map.remove(d);
        }
        lost
    }

    /// Refresh the expiry of routes via `next_hop` when the neighbour proves
    /// alive again (e.g. a fresh beacon).
    pub fn refresh_via(&mut self, next_hop: &NodeId, now: u64, ext_ms: u64) {
        for e in self.map.values_mut() {
            if &e.next_hop == next_hop {
                e.expires_ms = (now + ext_ms).max(e.expires_ms);
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &RouteEntry> {
        self.map.values()
    }

    pub fn prune(&mut self, now: u64) -> usize {
        let before = self.map.len();
        self.map.retain(|_, e| e.is_fresh(now));
        before - self.map.len()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
