//! The routing engine — pure state machine driven by `handle_rx` and
//! `maintenance`. Emits `RouteAction`s; the node runtime (`ucr-core`) turns
//! them into transmissions, deliveries and app events.
//!
//! Forwarding rules:
//! * flooded packets (`dst = BROADCAST`) — delivered locally AND re-broadcast
//!   while TTL remains, within the originator's declared scope;
//! * unicast packets — forwarded ONLY by the node named in `next_hop`,
//!   along the best-metric route; no route → drop (the originator retries
//!   via RREQ — intermediate nodes never originate RREQs for others' traffic,
//!   which prevents request storms);
//! * loop prevention — `(src, packet_id)` dedup, TTL decrement, RREQ path
//!   membership check;
//! * anti-spam — per-source relay rate budgets for flooded and unicast
//!   relayed traffic. Budgets gate RELAYING only; direct delivery to us is
//!   unaffected.

use std::collections::HashMap;

use ucr_protocol::header::{Priority, Scope};
use ucr_protocol::payload::{Payload, RouteReply, RouteRequest};
use ucr_protocol::{Header, NodeId, Packet, PacketId, BROADCAST};
use ucr_transport::RxMeta;

use crate::dedup::DedupCache;
use crate::neighbor::NeighborTable;
use crate::queue::OutboundQueue;
use crate::route::{RouteEntry, RouteOrigin, RouteTable};
use crate::{
    DEDUP_TTL_MS, FLOOD_RELAY_PER_MIN, ROUTE_PASSIVE_MS, ROUTE_RREP_MS, UNICAST_RELAY_PER_MIN,
};

/// Minimum spacing between RREQs to the same target.
pub const RREQ_RATE_MS: u64 = 5_000;
/// Global RREQ budget per minute per node.
pub const RREQ_PER_MIN: u32 = 20;
/// Backoff base for queued-packet retries.
pub const RETRY_BACKOFF_MS: u64 = 8_000;
/// Max delivery attempts (RREQs) before a queued packet is abandoned.
pub const MAX_DELIVERY_ATTEMPTS: u8 = 5;
/// Default link cost assumed for passive multi-hop estimation.
const DEFAULT_LINK_COST: u32 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    Duplicate,
    TtlExhausted,
    NoRoute,
    NotForUs,
    RateLimited,
    LoopDetected,
    Expired,
    SelfOrigin,
}

#[derive(Debug)]
pub enum RouteAction {
    /// Transmit this packet on the medium (already TTL-decremented, relay
    /// rewritten). Broadcasts and unicasts both go through `Transport::send`.
    Transmit { packet: Packet },
    /// Deliver to the local application layer.
    Deliver { packet: Packet, meta: RxMeta },
    /// The application should send an authenticated Ack for this packet.
    SendAck { orig: Packet },
    /// No route to `target` — the originator should run route discovery.
    NeedRoute { target: NodeId },
    /// Someone asked for our public key — reply with a KeyReply.
    ReplyKey { to: NodeId },
    /// Packet dropped — diagnostic record.
    Drop { reason: DropReason, packet: Packet },
}

/// Cumulative counters for diagnostics and the Network UI.
#[derive(Debug, Default, Clone, Copy)]
pub struct RouterStats {
    pub rx_total: u64,
    pub tx_total: u64,
    pub forwarded: u64,
    pub delivered: u64,
    pub dropped_dup: u64,
    pub dropped_noroute: u64,
    pub dropped_ttl: u64,
    pub dropped_ratelimit: u64,
    pub rreq_sent: u64,
    pub rreq_recv: u64,
    pub rrep_sent: u64,
    pub rrep_recv: u64,
}

/// One node's routing state machine.
pub struct Router {
    pub me: NodeId,
    pub neighbors: NeighborTable,
    pub routes: RouteTable,
    pub queue: OutboundQueue,
    pub stats: RouterStats,
    dedup: DedupCache,
    /// target → last RREQ time (rate limit)
    rreq_sent_at: HashMap<NodeId, u64>,
    /// global RREQ counter, minute window
    rreq_minute: (u64, u32),
    /// per-src relay budgets: src → (window_minute, count)
    flood_budget: HashMap<NodeId, (u64, u32)>,
    unicast_budget: HashMap<NodeId, (u64, u32)>,
    /// packet id counter — seeded per-node so ids don't collide on restart
    pid: u32,
    rreq_seq: u32,
    /// tracks whether the neighbour set changed recently (beacon pacing)
    topology_changed_at: u64,
}

