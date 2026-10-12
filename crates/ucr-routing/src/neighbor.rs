//! Neighbour table — directly-heard nodes and their measured link quality.

use std::collections::HashMap;

use ucr_protocol::payload::Beacon;
use ucr_protocol::station::{caps, CoarsePos, StationClass};
use ucr_protocol::NodeId;
use ucr_transport::RxMeta;

use crate::EWMA_ALPHA;

/// A neighbour is "fresh" (usable for routing) while seen within this window.
pub const NEIGHBOR_TIMEOUT_MS: u64 = 5 * 60 * 1000;
/// Neighbour entries are dropped entirely after this.
pub const NEIGHBOR_EXPIRE_MS: u64 = 24 * 60 * 60 * 1000;

/// Everything a node knows about a directly reachable neighbour.
#[derive(Clone, Debug)]
pub struct Neighbor {
    pub node_id: NodeId,
    /// Long-term Ed25519 pubkey when learned (from beacons/key replies).
    pub pubkey: Option<[u8; 32]>,
    pub callsign: String,
    pub desc: String,
    pub class: StationClass,
    pub caps: u16,
    pub profile: u8,
    /// EWMA of RSSI in dBm — NaN until measured.
    pub rssi_ewma: f32,
    /// EWMA of SNR in dB — NaN until measured.
    pub snr_ewma: f32,
    pub last_seen_ms: u64,
    pub first_seen_ms: u64,
    /// Packets received directly from this neighbour.
    pub rx_count: u32,
    /// Our transmissions to them that were acknowledged/confirmed delivered.
    pub tx_ok: u32,
    /// Our transmissions to them that were not confirmed.
    pub tx_fail: u32,
    /// True when this neighbour lists *us* in its beacon `seen` list —
    /// a real bidirectional check, not an assumption.
    pub bidirectional: bool,
    /// Airtime-busy fraction reported by the neighbour, 0..=255.
    pub congestion: u8,
    /// Battery percent or 255 = unknown.
    pub battery: u8,
    pub pos: Option<CoarsePos>,
    /// Identification tone id from its last beacon.
    pub tone_id: u8,
    /// Beacon sequence seen.
    pub beacon_seq: u16,
}

impl Neighbor {
    pub fn new(node_id: NodeId, now: u64) -> Self {
        Self {
            node_id,
            pubkey: None,
            callsign: String::new(),
            desc: String::new(),
            class: StationClass::Individual,
            caps: 0,
            profile: 0,
            rssi_ewma: f32::NAN,
            snr_ewma: f32::NAN,
            last_seen_ms: now,
            first_seen_ms: now,
            rx_count: 0,
            tx_ok: 0,
            tx_fail: 0,
            bidirectional: false,
            congestion: 0,
            battery: 255,
            pos: None,
            tone_id: 0,
            beacon_seq: 0,
        }
    }

    /// Any direct reception from this neighbour.
    pub fn observe_rx(&mut self, meta: &RxMeta) {
        self.rx_count = self.rx_count.saturating_add(1);
        self.last_seen_ms = meta.at_ms;
        if meta.rssi_dbm.is_finite() {
            self.rssi_ewma = if self.rssi_ewma.is_nan() {
                meta.rssi_dbm
            } else {
                EWMA_ALPHA * meta.rssi_dbm + (1.0 - EWMA_ALPHA) * self.rssi_ewma
            };
        }
        if meta.snr_db.is_finite() {
            self.snr_ewma = if self.snr_ewma.is_nan() {
                meta.snr_db
            } else {
                EWMA_ALPHA * meta.snr_db + (1.0 - EWMA_ALPHA) * self.snr_ewma
            };
        }
    }

    /// Merge a beacon's declared state.
    pub fn observe_beacon(&mut self, b: &Beacon, meta: &RxMeta, me: &NodeId) {
        self.observe_rx(meta);
        self.pubkey = Some(b.pubkey);
        self.callsign = b.callsign.clone();
        self.desc = b.desc.clone();
        self.class = b.class;
        self.caps = b.caps;
        self.profile = b.profile;
        self.congestion = b.congestion;
        self.battery = b.battery;
        self.pos = b.pos;
        self.tone_id = b.tone_id;
        self.beacon_seq = b.seq;
        self.bidirectional = b.seen.contains(me);
    }

    pub fn is_fresh(&self, now: u64) -> bool {
        now.saturating_sub(self.last_seen_ms) < NEIGHBOR_TIMEOUT_MS
    }

    /// Whether this neighbour offers to relay for others.
    pub fn is_relay(&self) -> bool {
        self.caps & caps::RELAY != 0
    }

