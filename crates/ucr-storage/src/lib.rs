//! # UCR Storage
//!
//! Local-first persistence on top of `redb` (single-file embedded KV).
//! Everything the node learns and the user produces lives here:
//!
//! * `meta` — identity secret, profile, settings, counters;
//! * `contacts` — known identities: callsign, pubkey, trust, block/mute;
//! * `channels` — channel registry (joined, favourites, discovered);
//! * `messages` — per-peer/channel message history;
//! * `groups` — private group state incl. current key epoch;
//! * `state` — learned node keys, neighbour snapshots, dedup warmup.
//!
//! The secret key is stored only in this file, which the app creates with
//! 0600 permissions; there are no embedded or hardcoded identities anywhere.

use std::path::Path;

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use ucr_identity::{Identity, StoredIdentity};
use ucr_protocol::station::StationClass;
use ucr_protocol::{ChannelId, GroupId, NodeId};

const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");
const CONTACTS: TableDefinition<&[u8; 16], &[u8]> = TableDefinition::new("contacts");
const CHANNELS: TableDefinition<&[u8; 8], &[u8]> = TableDefinition::new("channels");
const MESSAGES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("messages");
const GROUPS: TableDefinition<&[u8; 8], &[u8]> = TableDefinition::new("groups");
const KEYS: TableDefinition<&[u8; 16], &[u8; 32]> = TableDefinition::new("keys");

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Db(Box<redb::DatabaseError>),
    #[error("transaction error: {0}")]
    Tx(Box<redb::TransactionError>),
    #[error("table error: {0}")]
    Table(Box<redb::TableError>),
    #[error("commit error: {0}")]
    Commit(Box<redb::CommitError>),
    #[error("storage error: {0}")]
    Storage(Box<redb::StorageError>),
    #[error("codec error")]
    Codec,
    #[error("missing required record: {0}")]
    Missing(&'static str),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

macro_rules! boxed_from {
    ($variant:ident, $ty:ty) => {
        impl From<$ty> for StorageError {
            fn from(e: $ty) -> Self {
                Self::$variant(Box::new(e))
            }
        }
    };
}
boxed_from!(Db, redb::DatabaseError);
boxed_from!(Tx, redb::TransactionError);
boxed_from!(Table, redb::TableError);
boxed_from!(Commit, redb::CommitError);
boxed_from!(Storage, redb::StorageError);

/// A stored contact — identity-level, not callsign-level. Blocking and trust
/// always apply to the `NodeId`, so renaming a callsign cannot evade them.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contact {
    pub node_id: NodeId,
    pub callsign: String,
    pub desc: String,
    pub class: StationClass,
    pub pubkey: Option<[u8; 32]>,
    /// Verification state of the pubkey↔node_id binding.
    pub verified: bool,
    pub blocked: bool,
    pub muted_until_ms: Option<u64>,
    pub favourite: bool,
    /// Trust marker — user explicitly verified (e.g. compared fingerprints).
    pub trusted: bool,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    /// Received via contact-request / manual import / over-the-air.
    pub source: ContactSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContactSource {
    OverTheAir,
    ContactRequest,
    ManualImport,
    GroupMember,
}

/// A stored message in a channel or DM history.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredMessage {
    /// Unique key: `{kind}:{peer_or_channel}:{msg_id}`.
    pub msg_id: u64,
    pub peer: NodeId,
    pub channel: Option<ChannelId>,
    pub direction: Direction,
    pub text: String,
    pub at_ms: u64,
    pub delivery: DeliveryState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Inbound,
    Outbound,
}

/// Honest delivery states — "reached the destination's protocol stack" is
/// the strongest claim an ack supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryState {
    /// Not yet transmitted.
    Queued,
    /// Transmitted; awaiting ack where requested.
    Sent,
    /// Authenticated ack from the destination node received.
    Delivered,
    /// Propagated on ≥1 hop by observed re-broadcast (flood messages).
    Propagated,
    /// Expired in the outbound queue.
    Expired,
    /// Dropped — no route after max attempts.
    Failed,
}

impl DeliveryState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Sent => "Sent",
            Self::Delivered => "Delivered",
            Self::Propagated => "Propagated",
            Self::Expired => "Expired",
            Self::Failed => "Failed",
        }
    }
}

/// Stored private group.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredGroup {
    pub group_id: GroupId,
    pub name: String,
    pub desc: String,
    pub creator: NodeId,
    pub epoch: u32,
    /// Current symmetric key — sensitive; lives only in this store.
    pub key: [u8; 32],
    pub members: Vec<GroupMember>,
    pub muted_until_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupMember {
    pub node_id: NodeId,
    pub callsign: String,
    pub admin: bool,
    pub joined_ms: u64,
}

