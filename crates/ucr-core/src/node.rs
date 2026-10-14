//! The node runtime — see module docs for the composed behaviour.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rand::Rng;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use ucr_crypto::SessionManager;
use ucr_identity::{check_signature, Identity, SignatureStatus};
use ucr_protocol::channel::ChannelCategory;
use ucr_protocol::header::{Priority, Scope};
use ucr_protocol::ids::{ChannelId, GroupId, MsgId, NodeId};
use ucr_protocol::payload::*;
use ucr_protocol::profile::{profile_by_id, Modulation};
use ucr_protocol::station::{caps, CoarsePos, StationClass};
use ucr_protocol::{decode, ChannelInfo, Packet, TonePattern};
use ucr_routing::{DropReason, RouteAction, Router};
use ucr_storage::{
    Contact, ContactSource, DeliveryState, Direction, GroupMember, Settings, StorageError, Store,
    StoredGroup, StoredMessage,
};
use ucr_transport::{RxMeta, Transport};

use crate::events::NodeEvent;
use crate::now_ms;

/// Runtime configuration for a node.
#[derive(Clone, Debug)]
pub struct NodeConfig {
    pub callsign: String,
    pub desc: String,
    pub class: StationClass,
    /// Identification tone id (0 = none).
    pub tone_id: u8,
    /// Offer to relay for others.
    pub relay: bool,
    /// Offer to relay emergency traffic.
    pub emergency_relay: bool,
    /// Optional coarse position. GPS is never required — None is fully valid.
    pub pos: Option<CoarsePos>,
    /// Announce `pos` in beacons. The GPS kill-switch: off keeps the fix
    /// locally but the node goes position-silent — every product with a
    /// GPS exposes this as a toggle. Persists via Settings.
    pub share_position: bool,
    /// Battery percent (255 = unknown/mains).
    pub battery: u8,
    /// Beacon base interval (adaptive around this).
    pub beacon_base_ms: u64,
    /// Maintenance tick.
    pub maintenance_ms: u64,
    /// Outbound queue TTL.
    pub queue_ttl_ms: u64,
    /// Minimum spacing between origin emergencies.
    pub emergency_cooldown_ms: u64,
    /// Max unicast resend attempts while waiting for an ack.
    pub ack_retries: u8,
    /// Ack wait deadline per attempt.
    pub ack_timeout_ms: u64,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            callsign: String::new(),
            desc: String::new(),
            class: StationClass::Individual,
            tone_id: 1,
            relay: true,
            emergency_relay: true,
            pos: None,
            share_position: true,
            battery: 255,
            beacon_base_ms: 60_000,
            maintenance_ms: 300,
            queue_ttl_ms: 2 * 60 * 60 * 1000,
            emergency_cooldown_ms: 60_000,
            ack_retries: 3,
            ack_timeout_ms: 15_000,
        }
    }
}

/// Rollup node statistics for diagnostics.
#[derive(Clone, Debug)]
pub struct NodeStats {
    pub router: ucr_routing::RouterStats,
    pub neighbors: usize,
    pub routes: usize,
    pub queue_len: usize,
    pub sessions: usize,
    pub congestion: u8,
    pub pending_acks: usize,
    pub uptime_ms: u64,
}

struct PendingAck {
    msg_id: Option<MsgId>,
    peer: NodeId,
    next_hop: NodeId,
    channel: Option<ChannelId>,
    deadline_ms: u64,
    retries_left: u8,
    /// The transmitted packet — re-sent with a fresh packet id on ack timeout
    /// (payload is immutable, so resending identical ciphertext is safe).
    packet: Packet,
}

struct PendingE2e {
    inner: InnerPlain,
    msg_id: MsgId,
}

/// One received emergency broadcast kept in local history.
#[derive(Clone, Debug)]
pub struct EmergRecord {
    pub from: NodeId,
    pub emergency_id: u64,
    pub at_ms: u64,
}

struct NodeInner {
    identity: Identity,
    transport: Arc<dyn Transport>,
    router: Mutex<Router>,
    sessions: Mutex<SessionManager>,
    store: Store,
    cfg: Mutex<NodeConfig>,
    events: broadcast::Sender<NodeEvent>,
    started_at: u64,
    msg_seq: AtomicU64,
    pending_acks: Mutex<HashMap<u32, PendingAck>>,
    pending_e2e: Mutex<HashMap<NodeId, VecDeque<PendingE2e>>>,
    /// dest → deadline: suppress repeat RREQ while one is in flight.
    rreq_inflight: Mutex<HashMap<NodeId, u64>>,
    /// Per-src emergency relay/event rate limiting.
    emergency_last_event: Mutex<HashMap<NodeId, u64>>,
    emergency_last_sent: Mutex<u64>,
    emergency_log: Mutex<Vec<EmergRecord>>,
    /// app-level message dedup: (src, msg_id)
    seen_msgs: Mutex<HashSet<(NodeId, MsgId)>>,
    /// Voice frame dedup — (src, tx_id, seq); bounded and cleared.
    seen_voice: Mutex<HashSet<(NodeId, u16, u16)>>,
    /// known custom tones per node
    tones: Mutex<HashMap<NodeId, TonePattern>>,
    /// Serialised outbound wire frames — preserves packet order on the medium.
    tx_pump: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    /// Digital lock: when set, all user-originated sends are refused.
    /// Reception, relaying, and beacons continue — a locked radio still
    /// hears and still carries traffic for others. Persists via the PIN
    /// hash in Settings; the node boots locked until the PIN unlocks it.
    locked: AtomicBool,
    /// last contact-request texts seen (for accept/decline bookkeeping)
    shutdown: tokio::sync::Notify,
}

/// Blake3 hash of a digital-lock PIN — the stored form. Domain-tagged so
/// a lock hash can't be confused with any other hash in the store.
fn lock_hash(pin: &str) -> String {
    blake3::hash(format!("ucr-lock:{pin}").as_bytes())
        .to_hex()
        .to_string()
}

/// A running UCR node. Clone-cheap handle — all state is shared.
#[derive(Clone)]
pub struct Node {
    inner: Arc<NodeInner>,
}

/// Builder for a `Node`.
pub struct NodeBuilder {
    cfg: NodeConfig,
    store: Option<Store>,
    pid_seed: Option<u32>,
}

impl NodeBuilder {
    pub fn new() -> Self {
        Self {
            cfg: NodeConfig::default(),
            store: None,
            pid_seed: None,
        }
    }

    pub fn config(mut self, cfg: NodeConfig) -> Self {
        self.cfg = cfg;
        self
    }

    pub fn store(mut self, store: Store) -> Self {
        self.store = Some(store);
        self
    }

    /// Fixed packet-id seed — deterministic sim runs.
    pub fn pid_seed(mut self, seed: u32) -> Self {
        self.pid_seed = Some(seed);
        self
    }

    /// Build + start the node on the given transport. Loads identity,
    /// settings and channels from the store (or uses in-memory storage).
    pub async fn start(self, transport: Arc<dyn Transport>) -> anyhow::Result<Node> {
        let store = match self.store {
            Some(s) => s,
            None => Store::in_memory()?,
        };
        let identity = store.identity()?;
        let stored = store.settings().unwrap_or_default();
        let have_stored_profile = !stored.callsign.is_empty();
        let mut cfg = self.cfg;
        if cfg.callsign.is_empty() {
            cfg.callsign = stored.callsign;
        }
        if cfg.desc.is_empty() {
            cfg.desc = stored.desc;
        }
        if have_stored_profile {
            cfg.class = stored.class;
            cfg.tone_id = stored.tone_id;
            cfg.relay = stored.relay;
            // An explicit pos passed this run is intent to announce; only
            // restore the stored toggle when no pos was given this boot.
            if cfg.pos.is_none() {
                cfg.share_position = stored.share_position;
            }
        }
        let pid_seed = match self.pid_seed {
            Some(s) => s,
            None => store.next_pid_seed().unwrap_or(0),
        };
        let (events, _) = broadcast::channel(1024);
        let (tx_pump, tx_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let now = now_ms();
        let me = identity.node_id();
        let t_for_pump = transport.clone();
        tokio::spawn(async move {
            let mut rx = tx_rx;
            while let Some(frame) = rx.recv().await {
                if let Err(e) = t_for_pump.send(&frame).await {
                    warn!("transport send failed: {e}");
                }
            }
        });
        let node = Node {
            inner: Arc::new(NodeInner {
                identity,
                transport,
                router: Mutex::new(Router::new(me, pid_seed)),
                sessions: Mutex::new(SessionManager::new(me)),
                store,
                cfg: Mutex::new(cfg),
                events,
                started_at: now,
                msg_seq: AtomicU64::new(rand::thread_rng().gen::<u32>() as u64),
                pending_acks: Mutex::new(HashMap::new()),
                pending_e2e: Mutex::new(HashMap::new()),
                rreq_inflight: Mutex::new(HashMap::new()),
                emergency_last_event: Mutex::new(HashMap::new()),
                emergency_last_sent: Mutex::new(0),
                emergency_log: Mutex::new(Vec::new()),
                seen_msgs: Mutex::new(HashSet::new()),
                seen_voice: Mutex::new(HashSet::new()),
                tones: Mutex::new(HashMap::new()),
                tx_pump,
                // A stored PIN hash means this node locks itself at boot —
                // the radio wakes up locked until unlocked, like a real
                // keypad-lock device.
                locked: AtomicBool::new(stored.lock_pin_hash.is_some()),
                shutdown: tokio::sync::Notify::new(),
            }),
        };
        // Seed the default channel directory on first run.
        if node.inner.store.channels()?.is_empty() {
            for ch in ucr_protocol::channel::default_channels() {
                let _ = node.inner.store.upsert_channel(&ch);
            }
        }
        node.spawn_rx_task();
        node.spawn_beacon_task();
        node.spawn_maintenance_task();
        info!(node = %me, "ucr node started");
        Ok(node)
    }
}

impl Default for NodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// =====================================================================
// Public API
// =====================================================================

impl Node {
    pub fn node_id(&self) -> NodeId {
        self.inner.identity.node_id()
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.inner.identity.public_key()
    }

