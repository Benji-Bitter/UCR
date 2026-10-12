//! Packet payloads.
//!
//! Two payload layers exist:
//!
//! * `Payload` — top-level, visible to every relay (routing metadata must be
//!   readable by intermediate nodes).
//! * `InnerPlain` — the plaintext of end-to-end encrypted traffic, only ever
//!   serialised inside an `AppData::Encrypted` ciphertext. Relay nodes cannot
//!   read it.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::channel::ChannelCategory;
use crate::header::Scope;
use crate::ids::{ChannelId, GroupId, MsgId, NodeId, PacketId};
use crate::station::{CoarsePos, StationClass};

/// Top-level packet payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Payload {
    /// Neighbour/identity announcement.
    Beacon(Beacon),
    /// Reactive route discovery (AODV-lite).
    RouteRequest(RouteRequest),
    RouteReply(RouteReply),
    /// Routed application traffic (channel broadcast or unicast).
    Data(DataMsg),
    /// Authenticated end-to-end delivery acknowledgement.
    Ack(Ack),
    /// Fetch a node's long-term public key.
    KeyRequest {
        target: NodeId,
    },
    /// Reply carrying the responder's long-term public key.
    KeyReply {
        pubkey: [u8; 32],
    },
    /// Session handshake for encrypted direct traffic.
    SessionInit(SessionInit),
    SessionAccept(SessionAccept),
    /// Signed emergency/distress traffic — top level so every relay can
    /// apply priority queueing without decrypting anything.
    Emergency(Emergency),
}

/// Neighbour announcement. Beacons are signed; the embedded `pubkey` lets any
/// receiver verify them immediately (`blake3(pubkey)[..16] == header.src`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Beacon {
    /// Sender's long-term Ed25519 public key — self-certifying vs `src`.
    pub pubkey: [u8; 32],
    /// Monotone beacon sequence (restarts allowed; freshness uses `seen_ms`).
    pub seq: u16,
    pub callsign: String,
    pub desc: String,
    pub class: StationClass,
    /// Capability bitflags (`caps` module).
    pub caps: u16,
    /// Radio profile id currently in use.
    pub profile: u8,
    /// Recently-heard neighbours — lets receivers detect bidirectional links.
    pub seen: Vec<NodeId>,
    /// Airtime busy fraction, 0..=255.
    pub congestion: u8,
    /// Battery percent, or 255 = unknown/mains.
    pub battery: u8,
    /// Optional coarse position. Never required.
    pub pos: Option<CoarsePos>,
    /// Built-in identification tone id, 0 = none.
    pub tone_id: u8,
}

/// Reactive route request, flooded toward `target` (bounded by `scope`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteRequest {
    /// Unique request id (dedup keyed by `(src, rreq_id)`).
    pub rreq_id: u32,
    /// Node we are trying to reach.
    pub target: NodeId,
    /// Accumulated path cost (scaled link metric, see ucr-routing).
    pub metric_acc: u32,
    /// Accumulated path, capped at `ROUTE_PATH_MAX` — loop prevention and
    /// route diagnostics.
    pub path: Vec<NodeId>,
}

/// Route reply, unicast back to the RREQ originator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteReply {
    /// Copied from the request.
    pub rreq_id: u32,
    /// The node this reply describes (the RREQ target).
    pub route_dest: NodeId,
    /// End-to-end accumulated metric from replier to originator's neighbour.
    pub metric_acc: u32,
    /// Hops traversed so far on the reply path.
    pub hop_count: u8,
}

/// Routed application traffic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataMsg {
    pub app: AppData,
}

/// Application payload inside a `Data` packet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AppData {
    /// Signed, unencrypted public channel text. `name` is carried so
    /// receivers can display channels they have not seen before without a
    /// directory lookup.
    ChannelText(ChannelText),
    /// Cleartext application traffic (profile info, digital Morse, tone
    /// announcements). Signed at packet level; not confidential.
    Plain(PlainApp),
    /// End-to-end encrypted direct traffic. Cleartext is `postcard(InnerPlain)`.
    Encrypted(Encrypted),
    /// Symmetric group-encrypted traffic. Cleartext is `postcard(InnerGroup)`.
    GroupEncrypted(GroupEncrypted),
}

/// Public channel message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelText {
    pub channel: ChannelId,
    /// Last-seen channel name — lets receivers resolve the id without a
    /// directory. The id is authoritative; names may collide.
    pub name: String,
    /// Channel category hint for directory display.
    pub category: ChannelCategory,
    /// Sender-declared scope copied from the channel config for receivers.
    pub scope: Scope,
    pub msg_id: MsgId,
    pub text: String,
}

/// Cleartext application payloads.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlainApp {
    /// Digital Morse message — symbols carried as data. This is NOT CW: it
    /// cannot be heard by a conventional receiver, it is Morse semantics over
    /// UCR packets.
    Morse { text: String, wpm: u8 },
    /// Compact identification tone pattern (see `TonePattern`).
    ToneAnnounce(TonePattern),
    /// Extended profile info, sent on request.
    ProfileInfo {
        callsign: String,
        desc: String,
        class: StationClass,
        caps: u16,
    },
}

/// E2E-encrypted blob. `session_id` identifies the negotiated session; the
/// nonce is random per message (ChaCha20-Poly1305, 12-byte nonces).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Encrypted {
    pub session_id: u32,
    pub nonce: [u8; 12],
    pub ct: Vec<u8>,
}

