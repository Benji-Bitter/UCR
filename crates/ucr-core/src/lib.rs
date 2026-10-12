//! # UCR Core
//!
//! The node runtime — composes transport, routing, crypto, identity and
//! storage into a running UCR node:
//!
//! * receive/transmit loops over any `Transport`;
//! * adaptive beacons and maintenance scheduling;
//! * public channel messaging (signed floods, scoped by channel);
//! * session-established encrypted direct messaging with authenticated acks;
//! * invitation groups with rotating symmetric keys;
//! * emergency traffic with honest delivery semantics;
//! * contact/block/mute management on stable `NodeId`s.
//!
//! The runtime never fabricates network state: everything reported through
//! `NodeEvent` derives from actual packet flow on the node's transport.

mod events;
mod node;

pub use events::*;
pub use node::{Node, NodeBuilder, NodeConfig, NodeStats, SendError};

/// Millisecond clock (UNIX epoch).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
