//! Session establishment and per-peer session state.
//!
//! Flow:
//! 1. `SessionManager::initiate(peer)` → `SessionInit` payload + pending state.
//! 2. Responder: `accept(init_pkt)` → verifies freshness, generates its own
//!    ephemeral, derives the session key, returns `SessionAccept` + session.
//! 3. Initiator: `complete(accept_pkt)` → derives the same key.
//!
//! Replay protection: `SessionInit` carries a timestamp checked against a
//! 10-minute window. The packet signature (verified by `ucr-identity` before
//! this layer runs) binds the ephemeral to the sender's `NodeId`.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::rngs::OsRng;
use ucr_protocol::payload::{SessionAccept, SessionInit};
use ucr_protocol::NodeId;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

use crate::{aead_decrypt, aead_encrypt, derive_session, CryptoError};

/// Freshness window for handshake timestamps.
pub const HANDSHAKE_WINDOW_MS: u64 = 10 * 60 * 1000;
/// Sessions idle longer than this are eligible for renegotiation.
pub const SESSION_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct PendingSession {
    eph_secret: StaticSecret,
    eph_pub: [u8; 32],
    /// Bound into key derivation — immutable for the life of the pending
    /// handshake so every retransmitted `SessionInit` is identical and any
    /// `SessionAccept` it produces matches this pending state.
    timestamp_ms: u64,
    /// Last time the init was (re)sent — pacing only, not cryptographic.
    last_retry_ms: u64,
}

impl Drop for PendingSession {
    fn drop(&mut self) {
        self.eph_secret.zeroize();
    }
}

/// An established E2E session with one peer.
pub struct Session {
    pub peer: NodeId,
    pub session_id: u32,
    key: [u8; 32],
    pub established_ms: u64,
    pub last_used_ms: u64,
    /// Number of messages exchanged — for diagnostics.
    pub msg_count: u64,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl Session {
    pub fn encrypt(&mut self, plaintext: &[u8]) -> ([u8; 12], Vec<u8>) {
        self.last_used_ms = now_ms();
        self.msg_count += 1;
        // AAD must be identical on both ends — the session id is derived
        // from the same transcript on both sides, `peer` is not.
        aead_encrypt(&self.key, &self.session_id.to_be_bytes(), plaintext)
    }

    pub fn decrypt(&mut self, nonce: &[u8; 12], ct: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let pt = aead_decrypt(&self.key, &self.session_id.to_be_bytes(), nonce, ct)?;
        self.last_used_ms = now_ms();
        self.msg_count += 1;
        Ok(pt)
    }

    pub fn is_stale(&self) -> bool {
        now_ms().saturating_sub(self.established_ms) > SESSION_MAX_AGE_MS
    }
}

/// Owns handshake state and established sessions for one node.
pub struct SessionManager {
    me: NodeId,
    pending: HashMap<NodeId, PendingSession>,
    sessions: HashMap<NodeId, Session>,
    /// session_id → peer, for routing inbound ciphertexts.
    by_sid: HashMap<u32, NodeId>,
}

impl SessionManager {
    pub fn new(me: NodeId) -> Self {
        Self {
            me,
            pending: HashMap::new(),
            sessions: HashMap::new(),
            by_sid: HashMap::new(),
        }
    }

    /// Do we have a live session with `peer`?
    pub fn has_session(&self, peer: &NodeId) -> bool {
        self.sessions
            .get(peer)
            .map(|s| !s.is_stale())
            .unwrap_or(false)
    }

    /// Is a handshake already in flight with `peer`?
    pub fn has_pending(&self, peer: &NodeId) -> bool {
        self.pending.contains_key(peer)
    }

    /// Begin a handshake with `peer`. Returns the `SessionInit` to embed in a
    /// signed packet.
    pub fn initiate(&mut self, peer: NodeId) -> SessionInit {
        let eph_secret = StaticSecret::random_from_rng(OsRng);
        let eph_pub = PublicKey::from(&eph_secret).to_bytes();
        let timestamp_ms = now_ms();
        // Tear down an older established session being replaced.
        if let Some(old) = self.sessions.remove(&peer) {
            self.by_sid.remove(&old.session_id);
        }
        self.pending.insert(
            peer,
            PendingSession {
                eph_secret,
                eph_pub,
                timestamp_ms,
                last_retry_ms: timestamp_ms,
            },
        );
        SessionInit {
            pubkey: [0u8; 32], // filled by caller with its own long-term key
            eph_pub,
            timestamp_ms,
        }
    }