    /// Estimated one-hop delivery probability in (0, 1].
    ///
    /// Combines three honest signals:
    /// * measured TX results (ack-observed success), once ≥3 samples exist;
    /// * SNR margin → reception-probability mapping, when measured;
    /// * a conservative 0.5 prior when nothing is measured.
    pub fn delivery_prob(&self) -> f32 {
        let total = self.tx_ok + self.tx_fail;
        if total >= 3 {
            // Laplace-smoothed observed success rate.
            return (self.tx_ok as f32 + 1.0) / (total as f32 + 2.0);
        }
        if self.snr_ewma.is_finite() {
            // SNR -5 dB → ~0.10, +14 dB → ~0.95. Linear between.
            return (0.05 + (self.snr_ewma + 5.0) / 20.0).clamp(0.05, 0.99);
        }
        if self.rx_count >= 4 {
            // We hear them reliably; without SNR assume moderate.
            return 0.6;
        }
        0.5
    }

    /// Link cost used by the router (lower = better).
    ///
    /// `100/p` is an ETX-like expected-transmission count scaled ×100, plus
    /// penalties for congestion, low battery, unidirectionality and lack of
    /// relay willingness. The furthest neighbour is only preferred when it
    /// wins this composite.
    pub fn link_cost(&self) -> u32 {
        let p = self.delivery_prob().max(0.05);
        let mut cost = (100.0 / p) as u32;
        cost += (self.congestion as u32) / 8;
        if !self.bidirectional {
            cost += 40;
        }
        if !self.is_relay() {
            cost += 25;
        }
        if self.battery != 255 && self.battery < 20 {
            cost += 15;
        }
        cost
    }

    /// 0..=100 quality score for display — derived from delivery prob + SNR.
    pub fn quality_score(&self) -> u8 {
        let p = self.delivery_prob();
        let mut q = (p * 80.0) as i32;
        if self.snr_ewma.is_finite() {
            q += ((self.snr_ewma.clamp(-10.0, 20.0) + 10.0) * (20.0 / 30.0)) as i32;
        } else {
            q += 10;
        }
        q.clamp(0, 100) as u8
    }
}

/// Bounded neighbour table; evicts the stalest entry when full.
#[derive(Debug)]
pub struct NeighborTable {
    map: HashMap<NodeId, Neighbor>,
    max: usize,
}

impl Default for NeighborTable {
    fn default() -> Self {
        Self::new(crate::NEIGHBOR_TABLE_MAX)
    }
}

impl NeighborTable {
    pub fn new(max: usize) -> Self {
        Self {
            map: HashMap::new(),
            max,
        }
    }

    pub fn get(&self, id: &NodeId) -> Option<&Neighbor> {
        self.map.get(id)
    }

    pub fn get_mut(&mut self, id: &NodeId) -> Option<&mut Neighbor> {
        self.map.get_mut(id)
    }

    /// Insert-or-fetch. Evicts the oldest non-fresh entry when at capacity.
    pub fn entry(&mut self, id: NodeId, now: u64) -> &mut Neighbor {
        if !self.map.contains_key(&id) && self.map.len() >= self.max {
            let victim = self
                .map
                .iter()
                .min_by_key(|(_, n)| n.last_seen_ms)
                .map(|(k, _)| *k);
            if let Some(v) = victim {
                self.map.remove(&v);
            }
        }
        self.map.entry(id).or_insert_with(|| Neighbor::new(id, now))
    }

    /// All fresh (recently-seen) neighbours.
    pub fn fresh(&self, now: u64) -> impl Iterator<Item = &Neighbor> {
        let n = now;
        self.map.values().filter(move |nb| nb.is_fresh(n))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Neighbor> {
        self.map.values()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Remove entries not seen for `NEIGHBOR_EXPIRE_MS`.
    pub fn prune(&mut self, now: u64) -> usize {
        let before = self.map.len();
        self.map
            .retain(|_, n| now.saturating_sub(n.last_seen_ms) < NEIGHBOR_EXPIRE_MS);
        before - self.map.len()
    }

    /// Whether `id` is a fresh direct neighbour.
    pub fn is_neighbor(&self, id: &NodeId, now: u64) -> bool {
        self.map.get(id).map(|n| n.is_fresh(now)).unwrap_or(false)
    }

    /// Neighbour node ids heard most recently, for the beacon `seen` list.
    pub fn recently_heard(&self, now: u64, limit: usize) -> Vec<NodeId> {
        let mut v: Vec<_> = self.fresh(now).collect();
        v.sort_by_key(|n| std::cmp::Reverse(n.last_seen_ms));
        v.into_iter().take(limit).map(|n| n.node_id).collect()
    }
}