/// User settings persisted across restarts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub callsign: String,
    pub desc: String,
    pub class: StationClass,
    pub tone_id: u8,
    pub relay: bool,
    /// Notification preference for channels.
    pub notify_channel: NotifyMode,
    /// Play identification tones on inbound messages.
    pub play_tones: bool,
    /// UI theme: "dark" | "light".
    pub theme: String,
    /// Optional coarse position disclosure (none = GPS-less).
    pub share_position: bool,
    /// Queue expiry for outbound messages, ms.
    pub queue_ttl_ms: u64,
    /// Blake3 hash of the digital-lock PIN. `Some` = the node boots locked;
    /// the PIN itself is never stored. Older stores without the field
    /// deserialize as `None` (unlocked).
    #[serde(default)]
    pub lock_pin_hash: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotifyMode {
    All,
    MentionsOnly,
    Muted,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            callsign: String::new(),
            desc: String::new(),
            class: StationClass::Individual,
            tone_id: 1,
            relay: true,
            notify_channel: NotifyMode::All,
            play_tones: true,
            theme: "dark".into(),
            share_position: false,
            queue_ttl_ms: 2 * 60 * 60 * 1000,
            lock_pin_hash: None,
        }
    }
}

/// The node store.
pub struct Store {
    db: Database,
}

