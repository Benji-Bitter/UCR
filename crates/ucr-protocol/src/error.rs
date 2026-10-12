//! Protocol errors.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("packet exceeds transport MTU ({size} > {mtu})")]
    PacketTooLarge { size: usize, mtu: usize },
    #[error("malformed packet: {0}")]
    Malformed(&'static str),
    #[error("bad magic byte")]
    BadMagic,
    #[error("unsupported protocol version {0}")]
    BadVersion(u8),
    #[error("signature invalid")]
    BadSignature,
    #[error("serialization error: {0}")]
    Codec(&'static str),
    #[error("field too long: {0}")]
    FieldTooLong(&'static str),
}

impl From<postcard::Error> for ProtocolError {
    fn from(_: postcard::Error) -> Self {
        ProtocolError::Codec("postcard")
    }
}
