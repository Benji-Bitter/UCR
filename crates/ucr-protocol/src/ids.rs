//! Compact identifiers used across the protocol.

use alloc::string::String;
use core::fmt;
use serde::{Deserialize, Serialize};

/// Permanent network identity of a node.
///
/// `NodeId` is the first 16 bytes of `BLAKE3(ed25519_public_key)`. This makes
/// every node identity self-certifying: a packet that carries the sender's
/// public key can be bound to its `NodeId` by hashing — no directory needed.
///
/// Callsigns are display names only; `NodeId` is the routing/security
/// identifier. Duplicate callsigns are therefore harmless to addressing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub [u8; 16]);

impl NodeId {
    /// Derive the node id for an Ed25519 public key.
    pub fn from_pubkey(pubkey: &[u8; 32]) -> Self {
        let h = blake3::hash(pubkey);
        let mut id = [0u8; 16];
        id.copy_from_slice(&h.as_bytes()[..16]);
        NodeId(id)
    }

    pub fn is_broadcast(&self) -> bool {
        self == &crate::BROADCAST
    }

    /// Short human-readable form used in the UI, e.g. `a3f0-91bc`.
    pub fn short(&self) -> String {
        alloc::format!(
            "{:02x}{:02x}-{:02x}{:02x}",
            self.0[0],
            self.0[1],
            self.0[2],
            self.0[3]
        )
    }

    /// Full lowercase hex representation.
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(32);
        for b in self.0.iter() {
            s.push_str(&alloc::format!("{:02x}", b));
        }
        s
    }

    /// Parse a 32-char hex string into a `NodeId`.
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.len() != 32 {
            return None;
        }
        let mut out = [0u8; 16];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
        }
        Some(NodeId(out))
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_broadcast() {
            write!(f, "NodeId(BROADCAST)")
        } else {
            write!(f, "NodeId({})", self.short())
        }
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short())
    }
}

/// Identifier of a public channel: `BLAKE3("ucr-channel:" ++ name)[..8]`.
/// Two users typing the same channel name join the same channel without a
/// directory server.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChannelId(pub [u8; 8]);

impl ChannelId {
    pub fn from_name(name: &str) -> Self {
        let mut key = [0u8; 8];
        let h = blake3::hash(alloc::format!("ucr-channel:{}", name.to_lowercase()).as_bytes());
        key.copy_from_slice(&h.as_bytes()[..8]);
        ChannelId(key)
    }

    pub fn short(&self) -> String {
        alloc::format!(
            "{:02x}{:02x}{:02x}{:02x}",
            self.0[0],
            self.0[1],
            self.0[2],
            self.0[3]
        )
    }
}

impl fmt::Debug for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChannelId({})", self.short())
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.short())
    }
}

/// Identifier of a private group: `BLAKE3("ucr-group:" ++ creator || random)[..8]`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GroupId(pub [u8; 8]);

impl fmt::Debug for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GroupId({:02x}{:02x}{:02x}{:02x})",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Per-sender packet sequence used for duplicate detection together with
/// the source `NodeId`.
pub type PacketId = u32;

/// Application-level message id (per-sender monotone + random start).
pub type MsgId = u64;

/// An Ed25519 signature. Newtype so it serialises as raw bytes — serde's
/// derive doesn't cover `[u8; 64]` natively.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Signature(pub [u8; 64]);

impl Serialize for Signature {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Signature;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a 64-byte signature")
            }
            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Signature, E> {
                let arr: [u8; 64] = v
                    .try_into()
                    .map_err(|_| E::custom("bad signature length"))?;
                Ok(Signature(arr))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Signature, A::Error> {
                let mut out = [0u8; 64];
                for b in out.iter_mut() {
                    *b = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::custom("short signature"))?;
                }
                Ok(Signature(out))
            }
        }
        d.deserialize_bytes(V)
    }
}