impl Router {
    pub fn new(me: NodeId, seed: u32) -> Self {
        Self {
            me,
            neighbors: NeighborTable::default(),
            routes: RouteTable::default(),
            queue: OutboundQueue::default(),
            stats: RouterStats::default(),
            dedup: DedupCache::new(8192, DEDUP_TTL_MS),
            rreq_sent_at: HashMap::new(),
            rreq_minute: (0, 0),
            flood_budget: HashMap::new(),
            unicast_budget: HashMap::new(),
            pid: seed,
            rreq_seq: 0,
            topology_changed_at: 0,
        }
    }

    pub fn next_packet_id(&mut self) -> PacketId {
        self.pid = self.pid.wrapping_add(1);
        self.pid
    }

    // ------------------------------------------------------------------
    // Origination — build packets this node wants to send.
    // ------------------------------------------------------------------

    /// Build a unicast packet to `dst` if a next hop is known.
    pub fn originate_unicast(
        &mut self,
        dst: NodeId,
        payload: Payload,
        prio: Priority,
        want_ack: bool,
        now: u64,
    ) -> Result<Packet, DropReason> {
        let next_hop = self.next_hop_for(&dst, now).ok_or(DropReason::NoRoute)?;
        let mut h = Header::unicast(self.next_packet_id(), self.me, dst);
        h.next_hop = next_hop;
        h.prio = prio;
        h.want_ack = want_ack;
        Ok(Packet::new(h, payload))
    }

    /// Build a broadcast/flood packet bounded by `scope`.
    pub fn originate_broadcast(
        &mut self,
        scope: Scope,
        payload: Payload,
        prio: Priority,
    ) -> Packet {
        let mut h = Header::broadcast(self.next_packet_id(), self.me, scope);
        h.prio = prio;
        Packet::new(h, payload)
    }

    /// Build an RREQ for `target` if rate limits allow.
    pub fn originate_rreq(&mut self, target: NodeId, now: u64) -> Option<Packet> {
        let minute = now / 60_000;
        if minute != self.rreq_minute.0 {
            self.rreq_minute = (minute, 0);
        }
        if self.rreq_minute.1 >= RREQ_PER_MIN {
            return None;
        }
        if let Some(&last) = self.rreq_sent_at.get(&target) {
            if now - last < RREQ_RATE_MS {
                return None;
            }
        }
        self.rreq_sent_at.insert(target, now);
        self.rreq_minute.1 += 1;
        self.rreq_seq = self.rreq_seq.wrapping_add(1);
        self.stats.rreq_sent += 1;
        Some(self.originate_broadcast(
            Scope::Regional,
            Payload::RouteRequest(RouteRequest {
                rreq_id: self.rreq_seq,
                target,
                metric_acc: 0,
                path: vec![self.me],
            }),
            Priority::Control,
        ))
    }

    /// Build an Ack for a received unicast packet that requested one.
    /// Returns None if no route back is known.
    pub fn originate_ack(&mut self, orig: &Packet, now: u64) -> Option<Packet> {
        self.originate_unicast(
            orig.header.src,
            Payload::Ack(ucr_protocol::payload::Ack {
                acked_id: orig.header.packet_id,
                hops_taken: orig.header.hops,
            }),
            Priority::Control,
            false,
            now,
        )
        .ok()
    }

    /// Choose the best next hop toward `dst`:
    /// fresh direct neighbour first, else best route.
    pub fn next_hop_for(&self, dst: &NodeId, now: u64) -> Option<NodeId> {
        if self.neighbors.is_neighbor(dst, now) {
            return Some(*dst);
        }
        self.routes.route_to(dst, now).map(|r| r.next_hop)
    }

