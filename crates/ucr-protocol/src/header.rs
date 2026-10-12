//! Packet header and routing control fields.

use serde::{Deserialize, Serialize};

use crate::ids::{NodeId, PacketId};

/// Distribution scope — bounds how far a packet may flood.
///
/// The scope is translated into a TTL budget by the originating node and is
/// also carried in the header so intermediate nodes never extend a packet
/// beyond the originator's intended distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Scope {
    /// Direct neighbourhood only — 1 hop.
    Direct,
    /// Local area — up to 4 hops.
    Local,
    /// Regional — up to 8 hops.
    Regional,
    /// Extended multi-hop — up to 16 hops.
    Extended,
}

impl Scope {
    /// TTL budget granted to packets sent with this scope.
    pub fn ttl(&self) -> u8 {
        match self {
            Scope::Direct => 1,
            Scope::Local => 4,
            Scope::Regional => 8,
            Scope::Extended => 16,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Scope::Direct => "Direct",
            Scope::Local => "Local",
            Scope::Regional => "Regional",
            Scope::Extended => "Extended",
        }
    }
}

/// Relay queue priority. Higher-priority packets are forwarded first, but
/// emergency priority does not exempt a node from duty-cycle/airtime limits
/// or per-source rate limits — it orders the queue, nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    /// Bulk/background traffic (beacons are normal-priority control).
    Bulk = 0,
    Normal = 1,
    /// Interactive control traffic (acks, route replies).
    Control = 2,
    /// Priority traffic such as emergency distress.
    Emergency = 3,
}

/// Fixed-size packet header.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// Sender-chosen packet id. `(src, packet_id)` deduplicates the mesh.
    pub packet_id: PacketId,
    /// Originator identity.
    pub src: NodeId,
    /// Destination — a `NodeId`, or `BROADCAST` for flooded traffic.
    pub dst: NodeId,
    /// Identity of the node that last transmitted this packet. Set to `src`
    /// at the origin and rewritten by every forwarder — lets receivers
    /// measure per-neighbour link quality and learn passive routes.
    pub relay: NodeId,
    /// Designated next hop for unicast packets; `BROADCAST` for flooded
    /// traffic. Only the named node may forward a unicast packet — this is
    /// what prevents every overhearing node from retransmitting.
    pub next_hop: NodeId,
    /// Remaining hop budget. Decremented by every forwarder; a packet at
    /// TTL 0 is delivered locally but never relayed.
    pub ttl: u8,
    /// Originator-declared distribution scope.
    pub scope: Scope,
    /// Hop counter for diagnostics — incremented per forward.
    pub hops: u8,
    /// Relay priority.
    pub prio: Priority,
    /// Originator requests an authenticated end-to-end acknowledgement.
    pub want_ack: bool,
}

impl Header {
    pub fn unicast(packet_id: PacketId, src: NodeId, dst: NodeId) -> Self {
        Self {
            packet_id,
            src,
            dst,
            relay: src,
            next_hop: dst,
            ttl: Scope::Extended.ttl(),
            scope: Scope::Extended,
            hops: 0,
            prio: Priority::Normal,
            want_ack: false,
        }
    }

    pub fn broadcast(packet_id: PacketId, src: NodeId, scope: Scope) -> Self {
        Self {
            packet_id,
            src,
            dst: crate::BROADCAST,
            relay: src,
            next_hop: crate::BROADCAST,
            ttl: scope.ttl(),
            scope,
            hops: 0,
            prio: Priority::Normal,
            want_ack: false,
        }
    }

    /// Produce the forwarded copy of this packet as transmitted by `me`:
    /// decremented TTL, incremented hop count, `relay`/`next_hop` rewritten.
    pub fn forwarded(&self, me: NodeId, next_hop: NodeId) -> Self {
        let mut h = self.clone();
        h.ttl = h.ttl.saturating_sub(1);
        h.hops = h.hops.saturating_add(1);
        h.relay = me;
        h.next_hop = next_hop;
        h
    }
}