    /// Handle an inbound `SessionInit`. `peer` is the verified sender NodeId
    /// (packet signature must already be verified). Returns the `SessionAccept`
    /// payload to send back.
    pub fn accept(
        &mut self,
        peer: NodeId,
        init: &SessionInit,
    ) -> Result<SessionAccept, CryptoError> {
        let skew = now_ms().abs_diff(init.timestamp_ms);
        if skew > HANDSHAKE_WINDOW_MS {
            return Err(CryptoError::BadBinding);
        }
        let eph_secret = StaticSecret::random_from_rng(OsRng);
        let eph_pub_bytes = PublicKey::from(&eph_secret).to_bytes();
        let peer_pub = PublicKey::from(init.eph_pub);
        let shared = eph_secret.diffie_hellman(&peer_pub).to_bytes();
        let (key, session_id) = derive_session(
            &shared,
            &peer,
            &self.me,
            &init.eph_pub,
            &eph_pub_bytes,
            init.timestamp_ms,
        );
        if let Some(old) = self.sessions.remove(&peer) {
            self.by_sid.remove(&old.session_id);
        }
        self.by_sid.insert(session_id, peer);
        self.sessions.insert(
            peer,
            Session {
                peer,
                session_id,
                key,
                established_ms: now_ms(),
                last_used_ms: now_ms(),
                msg_count: 0,
            },
        );
        Ok(SessionAccept {
            pubkey: [0u8; 32], // caller fills
            eph_pub: eph_pub_bytes,
            session_id,
        })
    }

    /// Handle a `SessionAccept` replying to our earlier `initiate`.
    pub fn complete(&mut self, peer: NodeId, accept: &SessionAccept) -> Result<(), CryptoError> {
        let pending = self
            .pending
            .remove(&peer)
            .ok_or(CryptoError::NoPendingSession)?;
        let peer_pub = PublicKey::from(accept.eph_pub);
        let shared = pending.eph_secret.diffie_hellman(&peer_pub).to_bytes();
        let (key, session_id) = derive_session(
            &shared,
            &self.me,
            &peer,
            &pending.eph_pub,
            &accept.eph_pub,
            pending.timestamp_ms,
        );
        if session_id != accept.session_id {
            // Keep the pending handshake — a mismatched accept (stale or
            // malformed) must not kill retries for the real one.
            self.pending.insert(peer, pending);
            return Err(CryptoError::BadBinding);
        }
        if let Some(old) = self.sessions.remove(&peer) {
            self.by_sid.remove(&old.session_id);
        }
        self.by_sid.insert(session_id, peer);
        self.sessions.insert(
            peer,
            Session {
                peer,
                session_id,
                key,
                established_ms: now_ms(),
                last_used_ms: now_ms(),
                msg_count: 0,
            },
        );
        Ok(())
    }

    /// Look up which peer a ciphertext's `session_id` belongs to.
    pub fn peer_for_session(&self, session_id: u32) -> Option<NodeId> {
        self.by_sid.get(&session_id).copied()
    }

    /// Encrypt for an established peer.
    pub fn encrypt(&mut self, peer: &NodeId, plaintext: &[u8]) -> Option<(u32, [u8; 12], Vec<u8>)> {
        let s = self.sessions.get_mut(peer)?;
        let (n, ct) = s.encrypt(plaintext);
        Some((s.session_id, n, ct))
    }

    /// Decrypt an inbound ciphertext.
    pub fn decrypt(
        &mut self,
        session_id: u32,
        nonce: &[u8; 12],
        ct: &[u8],
    ) -> Result<(NodeId, Vec<u8>), CryptoError> {
        let peer = self
            .by_sid
            .get(&session_id)
            .copied()
            .ok_or(CryptoError::UnknownSession)?;
        let s = self
            .sessions
            .get_mut(&peer)
            .ok_or(CryptoError::UnknownSession)?;
        Ok((peer, s.decrypt(nonce, ct)?))
    }