    pub fn callsign(&self) -> String {
        self.inner.cfg.lock().callsign.clone()
    }

    pub fn config(&self) -> NodeConfig {
        self.inner.cfg.lock().clone()
    }

    /// Update identity/profile fields; the next beacon advertises them.
    pub fn set_profile(&self, callsign: &str, desc: &str, class: StationClass, tone_id: u8) {
        let mut cfg = self.inner.cfg.lock();
        cfg.callsign = callsign.chars().take(ucr_protocol::CALLSIGN_MAX).collect();
        cfg.desc = desc.chars().take(ucr_protocol::DESC_MAX).collect();
        cfg.class = class;
        cfg.tone_id = tone_id;
        let s = Settings {
            callsign: cfg.callsign.clone(),
            desc: cfg.desc.clone(),
            class: cfg.class,
            tone_id: cfg.tone_id,
            relay: cfg.relay,
            ..self.inner.store.settings().unwrap_or_default()
        };
        let _ = self.inner.store.save_settings(&s);
    }

    pub fn set_relay(&self, relay: bool) {
        self.inner.cfg.lock().relay = relay;
    }

    pub fn set_position(&self, pos: Option<CoarsePos>) {
        self.inner.cfg.lock().pos = pos;
    }

    /// GPS kill-switch: toggle whether the next beacons announce the
    /// coarse position. Off keeps the fix in cfg — the node just stops
    /// broadcasting it. Persists so a privacy-off unit stays off.
    pub fn set_share_position(&self, share: bool) {
        self.inner.cfg.lock().share_position = share;
        if let Ok(mut s) = self.inner.store.settings() {
            s.share_position = share;
            let _ = self.inner.store.save_settings(&s);
        }
    }

    pub fn share_position(&self) -> bool {
        self.inner.cfg.lock().share_position
    }

    /// The locally-held coarse fix (kept even when sharing is off).
    pub fn pos(&self) -> Option<CoarsePos> {
        self.inner.cfg.lock().pos
    }

    /// Subscribe to node events.
    pub fn events(&self) -> broadcast::Receiver<NodeEvent> {
        self.inner.events.subscribe()
    }

    fn emit(&self, ev: NodeEvent) {
        let _ = self.inner.events.send(ev);
    }

    // ---- queries ------------------------------------------------------

    pub fn neighbors(&self) -> Vec<ucr_routing::Neighbor> {
        self.inner.router.lock().neighbors.iter().cloned().collect()
    }

    pub fn routes(&self) -> Vec<ucr_routing::RouteEntry> {
        self.inner.router.lock().routes.iter().cloned().collect()
    }

    pub fn stats(&self) -> NodeStats {
        // Never hold one lock while acquiring another — lock-order
        // inversions between stats/send/rx paths have deadlocked before.
        let (router, neighbors, routes, queue_len) = {
            let r = self.inner.router.lock();
            (r.stats, r.neighbors.len(), r.routes.len(), r.queue.len())
        };
        NodeStats {
            router,
            neighbors,
            routes,
            queue_len,
            sessions: self.inner.sessions.lock().sessions().count(),
            congestion: self.inner.transport.congestion(),
            pending_acks: self.inner.pending_acks.lock().len(),
            uptime_ms: now_ms() - self.inner.started_at,
        }
    }

    pub fn channels(&self) -> Vec<ChannelInfo> {
        self.inner.store.channels().unwrap_or_default()
    }

    pub fn contacts(&self) -> Vec<Contact> {
        self.inner.store.contacts().unwrap_or_default()
    }

    // ---- channels -------------------------------------------------------

    /// Join (or create) a public channel by name. Returns its id.
    pub fn join_channel(&self, name: &str, category: ChannelCategory, scope: Scope) -> ChannelId {
        let mut ch = ChannelInfo::new(name, category, scope);
        ch.user_created = true;
        let _ = self.inner.store.upsert_channel(&ch);
        ch.id
    }

    // ---- digital lock ------------------------------------------------------

    /// PIN-gated transmission — the "keypad lock" of a real handheld.
    /// While locked, every user-originated send fails with
    /// `SendError::Locked`; receiving, relaying for others, and beacons
    /// continue unaffected. The PIN's blake3 hash persists in settings —
    /// never the PIN — so a locked node stays locked across restarts.
    pub fn lock(&self, pin: &str) -> Result<(), SendError> {
        if pin.chars().count() < 4 {
            return Err(SendError::Invalid("pin needs at least 4 characters"));
        }
        let mut s = self.inner.store.settings()?;
        s.lock_pin_hash = Some(lock_hash(pin));
        self.inner.store.save_settings(&s)?;
        self.inner.locked.store(true, Ordering::Relaxed);
        self.emit(NodeEvent::LockChanged { locked: true });
        Ok(())
    }

    /// Attempt to unlock. Wrong PIN leaves the node locked (returns false).
    pub fn unlock(&self, pin: &str) -> bool {
        let ok = self
            .inner
            .store
            .settings()
            .ok()
            .and_then(|s| s.lock_pin_hash)
            .map(|h| h == lock_hash(pin))
            .unwrap_or(false);
        if ok {
            self.inner.locked.store(false, Ordering::Relaxed);
            self.emit(NodeEvent::LockChanged { locked: false });
        }
        ok
    }

    /// Remove the lock entirely. Requires the PIN — otherwise the lock
    /// could be stripped off by whoever found the radio.
    pub fn clear_lock(&self, pin: &str) -> bool {
        if !self.unlock(pin) {
            return false;
        }
        if let Ok(mut s) = self.inner.store.settings() {
            s.lock_pin_hash = None;
            let _ = self.inner.store.save_settings(&s);
        }
        true
    }

    pub fn is_locked(&self) -> bool {
        self.inner.locked.load(Ordering::Relaxed)
    }

    fn check_unlocked(&self) -> Result<(), SendError> {
        if self.is_locked() {
            return Err(SendError::Locked);
        }
        Ok(())
    }

    /// Send a text message to a channel. Returns the first msg id.
    /// Channel traffic is a signed, scoped flood — readable by anyone in
    /// range of any relay; never encrypted.
    ///
    /// Text longer than fits one wire frame on the governing link is split
    /// across multiple packets (APRS-style chunking): each chunk becomes its
    /// own flood packet and history line, since UCR never reassembles.
    pub fn send_channel_text(&self, channel: &ChannelId, text: &str) -> Result<MsgId, SendError> {
        self.check_unlocked()?;
        let ch = self
            .channels()
            .into_iter()
            .find(|c| &c.id == channel)
            .ok_or(SendError::UnknownChannel)?;
        // Payload budget = mtu minus real wire overhead (header + signature
        // + appdata meta), measured by encoding a probe frame — on LoRa a
        // signed packet carries ~170B of overhead, leaving ~50B of text.
        let probe = {
            let mut r = self.inner.router.lock();
            r.originate_broadcast(
                ch.scope,
                Payload::Data(DataMsg {
                    app: AppData::ChannelText(ChannelText {
                        channel: ch.id,
                        name: ch.name.clone(),
                        category: ch.category,
                        scope: ch.scope,
                        msg_id: 0,
                        text: String::new(),
                    }),
                }),
                Priority::Normal,
            )
        };
        let mtu = self.inner.transport.mtu();
        let signer = self.inner.identity.packet_signer();
        let budget = mtu
            .saturating_sub(probe.encode(Some(&signer)).map(|w| w.len()).unwrap_or(0) + 8)
            .min(ucr_protocol::TEXT_MAX);
        let text: &str = {
            let mut end = text.len().min(4096);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            &text[..end]
        };
        let mut chunks: Vec<String> = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            let mut n = rest.len().min(budget);
            while !rest.is_char_boundary(n) {
                n -= 1;
            }
            chunks.push(rest[..n].to_string());
            rest = &rest[n..];
        }
        let first_id = self.next_msg_id();
        let now = now_ms();
        for (i, chunk) in chunks.iter().enumerate() {
            let msg_id = if i == 0 { first_id } else { self.next_msg_id() };
            let pkt = {
                let mut r = self.inner.router.lock();
                r.originate_broadcast(
                    ch.scope,
                    Payload::Data(DataMsg {
                        app: AppData::ChannelText(ChannelText {
                            channel: ch.id,
                            name: ch.name.clone(),
                            category: ch.category,
                            scope: ch.scope,
                            msg_id,
                            text: chunk.clone(),
                        }),
                    }),
                    Priority::Normal,
                )
            };
            self.transmit_signed(&pkt)?;
            self.record_outbound(None, None, &pkt, Some(msg_id));
        }
        // Our own message appears in history immediately (as one line).
        let _ = self.inner.store.put_message(&StoredMessage {
            msg_id: first_id,
            peer: self.node_id(),
            channel: Some(*channel),
            direction: Direction::Outbound,
            text: text.into(),
            at_ms: now,
            delivery: DeliveryState::Sent,
        });
        Ok(first_id)
    }