    /// Full route info toward `dst` for display/diagnostics.
    pub fn route_info(&self, dst: &NodeId, now: u64) -> Option<(NodeId, u32, u8)> {
        if self.neighbors.is_neighbor(dst, now) {
            let n = self.neighbors.get(dst)?;
            return Some((*dst, n.link_cost(), 1));
        }
        self.routes
            .route_to(dst, now)
            .map(|r| (r.next_hop, r.metric, r.hops))
    }

    /// Record an ack-observed delivery outcome for a next hop.
    pub fn note_tx_result(&mut self, next_hop: &NodeId, success: bool) {
        if let Some(n) = self.neighbors.get_mut(next_hop) {
            if success {
                n.tx_ok = n.tx_ok.saturating_add(1);
            } else {
                n.tx_fail = n.tx_fail.saturating_add(1);
            }
        }
    }

    /// True while the neighbour set has been stable (used for adaptive
    /// beacon intervals).
    pub fn topology_stable(&self, now: u64) -> bool {
        now.saturating_sub(self.topology_changed_at) > 5 * 60 * 1000
            && self.topology_changed_at != 0
    }

    /// Record our own packet in the dedup cache so we never re-process
    /// a looped-back copy.
    pub fn mark_self_sent(&mut self, pkt: &Packet, now: u64) {
        self.dedup
            .insert(&pkt.header.src, pkt.header.packet_id, now);
        self.stats.tx_total += 1;
    }

    // ------------------------------------------------------------------
    // Reception — the main dispatch.
    // ------------------------------------------------------------------