    /// Pending handshakes due for retransmission — for the caller to resend
    /// the `SessionInit`. A lost handshake packet would otherwise leave the
    /// pending state stuck until session age-out. `last_retry_ms` is bumped
    /// so the next retry waits another `max_age_ms`; `timestamp_ms` stays
    /// fixed — it is bound into the derived session id, so retransmissions
    /// must be byte-identical to match any `SessionAccept` they produce.
    pub fn stale_pending(&mut self, max_age_ms: u64) -> Vec<(NodeId, SessionInit)> {
        let now = now_ms();
        self.pending
            .iter_mut()
            .filter(|(_, p)| now.saturating_sub(p.last_retry_ms) >= max_age_ms)
            .map(|(peer, p)| {
                p.last_retry_ms = now;
                (
                    *peer,
                    SessionInit {
                        pubkey: [0u8; 32], // caller fills
                        eph_pub: p.eph_pub,
                        timestamp_ms: p.timestamp_ms,
                    },
                )
            })
            .collect()
    }

    /// Forget a session (e.g. peer blocked, identity revoked).
    pub fn drop_session(&mut self, peer: &NodeId) {
        if let Some(s) = self.sessions.remove(peer) {
            self.by_sid.remove(&s.session_id);
        }
    }

    /// Iterate established sessions (peer, session_id, age).
    pub fn sessions(&self) -> impl Iterator<Item = (&NodeId, &Session)> {
        self.sessions.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ucr_protocol::NodeId;

    fn pair() -> (SessionManager, SessionManager, NodeId, NodeId) {
        let a = NodeId([1u8; 16]);
        let b = NodeId([2u8; 16]);
        (SessionManager::new(a), SessionManager::new(b), a, b)
    }

    #[test]
    fn handshake_roundtrip() {
        let (mut alice, mut bob, a, b) = pair();
        let init = alice.initiate(b);
        let accept = bob.accept(a, &init).unwrap();
        alice.complete(b, &accept).unwrap();

        assert!(alice.has_session(&b));
        assert!(bob.has_session(&a));

        // alice → bob
        let (sid, nonce, ct) = alice.encrypt(&b, b"hello bob").unwrap();
        let (peer, pt) = bob.decrypt(sid, &nonce, &ct).unwrap();
        assert_eq!(peer, a);
        assert_eq!(pt, b"hello bob");

        // bob → alice
        let (sid2, nonce2, ct2) = bob.encrypt(&a, b"hi alice").unwrap();
        let (peer2, pt2) = alice.decrypt(sid2, &nonce2, &ct2).unwrap();
        assert_eq!(peer2, b);
        assert_eq!(pt2, b"hi alice");
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let (mut alice, mut bob, _, b) = pair();
        let init = alice.initiate(b);
        let accept = bob.accept(NodeId([1u8; 16]), &init).unwrap();
        alice.complete(b, &accept).unwrap();
        let (sid, nonce, mut ct) = alice.encrypt(&b, b"secret").unwrap();
        ct[0] ^= 0xFF;
        assert!(bob.decrypt(sid, &nonce, &ct).is_err());
    }

    #[test]
    fn complete_without_pending_fails() {
        let (mut alice, _, _, b) = pair();
        let accept = SessionAccept {
            pubkey: [0; 32],
            eph_pub: [1; 32],
            session_id: 7,
        };
        assert!(matches!(
            alice.complete(b, &accept),
            Err(CryptoError::NoPendingSession)
        ));
    }

    #[test]
    fn stale_init_rejected() {
        let (mut alice, mut bob, _, b) = pair();
        let mut init = alice.initiate(b);
        init.timestamp_ms = now_ms() - HANDSHAKE_WINDOW_MS - 1;
        let a = NodeId([1u8; 16]);
        assert!(bob.accept(a, &init).is_err());
    }
}