    /// Push-to-talk voice: flood one encoded frame to a channel. Real-time
    /// traffic — signed so senders are authenticatable, flooded so every
    /// listener hears it, and deliberately unacked/unstored (voice loss is
    /// skipped, never replayed). `tx_id` groups a press, `seq` orders it,
    /// `last` ends it.
    pub fn send_channel_voice(
        &self,
        channel: &ChannelId,
        codec: u8,
        tx_id: u16,
        seq: u16,
        last: bool,
        data: &[u8],
    ) -> Result<MsgId, SendError> {
        self.check_unlocked()?;
        let ch = self
            .channels()
            .into_iter()
            .find(|c| &c.id == channel)
            .ok_or(SendError::UnknownChannel)?;
        // Voice fans out whole to every link — measure real wire overhead
        // with an empty-frame probe and refuse what the air can't carry.
        let probe = {
            let mut r = self.inner.router.lock();
            r.originate_broadcast(
                ch.scope,
                Payload::Data(DataMsg {
                    app: AppData::ChannelVoice(ChannelVoice {
                        channel: ch.id,
                        name: ch.name.clone(),
                        scope: ch.scope,
                        codec,
                        tx_id,
                        seq,
                        last,
                        data: Vec::new(),
                    }),
                }),
                Priority::Control,
            )
        };
        let mtu = self.inner.transport.mtu();
        let signer = self.inner.identity.packet_signer();
        let budget =
            mtu.saturating_sub(probe.encode(Some(&signer)).map(|w| w.len()).unwrap_or(0) + 8);
        if data.len() > budget {
            return Err(SendError::TooLarge {
                size: data.len(),
                mtu: budget,
            });
        }
        let msg_id = self.next_msg_id();
        let pkt = {
            let mut r = self.inner.router.lock();
            r.originate_broadcast(
                ch.scope,
                Payload::Data(DataMsg {
                    app: AppData::ChannelVoice(ChannelVoice {
                        channel: ch.id,
                        name: ch.name.clone(),
                        scope: ch.scope,
                        codec,
                        tx_id,
                        seq,
                        last,
                        data: data.to_vec(),
                    }),
                }),
                Priority::Control,
            )
        };
        self.transmit_signed(&pkt)?;
        self.record_outbound(None, None, &pkt, Some(msg_id));
        Ok(msg_id)
    }

    // ---- direct messaging -------------------------------------------------

    /// Send an E2E-encrypted direct text. Establishes a session if needed;
    /// the message is queued behind the handshake automatically.
    pub fn send_dm(&self, peer: NodeId, text: &str) -> Result<MsgId, SendError> {
        self.check_unlocked()?;
        if self.inner.store.is_blocked(&peer)? {
            return Err(SendError::Blocked);
        }
        let msg_id = self.next_msg_id();
        let inner = InnerPlain::DirectText {
            msg_id,
            text: text.chars().take(ucr_protocol::TEXT_MAX).collect(),
        };
        self.queue_or_send_e2e(peer, inner, msg_id, true)?;
        let _ = self.inner.store.put_message(&StoredMessage {
            msg_id,
            peer,
            channel: None,
            direction: Direction::Outbound,
            text: text.into(),
            at_ms: now_ms(),
            delivery: DeliveryState::Queued,
        });
        Ok(msg_id)
    }

    /// Send a contact request (E2E, carries our profile).
    pub fn send_contact_request(&self, peer: NodeId, text: &str) -> Result<MsgId, SendError> {
        self.check_unlocked()?;
        let cfg = self.inner.cfg.lock().clone();
        let msg_id = self.next_msg_id();
        let inner = InnerPlain::ContactRequest {
            callsign: cfg.callsign.clone(),
            desc: cfg.desc.clone(),
            class: cfg.class,
            text: text.into(),
        };
        self.queue_or_send_e2e(peer, inner, msg_id, true)?;
        Ok(msg_id)
    }

    /// Accept a contact — mark verified + trusted-pending contact record.
    pub fn accept_contact(&self, peer: NodeId) -> Result<(), SendError> {
        let mut c = self.contact_or_new(peer);
        c.source = ContactSource::ContactRequest;
        c.blocked = false;
        self.inner.store.upsert_contact(&c)?;
        Ok(())
    }

    pub fn contact_or_new(&self, peer: NodeId) -> Contact {
        self.inner
            .store
            .contact(&peer)
            .ok()
            .flatten()
            .unwrap_or_else(|| Contact {
                node_id: peer,
                callsign: String::new(),
                desc: String::new(),
                class: StationClass::Individual,
                pubkey: None,
                verified: false,
                blocked: false,
                muted_until_ms: None,
                favourite: false,
                trusted: false,
                first_seen_ms: now_ms(),
                last_seen_ms: now_ms(),
                source: ContactSource::OverTheAir,
            })
    }

    /// Import a contact manually (e.g. exchanged identity card offline).
    pub fn import_contact(
        &self,
        node_id: NodeId,
        callsign: &str,
        pubkey: Option<[u8; 32]>,
    ) -> Result<(), SendError> {
        if let Some(pk) = pubkey {
            if ucr_identity::check_binding(&pk, &node_id).is_err() {
                return Err(SendError::Invalid("pubkey does not match node id"));
            }
            self.inner.store.learn_key(&node_id, &pk)?;
        }
        let mut c = self.contact_or_new(node_id);
        c.callsign = callsign.into();
        c.source = ContactSource::ManualImport;
        c.pubkey = pubkey.or(c.pubkey);
        c.verified = pubkey.is_some();
        self.inner.store.upsert_contact(&c)?;
        Ok(())
    }

    // ---- user controls ------------------------------------------------------

    /// Block an identity (NodeId-level — callsign changes cannot evade it).
    /// Blocking affects LOCAL delivery only; this node still relays the
    /// blocked station's packets as a mesh participant.
    pub fn block(&self, peer: NodeId) -> Result<(), SendError> {
        let mut c = self.contact_or_new(peer);
        c.blocked = true;
        self.inner.store.upsert_contact(&c)?;
        self.inner.sessions.lock().drop_session(&peer);
        Ok(())
    }

    pub fn unblock(&self, peer: NodeId) -> Result<(), SendError> {
        let mut c = self.contact_or_new(peer);
        c.blocked = false;
        self.inner.store.upsert_contact(&c)?;
        Ok(())
    }

    pub fn mute(&self, peer: NodeId, until_ms: Option<u64>) -> Result<(), SendError> {
        let mut c = self.contact_or_new(peer);
        c.muted_until_ms = Some(until_ms.unwrap_or(u64::MAX));
        self.inner.store.upsert_contact(&c)?;
        Ok(())
    }

    pub fn unmute(&self, peer: NodeId) -> Result<(), SendError> {
        let mut c = self.contact_or_new(peer);
        c.muted_until_ms = None;
        self.inner.store.upsert_contact(&c)?;
        Ok(())
    }

    pub fn favourite(&self, peer: NodeId, fav: bool) -> Result<(), SendError> {
        let mut c = self.contact_or_new(peer);
        c.favourite = fav;
        self.inner.store.upsert_contact(&c)?;
        Ok(())
    }

    /// Explicit user-verified trust (e.g. fingerprint comparison offline).
    pub fn trust(&self, peer: NodeId, trusted: bool) -> Result<(), SendError> {
        let mut c = self.contact_or_new(peer);
        c.trusted = trusted;
        self.inner.store.upsert_contact(&c)?;
        Ok(())
    }

    // ---- groups --------------------------------------------------------------

    /// Create a private group. The creator is the first admin.
    pub fn create_group(&self, name: &str, desc: &str) -> Result<GroupId, SendError> {
        self.check_unlocked()?;
        let mut key_src = Vec::new();
        key_src.extend_from_slice(b"ucr-group");
        key_src.extend_from_slice(&self.node_id().0);
        key_src.extend_from_slice(&rand::thread_rng().gen::<u64>().to_be_bytes());
        let h = blake3::hash(&key_src);
        let mut gid = [0u8; 8];
        gid.copy_from_slice(&h.as_bytes()[..8]);
        let group_id = GroupId(gid);
        let mut key = [0u8; 32];
        rand::thread_rng().fill(&mut key);
        let g = StoredGroup {
            group_id,
            name: name.chars().take(ucr_protocol::GROUP_NAME_MAX).collect(),
            desc: desc.into(),
            creator: self.node_id(),
            epoch: 0,
            key,
            members: vec![GroupMember {
                node_id: self.node_id(),
                callsign: self.callsign(),
                admin: true,
                joined_ms: now_ms(),
            }],
            muted_until_ms: None,
        };
        self.inner.store.upsert_group(&g)?;
        self.emit(NodeEvent::GroupRoster {
            group_id,
            members: vec![self.node_id()],
        });
        Ok(group_id)
    }

    /// Invite a peer (admin only) — the key travels inside the E2E session.
    pub fn invite_to_group(&self, group_id: GroupId, peer: NodeId) -> Result<(), SendError> {
        self.check_unlocked()?;
        let g = self.must_group(&group_id)?;
        self.must_admin(&g)?;
        let msg_id = self.next_msg_id();
        self.queue_or_send_e2e(
            peer,
            InnerPlain::GroupInvite {
                group_id,
                name: g.name.clone(),
                desc: g.desc.clone(),
                epoch: g.epoch,
                key: g.key,
            },
            msg_id,
            false,
        )?;
        Ok(())
    }

    pub fn accept_group(&self, group_id: GroupId) -> Result<(), SendError> {
        self.check_unlocked()?;
        let g = self.must_group(&group_id)?;
        let msg_id = self.next_msg_id();
        self.queue_or_send_e2e(
            g.creator,
            InnerPlain::GroupInviteReply {
                group_id,
                accepted: true,
            },
            msg_id,
            false,
        )?;
        self.emit(NodeEvent::GroupRoster {
            group_id,
            members: g.members.iter().map(|m| m.node_id).collect(),
        });
        Ok(())
    }

    pub fn decline_group(&self, group_id: GroupId) -> Result<(), SendError> {
        self.check_unlocked()?;
        let g = self.must_group(&group_id)?;
        let msg_id = self.next_msg_id();
        self.queue_or_send_e2e(
            g.creator,
            InnerPlain::GroupInviteReply {
                group_id,
                accepted: false,
            },
            msg_id,
            false,
        )?;
        self.inner.store.delete_group(&group_id)?;
        Ok(())
    }

