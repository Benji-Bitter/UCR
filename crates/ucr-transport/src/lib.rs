//! # UCR Transport
//!
//! The transport abstraction decouples the UCR protocol from the physical
//! layer. Anything that can deliver opaque wire frames is a transport —
//! simulated RF, a Semtech SX1262 over SPI behind firmware, USB serial to a
//! radio accessory, BLE to a companion radio, or a TCP gateway.
//!
//! A transport is a *broadcast* medium abstraction: `send` transmits one
//! frame; `recv` yields every frame the medium delivered to this node along
//! with reception metadata (RSSI/SNR). Transports that are naturally
//! point-to-point (TCP, serial) emulate broadcast by delivering each frame to
//! the connected peer(s).
//!
//! Implementations provided here:
//! * `TcpTransport` — length-framed gateway transport (also used in tests).
//! * `SerialTransport` (feature `serial`) — USB serial to a UCR accessory
//!   speaking the framed protocol in `docs/protocol/serial.md`.
//!
//! The simulated RF transport lives in `ucr-simulator` — it is a full
//! transport, not a mock, so the entire protocol stack runs unmodified.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(feature = "serial")]
pub mod detect;
pub mod mux;
#[cfg(feature = "serial")]
pub mod serial;
pub mod tcp;
pub mod udp;

pub use mux::MuxTransport;
pub use tcp::{TcpTransport, TCP_PROTOCOL_VERSION};
pub use udp::{UdpBroadcastTransport, DEFAULT_UDP_PORT};

/// Reception metadata reported alongside each received frame.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RxMeta {
    /// Received signal strength in dBm, NaN if the transport cannot measure.
    pub rssi_dbm: f32,
    /// Signal-to-noise ratio in dB, NaN if unavailable.
    pub snr_db: f32,
    /// Timestamp of reception (milliseconds, transport-defined epoch).
    pub at_ms: u64,
}

impl RxMeta {
    /// Placeholder metadata for transports without RF measurements (TCP,
    /// serial adapters that don't report link stats). Nodes treat NaN
    /// honestly — link quality stays "unmeasured" instead of fabricated.
    pub fn unmeasured(at_ms: u64) -> Self {
        Self {
            rssi_dbm: f32::NAN,
            snr_db: f32::NAN,
            at_ms,
        }
    }

    pub fn measured(rssi_dbm: f32, snr_db: f32, at_ms: u64) -> Self {
        Self {
            rssi_dbm,
            snr_db,
            at_ms,
        }
    }
}

/// What kind of transport this is — drives UI labelling and capability logic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportKind {
    /// Deterministic simulated RF (ucr-simulator).
    SimulatedRf,
    /// Real radio hardware via USB serial accessory.
    Serial,
    /// Bluetooth Low Energy accessory link.
    Ble,
    /// TCP/IP gateway link.
    Tcp,
    /// UDP subnet broadcast — the LAN analogue of shouting into the air.
    Udp,
    /// Several transports combined behind one face.
    Mux,
    /// Direct SPI/embedded radio driver (in-firmware).
    Spi,
}

impl TransportKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SimulatedRf => "Simulated RF",
            Self::Serial => "USB Serial",
            Self::Ble => "Bluetooth LE",
            Self::Tcp => "TCP Gateway",
            Self::Udp => "LAN Broadcast",
            Self::Mux => "Multi-transport",
            Self::Spi => "SPI Radio",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("transport closed")]
    Closed,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("frame too large for transport ({0} bytes)")]
    TooLarge(usize),
    #[error("transmission not permitted on this transport/profile")]
    TxNotPermitted,
}

/// One physical/link layer. `send` is best-effort broadcast; `recv` waits for
/// the next inbound frame. Implementations must be `Send + Sync` and safe to
/// call concurrently from the node's tasks.
#[async_trait]
pub trait Transport: Send + Sync + Debug {
    /// Transmit a wire frame. The transport may drop (simulated loss) or
    /// delay (airtime) the frame — reliability is a protocol-layer concern.
    async fn send(&self, frame: &[u8]) -> Result<(), TransportError>;

    /// Receive the next inbound frame. Must not spin — implementations block
    /// asynchronously until a frame arrives or the transport closes.
    async fn recv(&self) -> Result<(Vec<u8>, RxMeta), TransportError>;

    /// Maximum frame size in bytes.
    fn mtu(&self) -> usize;

    /// The radio profile id this transport operates under.
    fn profile_id(&self) -> u8;

    /// Transport classification.
    fn kind(&self) -> TransportKind;

    /// Whether transmission is currently permitted (regulatory/profile
    /// gate — mirrors `RadioProfile::tx_allowed`).
    fn may_transmit(&self) -> bool {
        true
    }

    /// Locally measured channel-busy fraction 0..=255 (share of recent
    /// airtime heard occupied). 0 = unknown/not measured — callers must not
    /// treat 0 as "channel free" without checking `measures_congestion`.
    fn congestion(&self) -> u8 {
        0
    }

    /// Whether `congestion()` returns a real measurement.
    fn measures_congestion(&self) -> bool {
        false
    }
}