    pub fn handle_rx(&mut self, pkt: Packet, meta: &RxMeta, now: u64) -> Vec<RouteAction> {
        let mut out = Vec::with_capacity(4);
        self.stats.rx_total += 1;
        let h = &pkt.header;

        // Own transmissions echoed back by a shared medium are not processed.
        if h.src == self.me {
            return vec![RouteAction::Drop {
                reason: DropReason::SelfOrigin,
                packet: pkt,
            }];
        }

        // Observe the *transmitting* neighbour regardless of dedup outcome —
        // overheard packets still measure the link.
        let relay_id = h.relay;
        let had_neighbor = self.neighbors.get(&relay_id).is_some();
        {
            let n = self.neighbors.entry(relay_id, now);
            let was_fresh = n.is_fresh(now);
            n.observe_rx(meta);
            if !was_fresh {
                self.topology_changed_at = now;
            }
        }
        if !had_neighbor {
            self.topology_changed_at = now;
        }
        // A heard relay is a proven-alive next hop — refresh dependent routes.
        self.routes.refresh_via(&relay_id, now, ROUTE_PASSIVE_MS);

        // Duplicate suppression.
        if self.dedup.check_and_insert(&h.src, h.packet_id, now) {
            self.stats.dropped_dup += 1;
            return vec![RouteAction::Drop {
                reason: DropReason::Duplicate,
                packet: pkt,
            }];
        }

        // Passive route learning toward the *originator* via the relay.
        if relay_id != h.src && !self.neighbors.is_neighbor(&h.src, now) {
            let relay_cost = self
                .neighbors
                .get(&relay_id)
                .map(|n| n.link_cost())
                .unwrap_or(DEFAULT_LINK_COST);
            self.routes.learn(RouteEntry {
                dest: h.src,
                next_hop: relay_id,
                metric: relay_cost + (h.hops as u32) * DEFAULT_LINK_COST,
                hops: h.hops.saturating_add(1),
                origin: RouteOrigin::Passive,
                learned_ms: now,
                expires_ms: now + ROUTE_PASSIVE_MS,
            });
        }

        // Beacon merges identity/link state.
        if let Payload::Beacon(b) = &pkt.payload {
            let n = self.neighbors.entry(h.src, now);
            n.observe_beacon(b, meta, &self.me);
        }

        let addressed_to_me = h.dst == self.me;
        let is_broadcast = h.dst.is_broadcast();
        let designated = h.next_hop == self.me || h.next_hop.is_broadcast();

        match &pkt.payload {
            Payload::Beacon(_) => {
                out.push(RouteAction::Deliver {
                    packet: pkt.clone(),
                    meta: *meta,
                });
            }
            Payload::RouteRequest(rreq) => {
                self.stats.rreq_recv += 1;
                // Loop prevention: our id already in the accumulated path.
                if rreq.path.contains(&self.me) {
                    out.push(RouteAction::Drop {
                        reason: DropReason::LoopDetected,
                        packet: pkt,
                    });
                    return out;
                }
                // Reverse route to the RREQ originator.
                if h.src != self.me {
                    let relay_cost = self
                        .neighbors
                        .get(&relay_id)
                        .map(|n| n.link_cost())
                        .unwrap_or(DEFAULT_LINK_COST);
                    self.routes.learn(RouteEntry {
                        dest: h.src,
                        next_hop: relay_id,
                        metric: rreq.metric_acc + relay_cost,
                        hops: rreq.path.len() as u8,
                        origin: RouteOrigin::Passive,
                        learned_ms: now,
                        expires_ms: now + ROUTE_PASSIVE_MS,
                    });
                }
                if rreq.target == self.me {
                    // We are the destination — answer with an RREP.
                    if let Some(rrep) = self.build_rrep(&pkt, now) {
                        self.stats.rrep_sent += 1;
                        out.push(RouteAction::Transmit { packet: rrep });
                    }
                } else if pkt.header.ttl > 1 && self.flood_budget_ok(&h.src, now) {
                    // Accumulate our link cost and re-flood.
                    let mut fwd = pkt.clone();
                    if let Payload::RouteRequest(r2) = &mut fwd.payload {
                        let cost = self
                            .neighbors
                            .get(&relay_id)
                            .map(|n| n.link_cost())
                            .unwrap_or(DEFAULT_LINK_COST);
                        r2.metric_acc += cost;
                        r2.path.push(self.me);
                    }
                    fwd.header = pkt.header.forwarded(self.me, BROADCAST);
                    fwd.signature = None; // relayed traffic is re-signed by core
                    self.stats.forwarded += 1;
                    out.push(RouteAction::Transmit { packet: fwd });
                } else {
                    self.stats.dropped_ttl += 1;
                    out.push(RouteAction::Drop {
                        reason: DropReason::TtlExhausted,
                        packet: pkt,
                    });
                }
            }
            Payload::RouteReply(rrep) => {
                self.stats.rrep_recv += 1;
                // Every node on the reply path learns the route (AODV).
                let relay_cost = self
                    .neighbors
                    .get(&relay_id)
                    .map(|n| n.link_cost())
                    .unwrap_or(DEFAULT_LINK_COST);
                self.routes.learn(RouteEntry {
                    dest: rrep.route_dest,
                    next_hop: relay_id,
                    metric: rrep.metric_acc + relay_cost,
                    hops: rrep.hop_count.saturating_add(1),
                    origin: RouteOrigin::Rrep,
                    learned_ms: now,
                    expires_ms: now + ROUTE_RREP_MS,
                });
                if addressed_to_me {
                    out.push(RouteAction::Deliver {
                        packet: pkt.clone(),
                        meta: *meta,
                    });
                } else if designated && pkt.header.ttl > 1 {
                    // Forward toward the requester along its reverse route.
                    match self.next_hop_for(&pkt.header.dst, now) {
                        Some(nh) => {
                            let mut fwd = pkt.clone();
                            if let Payload::RouteReply(r2) = &mut fwd.payload {
                                r2.metric_acc += relay_cost;
                                r2.hop_count = r2.hop_count.saturating_add(1);
                            }
                            fwd.header = pkt.header.forwarded(self.me, nh);
                            fwd.signature = None;
                            self.stats.forwarded += 1;
                            out.push(RouteAction::Transmit { packet: fwd });
                        }
                        None => {
                            self.stats.dropped_noroute += 1;
                            out.push(RouteAction::Drop {
                                reason: DropReason::NoRoute,
                                packet: pkt,
                            });
                        }
                    }
                }
            }
            Payload::KeyRequest { target } => {
                if *target == self.me {
                    out.push(RouteAction::ReplyKey { to: h.src });
                }
                // KeyRequests for others are flooded at Direct scope; forward.
                if is_broadcast && pkt.header.ttl > 1 && self.flood_budget_ok(&h.src, now) {
                    let mut fwd = pkt.clone();
                    fwd.header = pkt.header.forwarded(self.me, BROADCAST);
                    // Payload unmodified — the originator's signature over
                    // the immutable fields stays valid across hops.
                    out.push(RouteAction::Transmit { packet: fwd });
                }
            }
            Payload::KeyReply { pubkey } => {
                if addressed_to_me || is_broadcast {
                    // Learn the key if it binds to src — verified at app layer
                    // against the packet signature as well.
                    if NodeId::from_pubkey(pubkey) == h.src {
                        let n = self.neighbors.entry(h.src, now);
                        n.pubkey = Some(*pubkey);
                    }
                    out.push(RouteAction::Deliver {
                        packet: pkt.clone(),
                        meta: *meta,
                    });
                } else if designated && pkt.header.ttl > 1 {
                    match self.next_hop_for(&pkt.header.dst, now) {
                        Some(nh) => {
                            let mut fwd = pkt.clone();
                            fwd.header = pkt.header.forwarded(self.me, nh);
                            out.push(RouteAction::Transmit { packet: fwd });
                        }
                        None => out.push(RouteAction::Drop {
                            reason: DropReason::NoRoute,
                            packet: pkt,
                        }),
                    }
                }
            }
            Payload::Ack(_) => {
                if addressed_to_me {
                    self.stats.delivered += 1;
                    out.push(RouteAction::Deliver {
                        packet: pkt.clone(),
                        meta: *meta,
                    });
                } else {
                    out.extend(self.forward_unicast(pkt, meta, now));
                }
            }
            Payload::Emergency(_)
            | Payload::Data(_)
            | Payload::SessionInit(_)
            | Payload::SessionAccept(_) => {
                if addressed_to_me || is_broadcast {
                    self.stats.delivered += 1;
                    // For unicast traffic, ack requests are honoured here.
                    if addressed_to_me && pkt.header.want_ack {
                        out.push(RouteAction::SendAck { orig: pkt.clone() });
                    }
                    out.push(RouteAction::Deliver {
                        packet: pkt.clone(),
                        meta: *meta,
                    });
                    // Scoped flood: re-broadcast while TTL remains.
                    if is_broadcast && pkt.header.ttl > 1 {
                        if self.flood_budget_ok(&h.src, now) {
                            let mut fwd = pkt.clone();
                            fwd.header = pkt.header.forwarded(self.me, BROADCAST);
                            self.stats.forwarded += 1;
                            out.push(RouteAction::Transmit { packet: fwd });
                        } else {
                            self.stats.dropped_ratelimit += 1;
                            out.push(RouteAction::Drop {
                                reason: DropReason::RateLimited,
                                packet: pkt,
                            });
                        }
                    }
                } else {
                    out.extend(self.forward_unicast(pkt, meta, now));
                }
            }
        }
        out
    }

