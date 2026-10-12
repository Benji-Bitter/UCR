//! # UCR Simulator
//!
//! A deterministic, seeded RF medium simulator. It is a real `Transport`
//! implementation — the full UCR protocol stack runs over it unmodified —
//! not a mock or a UI animation.
//!
//! Model:
//! * **Path loss** — free-space loss at the profile's nominal frequency, plus
//!   log-normal shadowing (seeded RNG) and per-obstacle attenuation.
//! * **Reception** — a frame is decoded when SNR at the receiver exceeds the
//!   spreading-factor threshold and no overlapping transmission interferes
//!   within 6 dB (capture effect approximation). Half-duplex: a node cannot
//!   receive while transmitting.
//! * **Airtime** — every transmission occupies the channel for
//!   `profile.airtime_ms(len)`; duty-cycle limits delay further TX.
//! * **Virtual time** — tests drive the clock explicitly (`step`,
//!   `run_for`); interactive use paces the clock against wall time.
//! * **Topology control** — positions, mobility waypoints, obstacles,
//!   blocked pairs (partitions), node enable/disable.
//!
//! Everything is driven by a seeded RNG: identical seed + identical event
//! sequence → identical outcome, making failures reproducible in CI.

mod medium;
mod node;
mod scenario;

pub use medium::{DeliveryReport, SimConfig, SimStats, SimWorld};
pub use node::SimTransport;
pub use scenario::{Mobility, Obstacle, Pos};

/// Nominal frequency used for path-loss when a profile declares none.
pub const NOMINAL_FREQ_HZ: f64 = 915e6;
/// Thermal noise floor (dBm) for a 125 kHz channel.
pub const NOISE_FLOOR_DBM: f32 = -125.0;
/// Minimum SNR (dB) required to decode, by spreading factor.
pub fn snr_threshold_db(sf: u8) -> f32 {
    match sf {
        5 => -5.0,
        6 => -7.5,
        7 => -7.5,
        8 => -10.0,
        9 => -12.5,
        10 => -15.0,
        11 => -17.5,
        _ => -20.0,
    }
}
/// Receiver sensitivity floor — below this nothing decodes regardless of SNR.
pub const SENSITIVITY_DBM: f32 = -137.0;
/// Capture margin: an overlapping interferer within this many dB of the
/// desired signal corrupts the frame.
pub const CAPTURE_MARGIN_DB: f32 = 6.0;