/// Symmetric-key group traffic. Relays see only group/epoch metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroupEncrypted {
    pub group_id: GroupId,
    /// Key epoch — incremented whenever membership changes.
    pub epoch: u32,
    pub nonce: [u8; 12],
    pub ct: Vec<u8>,
}

/// Plaintext of `AppData::Encrypted` — never sent unencrypted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum InnerPlain {
    DirectText {
        msg_id: MsgId,
        text: String,
    },
    /// Contact request — carries the requester's profile so the receiver can
    /// decide whether to accept.
    ContactRequest {
        callsign: String,
        desc: String,
        class: StationClass,
        text: String,
    },
    /// Group invitation — carries the current group symmetric key.
    GroupInvite {
        group_id: GroupId,
        name: String,
        desc: String,
        epoch: u32,
        key: [u8; 32],
    },
    /// Response to a group invitation.
    GroupInviteReply {
        group_id: GroupId,
        accepted: bool,
    },
    /// New key epoch after membership change.
    GroupKeyUpdate {
        group_id: GroupId,
        epoch: u32,
        key: [u8; 32],
    },
    /// Member departure notice / removal notice.
    GroupLeave {
        group_id: GroupId,
        /// If `Some`, the leaving/removing node — otherwise the sender itself.
        member: Option<NodeId>,
    },
}

/// Plaintext of `AppData::GroupEncrypted`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum InnerGroup {
    Text {
        msg_id: MsgId,
        text: String,
    },
    /// Member list snapshot (admin → members), lets late joiners learn state.
    Roster {
        members: Vec<NodeId>,
        epoch: u32,
    },
}

/// Authenticated delivery acknowledgement, signed by the *destination*.
///
/// A UCR acknowledgement confirms that the destination node's protocol stack
/// received the packet — nothing more. It must never be presented as proof
/// of human receipt or of delivery to any external service.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ack {
    /// Packet being acknowledged (keyed with `src` of the original packet —
    /// i.e. this Ack's `dst`).
    pub acked_id: PacketId,
    /// Hops the acknowledged packet took (copied from its header).
    pub hops_taken: u8,
}

/// Session handshake — initiator → responder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionInit {
    /// Initiator's long-term signing pubkey (self-certifying vs `src`).
    pub pubkey: [u8; 32],
    /// Initiator's ephemeral X25519 public key for this session.
    pub eph_pub: [u8; 32],
    /// Initiator timestamp (ms since epoch) — replay-window checked.
    pub timestamp_ms: u64,
}

/// Session handshake — responder → initiator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionAccept {
    /// Responder's long-term pubkey.
    pub pubkey: [u8; 32],
    /// Responder's ephemeral X25519 public key.
    pub eph_pub: [u8; 32],
    /// Derived session id the responder will recognise.
    pub session_id: u32,
}

/// Emergency/distress traffic.
///
/// `pubkey` is embedded so receivers and relays can authenticate the packet
/// immediately. Priority handling is a *queue-ordering* property only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Emergency {
    /// Sender's long-term pubkey — self-certifying vs `header.src`.
    pub pubkey: [u8; 32],
    pub category: EmergencyCategory,
    /// Unique id for duplicate suppression (sender-chosen).
    pub emergency_id: u64,
    pub text: String,
    /// Optional coarse position.
    pub pos: Option<CoarsePos>,
    /// Originator timestamp.
    pub timestamp_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmergencyCategory {
    Medical,
    Fire,
    Flood,
    Accident,
    VesselDistress,
    PersonMissing,
    Other,
}

impl EmergencyCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Medical => "Medical",
            Self::Fire => "Fire",
            Self::Flood => "Flood",
            Self::Accident => "Accident",
            Self::VesselDistress => "Vessel distress",
            Self::PersonMissing => "Person missing",
            Self::Other => "Other",
        }
    }
}

/// Bandwidth-efficient tone description. A tone is a short ordered note list;
/// receivers render it locally. Audio files are never placed in packets —
/// custom sounds are exchanged once (or imported locally) and referenced by
/// `TonePattern::id` afterwards.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TonePattern {
    /// Local id of this pattern in the sender's table (0..=31 built-in ids
    /// are reserved; custom patterns use 32..=255).
    pub id: u8,
    pub name: String,
    /// Ordered notes; silence = freq 0.
    pub notes: Vec<ToneNote>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToneNote {
    /// Hz; 0 = rest.
    pub freq_hz: u16,
    /// Duration in 10 ms units (1..=255 → 10 ms..2.55 s).
    pub dur_10ms: u8,
}

/// A few built-in identification tones.
pub fn builtin_tones() -> alloc::vec::Vec<TonePattern> {
    use alloc::string::ToString;
    let mut t = alloc::vec::Vec::new();
    let mk = |id: u8, name: &str, notes: &[(u16, u8)]| TonePattern {
        id,
        name: name.to_string(),
        notes: notes
            .iter()
            .map(|&(f, d)| ToneNote {
                freq_hz: f,
                dur_10ms: d,
            })
            .collect(),
    };
    t.push(mk(1, "Beep", &[(880, 8)]));
    t.push(mk(2, "Double beep", &[(880, 6), (0, 4), (880, 6)]));
    t.push(mk(3, "Rise", &[(660, 6), (880, 6), (1100, 8)]));
    t.push(mk(4, "Fall", &[(1100, 6), (880, 6), (660, 8)]));
    t.push(mk(5, "K (over)", &[(1046, 5), (0, 3), (1568, 12)]));
    t
}