    /// Send an encrypted group message — flooded within the group's scope
    /// (`Regional`), ciphertext readable only by key-holding members.
    pub fn send_group_text(&self, group_id: GroupId, text: &str) -> Result<MsgId, SendError> {
        self.check_unlocked()?;
        let g = self.must_group(&group_id)?;
        let msg_id = self.next_msg_id();
        let inner = InnerGroup::Text {
            msg_id,
            text: text.chars().take(ucr_protocol::TEXT_MAX).collect(),
        };
        let pt = postcard::to_allocvec(&inner).map_err(|_| SendError::Codec)?;
        let (nonce, ct) =
            ucr_crypto::aead_encrypt(&g.key, &ucr_crypto::group_aad(&group_id, g.epoch), &pt);
        let pkt = {
            let mut r = self.inner.router.lock();
            r.originate_broadcast(
                Scope::Regional,
                Payload::Data(DataMsg {
                    app: AppData::GroupEncrypted(GroupEncrypted {
                        group_id,
                        epoch: g.epoch,
                        nonce,
                        ct,
                    }),
                }),
                Priority::Normal,
            )
        };
        self.transmit_signed(&pkt)?;
        let _ = self.inner.store.put_message(&StoredMessage {
            msg_id,
            peer: self.node_id(),
            channel: None,
            direction: Direction::Outbound,
            text: text.into(),
            at_ms: now_ms(),
            delivery: DeliveryState::Sent,
        });
        Ok(msg_id)
    }

    /// Remove a member (admin): epoch bumps, a fresh key goes to everyone
    /// EXCEPT the removed member, who keeps only pre-removal readability.
    pub fn remove_member(&self, group_id: GroupId, peer: NodeId) -> Result<(), SendError> {
        let mut g = self.must_group(&group_id)?;
        self.must_admin(&g)?;
        g.members.retain(|m| m.node_id != peer);
        g.epoch += 1;
        rand::thread_rng().fill(&mut g.key);
        let remaining: Vec<NodeId> = g.members.iter().map(|m| m.node_id).collect();
        for m in &remaining {
            if *m == self.node_id() {
                continue;
            }
            let _ = self.queue_or_send_e2e(
                *m,
                InnerPlain::GroupKeyUpdate {
                    group_id,
                    epoch: g.epoch,
                    key: g.key,
                },
                self.next_msg_id(),
                false,
            );
        }
        // Tell the removed member they're out (best-effort, via session).
        let _ = self.queue_or_send_e2e(
            peer,
            InnerPlain::GroupLeave {
                group_id,
                member: Some(peer),
            },
            self.next_msg_id(),
            false,
        );
        self.inner.store.upsert_group(&g)?;
        self.emit(NodeEvent::GroupEpoch {
            group_id,
            epoch: g.epoch,
        });
        self.emit(NodeEvent::GroupRoster {
            group_id,
            members: remaining,
        });
        Ok(())
    }

    pub fn leave_group(&self, group_id: GroupId) -> Result<(), SendError> {
        let g = self.must_group(&group_id)?;
        let _ = self.queue_or_send_e2e(
            g.creator,
            InnerPlain::GroupLeave {
                group_id,
                member: None,
            },
            self.next_msg_id(),
            false,
        );
        self.inner.store.delete_group(&group_id)?;
        self.emit(NodeEvent::GroupRoster {
            group_id,
            members: vec![],
        });
        Ok(())
    }

    pub fn groups(&self) -> Vec<StoredGroup> {
        self.inner.store.groups().unwrap_or_default()
    }

    // ---- emergency -------------------------------------------------------------

    /// Originate a signed distress message, flooded at `Regional` scope with
    /// `Emergency` priority. Rate-limited per spec — this bounds airtime
    /// abuse; relays apply their own per-source budgets too.
    pub fn send_emergency(
        &self,
        category: EmergencyCategory,
        text: &str,
        pos: Option<CoarsePos>,
    ) -> Result<u64, SendError> {
        // Deliberately NOT lock-gated: emergency is the SOS button on a
        // locked phone — found or borrowed hardware must still be able to
        // call for help. Everything else respects the digital lock.
        let now = now_ms();
        let cooldown = self.inner.cfg.lock().emergency_cooldown_ms;
        {
            let last = self.inner.emergency_last_sent.lock();
            if now - *last < cooldown {
                return Err(SendError::RateLimited(cooldown - (now - *last)));
            }
        }
        let emergency_id = rand::thread_rng().gen::<u64>();
        let pkt = {
            let mut r = self.inner.router.lock();
            r.originate_broadcast(
                Scope::Regional,
                Payload::Emergency(Emergency {
                    pubkey: self.public_key(),
                    category,
                    emergency_id,
                    text: text.chars().take(ucr_protocol::TEXT_MAX).collect(),
                    pos,
                    timestamp_ms: now,
                }),
                Priority::Emergency,
            )
        };
        self.transmit_signed(&pkt)?;
        *self.inner.emergency_last_sent.lock() = now;
        self.emit(NodeEvent::Delivery {
            packet_id: pkt.header.packet_id,
            msg_id: Some(emergency_id),
            peer: None,
            channel: None,
            state: DeliveryState::Sent,
        });
        Ok(emergency_id)
    }

    /// Received emergency broadcasts, oldest first.
    pub fn emergency_history(&self) -> Vec<EmergRecord> {
        self.inner.emergency_log.lock().clone()
    }

    // ---- morse / tones -----------------------------------------------------------

    /// Send a digital Morse message to a peer (E2E when a session exists,
    /// signed always). Digital Morse ≠ CW.
    pub fn send_morse(&self, peer: NodeId, text: &str, wpm: u8) -> Result<MsgId, SendError> {
        self.check_unlocked()?;
        let msg_id = self.next_msg_id();
        let pkt = {
            let mut r = self.inner.router.lock();
            r.originate_unicast(
                peer,
                Payload::Data(DataMsg {
                    app: AppData::Plain(PlainApp::Morse {
                        text: text.chars().take(ucr_protocol::TEXT_MAX).collect(),
                        wpm: wpm.clamp(5, 60),
                    }),
                }),
                Priority::Normal,
                true,
                now_ms(),
            )
            .map_err(|_| SendError::NoRoute)?
        };
        self.transmit_signed(&pkt)?;
        self.record_outbound(Some(peer), None, &pkt, Some(msg_id));
        Ok(msg_id)
    }

    /// Announce an identification tone to the neighbourhood.
    pub fn announce_tone(&self, pattern: TonePattern) -> Result<(), SendError> {
        let pkt = {
            let mut r = self.inner.router.lock();
            r.originate_broadcast(
                Scope::Direct,
                Payload::Data(DataMsg {
                    app: AppData::Plain(PlainApp::ToneAnnounce(pattern)),
                }),
                Priority::Normal,
            )
        };
        self.transmit_signed(&pkt)?;
        Ok(())
    }

    /// Known tone pattern for a node (built-in or announced custom).
    pub fn tone_for(&self, node: &NodeId) -> Option<TonePattern> {
        self.inner.tones.lock().get(node).cloned()
    }

    // ---- history / unread ------------------------------------------------------

    pub fn channel_history(&self, channel: &ChannelId, limit: usize) -> Vec<StoredMessage> {
        self.inner
            .store
            .history(Some(channel), &self.node_id(), limit)
            .unwrap_or_default()
    }

    pub fn dm_history(&self, peer: &NodeId, limit: usize) -> Vec<StoredMessage> {
        self.inner
            .store
            .history(None, peer, limit)
            .unwrap_or_default()
    }

    /// Mark a conversation read (key: `dm:<peerhex>` or `ch:<id>`).
    pub fn mark_read(&self, key: &str) {
        let _ = self
            .inner
            .store
            .put_meta_u64(&format!("read:{key}"), now_ms());
    }

    pub fn last_read(&self, key: &str) -> u64 {
        self.inner
            .store
            .get_meta_u64(&format!("read:{key}"))
            .ok()
            .flatten()
            .unwrap_or(0)
    }

    /// Unread inbound counts: dm → per peer, channels → per channel.
    pub fn unread(&self) -> (HashMap<NodeId, u32>, HashMap<ChannelId, u32>) {
        let mut dm: HashMap<NodeId, u32> = HashMap::new();
        let mut ch: HashMap<ChannelId, u32> = HashMap::new();
        // In a real UI this uses an index; for the store scan scale is small.
        if let Ok(msgs) = self.inner.store.messages_since(0) {
            for m in msgs {
                if m.direction != Direction::Inbound {
                    continue;
                }
                match m.channel {
                    Some(c) => {
                        if m.at_ms > self.last_read(&format!("ch:{}", c.short())) {
                            *ch.entry(c).or_default() += 1;
                        }
                    }
                    None => {
                        if m.at_ms > self.last_read(&format!("dm:{}", m.peer.to_hex())) {
                            *dm.entry(m.peer).or_default() += 1;
                        }
                    }
                }
            }
        }
        (dm, ch)
    }

    /// Graceful shutdown of runtime tasks.
    pub fn stop(&self) {
        self.inner.shutdown.notify_waiters();
    }

    // ==================================================================
    // Internals
    // ==================================================================

