//! Wire packet assembly, encoding and decoding.
//!
//! ## Signed region
//!
//! Relay hops mutate `ttl`, `hops`, `relay` and `next_hop`, so the signature
//! covers only the immutable end-to-end fields:
//!
//! ```text
//! SignedPortion = postcard({ packet_id, src, dst, scope, prio, want_ack, payload })
//! WireBody      = postcard({ mh: MutableHeader, sp: Vec<u8> })   // sp = SignedPortion bytes
//! wire          = magic | version | wflags | WireBody | sig(64B, optional)
//! ```
//!
//! A forwarded packet keeps its original signature — end-to-end authenticity
//! survives arbitrary relays, and relays cannot tamper with the signed
//! portion without detection. Cryptographic signing is injected by the
//! caller so this crate stays free of a specific Ed25519 implementation.

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::error::ProtocolError;
use crate::header::{Header, Priority, Scope};
use crate::ids::{NodeId, PacketId};
use crate::payload::Payload;
use crate::{MAGIC, PROTOCOL_VERSION};

const WFLAG_SIGNED: u8 = 0x01;

/// Immutable, signature-covered end-to-end fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignedPortion {
    pub packet_id: PacketId,
    pub src: NodeId,
    pub dst: NodeId,
    pub scope: Scope,
    pub prio: Priority,
    pub want_ack: bool,
    pub payload: Payload,
}

/// Per-hop mutable fields — not signature covered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MutableHeader {
    pub ttl: u8,
    pub hops: u8,
    pub relay: NodeId,
    pub next_hop: NodeId,
}

/// The serialised body on the wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct WireBody {
    pub mh: MutableHeader,
    /// postcard-encoded `SignedPortion` — the bytes a signature covers.
    pub sp: Vec<u8>,
}

/// A complete packet — header, payload, optional signature.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Packet {
    pub header: Header,
    pub payload: Payload,
    /// Ed25519 signature over the encoded `SignedPortion`.
    pub signature: Option<crate::ids::Signature>,
}

/// Signature callback: signs `SignedPortion` bytes, returns 64 bytes.
pub type PacketSigner<'a> = dyn Fn(&[u8]) -> [u8; 64] + 'a;

impl Packet {
    pub fn new(header: Header, payload: Payload) -> Self {
        Self {
            header,
            payload,
            signature: None,
        }
    }

    /// Extract the immutable portion the signature covers.
    pub fn signed_portion(&self) -> SignedPortion {
        SignedPortion {
            packet_id: self.header.packet_id,
            src: self.header.src,
            dst: self.header.dst,
            scope: self.header.scope,
            prio: self.header.prio,
            want_ack: self.header.want_ack,
            payload: self.payload.clone(),
        }
    }

    fn mutable_header(&self) -> MutableHeader {
        MutableHeader {
            ttl: self.header.ttl,
            hops: self.header.hops,
            relay: self.header.relay,
            next_hop: self.header.next_hop,
        }
    }

    /// Postcard-encode the `SignedPortion` — these exact bytes get signed.
    pub fn signed_bytes(&self) -> Result<Vec<u8>, ProtocolError> {
        Ok(postcard::to_allocvec(&self.signed_portion())?)
    }

    /// Encode to the wire. `signer`, if provided, is called with the
    /// `SignedPortion` bytes and must return a 64-byte signature.
    pub fn encode(&self, signer: Option<&PacketSigner>) -> Result<Vec<u8>, ProtocolError> {
        let sp = self.signed_bytes()?;
        let body = postcard::to_allocvec(&WireBody {
            mh: self.mutable_header(),
            sp: sp.clone(),
        })?;
        let sig = match signer {
            Some(s) => Some(crate::ids::Signature(s(&sp))),
            None => self.signature,
        };
        let mut out = Vec::with_capacity(body.len() + 67);
        out.push(MAGIC);
        out.push(PROTOCOL_VERSION);
        out.push(if sig.is_some() { WFLAG_SIGNED } else { 0 });
        out.extend_from_slice(&body);
        if let Some(s) = sig {
            out.extend_from_slice(&s.0);
        }
        Ok(out)
    }

