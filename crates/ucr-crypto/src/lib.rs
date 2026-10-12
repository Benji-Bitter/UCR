//! # UCR Crypto
//!
//! End-to-end encryption built from reviewed primitives only — no custom
//! constructions beyond standard composition:
//!
//! * **Handshake** — ephemeral X25519 key exchange. `SessionInit`/`SessionAccept`
//!   packets are Ed25519-signed by the sender's long-term identity key (at the
//!   packet layer), binding the ephemeral keys to both `NodeId`s. Fresh
//!   ephemeral keys per session give forward secrecy.
//! * **Transport** — ChaCha20-Poly1305 AEAD. Random 96-bit nonces, never reused.
//! * **KDF** — HKDF-SHA256 over `X25519(eph_self, eph_peer)` with a transcript
//!   binding both node ids and both ephemeral keys.
//! * **Groups** — symmetric `ChaCha20-Poly1305` group keys rotated on every
//!   membership change. This is a pragmatic v1 mechanism — NOT MLS. It does
//!   not provide post-compromise security or per-member authentication inside
//!   the group ciphertext (packet-level signatures still authenticate senders).

mod session;

pub use session::*;

use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng as AeadOsRng};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("decryption failed (wrong key or tampered ciphertext)")]
    DecryptFailed,
    #[error("unknown session")]
    UnknownSession,
    #[error("session already exists")]
    SessionExists,
    #[error("no pending session for this peer")]
    NoPendingSession,
    #[error("handshake key binding failed")]
    BadBinding,
    #[error("serialization error")]
    Codec,
}

/// Encrypt `plaintext` under a raw 32-byte key with a random nonce.
/// Returns `(nonce, ciphertext)`; `aad` is authenticated but not encrypted.
pub fn aead_encrypt(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> ([u8; 12], Vec<u8>) {
    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut AeadOsRng);
    let ct = cipher
        .encrypt(
            &nonce,
            chacha20poly1305::aead::Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("aead encrypt");
    let mut n = [0u8; 12];
    n.copy_from_slice(&nonce);
    (n, ct)
}

/// Decrypt a ciphertext produced by `aead_encrypt`.
pub fn aead_decrypt(
    key: &[u8; 32],
    aad: &[u8],
    nonce: &[u8; 12],
    ct: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            chacha20poly1305::aead::Payload { msg: ct, aad },
        )
        .map_err(|_| CryptoError::DecryptFailed)
}

/// Derive the session key and id from a completed handshake transcript.
///
/// Both sides compute the same values: the salt binds the ordered node ids,
/// the info string binds both ephemeral keys and the initiator's timestamp.
pub fn derive_session(
    shared: &[u8; 32],
    node_init: &ucr_protocol::NodeId,
    node_resp: &ucr_protocol::NodeId,
    eph_init: &[u8; 32],
    eph_resp: &[u8; 32],
    timestamp_ms: u64,
) -> ([u8; 32], u32) {
    let mut salt = Vec::with_capacity(32);
    salt.extend_from_slice(&node_init.0);
    salt.extend_from_slice(&node_resp.0);
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(&salt), shared);
    let mut info = Vec::with_capacity(128);
    info.extend_from_slice(b"ucr-e2e-v1");
    info.extend_from_slice(eph_init);
    info.extend_from_slice(eph_resp);
    info.extend_from_slice(&timestamp_ms.to_be_bytes());
    let mut key = [0u8; 32];
    hk.expand(&info, &mut key).expect("hkdf expand");

    let mut sid_src = Vec::new();
    sid_src.extend_from_slice(b"ucr-session-id");
    sid_src.extend_from_slice(&key);
    let sid_hash = blake3::hash(&sid_src);
    let mut sid = [0u8; 4];
    sid.copy_from_slice(&sid_hash.as_bytes()[..4]);
    (key, u32::from_be_bytes(sid))
}

/// Derive the AAD used for group ciphertexts: binds group id + key epoch.
pub fn group_aad(group_id: &ucr_protocol::GroupId, epoch: u32) -> [u8; 12] {
    let mut aad = [0u8; 12];
    aad[..8].copy_from_slice(&group_id.0);
    aad[8..].copy_from_slice(&epoch.to_be_bytes());
    aad
}
