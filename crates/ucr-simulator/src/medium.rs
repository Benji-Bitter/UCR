//! The simulated RF medium — event-driven, deterministic, virtual-time.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use tokio::sync::mpsc;
use ucr_protocol::RadioProfile;
use ucr_transport::RxMeta;

use crate::node::SimTransport;
use crate::scenario::{Mobility, Obstacle, Pos};
use crate::{
    snr_threshold_db, CAPTURE_MARGIN_DB, NOISE_FLOOR_DBM, NOMINAL_FREQ_HZ, SENSITIVITY_DBM,
};

/// Simulator configuration. `seed` makes an entire run reproducible.
#[derive(Clone, Debug)]
pub struct SimConfig {
    pub seed: u64,
    /// Radio profile simulated nodes operate.
    pub profile: RadioProfile,
    /// Log-normal shadowing standard deviation, dB.
    pub shadow_sigma_db: f32,
    /// Additional random frame loss probability 0..1 (rain, margin cases).
    pub extra_loss: f32,
    /// Duty-cycle accounting window.
    pub duty_window_ms: u64,
    /// Environmental range ceiling (m): beyond this no reception occurs
    /// regardless of power — models terrain/horizon/clutter that free-space
    /// loss alone understates. NOT a physical constant; tune per scenario.
    pub range_limit_m: f32,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            profile: ucr_protocol::profile::builtin_profiles()[0].clone(),
            shadow_sigma_db: 4.0,
            extra_loss: 0.0,
            duty_window_ms: 3_600_000,
            range_limit_m: 5_000.0,
        }
    }
}

/// A delivered frame queued into a node's transport.
pub struct Delivery {
    pub frame: Vec<u8>,
    pub meta: RxMeta,
}

/// Aggregate medium statistics — for tests and the diagnostics UI.
#[derive(Clone, Copy, Debug, Default)]
pub struct SimStats {
    pub tx_scheduled: u64,
    pub delivered: u64,
    pub dropped_loss: u64,
    pub dropped_collision: u64,
    pub dropped_halfduplex: u64,
    pub dropped_disabled: u64,
    pub dropped_blocked: u64,
    pub airtime_ms: u64,
}

/// One scheduled medium event. Ordered by (time, seq) for determinism.
enum Event {
    /// End of a node's own transmission — frees its half-duplex radio.
    TxEnd,
    /// Frame delivery attempt at a receiver.
    Deliver {
        to: usize,
        from: usize,
        tx_start: u64,
        tx_end: u64,
        frame: Arc<Vec<u8>>,
    },
}

struct NodeState {
    pos: Pos,
    tx_power_dbm: f32,
    enabled: bool,
    mobility: Mobility,
    mobility_progress: (usize, f32),
    /// Own-TX busy end time — half-duplex: cannot receive before this.
    tx_busy_until: u64,
    /// Airtime used inside the current duty window.
    duty_used_ms: u64,
    duty_window_start: u64,
    /// Heard airtime slices (start_ms, dur_ms) for congestion estimation.
    heard: VecDeque<(u64, u64)>,
    /// Deliveries into the node transport.
    tx: mpsc::Sender<Delivery>,
    /// Inbound RSSI measured at last delivery per peer — the world's truth
    /// for the topology view (not the node's own estimate).
    last_rssi_to: HashMap<usize, f32>,
}

struct InFlightTx {
    from: usize,
    tx_start: u64,
    tx_end: u64,
    /// Mean-path-loss power at each candidate receiver (no shadowing) —
    /// deterministic interference estimate.
    power_at: HashMap<usize, f32>,
}

struct Inner {
    nodes: HashMap<usize, NodeState>,
    order: Vec<usize>,
    obstacles: Vec<Obstacle>,
    blocked: HashSet<(usize, usize)>,
    events: BinaryHeap<Reverse<(u64, u64, EventKey)>>,
    event_store: HashMap<u64, Event>,
    inflight: Vec<InFlightTx>,
    seq: u64,
    now_ms: u64,
    rng: ChaCha8Rng,
    config: SimConfig,
    stats: SimStats,
    next_id: usize,
}

// The event heap key carries an index into event_store because Event isn't Ord.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct EventKey(u64);