    /// Forward a unicast packet not addressed to us — only if we are the
    /// designated next hop.
    fn forward_unicast(&mut self, pkt: Packet, _meta: &RxMeta, now: u64) -> Vec<RouteAction> {
        if pkt.header.next_hop != self.me {
            return vec![RouteAction::Drop {
                reason: DropReason::NotForUs,
                packet: pkt,
            }];
        }
        if pkt.header.ttl <= 1 {
            self.stats.dropped_ttl += 1;
            return vec![RouteAction::Drop {
                reason: DropReason::TtlExhausted,
                packet: pkt,
            }];
        }
        if !self.unicast_budget_ok(&pkt.header.src, now) {
            self.stats.dropped_ratelimit += 1;
            return vec![RouteAction::Drop {
                reason: DropReason::RateLimited,
                packet: pkt,
            }];
        }
        match self.next_hop_for(&pkt.header.dst, now) {
            Some(nh) => {
                let mut fwd = pkt.clone();
                fwd.header = pkt.header.forwarded(self.me, nh);
                self.stats.forwarded += 1;
                vec![RouteAction::Transmit { packet: fwd }]
            }
            None => {
                self.stats.dropped_noroute += 1;
                vec![RouteAction::Drop {
                    reason: DropReason::NoRoute,
                    packet: pkt,
                }]
            }
        }
        .into_iter()
        .collect()
    }