impl Store {
    /// Open (or create) a store at `path`. The file is created 0600.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            if !path.exists() {
                let f = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .mode(0o600)
                    .open(path)?;
                drop(f);
            }
        }
        let db = Database::create(path)?;
        let store = Self { db };
        // Ensure tables exist.
        let wtx = store.db.begin_write()?;
        {
            let _ = wtx.open_table(META)?;
            let _ = wtx.open_table(CONTACTS)?;
            let _ = wtx.open_table(CHANNELS)?;
            let _ = wtx.open_table(MESSAGES)?;
            let _ = wtx.open_table(GROUPS)?;
            let _ = wtx.open_table(KEYS)?;
        }
        wtx.commit()?;
        Ok(store)
    }

    /// In-memory store — for tests.
    pub fn in_memory() -> Result<Self, StorageError> {
        let db = Database::builder().create_with_backend(redb::backends::InMemoryBackend::new())?;
        let store = Self { db };
        let wtx = store.db.begin_write()?;
        {
            let _ = wtx.open_table(META)?;
            let _ = wtx.open_table(CONTACTS)?;
            let _ = wtx.open_table(CHANNELS)?;
            let _ = wtx.open_table(MESSAGES)?;
            let _ = wtx.open_table(GROUPS)?;
            let _ = wtx.open_table(KEYS)?;
        }
        wtx.commit()?;
        Ok(store)
    }

    fn put<T: Serialize>(
        tx: &redb::WriteTransaction,
        t: TableDefinition<&str, &[u8]>,
        key: &str,
        v: &T,
    ) -> Result<(), StorageError> {
        let bytes = postcard::to_allocvec(v).map_err(|_| StorageError::Codec)?;
        tx.open_table(t)?.insert(key, bytes.as_slice())?;
        Ok(())
    }

    fn get_bytes(
        &self,
        t: TableDefinition<&str, &[u8]>,
        key: &str,
    ) -> Result<Option<Vec<u8>>, StorageError> {
        let rtx = self.db.begin_read()?;
        Ok(rtx.open_table(t)?.get(key)?.map(|v| v.value().to_vec()))
    }

    // ---- identity ------------------------------------------------------

    /// Load or create the node's identity.
    pub fn identity(&self) -> Result<Identity, StorageError> {
        if let Some(bytes) = self.get_bytes(META, "identity")? {
            if let Some(rec) = StoredIdentity::decode(&bytes) {
                return Ok(Identity::from_secret(&rec.secret));
            }
        }
        let id = Identity::generate();
        let rec = StoredIdentity {
            secret: id.secret_bytes(),
        };
        let enc = rec.encode().ok_or(StorageError::Codec)?;
        let wtx = self.db.begin_write()?;
        wtx.open_table(META)?.insert("identity", enc.as_slice())?;
        wtx.commit()?;
        Ok(id)
    }

    // ---- settings --------------------------------------------------------

    pub fn settings(&self) -> Result<Settings, StorageError> {
        match self.get_bytes(META, "settings")? {
            Some(b) => postcard::from_bytes(&b).map_err(|_| StorageError::Codec),
            None => Ok(Settings::default()),
        }
    }

    pub fn save_settings(&self, s: &Settings) -> Result<(), StorageError> {
        let wtx = self.db.begin_write()?;
        Self::put(&wtx, META, "settings", s)?;
        wtx.commit()?;
        Ok(())
    }

    /// Store a u64 under a meta key (read cursors, counters).
    pub fn put_meta_u64(&self, key: &str, v: u64) -> Result<(), StorageError> {
        let wtx = self.db.begin_write()?;
        wtx.open_table(META)?
            .insert(key, v.to_be_bytes().as_slice())?;
        wtx.commit()?;
        Ok(())
    }

    pub fn get_meta_u64(&self, key: &str) -> Result<Option<u64>, StorageError> {
        match self.get_bytes(META, key)? {
            Some(b) if b.len() == 8 => {
                Ok(Some(u64::from_be_bytes(b.as_slice().try_into().unwrap())))
            }
            _ => Ok(None),
        }
    }

    /// Persistent packet-id counter — keeps ids unique across restarts.
    pub fn next_pid_seed(&self) -> Result<u32, StorageError> {
        const STEP: u32 = 100_000;
        let cur: u32 = match self.get_bytes(META, "pid")? {
            Some(b) => postcard::from_bytes(&b).map_err(|_| StorageError::Codec)?,
            None => 0,
        };
        let wtx = self.db.begin_write()?;
        Self::put(&wtx, META, "pid", &(cur + STEP))?;
        wtx.commit()?;
        Ok(cur)
    }

    // ---- known peer keys -------------------------------------------------

    pub fn learn_key(&self, node: &NodeId, pubkey: &[u8; 32]) -> Result<(), StorageError> {
        let wtx = self.db.begin_write()?;
        wtx.open_table(KEYS)?.insert(&node.0, pubkey)?;
        wtx.commit()?;
        Ok(())
    }

    pub fn key_for(&self, node: &NodeId) -> Result<Option<[u8; 32]>, StorageError> {
        let rtx = self.db.begin_read()?;
        Ok(rtx.open_table(KEYS)?.get(&node.0)?.map(|v| *v.value()))
    }

    // ---- contacts --------------------------------------------------------

    pub fn upsert_contact(&self, c: &Contact) -> Result<(), StorageError> {
        let bytes = postcard::to_allocvec(c).map_err(|_| StorageError::Codec)?;
        let wtx = self.db.begin_write()?;
        wtx.open_table(CONTACTS)?
            .insert(&c.node_id.0, bytes.as_slice())?;
        wtx.commit()?;
        Ok(())
    }

    pub fn contact(&self, node: &NodeId) -> Result<Option<Contact>, StorageError> {
        let rtx = self.db.begin_read()?;
        match rtx.open_table(CONTACTS)?.get(&node.0)? {
            Some(v) => postcard::from_bytes::<Contact>(v.value())
                .map(Some)
                .map_err(|_| StorageError::Codec),
            None => Ok(None),
        }
    }

    pub fn contacts(&self) -> Result<Vec<Contact>, StorageError> {
        let rtx = self.db.begin_read()?;
        let t = rtx.open_table(CONTACTS)?;
        let mut out = Vec::new();
        for r in t.iter()? {
            let (_, v) = r?;
            if let Ok(c) = postcard::from_bytes::<Contact>(v.value()) {
                out.push(c);
            }
        }
        Ok(out)
    }

    pub fn is_blocked(&self, node: &NodeId) -> Result<bool, StorageError> {
        Ok(self.contact(node)?.map(|c| c.blocked).unwrap_or(false))
    }

    // ---- channels ----------------------------------------------------------

    pub fn upsert_channel(&self, ch: &ucr_protocol::ChannelInfo) -> Result<(), StorageError> {
        let bytes = postcard::to_allocvec(ch).map_err(|_| StorageError::Codec)?;
        let wtx = self.db.begin_write()?;
        wtx.open_table(CHANNELS)?
            .insert(&ch.id.0, bytes.as_slice())?;
        wtx.commit()?;
        Ok(())
    }

    pub fn channels(&self) -> Result<Vec<ucr_protocol::ChannelInfo>, StorageError> {
        let rtx = self.db.begin_read()?;
        let t = rtx.open_table(CHANNELS)?;
        let mut out = Vec::new();
        for r in t.iter()? {
            let (_, v) = r?;
            if let Ok(c) = postcard::from_bytes::<ucr_protocol::ChannelInfo>(v.value()) {
                out.push(c);
            }
        }
        Ok(out)
    }

    // ---- messages ----------------------------------------------------------

    /// Store a message. Key = `{scope_byte}:{peer_hex}:{msg_id}` for
    /// per-conversation ordering.
    pub fn put_message(&self, m: &StoredMessage) -> Result<(), StorageError> {
        let key = message_key(m.channel.as_ref(), &m.peer, m.msg_id);
        let bytes = postcard::to_allocvec(m).map_err(|_| StorageError::Codec)?;
        let wtx = self.db.begin_write()?;
        wtx.open_table(MESSAGES)?
            .insert(key.as_slice(), bytes.as_slice())?;
        wtx.commit()?;
        Ok(())
    }

    /// Update only the delivery state of a stored outbound message.
    pub fn set_delivery(
        &self,
        channel: Option<&ChannelId>,
        peer: &NodeId,
        msg_id: u64,
        state: DeliveryState,
    ) -> Result<(), StorageError> {
        let key = message_key(channel, peer, msg_id);
        let wtx = self.db.begin_write()?;
        {
            let mut t = wtx.open_table(MESSAGES)?;
            let existing: Option<StoredMessage> = match t.get(key.as_slice())? {
                Some(v) => Some(
                    postcard::from_bytes::<StoredMessage>(v.value())
                        .map_err(|_| StorageError::Codec)?,
                ),
                None => None,
            };
            if let Some(mut m) = existing {
                m.delivery = state;
                let bytes = postcard::to_allocvec(&m).map_err(|_| StorageError::Codec)?;
                t.insert(key.as_slice(), bytes.as_slice())?;
            }
        }
        wtx.commit()?;
        Ok(())
    }

    /// History for a conversation (channel = Some → channel msgs; None → DM
    /// with `peer`).
    pub fn history(
        &self,
        channel: Option<&ChannelId>,
        peer: &NodeId,
        limit: usize,
    ) -> Result<Vec<StoredMessage>, StorageError> {
        let prefix = message_prefix(channel, peer);
        let rtx = self.db.begin_read()?;
        let t = rtx.open_table(MESSAGES)?;
        let mut out = Vec::new();
        for r in t.iter()? {
            let (k, v) = r?;
            if k.value().starts_with(&prefix) {
                if let Ok(m) = postcard::from_bytes::<StoredMessage>(v.value()) {
                    out.push(m);
                }
            }
        }
        out.sort_by_key(|m| m.at_ms);
        if out.len() > limit {
            out.drain(..out.len() - limit);
        }
        Ok(out)
    }

    /// All messages newer than `since_ms` — used for unread indicators.
    pub fn messages_since(&self, since_ms: u64) -> Result<Vec<StoredMessage>, StorageError> {
        let rtx = self.db.begin_read()?;
        let t = rtx.open_table(MESSAGES)?;
        let mut out = Vec::new();
        for r in t.iter()? {
            let (_, v) = r?;
            if let Ok(m) = postcard::from_bytes::<StoredMessage>(v.value()) {
                if m.at_ms >= since_ms {
                    out.push(m);
                }
            }
        }
        Ok(out)
    }

    // ---- groups -------------------------------------------------------------

    pub fn upsert_group(&self, g: &StoredGroup) -> Result<(), StorageError> {
        let bytes = postcard::to_allocvec(g).map_err(|_| StorageError::Codec)?;
        let wtx = self.db.begin_write()?;
        wtx.open_table(GROUPS)?
            .insert(&g.group_id.0, bytes.as_slice())?;
        wtx.commit()?;
        Ok(())
    }

    pub fn group(&self, id: &GroupId) -> Result<Option<StoredGroup>, StorageError> {
        let rtx = self.db.begin_read()?;
        match rtx.open_table(GROUPS)?.get(&id.0)? {
            Some(v) => postcard::from_bytes::<StoredGroup>(v.value())
                .map(Some)
                .map_err(|_| StorageError::Codec),
            None => Ok(None),
        }
    }

    pub fn groups(&self) -> Result<Vec<StoredGroup>, StorageError> {
        let rtx = self.db.begin_read()?;
        let t = rtx.open_table(GROUPS)?;
        let mut out = Vec::new();
        for r in t.iter()? {
            let (_, v) = r?;
            if let Ok(g) = postcard::from_bytes::<StoredGroup>(v.value()) {
                out.push(g);
            }
        }
        Ok(out)
    }

    pub fn delete_group(&self, id: &GroupId) -> Result<(), StorageError> {
        let wtx = self.db.begin_write()?;
        wtx.open_table(GROUPS)?.remove(&id.0)?;
        wtx.commit()?;
        Ok(())
    }
}

fn message_prefix(channel: Option<&ChannelId>, peer: &NodeId) -> Vec<u8> {
    let mut k = Vec::with_capacity(25);
    match channel {
        Some(c) => {
            k.push(b'C');
            k.extend_from_slice(&c.0);
        }
        None => {
            k.push(b'D');
            k.extend_from_slice(&peer.0[..8]);
        }
    }
    k
}

fn message_key(channel: Option<&ChannelId>, peer: &NodeId, msg_id: u64) -> Vec<u8> {
    let mut k = message_prefix(channel, peer);
    k.extend_from_slice(&peer.0[8..]);
    k.extend_from_slice(&msg_id.to_be_bytes());
    k
}