    fn next_msg_id(&self) -> MsgId {
        self.inner.msg_seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn caps_flags(&self) -> u16 {
        let cfg = self.inner.cfg.lock();
        let mut c = caps::MORSE | caps::STORE_FORWARD | caps::DUTY_CYCLE_AWARE;
        if cfg.relay {
            c |= caps::RELAY;
        }
        if cfg.emergency_relay {
            c |= caps::EMERGENCY_RELAY;
        }
        if cfg.pos.is_some() && cfg.share_position {
            c |= caps::POSITIONED;
        }
        if cfg.battery == 255 {
            c |= caps::POWERED;
        }
        c
    }

    fn record_outbound(
        &self,
        peer: Option<NodeId>,
        channel: Option<ChannelId>,
        packet: &Packet,
        msg_id: Option<MsgId>,
    ) {
        if let Some(p) = peer {
            // Guard must be dropped before locking pending_acks.
            let nh = {
                let g = self.inner.router.lock();
                g.next_hop_for(&p, now_ms())
            };
            if let Some(nh) = nh {
                // cfg reads must not sit inside a pending_acks statement —
                // their guards live to the end of it, and two cfg.lock()
                // temporaries in one statement self-deadlock.
                let (ack_to, ack_retries) = {
                    let c = self.inner.cfg.lock();
                    (c.ack_timeout_ms, c.ack_retries)
                };
                self.inner.pending_acks.lock().insert(
                    packet.header.packet_id,
                    PendingAck {
                        msg_id,
                        peer: p,
                        next_hop: nh,
                        channel,
                        deadline_ms: now_ms() + ack_to,
                        retries_left: ack_retries,
                        packet: packet.clone(),
                    },
                );
            }
        }
    }

    /// Encode+sign+send a packet originated by us (ordered via the tx pump).
    fn transmit_signed(&self, pkt: &Packet) -> Result<(), SendError> {
        let signer = self.inner.identity.packet_signer();
        let wire = pkt.encode(Some(&signer))?;
        let mtu = self.inner.transport.mtu();
        if wire.len() > mtu {
            return Err(SendError::TooLarge {
                size: wire.len(),
                mtu,
            });
        }
        if !self.inner.transport.may_transmit() {
            return Err(SendError::TxNotPermitted);
        }
        self.inner.router.lock().mark_self_sent(pkt, now_ms());
        let _ = self.inner.tx_pump.send(wire);
        Ok(())
    }

    /// Transmit a packet preserving its existing signature (relayed traffic).
    fn transmit_raw(&self, pkt: &Packet) -> Result<(), SendError> {
        let wire = pkt.encode(None)?;
        let _ = self.inner.tx_pump.send(wire);
        Ok(())
    }

    /// Queue-or-send an E2E `InnerPlain` to `peer`. Establishes the session
    /// when missing — the item flushes automatically on completion.
    fn queue_or_send_e2e(
        &self,
        peer: NodeId,
        inner: InnerPlain,
        msg_id: MsgId,
        want_ack: bool,
    ) -> Result<(), SendError> {
        // Ensure a session exists (or a handshake is already in flight —
        // re-initiating would replace the pending ephemeral and break the
        // in-flight exchange).
        {
            let sm = self.inner.sessions.lock();
            if !sm.has_session(&peer) && !sm.has_pending(&peer) {
                drop(sm);
                self.initiate_session(peer)?;
            }
        }
        self.inner
            .pending_e2e
            .lock()
            .entry(peer)
            .or_default()
            .push_back(PendingE2e { inner, msg_id });
        let _ = want_ack;
        // If a session already existed, flush now.
        self.flush_e2e(peer)?;
        Ok(())
    }

    /// Attempt to flush queued E2E items for `peer`.
    fn flush_e2e(&self, peer: NodeId) -> Result<(), SendError> {
        loop {
            let next = self
                .inner
                .pending_e2e
                .lock()
                .get_mut(&peer)
                .and_then(|q| q.front().map(|p| (p.msg_id, p.inner.clone())));
            let Some((msg_id, inner)) = next else { break };
            if !self.inner.sessions.lock().has_session(&peer) {
                break;
            }
            let pt = postcard::to_allocvec(&inner).map_err(|_| SendError::Codec)?;
            let enc = self.inner.sessions.lock().encrypt(&peer, &pt);
            let Some((session_id, nonce, ct)) = enc else {
                break;
            };
            let pkt_res = {
                let mut r = self.inner.router.lock();
                r.originate_unicast(
                    peer,
                    Payload::Data(DataMsg {
                        app: AppData::Encrypted(Encrypted {
                            session_id,
                            nonce,
                            ct: ct.clone(),
                        }),
                    }),
                    Priority::Normal,
                    true,
                    now_ms(),
                )
            };
            match pkt_res {
                Ok(pkt) => {
                    self.transmit_signed(&pkt)?;
                    let (ack_to, ack_retries) = {
                        let c = self.inner.cfg.lock();
                        (c.ack_timeout_ms, c.ack_retries)
                    };
                    self.inner.pending_acks.lock().insert(
                        pkt.header.packet_id,
                        PendingAck {
                            msg_id: Some(msg_id),
                            peer,
                            next_hop: pkt.header.next_hop,
                            channel: None,
                            deadline_ms: now_ms() + ack_to,
                            retries_left: ack_retries,
                            packet: pkt.clone(),
                        },
                    );
                    self.inner
                        .pending_e2e
                        .lock()
                        .get_mut(&peer)
                        .map(|q| q.pop_front());
                }
                Err(DropReason::NoRoute) => {
                    // Queue at the router for store-and-forward + RREQ.
                    // The router guard is scoped: maybe_rreq locks it again.
                    // cfg read hoisted: a cfg guard inside the router-held
                    // section would nest locks for no benefit.
                    let queue_ttl = self.inner.cfg.lock().queue_ttl_ms;
                    let pkt = {
                        let mut r = self.inner.router.lock();
                        let mut h =
                            ucr_protocol::Header::unicast(r.next_packet_id(), self.node_id(), peer);
                        h.want_ack = true;
                        let pkt = Packet::new(
                            h,
                            Payload::Data(DataMsg {
                                app: AppData::Encrypted(Encrypted {
                                    session_id,
                                    nonce,
                                    ct,
                                }),
                            }),
                        );
                        let _ = r.queue.enqueue(pkt.clone(), peer, now_ms(), queue_ttl);
                        pkt
                    };
                    self.inner
                        .pending_e2e
                        .lock()
                        .get_mut(&peer)
                        .map(|q| q.pop_front());
                    self.maybe_rreq(peer);
                    self.emit(NodeEvent::Delivery {
                        packet_id: pkt.header.packet_id,
                        msg_id: Some(msg_id),
                        peer: Some(peer),
                        channel: None,
                        state: DeliveryState::Queued,
                    });
                }
                Err(_) => break,
            }
        }
        Ok(())
    }

    fn initiate_session(&self, peer: NodeId) -> Result<(), SendError> {
        let mut init = self.inner.sessions.lock().initiate(peer);
        init.pubkey = self.public_key();
        let res = {
            let mut r = self.inner.router.lock();
            r.originate_unicast(
                peer,
                Payload::SessionInit(init.clone()),
                Priority::Control,
                false,
                now_ms(),
            )
        };
        match res {
            Ok(pkt) => self.transmit_signed(&pkt),
            Err(_) => {
                // No route — SessionInit queued for store-and-forward.
                // Guard scoped: maybe_rreq takes the router lock again.
                let queue_ttl = self.inner.cfg.lock().queue_ttl_ms;
                {
                    let mut r = self.inner.router.lock();
                    let mut h =
                        ucr_protocol::Header::unicast(r.next_packet_id(), self.node_id(), peer);
                    h.prio = Priority::Control;
                    let pkt = Packet::new(h, Payload::SessionInit(init));
                    let _ = r.queue.enqueue(pkt, peer, now_ms(), queue_ttl);
                }
                self.maybe_rreq(peer);
                Ok(())
            }
        }
    }

    fn maybe_rreq(&self, dest: NodeId) {
        let now = now_ms();
        {
            let mut m = self.inner.rreq_inflight.lock();
            if let Some(&dl) = m.get(&dest) {
                if now < dl {
                    return;
                }
            }
            m.insert(dest, now + 20_000);
        }
        let pkt = self.inner.router.lock().originate_rreq(dest, now);
        if let Some(pkt) = pkt {
            let _ = self.transmit_signed(&pkt);
        }
    }

    // ---- tasks ------------------------------------------------------------

    fn spawn_rx_task(&self) {
        let node = self.clone();
        tokio::spawn(async move {
            loop {
                let t = node.inner.transport.clone();
                tokio::select! {
                    r = t.recv() => match r {
                        Ok((frame, meta)) => node.handle_frame(&frame, &meta),
                        Err(_) => {
                            node.emit(NodeEvent::Log {
                                level: 1,
                                msg: "transport closed; rx loop exiting".into(),
                            });
                            return;
                        }
                    },
                    _ = node.inner.shutdown.notified() => return,
                }
            }
        });
    }

    fn spawn_beacon_task(&self) {
        let node = self.clone();
        tokio::spawn(async move {
            let mut seq: u16 = rand::thread_rng().gen();
            // First beacon goes out quickly so neighbours discover us, with
            // a random delay so nodes booting together don't transmit in
            // lockstep.
            let first_delay = 400 + rand::thread_rng().gen_range(0..1_200);
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(first_delay)) => {}
                _ = node.inner.shutdown.notified() => return,
            }
            loop {
                seq = seq.wrapping_add(1);
                let seen = node
                    .inner
                    .router
                    .lock()
                    .neighbors
                    .recently_heard(now_ms(), ucr_protocol::BEACON_SEEN_MAX);
                let cfg = node.inner.cfg.lock().clone();
                let beacon = Beacon {
                    pubkey: node.public_key(),
                    seq,
                    callsign: cfg.callsign.clone(),
                    desc: cfg.desc.clone(),
                    class: cfg.class,
                    caps: node.caps_flags(),
                    profile: node.inner.transport.profile_id(),
                    seen,
                    congestion: node.inner.transport.congestion(),
                    battery: cfg.battery,
                    pos: if cfg.share_position { cfg.pos } else { None },
                    tone_id: cfg.tone_id,
                };
                let pkt = {
                    let mut r = node.inner.router.lock();
                    r.originate_broadcast(Scope::Direct, Payload::Beacon(beacon), Priority::Normal)
                };
                let _ = node.transmit_signed(&pkt);

                // Adaptive interval: faster while still discovering, slower
                // when the neighbourhood is stable. ±25% jitter de-correlates
                // beacons across the mesh.
                let cfg2 = node.inner.cfg.lock().clone();
                let n_count = node.inner.router.lock().neighbors.len();
                let stable = node.inner.router.lock().topology_stable(now_ms());
                let base = if n_count < 3 {
                    (cfg2.beacon_base_ms / 2).max(5_000)
                } else if stable {
                    cfg2.beacon_base_ms * 2
                } else {
                    cfg2.beacon_base_ms
                };
                // Airtime floor on real RF links: a ~160B beacon at SF9
                // (1.75kbit/s) costs ~750ms on air, so beacons can never
                // exceed ~2% of the medium — discovery runs at ~37s, not 3s.
                let base = match profile_by_id(node.inner.transport.profile_id()) {
                    Some(p) if matches!(p.modulation, Modulation::LoRa | Modulation::Gfsk) => {
                        base.max(p.airtime_ms(160) * 50)
                    }
                    _ => base,
                };
                let jitter = rand::thread_rng().gen_range(0..base / 4 + 1);
                let wait = (base + jitter).min(240_000);
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(wait)) => {}
                    _ = node.inner.shutdown.notified() => return,
                }
            }
        });
    }

    fn spawn_maintenance_task(&self) {
        let node = self.clone();
        tokio::spawn(async move {
            loop {
                let tick = node.inner.cfg.lock().maintenance_ms;
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(tick)) => {}
                    _ = node.inner.shutdown.notified() => return,
                }
                node.maintenance();
            }
        });
    }

    // ---- inbound processing -----------------------------------------------------

    fn handle_frame(&self, frame: &[u8], meta: &RxMeta) {
        let decoded = match decode(frame) {
            Ok(d) => d,
            Err(e) => {
                self.emit(NodeEvent::InvalidPacket {
                    from: None,
                    reason: format!("{e}"),
                });
                return;
            }
        };
        let pkt = decoded.packet.clone();
        let src = pkt.header.src;

        // Signature check against known/embedded keys.
        let status = check_signature(&decoded, |node| self.lookup_key(node));
        match status {
            SignatureStatus::Invalid | SignatureStatus::PubkeyMismatch => {
                self.emit(NodeEvent::InvalidPacket {
                    from: Some(src),
                    reason: format!("{status:?}"),
                });
                return; // never relay unauthenticated-as-invalid traffic
            }
            _ => {}
        }
        let verified = status == SignatureStatus::Verified;

        // Our own flooded packet coming back relayed = observed propagation.
        if src == self.node_id() && pkt.header.hops > 0 {
            self.emit(NodeEvent::Propagated {
                packet_id: pkt.header.packet_id,
                hops: pkt.header.hops,
            });
        }
        let actions = {
            let mut _g = self.inner.router.lock();
            _g.handle_rx(pkt.clone(), meta, now_ms())
        };
        for action in actions {
            self.exec_action(action, meta, verified);
        }
    }

    fn exec_action(&self, action: RouteAction, _meta: &RxMeta, verified: bool) {
        match action {
            RouteAction::Transmit { packet } => {
                // Forwarded packets keep the originator's signature (signed
                // portion untouched); RREQ-style modified floods go unsigned.
                let _ = self.transmit_raw(&packet);
            }
            RouteAction::Deliver { packet, meta } => {
                self.deliver(packet, &meta, verified);
            }
            RouteAction::SendAck { orig } => {
                let ack = self.inner.router.lock().originate_ack(&orig, now_ms());
                if let Some(ack) = ack {
                    let _ = self.transmit_signed(&ack);
                }
            }
            RouteAction::NeedRoute { target } => {
                self.maybe_rreq(target);
            }
            RouteAction::ReplyKey { to } => {
                let res = self.inner.router.lock().originate_unicast(
                    to,
                    Payload::KeyReply {
                        pubkey: self.public_key(),
                    },
                    Priority::Control,
                    false,
                    now_ms(),
                );
                match res {
                    Ok(pkt) => {
                        let _ = self.transmit_signed(&pkt);
                    }
                    Err(DropReason::NoRoute) => {
                        let mut r = self.inner.router.lock();
                        let mut h =
                            ucr_protocol::Header::unicast(r.next_packet_id(), self.node_id(), to);
                        h.prio = Priority::Control;
                        let pkt = Packet::new(
                            h,
                            Payload::KeyReply {
                                pubkey: self.public_key(),
                            },
                        );
                        let _ = r.queue.enqueue(pkt, to, now_ms(), 60_000);
                    }
                    Err(_) => {}
                }
            }
            RouteAction::Drop { reason, packet } => {
                // Report expiry/failure for OUR packets — but not for our
                // own packets heard back off the medium: self-origin echoes
                // and duplicates of them are propagation evidence (a
                // `Propagated` event already fired), not delivery failures.
                let echo = matches!(reason, DropReason::SelfOrigin | DropReason::Duplicate);
                if packet.header.src == self.node_id() && !echo {
                    let msg = self
                        .inner
                        .pending_acks
                        .lock()
                        .get(&packet.header.packet_id)
                        .and_then(|p| p.msg_id);
                    self.emit(NodeEvent::Delivery {
                        packet_id: packet.header.packet_id,
                        msg_id: msg,
                        peer: if packet.header.dst.is_broadcast() {
                            None
                        } else {
                            Some(packet.header.dst)
                        },
                        channel: None,
                        state: match reason {
                            DropReason::Expired => DeliveryState::Expired,
                            _ => DeliveryState::Failed,
                        },
                    });
                }
                debug!(?reason, "packet dropped");
            }
        }
    }

    /// Application-layer delivery of a packet addressed to us (or flooded).
    fn deliver(&self, pkt: Packet, meta: &RxMeta, verified: bool) {
        let now = now_ms();
        match &pkt.payload {
            Payload::Beacon(_) => {
                // Update contact last-seen + pubkey learning.
                self.learn_from_beacon(&pkt);
                self.emit(NodeEvent::NeighborUpdate);
            }
            Payload::Data(DataMsg { app }) => match app {
                AppData::ChannelText(ct) => {
                    self.on_channel_text(&pkt, ct, verified, meta);
                }
                AppData::ChannelVoice(cv) => {
                    self.on_channel_voice(&pkt, cv);
                }
                AppData::Plain(PlainApp::Morse { text, wpm }) => {
                    if self.blocked_or_event(&pkt.header.src) {
                        return;
                    }
                    let callsign = self.callsign_of(&pkt.header.src);
                    self.emit(NodeEvent::Morse {
                        from: pkt.header.src,
                        callsign,
                        text: text.clone(),
                        wpm: *wpm,
                        at_ms: now,
                    });
                }
                AppData::Plain(PlainApp::ToneAnnounce(t)) => {
                    self.inner.tones.lock().insert(pkt.header.src, t.clone());
                    self.emit(NodeEvent::Tone {
                        from: pkt.header.src,
                        pattern: t.clone(),
                    });
                }
                AppData::Plain(PlainApp::ProfileInfo {
                    callsign,
                    desc,
                    class,
                    caps: _,
                }) => {
                    let mut c = self.contact_or_new(pkt.header.src);
                    c.callsign = callsign.clone();
                    c.desc = desc.clone();
                    c.class = *class;
                    c.last_seen_ms = now;
                    let _ = self.inner.store.upsert_contact(&c);
                }
                AppData::Encrypted(enc) => {
                    self.on_encrypted(&pkt, enc, verified);
                }
                AppData::GroupEncrypted(g) => {
                    self.on_group_encrypted(&pkt, g, verified);
                }
            },
            Payload::Ack(ack) => {
                self.on_ack(&pkt, ack, verified);
            }
            Payload::RouteReply(rrep) => {
                // Route was learned by the router already; flush queued traffic.
                let dest = rrep.route_dest;
                self.inner.rreq_inflight.lock().remove(&dest);
                self.emit(NodeEvent::RouteUpdate {
                    dest,
                    hops: rrep.hop_count,
                });
                self.flush_queue_for(dest);
            }
            Payload::SessionInit(init) => {
                if self.blocked_or_event(&pkt.header.src) {
                    return;
                }
                if ucr_identity::check_binding(&init.pubkey, &pkt.header.src).is_ok() {
                    let _ = self.inner.store.learn_key(&pkt.header.src, &init.pubkey);
                }
                if !verified {
                    // An unverified SessionInit can't establish a trusted
                    // session — the pubkey binding check protects src though.
                }
                let accepted = self.inner.sessions.lock().accept(pkt.header.src, init);
                match accepted {
                    Ok(mut accept) => {
                        accept.pubkey = self.public_key();
                        let res = self.inner.router.lock().originate_unicast(
                            pkt.header.src,
                            Payload::SessionAccept(accept),
                            Priority::Control,
                            false,
                            now,
                        );
                        if let Ok(pkt2) = res {
                            let _ = self.transmit_signed(&pkt2);
                        }
                        self.emit(NodeEvent::SessionEstablished {
                            peer: pkt.header.src,
                        });
                        let _ = self.flush_e2e(pkt.header.src);
                    }
                    Err(e) => {
                        warn!("session accept failed: {e}");
                    }
                }
            }
            Payload::SessionAccept(acc) => {
                if ucr_identity::check_binding(&acc.pubkey, &pkt.header.src).is_ok() {
                    let _ = self.inner.store.learn_key(&pkt.header.src, &acc.pubkey);
                }
                let completed = self.inner.sessions.lock().complete(pkt.header.src, acc);
                match completed {
                    Ok(()) => {
                        self.emit(NodeEvent::SessionEstablished {
                            peer: pkt.header.src,
                        });
                        let _ = self.flush_e2e(pkt.header.src);
                    }
                    Err(e) => warn!("session complete failed: {e}"),
                }
            }
            Payload::Emergency(e) => {
                self.on_emergency(&pkt, e, verified);
            }
            Payload::KeyReply { pubkey }
                if ucr_identity::check_binding(pubkey, &pkt.header.src).is_ok() =>
            {
                let _ = self.inner.store.learn_key(&pkt.header.src, pubkey);
                self.emit(NodeEvent::KeyLearned {
                    node: pkt.header.src,
                });
            }
            _ => {}
        }
    }

    fn learn_from_beacon(&self, pkt: &Packet) {
        if let Payload::Beacon(b) = &pkt.payload {
            let _ = self.inner.store.learn_key(&pkt.header.src, &b.pubkey);
            let mut c = self.contact_or_new(pkt.header.src);
            if !b.callsign.is_empty() {
                c.callsign = b.callsign.clone();
            }
            if !b.desc.is_empty() {
                c.desc = b.desc.clone();
            }
            c.class = b.class;
            c.pubkey = Some(b.pubkey);
            c.verified = ucr_identity::check_binding(&b.pubkey, &pkt.header.src).is_ok();
            c.last_seen_ms = now_ms();
            if c.source == ContactSource::ManualImport || c.source == ContactSource::ContactRequest
            {
                // keep source
            } else {
                c.source = ContactSource::OverTheAir;
            }
            let _ = self.inner.store.upsert_contact(&c);
        }
    }

    /// Voice frames are real-time: no store, no acks, no retry — emit
    /// straight to the UI for the jitter buffer. Dedup still applies at
    /// the router level (flood loops), so we key on (src, tx_id, seq).
    fn on_channel_voice(&self, pkt: &Packet, cv: &ChannelVoice) {
        let src = pkt.header.src;
        if src == self.node_id() {
            return;
        }
        if self.blocked_or_event(&src) {
            return;
        }
        if !self.inner.seen_voice.lock().insert((src, cv.tx_id, cv.seq)) {
            return;
        }
        if self.inner.seen_voice.lock().len() > 4096 {
            self.inner.seen_voice.lock().clear();
        }
        let callsign = self.callsign_of(&src);
        // Learn the channel like channel text does.
        if !self.channels().iter().any(|c| c.id == cv.channel) {
            let _ = self.inner.store.upsert_channel(&ChannelInfo::new(
                &cv.name,
                ChannelCategory::General,
                cv.scope,
            ));
        }
        self.emit(NodeEvent::VoiceFrame {
            channel: cv.channel,
            channel_name: cv.name.clone(),
            from: src,
            callsign,
            codec: cv.codec,
            tx_id: cv.tx_id,
            seq: cv.seq,
            last: cv.last,
            data: cv.data.clone(),
            hops: pkt.header.hops,
        });
    }

    fn on_channel_text(&self, pkt: &Packet, ct: &ChannelText, verified: bool, meta: &RxMeta) {
        let src = pkt.header.src;
        if src == self.node_id() {
            return;
        }
        if self.blocked_or_event(&src) {
            return;
        }
        // App-level msg dedup.
        if !self.inner.seen_msgs.lock().insert((src, ct.msg_id)) {
            return;
        }
        let now = now_ms();
        let callsign = self.callsign_of(&src);
        let _ = self.inner.store.put_message(&StoredMessage {
            msg_id: ct.msg_id,
            peer: src,
            channel: Some(ct.channel),
            direction: Direction::Inbound,
            text: ct.text.clone(),
            at_ms: now,
            delivery: DeliveryState::Delivered,
        });
        // Learn the channel itself (discovered over the air).
        if !self.channels().iter().any(|c| c.id == ct.channel) {
            let _ =
                self.inner
                    .store
                    .upsert_channel(&ChannelInfo::new(&ct.name, ct.category, ct.scope));
        }
        self.emit(NodeEvent::ChannelMessage {
            channel: ct.channel,
            channel_name: ct.name.clone(),
            from: src,
            callsign,
            text: ct.text.clone(),
            msg_id: ct.msg_id,
            at_ms: now,
            hops: pkt.header.hops,
            verified,
        });
        let _ = meta;
    }

    fn on_encrypted(&self, pkt: &Packet, enc: &Encrypted, _verified: bool) {
        let src = pkt.header.src;
        if self.blocked_or_event(&src) {
            return;
        }
        let dec = self
            .inner
            .sessions
            .lock()
            .decrypt(enc.session_id, &enc.nonce, &enc.ct);
        let Ok((peer, pt)) = dec else {
            self.emit(NodeEvent::InvalidPacket {
                from: Some(src),
                reason: "e2e decrypt failed".into(),
            });
            return;
        };
        if peer != src {
            self.emit(NodeEvent::InvalidPacket {
                from: Some(src),
                reason: "session peer/src mismatch".into(),
            });
            return;
        }
        let inner: InnerPlain = match postcard::from_bytes(&pt) {
            Ok(i) => i,
            Err(_) => return,
        };
        match inner {
            InnerPlain::DirectText { msg_id, text } => {
                if !self.inner.seen_msgs.lock().insert((src, msg_id)) {
                    return;
                }
                let now = now_ms();
                let callsign = self.callsign_of(&src);
                let _ = self.inner.store.put_message(&StoredMessage {
                    msg_id,
                    peer: src,
                    channel: None,
                    direction: Direction::Inbound,
                    text: text.clone(),
                    at_ms: now,
                    delivery: DeliveryState::Delivered,
                });
                self.emit(NodeEvent::DirectMessage {
                    from: src,
                    callsign,
                    text,
                    msg_id,
                    at_ms: now,
                });
            }
            InnerPlain::ContactRequest {
                callsign,
                desc,
                class,
                text,
            } => {
                let mut c = self.contact_or_new(src);
                c.callsign = callsign.clone();
                c.desc = desc.clone();
                c.class = class;
                c.source = ContactSource::ContactRequest;
                c.last_seen_ms = now_ms();
                let _ = self.inner.store.upsert_contact(&c);
                self.emit(NodeEvent::ContactRequest {
                    from: src,
                    callsign,
                    desc,
                    class,
                    text,
                    at_ms: now_ms(),
                });
            }
            InnerPlain::GroupInvite {
                group_id,
                name,
                desc,
                epoch,
                key,
            } => {
                let g = StoredGroup {
                    group_id,
                    name: name.clone(),
                    desc: desc.clone(),
                    creator: src,
                    epoch,
                    key,
                    members: vec![
                        GroupMember {
                            node_id: src,
                            callsign: self.callsign_of(&src),
                            admin: true,
                            joined_ms: now_ms(),
                        },
                        GroupMember {
                            node_id: self.node_id(),
                            callsign: self.callsign(),
                            admin: false,
                            joined_ms: now_ms(),
                        },
                    ],
                    muted_until_ms: None,
                };
                let _ = self.inner.store.upsert_group(&g);
                self.emit(NodeEvent::GroupInvite {
                    group_id,
                    name,
                    desc,
                    from: src,
                    callsign: self.callsign_of(&src),
                });
            }
            InnerPlain::GroupInviteReply { group_id, accepted } => {
                if let Ok(Some(mut g)) = self.inner.store.group(&group_id) {
                    if accepted {
                        if !g.members.iter().any(|m| m.node_id == src) {
                            g.members.push(GroupMember {
                                node_id: src,
                                callsign: self.callsign_of(&src),
                                admin: false,
                                joined_ms: now_ms(),
                            });
                        }
                    } else {
                        g.members.retain(|m| m.node_id != src);
                    }
                    let members: Vec<NodeId> = g.members.iter().map(|m| m.node_id).collect();
                    let _ = self.inner.store.upsert_group(&g);
                    self.emit(NodeEvent::GroupRoster { group_id, members });
                }
            }
            InnerPlain::GroupKeyUpdate {
                group_id,
                epoch,
                key,
            } => {
                if let Ok(Some(mut g)) = self.inner.store.group(&group_id) {
                    if epoch > g.epoch {
                        g.epoch = epoch;
                        g.key = key;
                        let _ = self.inner.store.upsert_group(&g);
                        self.emit(NodeEvent::GroupEpoch { group_id, epoch });
                    }
                }
            }
            InnerPlain::GroupLeave { group_id, member } => {
                let leaving = member.unwrap_or(src);
                if leaving == self.node_id() {
                    // We've been removed.
                    let _ = self.inner.store.delete_group(&group_id);
                    self.emit(NodeEvent::GroupRoster {
                        group_id,
                        members: vec![],
                    });
                } else if let Ok(Some(mut g)) = self.inner.store.group(&group_id) {
                    g.members.retain(|m| m.node_id != leaving);
                    let members: Vec<NodeId> = g.members.iter().map(|m| m.node_id).collect();
                    let _ = self.inner.store.upsert_group(&g);
                    self.emit(NodeEvent::GroupRoster { group_id, members });
                }
            }
        }
    }

    fn on_group_encrypted(&self, pkt: &Packet, enc: &GroupEncrypted, _v: bool) {
        let src = pkt.header.src;
        if src == self.node_id() {
            return;
        }
        if self.blocked_or_event(&src) {
            return;
        }
        let Ok(Some(g)) = self.inner.store.group(&enc.group_id) else {
            return; // not a group we belong to
        };
        if enc.epoch != g.epoch {
            return; // stale-epoch traffic we no longer hold the key for
        }
        let aad = ucr_crypto::group_aad(&enc.group_id, enc.epoch);
        let Ok(pt) = ucr_crypto::aead_decrypt(&g.key, &aad, &enc.nonce, &enc.ct) else {
            return;
        };
        let inner: InnerGroup = match postcard::from_bytes(&pt) {
            Ok(i) => i,
            Err(_) => return,
        };
        match inner {
            InnerGroup::Text { msg_id, text } => {
                if !self.inner.seen_msgs.lock().insert((src, msg_id)) {
                    return;
                }
                let now = now_ms();
                let callsign = self.callsign_of(&src);
                self.emit(NodeEvent::GroupMessage {
                    group_id: enc.group_id,
                    from: src,
                    callsign,
                    text: text.clone(),
                    msg_id,
                    at_ms: now,
                });
                let _ = self.inner.store.put_message(&StoredMessage {
                    msg_id,
                    peer: src,
                    channel: None,
                    direction: Direction::Inbound,
                    text,
                    at_ms: now,
                    delivery: DeliveryState::Delivered,
                });
            }
            InnerGroup::Roster { members, epoch } => {
                if let Ok(Some(mut g)) = self.inner.store.group(&enc.group_id) {
                    if epoch >= g.epoch {
                        // Merge roster — member set we don't know becomes bare ids.
                        for m in &members {
                            if !g.members.iter().any(|x| &x.node_id == m) {
                                g.members.push(GroupMember {
                                    node_id: *m,
                                    callsign: self.callsign_of(m),
                                    admin: *m == g.creator,
                                    joined_ms: now_ms(),
                                });
                            }
                        }
                        g.members.retain(|m| members.contains(&m.node_id));
                        let _ = self.inner.store.upsert_group(&g);
                        self.emit(NodeEvent::GroupRoster {
                            group_id: enc.group_id,
                            members,
                        });
                    }
                }
            }
        }
    }

    fn on_ack(&self, pkt: &Packet, ack: &Ack, verified: bool) {
        // Only a VERIFIED ack counts as authenticated delivery.
        let src = pkt.header.src;
        let rec = self.inner.pending_acks.lock().remove(&ack.acked_id);
        if let Some(rec) = rec {
            if rec.peer == src && verified {
                self.inner.router.lock().note_tx_result(&rec.next_hop, true);
                self.emit(NodeEvent::Delivery {
                    packet_id: ack.acked_id,
                    msg_id: rec.msg_id,
                    peer: Some(src),
                    channel: rec.channel,
                    state: DeliveryState::Delivered,
                });
                if let Some(msg_id) = rec.msg_id {
                    let _ = self.inner.store.set_delivery(
                        rec.channel.as_ref(),
                        &src,
                        msg_id,
                        DeliveryState::Delivered,
                    );
                }
            } else if rec.peer == src {
                // Unverified ack — honest: keep the message "Sent".
                self.emit(NodeEvent::Delivery {
                    packet_id: ack.acked_id,
                    msg_id: rec.msg_id,
                    peer: Some(src),
                    channel: rec.channel,
                    state: DeliveryState::Sent,
                });
            }
        }
    }

    fn on_emergency(&self, pkt: &Packet, e: &Emergency, verified: bool) {
        let src = pkt.header.src;
        if src == self.node_id() {
            return;
        }
        // Per-source event rate limit — relays already cap flooding; this
        // caps UI/event spam on repeat distress traffic.
        let now = now_ms();
        {
            let mut m = self.inner.emergency_last_event.lock();
            let last = m.get(&src).copied().unwrap_or(0);
            if now - last < 10_000 {
                return;
            }
            m.insert(src, now);
        }
        {
            let mut log = self.inner.emergency_log.lock();
            if log.len() >= 100 {
                log.remove(0);
            }
            log.push(EmergRecord {
                from: src,
                emergency_id: e.emergency_id,
                at_ms: e.timestamp_ms,
            });
        }
        let callsign = self.callsign_of(&src);
        self.emit(NodeEvent::Emergency {
            from: src,
            callsign,
            category: e.category,
            text: e.text.clone(),
            pos: e.pos,
            emergency_id: e.emergency_id,
            at_ms: e.timestamp_ms,
            hops: pkt.header.hops,
            verified,
        });
    }

    // ---- periodic -----------------------------------------------------------

    fn maintenance(&self) {
        let now = now_ms();
        let actions = {
            let mut g = self.inner.router.lock();
            g.maintenance(now)
        };
        for action in actions {
            self.exec_action(action, &RxMeta::unmeasured(now), false);
        }
        // Ack timeouts → resend (new packet id, same msg) or mark sent.
        let expired: Vec<u32> = self
            .inner
            .pending_acks
            .lock()
            .iter()
            .filter(|(_, p)| now >= p.deadline_ms)
            .map(|(k, _)| *k)
            .collect();
        for pid in expired {
            let Some(p) = self.inner.pending_acks.lock().remove(&pid) else {
                continue;
            };
            self.inner.router.lock().note_tx_result(&p.next_hop, false);
            if p.retries_left > 0 {
                // Resend the same payload under a fresh packet id (the
                // destination dedups app messages by msg_id and re-acks).
                let mut resend = p.packet.clone();
                resend.header.packet_id = self.inner.router.lock().next_packet_id();
                let nh = self.inner.router.lock().next_hop_for(&p.peer, now);
                if let Some(nh) = nh {
                    resend.header.next_hop = nh;
                    resend.signature = None;
                    let deadline = now + self.inner.cfg.lock().ack_timeout_ms;
                    let left = p.retries_left - 1;
                    let packet_id = resend.header.packet_id;
                    if self.transmit_signed(&resend).is_ok() {
                        self.inner.pending_acks.lock().insert(
                            packet_id,
                            PendingAck {
                                deadline_ms: deadline,
                                retries_left: left,
                                packet: resend,
                                ..p
                            },
                        );
                    }
                } else {
                    // Next hop vanished — hand back to route discovery.
                    self.maybe_rreq(p.peer);
                    let deadline = now + self.inner.cfg.lock().ack_timeout_ms;
                    self.inner.pending_acks.lock().insert(
                        pid,
                        PendingAck {
                            deadline_ms: deadline,
                            retries_left: p.retries_left - 1,
                            ..p
                        },
                    );
                }
            } else {
                // Retries exhausted — honestly still just "Sent" (no ack).
                self.emit(NodeEvent::Delivery {
                    packet_id: pid,
                    msg_id: p.msg_id,
                    peer: Some(p.peer),
                    channel: p.channel,
                    state: DeliveryState::Sent,
                });
            }
        }
        // Retransmit stale session handshakes — a lost SessionInit leaves
        // pending_e2e items stuck otherwise.
        let stale_after = self.inner.cfg.lock().ack_timeout_ms * 2;
        let retries = self.inner.sessions.lock().stale_pending(stale_after);
        for (peer, init) in retries {
            let mut init = init;
            init.pubkey = self.public_key();
            let res = {
                let mut r = self.inner.router.lock();
                r.originate_unicast(
                    peer,
                    Payload::SessionInit(init),
                    Priority::Control,
                    false,
                    now,
                )
            };
            if let Ok(pkt) = res {
                let _ = self.transmit_signed(&pkt);
            } else {
                self.maybe_rreq(peer);
            }
        }
        // Flush store-forward queue when routes became available.
        let queued_dests: Vec<NodeId> = {
            let r = self.inner.router.lock();
            r.queue.snapshot().iter().map(|q| q.dest).collect()
        };
        for d in queued_dests {
            // `if`-condition temporaries live for the whole statement —
            // bind first so the router guard drops before flush_queue_for
            // locks it again.
            let has_route = self.inner.router.lock().next_hop_for(&d, now).is_some();
            if has_route {
                self.flush_queue_for(d);
            }
        }
    }

    fn flush_queue_for(&self, dest: NodeId) {
        let queued = self.inner.router.lock().queue.flush_for(&dest);
        for qp in queued {
            let mut pkt = qp.packet;
            let nh = self.inner.router.lock().next_hop_for(&dest, now_ms());
            if let Some(nh) = nh {
                pkt.header.next_hop = nh;
            }
            // Queued packets were never signed — sign now if ours.
            if pkt.header.src == self.node_id() && pkt.signature.is_none() {
                let _ = self.transmit_signed(&pkt);
            } else {
                let _ = self.transmit_raw(&pkt);
            }
            if pkt.header.want_ack {
                let (ack_to, ack_retries) = {
                    let c = self.inner.cfg.lock();
                    (c.ack_timeout_ms, c.ack_retries)
                };
                let mut pa = self.inner.pending_acks.lock();
                pa.entry(pkt.header.packet_id)
                    .or_insert_with(|| PendingAck {
                        msg_id: None,
                        peer: dest,
                        next_hop: pkt.header.next_hop,
                        channel: None,
                        deadline_ms: now_ms() + ack_to,
                        retries_left: ack_retries,
                        packet: pkt.clone(),
                    });
            }
        }
    }

    fn blocked_or_event(&self, src: &NodeId) -> bool {
        match self.inner.store.is_blocked(src) {
            Ok(true) => {
                self.emit(NodeEvent::BlockedDrop { from: *src });
                true
            }
            _ => false,
        }
    }

    fn lookup_key(&self, node: &NodeId) -> Option<[u8; 32]> {
        if let Ok(Some(k)) = self.inner.store.key_for(node) {
            return Some(k);
        }
        self.inner
            .router
            .lock()
            .neighbors
            .get(node)
            .and_then(|n| n.pubkey)
    }

    pub fn callsign_of(&self, node: &NodeId) -> String {
        if let Ok(Some(c)) = self.inner.store.contact(node) {
            if !c.callsign.is_empty() {
                return c.callsign;
            }
        }
        self.inner
            .router
            .lock()
            .neighbors
            .get(node)
            .map(|n| n.callsign.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| node.short())
    }
}

