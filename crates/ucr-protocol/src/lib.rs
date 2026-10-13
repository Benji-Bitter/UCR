//! # UCR Protocol
//!
//! Wire-format definitions for the Universal Citizens Radio protocol.
//!
//! This crate is `no_std` compatible (requiring only `alloc`) so the exact
//! same packet definitions can be compiled into microcontroller firmware.
//!
//! ## Wire format
//!
//! ```text
//! +--------+---------+-----------+--------------------------+------------------+
//! | magic  | version | wflags    | postcard(Header,Payload) | signature (opt.) |
//! | 1 byte | 1 byte  | 1 byte    | variable                 | 64 bytes         |
//! +--------+---------+-----------+--------------------------+------------------+
//! ```
//!
//! * `magic` — `0x55` ('U')
//! * `version` — protocol major version, currently `1`
//! * `wflags` — bit 0: signature present. Other bits reserved (must be 0).
//! * signature — Ed25519 over `magic | version | wflags | body` bytes.
//!
//! See `PROTOCOL.md` in the repository root for the normative specification.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod channel;
pub mod error;
pub mod header;
pub mod ids;
pub mod morse;
pub mod packet;
pub mod payload;
pub mod profile;
pub mod station;

pub use channel::*;
pub use error::*;
pub use header::*;
pub use ids::*;
pub use packet::*;
pub use payload::*;
pub use profile::*;
pub use station::*;

/// Protocol magic byte ('U').
pub const MAGIC: u8 = 0x55;
/// Current protocol version. Version bumps are for incompatible wire changes.
pub const PROTOCOL_VERSION: u8 = 1;

/// Absolute maximum wire packet size any transport may carry.
pub const MAX_PACKET_SIZE: usize = 512;

/// Maximum callsign length in bytes.
pub const CALLSIGN_MAX: usize = 24;
/// Maximum station description length in bytes.
pub const DESC_MAX: usize = 96;
/// Maximum channel name length in bytes.
pub const CHANNEL_NAME_MAX: usize = 32;
/// Maximum text message length in bytes (before encoding).
pub const TEXT_MAX: usize = 512;

/// Maximum encoded bytes in a channel voice frame. Sized for ~100 ms of
/// IMA-ADPCM 8kHz mono (400 B) inside the 512-byte packet budget.
pub const VOICE_FRAME_MAX: usize = 420;
/// Maximum number of recently-heard neighbours advertised in a beacon.
pub const BEACON_SEEN_MAX: usize = 6;
/// Maximum accumulated path length in route discovery packets.
pub const ROUTE_PATH_MAX: usize = 12;
/// Maximum encoded tone pattern length.
pub const TONE_NOTES_MAX: usize = 24;
/// Maximum group size.
pub const GROUP_MEMBERS_MAX: usize = 64;
/// Maximum group name length.
pub const GROUP_NAME_MAX: usize = 48;

/// Node identifier broadcast sentinel — all bits set.
pub const BROADCAST: NodeId = NodeId([0xFF; 16]);
