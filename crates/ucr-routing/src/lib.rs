//! # UCR Routing
//!
//! Decentralised multi-hop routing. Every node maintains:
//!
//! * a **neighbour table** — nodes heard directly, with measured link quality
//!   (RSSI/SNR EWMA, observed delivery probability), capabilities, congestion
//!   and bidirectionality (detected via the `seen` list in beacons);
//! * a **route table** — next-hop routes learned passively from overheard
//!   traffic and reactively via AODV-lite RREQ/RREP;
//! * a **dedup cache** — `(src, packet_id)` duplicate suppression;
//! * an **outbound queue** — bounded store-and-forward for unreachable
//!   destinations.
//!
//! Design constraints baked in:
//! * GPS is never required — position improves route choice when present but
//!   all decisions work on topology + measured link quality alone;
//! * the "furthest reachable" neighbour is NOT automatically best — the
//!   metric combines delivery probability, congestion, bidirectionality and
//!   relay willingness;
//! * relayed traffic is rate-limited per source so one abusive node cannot
//!   monopolise airtime.

mod dedup;
mod neighbor;
mod queue;
mod route;
mod router;

pub use dedup::DedupCache;
pub use neighbor::{Neighbor, NeighborTable, NEIGHBOR_TIMEOUT_MS};
pub use queue::{OutboundQueue, QueuedPacket};
pub use route::{RouteEntry, RouteOrigin, RouteTable};
pub use router::{DropReason, RouteAction, Router, RouterStats, RREQ_RATE_MS};

/// EWMA factor for link-quality estimators.
pub const EWMA_ALPHA: f32 = 0.3;
/// Maximum neighbours tracked.
pub const NEIGHBOR_TABLE_MAX: usize = 64;
/// Maximum routes tracked.
pub const ROUTE_TABLE_MAX: usize = 128;
/// Outbound queue capacity (packets).
pub const OUTBOUND_QUEUE_MAX: usize = 128;
/// Default queued-message lifetime (ms): 2 hours.
pub const QUEUE_EXPIRY_MS: u64 = 2 * 60 * 60 * 1000;
/// Passive route lifetime.
pub const ROUTE_PASSIVE_MS: u64 = 5 * 60 * 1000;
/// RREP-learned route lifetime.
pub const ROUTE_RREP_MS: u64 = 10 * 60 * 1000;
/// Dedup entry lifetime.
pub const DEDUP_TTL_MS: u64 = 10 * 60 * 1000;
/// Per-source relayed-flood budget per minute (anti-spam for flooded kinds).
pub const FLOOD_RELAY_PER_MIN: u32 = 12;
/// Per-source relayed-unicast budget per minute.
pub const UNICAST_RELAY_PER_MIN: u32 = 40;