// =================================================================
// SendError
// =================================================================

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("no route to destination (queued for store-and-forward)")]
    NoRoute,
    #[error("peer is blocked")]
    Blocked,
    #[error("node is locked — unlock with the PIN to transmit")]
    Locked,
    #[error("unknown channel")]
    UnknownChannel,
    #[error("unknown group")]
    UnknownGroup,
    #[error("not a group admin")]
    NotAdmin,
    #[error("packet too large ({size} > {mtu})")]
    TooLarge { size: usize, mtu: usize },
    #[error("transmissions not permitted on this profile (receive-only)")]
    TxNotPermitted,
    #[error("rate limited — retry in {0} ms")]
    RateLimited(u64),
    #[error("codec error")]
    Codec,
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("transport error: {0}")]
    Transport(#[from] ucr_transport::TransportError),
    #[error("protocol error: {0}")]
    Protocol(#[from] ucr_protocol::ProtocolError),
    #[error("invalid input: {0}")]
    Invalid(&'static str),
}

impl Node {
    fn must_group(&self, id: &GroupId) -> Result<StoredGroup, SendError> {
        self.inner.store.group(id)?.ok_or(SendError::UnknownGroup)
    }

    fn must_admin(&self, g: &StoredGroup) -> Result<(), SendError> {
        let me = self.node_id();
        let is_admin = g.members.iter().any(|m| m.node_id == me && m.admin) || g.creator == me;
        if is_admin {
            Ok(())
        } else {
            Err(SendError::NotAdmin)
        }
    }
}