/// The simulation world. Cloneable handle around shared state — the world
/// and all `SimTransport`s reference the same medium.
#[derive(Clone)]
pub struct SimWorld {
    inner: Arc<Mutex<Inner>>,
}

/// A node's position/link summary for UI visualisation.
#[derive(Clone, Debug)]
pub struct NodeVis {
    pub id: usize,
    pub pos: Pos,
    pub enabled: bool,
    /// Links this node has received on recently: peer → rssi.
    pub heard_rssi: Vec<(usize, f32)>,
}

/// Why a transmit was rejected by the world.
#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum SimError {
    #[error("node not found or disabled")]
    NoSuchNode,
    #[error("transmission not permitted (duty budget or disabled)")]
    TxDenied,
}

/// What happened to a transmission — returned from `transmit` for tests.
#[derive(Clone, Debug)]
pub struct DeliveryReport {
    pub airtime_ms: u64,
    pub tx_start: u64,
    pub tx_end: u64,
    pub receivers: usize,
}

impl SimWorld {
    pub fn new(config: SimConfig) -> Self {
        Self::new_at(config, 0)
    }

    /// Construct with an explicit starting clock. Interactive/realtime use
    /// passes the wall-clock epoch so `RxMeta.at_ms` is meaningful to nodes;
    /// deterministic tests may pass 0 — protocol logic uses relative time.
    pub fn new_at(config: SimConfig, now_ms: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                nodes: HashMap::new(),
                order: Vec::new(),
                obstacles: Vec::new(),
                blocked: HashSet::new(),
                events: BinaryHeap::new(),
                event_store: HashMap::new(),
                inflight: Vec::new(),
                seq: 0,
                now_ms,
                rng: ChaCha8Rng::seed_from_u64(config.seed),
                config,
                stats: SimStats::default(),
                next_id: 0,
            })),
        }
    }

    pub fn now(&self) -> u64 {
        self.inner.lock().unwrap().now_ms
    }

    pub fn seed(&self) -> u64 {
        self.inner.lock().unwrap().config.seed
    }

    /// Register a radio at `pos`. Returns its transport handle.
    pub fn add_node(&self, pos: Pos, tx_power_dbm: f32, mobility: Mobility) -> SimTransport {
        let (tx, rx) = mpsc::channel(512);
        let mut g = self.inner.lock().unwrap();
        let id = g.next_id;
        g.next_id += 1;
        g.nodes.insert(
            id,
            NodeState {
                pos,
                tx_power_dbm,
                enabled: true,
                mobility,
                mobility_progress: (0, 0.0),
                tx_busy_until: 0,
                duty_used_ms: 0,
                duty_window_start: 0,
                heard: VecDeque::new(),
                tx,
                last_rssi_to: HashMap::new(),
            },
        );
        g.order.push(id);
        SimTransport::new(id, self.clone(), rx)
    }

    /// Register a node with the default 14 dBm / fixed mobility.
    pub fn add_simple(&self, pos: Pos) -> SimTransport {
        self.add_node(pos, 14.0, Mobility::Fixed)
    }

    pub fn remove_node(&self, id: usize) {
        let mut g = self.inner.lock().unwrap();
        g.nodes.remove(&id);
        g.order.retain(|n| *n != id);
    }

    pub fn set_enabled(&self, id: usize, enabled: bool) {
        if let Some(n) = self.inner.lock().unwrap().nodes.get_mut(&id) {
            n.enabled = enabled;
        }
    }

    pub fn is_enabled(&self, id: usize) -> bool {
        self.inner
            .lock()
            .unwrap()
            .nodes
            .get(&id)
            .map(|n| n.enabled)
            .unwrap_or(false)
    }

    pub fn set_position(&self, id: usize, pos: Pos) {
        if let Some(n) = self.inner.lock().unwrap().nodes.get_mut(&id) {
            n.pos = pos;
        }
    }

    pub fn position(&self, id: usize) -> Option<Pos> {
        self.inner.lock().unwrap().nodes.get(&id).map(|n| n.pos)
    }

    pub fn set_mobility(&self, id: usize, mobility: Mobility) {
        if let Some(n) = self.inner.lock().unwrap().nodes.get_mut(&id) {
            n.mobility = mobility;
            n.mobility_progress = (0, 0.0);
        }
    }

    /// Simulate a hard partition between two nodes (and only them).
    pub fn block_pair(&self, a: usize, b: usize) {
        self.inner
            .lock()
            .unwrap()
            .blocked
            .insert((a.min(b), a.max(b)));
    }

    pub fn unblock_pair(&self, a: usize, b: usize) {
        self.inner
            .lock()
            .unwrap()
            .blocked
            .remove(&(a.min(b), a.max(b)));
    }

    /// Isolate a node entirely (RF blackout — models a partition edge).
    pub fn isolate(&self, id: usize) {
        let others: Vec<usize> = self
            .inner
            .lock()
            .unwrap()
            .order
            .iter()
            .copied()
            .filter(|o| *o != id)
            .collect();
        for o in others {
            self.block_pair(id, o);
        }
    }

    pub fn unisolate(&self, id: usize) {
        let mut g = self.inner.lock().unwrap();
        g.blocked.retain(|(a, b)| *a != id && *b != id);
    }

    pub fn add_obstacle(&self, o: Obstacle) {
        self.inner.lock().unwrap().obstacles.push(o);
    }

    pub fn clear_obstacles(&self) {
        self.inner.lock().unwrap().obstacles.clear();
    }

    pub fn stats(&self) -> SimStats {
        self.inner.lock().unwrap().stats
    }

    pub fn node_ids(&self) -> Vec<usize> {
        self.inner.lock().unwrap().order.clone()
    }

    /// Perceived channel-busy fraction 0..=255 at a node — share of the last
    /// 60 s of airtime the node could hear occupied.
    pub fn congestion(&self, id: usize) -> u8 {
        let g = self.inner.lock().unwrap();
        let Some(n) = g.nodes.get(&id) else { return 0 };
        let window_start = g.now_ms.saturating_sub(60_000);
        let mut busy = 0u64;
        for &(s, d) in &n.heard {
            if s + d > window_start {
                busy += (s + d).min(g.now_ms) - s.max(window_start);
            }
        }
        ((busy.min(60_000) * 255) / 60_000) as u8
    }

    /// Positions + per-node last-received RSSI for the topology view.
    pub fn node_vis(&self) -> Vec<NodeVis> {
        let g = self.inner.lock().unwrap();
        g.order
            .iter()
            .filter_map(|id| {
                g.nodes.get(id).map(|n| NodeVis {
                    id: *id,
                    pos: n.pos,
                    enabled: n.enabled,
                    heard_rssi: n.last_rssi_to.iter().map(|(k, v)| (*k, *v)).collect(),
                })
            })
            .collect()
    }

    /// Free-space path loss between two positions, dB (lock already held).
    fn fspl_db_locked(g: &Inner, a: usize, b: usize) -> f32 {
        let (Some(na), Some(nb)) = (g.nodes.get(&a), g.nodes.get(&b)) else {
            return f32::INFINITY;
        };
        let d = na.pos.distance(&nb.pos).max(1.0) as f64;
        // FSPL = 20log10(d) + 20log10(f) + 20log10(4π/c)
        let fspl = 20.0 * d.log10()
            + 20.0 * NOMINAL_FREQ_HZ.log10()
            + 20.0 * (4.0 * std::f64::consts::PI / 299_792_458.0).log10();
        let mut loss = fspl as f32;
        for o in &g.obstacles {
            if o.crosses(na.pos, nb.pos) {
                loss += o.atten_db;
            }
        }
        loss
    }

    /// Transmit `frame` from `node`. Called by `SimTransport::send`.
    /// Schedules the TX after duty-cycle hold-off; delivery events land at
    /// the end of airtime. Returns a report (or error if node gone).
    pub fn transmit(&self, node: usize, frame: Vec<u8>) -> Result<DeliveryReport, SimError> {
        let mut g = self.inner.lock().unwrap();
        let Some(profile) = g.nodes.get(&node).map(|_| g.config.profile.clone()) else {
            return Err(SimError::NoSuchNode);
        };
        if !g.nodes.get(&node).map(|n| n.enabled).unwrap_or(false) {
            g.stats.dropped_disabled += 1;
            return Ok(DeliveryReport {
                airtime_ms: 0,
                tx_start: g.now_ms,
                tx_end: g.now_ms,
                receivers: 0,
            });
        }
        let airtime = profile.airtime_ms(frame.len()).max(1);
        let now_ms = g.now_ms;
        let duty_window_ms = g.config.duty_window_ms;
        // Duty-cycle hold-off.
        let mut tx_start = now_ms;
        {
            let n = g.nodes.get_mut(&node).unwrap();
            if now_ms >= n.duty_window_start + duty_window_ms {
                n.duty_window_start = now_ms;
                n.duty_used_ms = 0;
            }
            if profile.duty_cycle_permille > 0 {
                let budget = duty_window_ms * profile.duty_cycle_permille as u64 / 1000;
                if n.duty_used_ms + airtime > budget {
                    tx_start = n.duty_window_start + duty_window_ms;
                    n.duty_window_start = tx_start;
                    n.duty_used_ms = 0;
                }
                n.duty_used_ms += airtime;
            }
            // Half-duplex: a node can't start TX while it's mid-TX already.
            tx_start = tx_start.max(n.tx_busy_until);
            n.tx_busy_until = tx_start + airtime;
        }

        // Listen-before-talk (CAD/CSMA): if any in-flight transmission is
        // audible above the noise floor at this node, defer until it ends.
        // Deferring nodes add a small random backoff so several deferred
        // transmitters don't restart in lockstep.
        let mut deferred = false;
        for tx in &g.inflight {
            if tx.tx_end > tx_start {
                let audible = tx
                    .power_at
                    .get(&node)
                    .map(|&p| p > NOISE_FLOOR_DBM)
                    .unwrap_or(false);
                if audible {
                    tx_start = tx_start.max(tx.tx_end);
                    deferred = true;
                }
            }
        }
        if deferred {
            tx_start += g.rng.gen_range(0..=40);
            let n = g.nodes.get_mut(&node).unwrap();
            n.tx_busy_until = tx_start + airtime;
        }
        let tx_end = tx_start + airtime;
        g.stats.tx_scheduled += 1;
        g.stats.airtime_ms += airtime;

        // Interference bookkeeping: mean-path-loss power at every receiver.
        let mut power_at = HashMap::new();
        for &rid in g.nodes.keys() {
            if rid == node {
                continue;
            }
            let loss = Self::fspl_db_locked(&g, node, rid);
            let txp = g.nodes[&node].tx_power_dbm;
            power_at.insert(rid, txp - loss);
        }
        g.inflight.retain(|t| t.tx_end > tx_start);
        g.inflight.push(InFlightTx {
            from: node,
            tx_start,
            tx_end,
            power_at,
        });

        let frame = Arc::new(frame);
        let receivers = g.order.iter().copied().filter(|rid| *rid != node).count();
        for rid in g.order.clone() {
            if rid == node {
                continue;
            }
            g.seq += 1;
            let seq = g.seq;
            g.event_store.insert(
                seq,
                Event::Deliver {
                    to: rid,
                    from: node,
                    tx_start,
                    tx_end,
                    frame: frame.clone(),
                },
            );
            g.events.push(Reverse((tx_end, seq, EventKey(seq))));
        }
        g.seq += 1;
        let seq = g.seq;
        g.event_store.insert(seq, Event::TxEnd);
        g.events.push(Reverse((tx_end, seq, EventKey(seq))));
        Ok(DeliveryReport {
            airtime_ms: airtime,
            tx_start,
            tx_end,
            receivers,
        })
    }

    /// Advance mobility by `dt` at time `now`.
    fn apply_mobility(g: &mut Inner, dt: u64) {
        if dt == 0 {
            return;
        }
        for n in g.nodes.values_mut() {
            n.mobility.advance(&mut n.pos, &mut n.mobility_progress, dt);
        }
    }

    /// Process the next pending event. Returns false when nothing remains.
    pub fn step(&self) -> bool {
        let mut g = self.inner.lock().unwrap();
        let Some(Reverse((t, seq, _))) = g.events.peek().copied() else {
            return false;
        };
        g.events.pop();
        let Some(ev) = g.event_store.remove(&seq) else {
            return true;
        };
        if t > g.now_ms {
            let now = g.now_ms;
            Self::apply_mobility(&mut g, t - now);
            g.now_ms = t;
        }
        match ev {
            Event::TxEnd => {}
            Event::Deliver {
                to,
                from,
                tx_start,
                tx_end,
                frame,
            } => Self::deliver(&mut g, to, from, tx_start, tx_end, &frame),
        }
        true
    }

    /// Process all events until virtual time `t`.
    pub fn run_until(&self, t: u64) {
        loop {
            {
                let g = self.inner.lock().unwrap();
                match g.events.peek() {
                    Some(Reverse((et, _, _))) if *et <= t => {}
                    _ => break,
                }
            }
            if !self.step() {
                break;
            }
        }
        // Advance clock/mobility even with no events pending.
        let mut g = self.inner.lock().unwrap();
        if t > g.now_ms {
            let now = g.now_ms;
            Self::apply_mobility(&mut g, t - now);
            g.now_ms = t;
        }
    }

    /// Advance the medium `ms` virtual milliseconds, processing events.
    pub fn run_for(&self, ms: u64) {
        let target = self.now() + ms;
        self.run_until(target);
    }

    /// Drain every pending event regardless of time.
    pub fn drain(&self) {
        while self.step() {}
    }

    /// Delivery attempt — the reception model. Runs with the lock held.
    fn deliver(g: &mut Inner, to: usize, from: usize, tx_start: u64, tx_end: u64, frame: &[u8]) {
        let blocked = g.blocked.contains(&(to.min(from), to.max(from)));
        if blocked {
            g.stats.dropped_blocked += 1;
            return;
        }
        let Some(node) = g.nodes.get(&to) else { return };
        if !node.enabled {
            g.stats.dropped_disabled += 1;
            return;
        }
        // Half-duplex: receiving while transmitting fails.
        if tx_start < node.tx_busy_until {
            g.stats.dropped_halfduplex += 1;
            return;
        }
        // Environmental range ceiling — beyond it nothing is heard.
        let dist = {
            let (Some(a), Some(b)) = (g.nodes.get(&from), g.nodes.get(&to)) else {
                return;
            };
            a.pos.distance(&b.pos)
        };
        if dist > g.config.range_limit_m {
            g.stats.dropped_loss += 1;
            return;
        }
        let desired_power = {
            let loss = Self::fspl_db_locked(g, from, to);
            g.nodes[&from].tx_power_dbm - loss
        };
        // Shadowing sample (deterministic order-dependent).
        let shadow = if g.config.shadow_sigma_db > 0.0 {
            let u1: f32 = g.rng.gen::<f32>().max(1e-7);
            let u2: f32 = g.rng.gen();
            let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos();
            z * g.config.shadow_sigma_db
        } else {
            0.0
        };
        let rssi = desired_power + shadow;

        // Interference: any other in-flight TX overlapping our window whose
        // power at this receiver is within the capture margin corrupts us.
        let mut corrupted = false;
        for tx in &g.inflight {
            if tx.from == from {
                continue;
            }
            let overlaps = tx.tx_start < tx_end && tx.tx_end > tx_start;
            if !overlaps {
                continue;
            }
            if let Some(&p) = tx.power_at.get(&to) {
                if p >= desired_power - CAPTURE_MARGIN_DB {
                    corrupted = true;
                    break;
                }
            }
        }

        // Airtime heard for congestion accounting — anything above the noise
        // floor counts, decodable or not.
        if desired_power > NOISE_FLOOR_DBM {
            let node = g.nodes.get_mut(&to).unwrap();
            node.heard.push_back((tx_start, tx_end - tx_start));
            while node
                .heard
                .front()
                .map(|(s, d)| *s + *d < g.now_ms.saturating_sub(60_000))
                .unwrap_or(false)
            {
                node.heard.pop_front();
            }
        }

        let snr = rssi - NOISE_FLOOR_DBM;
        let sf = g.config.profile.sf;
        let decodable = !corrupted
            && rssi >= SENSITIVITY_DBM
            && snr >= snr_threshold_db(sf)
            && g.rng.gen::<f32>() >= g.config.extra_loss;

        if corrupted {
            g.stats.dropped_collision += 1;
            return;
        }
        if !decodable {
            g.stats.dropped_loss += 1;
            return;
        }

        let node = g.nodes.get_mut(&to).unwrap();
        node.last_rssi_to.insert(from, rssi);
        g.stats.delivered += 1;
        let meta = RxMeta::measured(rssi, snr, tx_end);
        // Channel full → the frame is lost (bounded queues are deliberate).
        let _ = node.tx.try_send(Delivery {
            frame: frame.to_vec(),
            meta,
        });
    }
}
