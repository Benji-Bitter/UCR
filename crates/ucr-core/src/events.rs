//! Events emitted by the node runtime to applications.

use serde::{Deserialize, Serialize};
use ucr_protocol::ids::{ChannelId, GroupId, MsgId, NodeId};
use ucr_protocol::payload::EmergencyCategory;
use ucr_protocol::station::{CoarsePos, StationClass};
use ucr_protocol::TonePattern;
use ucr_storage::DeliveryState;

/// Everything the UI/CLI can observe. Every event corresponds to real
/// protocol activity — there are no fabricated or "simulated-looking"
/// events.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeEvent {
    /// A public channel message was received (signature state included).
    ChannelMessage {
        channel: ChannelId,
        channel_name: String,
        from: NodeId,
        callsign: String,
        text: String,
        msg_id: MsgId,
        at_ms: u64,
        /// Hops the packet took to reach us.
        hops: u8,
        /// Whether the packet signature verified.
        verified: bool,
    },
    /// A push-to-talk voice frame arrived on a channel.
    VoiceFrame {
        channel: ChannelId,
        channel_name: String,
        from: NodeId,
        callsign: String,
        /// Codec id (see `ucr_protocol::payload::voice_codecs`).
        codec: u8,
        /// Transmission id — groups frames of one PTT press.
        tx_id: u16,
        /// Frame order within the transmission.
        seq: u16,
        /// True on the final frame.
        last: bool,
        /// Encoded audio bytes.
        data: Vec<u8>,
        hops: u8,
    },
    /// An encrypted direct message was received and decrypted.
    DirectMessage {
        from: NodeId,
        callsign: String,
        text: String,
        msg_id: MsgId,
        at_ms: u64,
    },
    /// A contact request arrived (inside an E2E session).
    ContactRequest {
        from: NodeId,
        callsign: String,
        desc: String,
        class: StationClass,
        text: String,
        at_ms: u64,
    },
    /// Delivery-state transition for an outbound message.
    Delivery {
        /// Packet id the update refers to (matches send return).
        packet_id: u32,
        /// Application message id when applicable.
        msg_id: Option<MsgId>,
        peer: Option<NodeId>,
        channel: Option<ChannelId>,
        state: DeliveryState,
    },
    /// Our flooded packet was observed being relayed (≥1 hop out).
    Propagated { packet_id: u32, hops: u8 },
    /// Neighbour table changed.
    NeighborUpdate,
    /// Route table changed (route learned/lost for `dest`).
    RouteUpdate { dest: NodeId, hops: u8 },
    /// E2E session established with `peer`.
    SessionEstablished { peer: NodeId },
    /// Sender key learned and bound to `node`.
    KeyLearned { node: NodeId },
    /// Signed emergency traffic received. `verified` is true only when the
    /// packet signature checked out.
    Emergency {
        from: NodeId,
        callsign: String,
        category: EmergencyCategory,
        text: String,
        pos: Option<CoarsePos>,
        emergency_id: u64,
        at_ms: u64,
        hops: u8,
        verified: bool,
    },
    /// An identification tone definition arrived for `from`.
    Tone { from: NodeId, pattern: TonePattern },
    /// Digital Morse message (over UCR packets — NOT CW).
    Morse {
        from: NodeId,
        callsign: String,
        text: String,
        wpm: u8,
        at_ms: u64,
    },
    /// A group invitation arrived.
    GroupInvite {
        group_id: GroupId,
        name: String,
        desc: String,
        from: NodeId,
        callsign: String,
    },
    /// A group message was decrypted and stored.
    GroupMessage {
        group_id: GroupId,
        from: NodeId,
        callsign: String,
        text: String,
        msg_id: MsgId,
        at_ms: u64,
    },
    /// Group key epoch advanced (membership change).
    GroupEpoch { group_id: GroupId, epoch: u32 },
    /// Group roster changed.
    GroupRoster {
        group_id: GroupId,
        members: Vec<NodeId>,
    },
    /// Inbound traffic from a blocked identity was discarded at the
    /// application layer (it was still relayed normally by routing).
    BlockedDrop { from: NodeId },
    /// A packet failed signature verification or structural checks.
    InvalidPacket {
        from: Option<NodeId>,
        reason: String,
    },
    /// Diagnostic log line.
    Log { level: u8, msg: String },
}