    /// Build an RREP answering `rreq_pkt` (we are the target).
    fn build_rrep(&mut self, rreq_pkt: &Packet, now: u64) -> Option<Packet> {
        let Payload::RouteRequest(rreq) = &rreq_pkt.payload else {
            return None;
        };
        let mut h = Header::unicast(self.next_packet_id(), self.me, rreq_pkt.header.src);
        h.prio = Priority::Control;
        h.next_hop = self.next_hop_for(&rreq_pkt.header.src, now)?;
        Some(Packet::new(
            h,
            Payload::RouteReply(RouteReply {
                rreq_id: rreq.rreq_id,
                route_dest: self.me,
                metric_acc: 0,
                hop_count: 0,
            }),
        ))
    }

    // ------------------------------------------------------------------
    // Maintenance — periodic housekeeping driven by the runtime.
    // ------------------------------------------------------------------

    /// Periodic work: prune stale state, expire queued packets, emit retry
    /// RREQs for due queued packets. Returns actions to process.
    pub fn maintenance(&mut self, now: u64) -> Vec<RouteAction> {
        let mut out = Vec::new();

        // Track topology changes (neighbour count/freshness transitions).
        let fresh_before = self.neighbors.fresh(now).count();
        self.neighbors.prune(now);
        self.routes.prune(now);
        if self.neighbors.fresh(now).count() != fresh_before {
            self.topology_changed_at = now;
        }

        // Invalidate routes whose next hop went stale.
        let stale_hops: Vec<NodeId> = self
            .routes
            .iter()
            .map(|r| r.next_hop)
            .filter(|nh| !self.neighbors.is_neighbor(nh, now))
            .collect();
        for nh in stale_hops {
            self.routes.invalidate_via(&nh);
        }

        // Expired queue entries → delivery failure events.
        for qp in self.queue.expire(now) {
            out.push(RouteAction::Drop {
                reason: DropReason::Expired,
                packet: qp.packet,
            });
        }

        // Due retries: either transmit via a now-known route or re-run
        // route discovery with backoff.
        for qp in self.queue.due(now) {
            if qp.attempts >= MAX_DELIVERY_ATTEMPTS {
                out.push(RouteAction::Drop {
                    reason: DropReason::NoRoute,
                    packet: qp.packet,
                });
                continue;
            }
            match self.next_hop_for(&qp.dest, now) {
                Some(nh) => {
                    let mut fwd = qp.packet.clone();
                    fwd.header.next_hop = nh;
                    self.stats.forwarded += 1;
                    out.push(RouteAction::Transmit { packet: fwd });
                }
                None => {
                    // Exponential backoff: 8s, 16s, 32s, 64s.
                    let backoff = RETRY_BACKOFF_MS << qp.attempts.min(3);
                    self.queue.requeue(qp.clone(), backoff);
                    out.push(RouteAction::NeedRoute { target: qp.dest });
                }
            }
        }
        out
    }

    // ------------------------------------------------------------------
    // Rate budgets — per-source relay limits.
    // ------------------------------------------------------------------

    fn flood_budget_ok(&mut self, src: &NodeId, now: u64) -> bool {
        let minute = now / 60_000;
        let e = self.flood_budget.entry(*src).or_insert((minute, 0));
        if e.0 != minute {
            *e = (minute, 0);
        }
        if e.1 >= FLOOD_RELAY_PER_MIN {
            return false;
        }
        e.1 += 1;
        true
    }

    fn unicast_budget_ok(&mut self, src: &NodeId, now: u64) -> bool {
        let minute = now / 60_000;
        let e = self.unicast_budget.entry(*src).or_insert((minute, 0));
        if e.0 != minute {
            *e = (minute, 0);
        }
        if e.1 >= UNICAST_RELAY_PER_MIN {
            return false;
        }
        e.1 += 1;
        true
    }
}
