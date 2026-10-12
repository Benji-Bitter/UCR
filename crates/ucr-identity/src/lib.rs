//! # UCR Identity
//!
//! Every UCR node has a permanent Ed25519 keypair. The public identity is
//! `NodeId = BLAKE3(pubkey)[..16]` — a self-certifying identifier: any data
//! that carries the public key can be bound to its `NodeId` by hashing, so
//! no central directory is needed.
//!
//! * Callsigns are display names. They are never used for security decisions
//!   (blocking, acknowledgements, sessions) — `NodeId` is.
//! * Secret keys are held in memory behind zeroize-on-drop. On-disk storage
//!   is the caller's responsibility (see `ucr-storage`).

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use ucr_protocol::payload::Payload;
use ucr_protocol::{DecodedPacket, NodeId, ProtocolError};

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("invalid secret key bytes")]
    BadSecretKey,
    #[error("invalid public key bytes")]
    BadPublicKey,
    #[error("signature verification failed")]
    BadSignature,
    #[error("public key does not match claimed node id")]
    PubkeyMismatch,
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("serialization error")]
    Codec,
}

/// A node's permanent cryptographic identity.
pub struct Identity {
    signing: SigningKey,
    node_id: NodeId,
}

impl Identity {
    /// Generate a fresh random identity.
    pub fn generate() -> Self {
        let signing = SigningKey::generate(&mut OsRng);
        let node_id = NodeId::from_pubkey(&signing.verifying_key().to_bytes());
        Self { signing, node_id }
    }

    /// Restore an identity from its 32-byte secret key.
    pub fn from_secret(secret: &[u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(secret);
        let node_id = NodeId::from_pubkey(&signing.verifying_key().to_bytes());
        Self { signing, node_id }
    }

    /// Secret key bytes — persist via `ucr-storage`, treat as sensitive.
    pub fn secret_bytes(&self) -> [u8; 32] {
        self.signing.to_bytes()
    }

    /// Ed25519 public key bytes — safe to share; hash = `node_id`.
    pub fn public_key(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    /// This node's permanent network id.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Sign arbitrary bytes.
    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.signing.sign(msg).to_bytes()
    }

    /// Packet signer closure suitable for `Packet::encode`.
    pub fn packet_signer(&self) -> impl Fn(&[u8]) -> [u8; 64] + '_ {
        move |b| self.sign(b)
    }
}

/// Verify a raw Ed25519 signature.
pub fn verify(pubkey: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> Result<(), IdentityError> {
    let vk = VerifyingKey::from_bytes(pubkey).map_err(|_| IdentityError::BadPublicKey)?;
    let signature = Signature::from_bytes(sig);
    vk.verify(msg, &signature)
        .map_err(|_| IdentityError::BadSignature)
}

/// Verify that `pubkey` is consistent with `node_id`.
pub fn check_binding(pubkey: &[u8; 32], node_id: &NodeId) -> Result<(), IdentityError> {
    if &NodeId::from_pubkey(pubkey) == node_id {
        Ok(())
    } else {
        Err(IdentityError::PubkeyMismatch)
    }
}

/// Outcome of checking a decoded packet's signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Packet was unsigned.
    Unsigned,
    /// Signature verified against a known sender key.
    Verified,
    /// Signature present but the sender's key is not known — the packet
    /// cannot yet be authenticated. Nodes may issue a `KeyRequest`.
    KeyUnknown,
    /// Signature present and invalid — drop or distrust the packet.
    Invalid,
    /// Carried key contradicts `src` (`blake3(pubkey)[..16] != src`).
    PubkeyMismatch,
}