    /// Field-length and structural validation applied to every decoded
    /// packet, in addition to transport MTU checks.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        use crate::payload::Payload;
        if self.header.ttl > self.header.scope.ttl() {
            return Err(ProtocolError::Malformed("ttl exceeds scope budget"));
        }
        match &self.payload {
            Payload::Beacon(b) => {
                if b.callsign.len() > crate::CALLSIGN_MAX {
                    return Err(ProtocolError::FieldTooLong("callsign"));
                }
                if b.desc.len() > crate::DESC_MAX {
                    return Err(ProtocolError::FieldTooLong("desc"));
                }
                if b.seen.len() > crate::BEACON_SEEN_MAX {
                    return Err(ProtocolError::Malformed("too many seen entries"));
                }
                if crate::ids::NodeId::from_pubkey(&b.pubkey) != self.header.src {
                    return Err(ProtocolError::Malformed("beacon pubkey/src mismatch"));
                }
            }
            Payload::Data(d) => match &d.app {
                crate::payload::AppData::ChannelText(c) => {
                    if c.name.len() > crate::CHANNEL_NAME_MAX {
                        return Err(ProtocolError::FieldTooLong("channel name"));
                    }
                    if c.text.len() > crate::TEXT_MAX {
                        return Err(ProtocolError::FieldTooLong("channel text"));
                    }
                    if c.channel != crate::ids::ChannelId::from_name(&c.name) {
                        return Err(ProtocolError::Malformed("channel id/name mismatch"));
                    }
                }
                crate::payload::AppData::Plain(p) => {
                    if let crate::payload::PlainApp::Morse { text, .. } = p {
                        if text.len() > crate::TEXT_MAX {
                            return Err(ProtocolError::FieldTooLong("morse text"));
                        }
                    }
                }
                crate::payload::AppData::Encrypted(e) => {
                    if e.ct.len() > crate::MAX_PACKET_SIZE {
                        return Err(ProtocolError::FieldTooLong("ciphertext"));
                    }
                }
                crate::payload::AppData::GroupEncrypted(g) => {
                    if g.ct.len() > crate::MAX_PACKET_SIZE {
                        return Err(ProtocolError::FieldTooLong("ciphertext"));
                    }
                }
            },
            Payload::RouteRequest(r) => {
                if r.path.len() > crate::ROUTE_PATH_MAX {
                    return Err(ProtocolError::Malformed("rreq path too long"));
                }
            }
            Payload::Emergency(e) => {
                if e.text.len() > crate::TEXT_MAX {
                    return Err(ProtocolError::FieldTooLong("emergency text"));
                }
                if crate::ids::NodeId::from_pubkey(&e.pubkey) != self.header.src {
                    return Err(ProtocolError::Malformed("emergency pubkey/src mismatch"));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Result of decoding a wire frame.
pub struct DecodedPacket {
    pub packet: Packet,
    /// The exact `SignedPortion` bytes the signature (if any) covers —
    /// feed these to the verifier together with the signature.
    pub signed_region: Option<Vec<u8>>,
    /// Signature present on the wire (unverified at this layer!).
    pub has_signature: bool,
}

/// Decode a wire frame. Performs structural validation only; signature
/// *verification* requires the sender's public key (`ucr-identity`).
pub fn decode(frame: &[u8]) -> Result<DecodedPacket, ProtocolError> {
    if frame.len() < 4 {
        return Err(ProtocolError::Malformed("frame too short"));
    }
    if frame[0] != MAGIC {
        return Err(ProtocolError::BadMagic);
    }
    if frame[1] != PROTOCOL_VERSION {
        return Err(ProtocolError::BadVersion(frame[1]));
    }
    if frame[2] & !WFLAG_SIGNED != 0 {
        return Err(ProtocolError::Malformed("unknown wire flags"));
    }
    let signed = frame[2] & WFLAG_SIGNED != 0;
    if frame.len() > crate::MAX_PACKET_SIZE {
        return Err(ProtocolError::PacketTooLarge {
            size: frame.len(),
            mtu: crate::MAX_PACKET_SIZE,
        });
    }
    let (body, sig) = if signed {
        if frame.len() < 3 + 64 + 1 {
            return Err(ProtocolError::Malformed("signed frame too short"));
        }
        let split = frame.len() - 64;
        let mut s = [0u8; 64];
        s.copy_from_slice(&frame[split..]);
        (&frame[3..split], Some(crate::ids::Signature(s)))
    } else {
        (&frame[3..], None)
    };
    let wb: WireBody = postcard::from_bytes(body)?;
    let sp: SignedPortion = postcard::from_bytes(&wb.sp)?;
    let packet = Packet {
        header: Header {
            packet_id: sp.packet_id,
            src: sp.src,
            dst: sp.dst,
            relay: wb.mh.relay,
            next_hop: wb.mh.next_hop,
            ttl: wb.mh.ttl,
            scope: sp.scope,
            hops: wb.mh.hops,
            prio: sp.prio,
            want_ack: sp.want_ack,
        },
        payload: sp.payload,
        signature: sig,
    };
    packet.validate()?;
    Ok(DecodedPacket {
        packet,
        signed_region: if signed { Some(wb.sp) } else { None },
        has_signature: signed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::Header;
    use crate::ids::NodeId;
    use crate::payload::{Beacon, RouteRequest};

    fn test_packet() -> Packet {
        let src = NodeId([7u8; 16]);
        Packet::new(
            Header::broadcast(42, src, Scope::Local),
            Payload::RouteRequest(RouteRequest {
                rreq_id: 9,
                target: NodeId([3u8; 16]),
                metric_acc: 0,
                path: alloc::vec![],
            }),
        )
    }

    #[test]
    fn roundtrip_unsigned() {
        let p = test_packet();
        let wire = p.encode(None).unwrap();
        let d = decode(&wire).unwrap();
        assert!(!d.has_signature);
        assert_eq!(d.packet, p);
    }

    #[test]
    fn roundtrip_signed() {
        let p = test_packet();
        let sig = [0xABu8; 64];
        let wire = p.encode(Some(&|_| sig)).unwrap();
        let d = decode(&wire).unwrap();
        assert!(d.has_signature);
        assert_eq!(d.packet.signature, Some(crate::ids::Signature(sig)));
        // signed region is the encoded SignedPortion
        assert_eq!(
            d.signed_region.as_ref().unwrap(),
            &p.signed_bytes().unwrap()
        );
    }

    #[test]
    fn signature_survives_forwarding() {
        let p = test_packet();
        let sig = [0x11u8; 64];
        let wire = p.encode(Some(&|_| sig)).unwrap();
        let mut d = decode(&wire).unwrap();
        // Simulate a relay mutating hop fields.
        d.packet.header.ttl -= 1;
        d.packet.header.hops += 1;
        d.packet.header.relay = NodeId([9u8; 16]);
        let wire2 = d.packet.encode(None).unwrap();
        let d2 = decode(&wire2).unwrap();
        // The signed bytes must be identical after relaying.
        assert_eq!(d.signed_region, d2.signed_region);
        assert_eq!(d2.packet.header.hops, p.header.hops + 1);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut wire = test_packet().encode(None).unwrap();
        wire[0] = 0x00;
        assert!(matches!(decode(&wire), Err(ProtocolError::BadMagic)));
    }

    #[test]
    fn rejects_bad_version() {
        let mut wire = test_packet().encode(None).unwrap();
        wire[1] = 0xEE;
        assert!(matches!(
            decode(&wire),
            Err(ProtocolError::BadVersion(0xEE))
        ));
    }

    #[test]
    fn beacon_validates_pubkey_binding() {
        let src = NodeId([1u8; 16]);
        let b = Beacon {
            pubkey: [9u8; 32],
            seq: 0,
            callsign: "TEST-01".into(),
            desc: "".into(),
            class: crate::station::StationClass::Individual,
            caps: 0,
            profile: 0,
            seen: alloc::vec![],
            congestion: 0,
            battery: 255,
            pos: None,
            tone_id: 0,
        };
        let p = Packet::new(Header::broadcast(1, src, Scope::Direct), Payload::Beacon(b));
        let wire = p.encode(None).unwrap();
        assert!(decode(&wire).is_err());
    }

    #[test]
    fn channel_name_id_binding_checked() {
        use crate::payload::{AppData, ChannelText, DataMsg};
        let src = NodeId([1u8; 16]);
        let ct = ChannelText {
            channel: crate::ids::ChannelId::from_name("ucr-general"),
            name: "wrong-name".into(),
            category: crate::channel::ChannelCategory::General,
            scope: Scope::Local,
            msg_id: 1,
            text: "hi".into(),
        };
        let p = Packet::new(
            Header::broadcast(1, src, Scope::Local),
            Payload::Data(DataMsg {
                app: AppData::ChannelText(ct),
            }),
        );
        let wire = p.encode(None).unwrap();
        assert!(decode(&wire).is_err());
    }
}