/// Check a decoded packet's signature given a known-key lookup.
///
/// `lookup_key` maps `NodeId → Ed25519 pubkey` for previously learned keys.
/// Payloads that *carry* the sender pubkey (beacons, session init/accept,
/// emergency) are verified against the embedded key after a binding check.
pub fn check_signature(
    decoded: &DecodedPacket,
    lookup_key: impl Fn(&NodeId) -> Option<[u8; 32]>,
) -> SignatureStatus {
    let (region, sig) = match (&decoded.signed_region, decoded.packet.signature) {
        (Some(r), Some(s)) => (r, s.0),
        _ => return SignatureStatus::Unsigned,
    };
    let src = &decoded.packet.header.src;

    // Prefer a key embedded in the payload (self-certifying after binding).
    let embedded: Option<[u8; 32]> = match &decoded.packet.payload {
        Payload::Beacon(b) => Some(b.pubkey),
        Payload::SessionInit(s) => Some(s.pubkey),
        Payload::SessionAccept(s) => Some(s.pubkey),
        Payload::Emergency(e) => Some(e.pubkey),
        _ => None,
    };
    if let Some(pk) = embedded {
        if check_binding(&pk, src).is_err() {
            return SignatureStatus::PubkeyMismatch;
        }
        return match verify(&pk, region, &sig) {
            Ok(()) => SignatureStatus::Verified,
            Err(_) => SignatureStatus::Invalid,
        };
    }

    match lookup_key(src) {
        Some(pk) => match verify(&pk, region, &sig) {
            Ok(()) => SignatureStatus::Verified,
            Err(_) => SignatureStatus::Invalid,
        },
        None => SignatureStatus::KeyUnknown,
    }
}

/// Serializable on-disk identity record. The secret is stored raw here —
/// `ucr-storage` wraps it in the OS keyring or a permissioned file.
#[derive(Serialize, Deserialize)]
pub struct StoredIdentity {
    pub secret: [u8; 32],
}

impl StoredIdentity {
    pub fn encode(&self) -> Option<Vec<u8>> {
        postcard::to_allocvec(self).ok()
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ucr_protocol::header::{Header, Scope};
    use ucr_protocol::packet::Packet;
    use ucr_protocol::payload::{Beacon, Payload};
    use ucr_protocol::station::StationClass;

    #[test]
    fn node_id_is_stable() {
        let id = Identity::generate();
        let id2 = Identity::from_secret(&id.secret_bytes());
        assert_eq!(id.node_id(), id2.node_id());
        assert_eq!(id.public_key(), id2.public_key());
    }

    #[test]
    fn sign_and_verify() {
        let id = Identity::generate();
        let sig = id.sign(b"hello ucr");
        verify(&id.public_key(), b"hello ucr", &sig).unwrap();
        assert!(verify(&id.public_key(), b"tampered", &sig).is_err());
    }

    #[test]
    fn binding_check() {
        let id = Identity::generate();
        check_binding(&id.public_key(), &id.node_id()).unwrap();
        let other = Identity::generate();
        assert!(check_binding(&other.public_key(), &id.node_id()).is_err());
    }

    #[test]
    fn signed_beacon_verifies_via_embedded_key() {
        let id = Identity::generate();
        let beacon = Beacon {
            pubkey: id.public_key(),
            seq: 1,
            callsign: "TEST-01".into(),
            desc: "test".into(),
            class: StationClass::Individual,
            caps: 3,
            profile: 0,
            seen: vec![],
            congestion: 0,
            battery: 255,
            pos: None,
            tone_id: 1,
        };
        let pkt = Packet::new(
            Header::broadcast(1, id.node_id(), Scope::Direct),
            Payload::Beacon(beacon),
        );
        let signer = id.packet_signer();
        let wire = pkt.encode(Some(&signer)).unwrap();
        let decoded = ucr_protocol::decode(&wire).unwrap();
        assert_eq!(
            check_signature(&decoded, |_| None),
            SignatureStatus::Verified
        );
    }

    #[test]
    fn embedded_key_must_bind_to_src() {
        let id = Identity::generate();
        let evil = Identity::generate();
        // beacon claims evil's node_id but carries id's pubkey and sig
        let beacon = Beacon {
            pubkey: id.public_key(),
            seq: 1,
            callsign: "SPOOF".into(),
            desc: "".into(),
            class: StationClass::Individual,
            caps: 0,
            profile: 0,
            seen: vec![],
            congestion: 0,
            battery: 255,
            pos: None,
            tone_id: 0,
        };
        let pkt = Packet::new(
            Header::broadcast(1, evil.node_id(), Scope::Direct),
            Payload::Beacon(beacon),
        );
        // encode succeeds (signing is fine) but decode-level validate() should
        // already reject pubkey/src mismatch.
        let signer = id.packet_signer();
        let wire = pkt.encode(Some(&signer)).unwrap();
        assert!(ucr_protocol::decode(&wire).is_err());
    }
}
