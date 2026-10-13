//! Radio profiles: region, modulation and regulatory parameters.
//!
//! A UCR radio profile describes *how* a radio transmits. The protocol is
//! profile-agnostic — routing, identity and application data are identical
//! across profiles — which is what allows different regional hardware to
//! interoperate through bridge nodes.
//!
//! IMPORTANT: profiles for licensed or restricted bands default to
//! `tx_allowed: false`. Never transmit without verifying that the profile is
//! authorised for the regulatory region and hardware in use.

use alloc::string::String;
use serde::{Deserialize, Serialize};

/// Modulation family. Parameters not applicable to a family are ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Modulation {
    /// Semtech LoRa chirp spread spectrum (uses `sf`, `cr`).
    LoRa,
    /// GFSK packet radio.
    Gfsk,
    /// Simulated transport — no real RF.
    Simulated,
}

/// Regulatory region tag. `Unverified` regions must not enable TX by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Region {
    /// Software simulator only.
    Simulated,
    /// Australia / NZ 915–928 MHz ISM.
    AuIsm915,
    /// EU 868 MHz ISM (duty-cycle limited).
    EuIsm868,
    /// US/CA 902–928 MHz ISM.
    UsIsm915,
    /// Generic 433 MHz ISM.
    Ism433,
    /// Region not yet verified by the project — receive-only.
    Unverified,
}

/// A complete description of one radio configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RadioProfile {
    /// Profile identifier carried in beacons (index into the node's table).
    pub id: u8,
    pub name: String,
    pub region: Region,
    pub modulation: Modulation,
    /// Centre frequency in Hz.
    pub freq_hz: u64,
    /// Channel bandwidth in Hz.
    pub bw_hz: u32,
    /// LoRa spreading factor 5..=12 (ignored for GFSK).
    pub sf: u8,
    /// LoRa coding rate denominator 5..=8 (4/cr), ignored otherwise.
    pub cr: u8,
    /// Conducted TX power limit in dBm.
    pub tx_power_dbm: i8,
    /// Effective payload bitrate in bits/s (used for airtime accounting).
    pub bitrate: u32,
    /// Maximum TX duty cycle in ‰ (per mille). 0 = unrestricted.
    /// e.g. EU868 sub-bands commonly allow 1% = 10 permille.
    pub duty_cycle_permille: u16,
    /// Transport MTU for this profile (wire bytes).
    pub mtu: usize,
    /// Whether transmissions on this profile are allowed. Profiles for
    /// unverified regions MUST default to false (receive-only).
    pub tx_allowed: bool,
}

impl RadioProfile {
    /// Estimated airtime for `n` wire bytes, in milliseconds.
    ///
    /// This is a payload-bitrate approximation suitable for airtime budgeting
    /// and duty-cycle accounting, not a substitute for the radio driver's own
    /// time-on-air calculation (which accounts for preamble, headers, etc.).
    pub fn airtime_ms(&self, bytes: usize) -> u64 {
        let bits = (bytes as u64) * 8;
        let base = bits.saturating_mul(1000) / self.bitrate.max(1) as u64;
        // approximate fixed overhead (preamble + header + ramp)
        base + 20
    }

    /// Whether this profile may transmit in the current configuration.
    pub fn may_transmit(&self) -> bool {
        self.tx_allowed
    }
}

/// Well-known profile identifiers.
pub mod profile_ids {
    /// Simulated radio (developer/testing). Always TX-allowed.
    pub const SIM: u8 = 0;
    /// AU 915–928 MHz LoRa profile — receive-only pending verification.
    pub const AU_LORA: u8 = 1;
    /// EU 868 MHz LoRa profile — receive-only pending verification.
    pub const EU_LORA: u8 = 2;
    /// US 902–928 MHz LoRa profile — receive-only pending verification.
    pub const US_LORA: u8 = 3;
    /// TCP/IP gateway transport profile.
    pub const TCP: u8 = 250;
}

/// Built-in profile table. Region profiles are receive-only by default —
/// see `docs/hardware/` for the regulatory verification process that a
/// deployment must complete before `tx_allowed` is set.
pub fn builtin_profiles() -> alloc::vec::Vec<RadioProfile> {
    alloc::vec![
        RadioProfile {
            id: profile_ids::SIM,
            name: "Simulator".into(),
            region: Region::Simulated,
            modulation: Modulation::Simulated,
            freq_hz: 0,
            bw_hz: 125_000,
            sf: 7,
            cr: 5,
            tx_power_dbm: 14,
            bitrate: 5_500,
            duty_cycle_permille: 0,
            mtu: MAX_PACKET_SIZE_SIM,
            tx_allowed: true,
        },
        RadioProfile {
            id: profile_ids::AU_LORA,
            name: "AU ISM 915 LoRa".into(),
            region: Region::AuIsm915,
            modulation: Modulation::LoRa,
            freq_hz: 916_000_000,
            bw_hz: 125_000,
            sf: 9,
            cr: 5,
            tx_power_dbm: 20,
            bitrate: 1_750,
            duty_cycle_permille: 0,
            mtu: 222,
            // Receive-only until the deployment completes the regulatory
            // checklist in docs/hardware/regulatory.md.
            tx_allowed: false,
        },
        RadioProfile {
            id: profile_ids::EU_LORA,
            name: "EU ISM 868 LoRa".into(),
            region: Region::EuIsm868,
            modulation: Modulation::LoRa,
            freq_hz: 868_100_000,
            bw_hz: 125_000,
            sf: 9,
            cr: 5,
            tx_power_dbm: 14,
            bitrate: 1_750,
            duty_cycle_permille: 10,
            mtu: 222,
            tx_allowed: false,
        },
        RadioProfile {
            id: profile_ids::US_LORA,
            name: "US ISM 915 LoRa".into(),
            region: Region::UsIsm915,
            modulation: Modulation::LoRa,
            freq_hz: 903_900_000,
            bw_hz: 125_000,
            sf: 9,
            cr: 5,
            tx_power_dbm: 22,
            bitrate: 1_750,
            duty_cycle_permille: 0,
            mtu: 222,
            tx_allowed: false,
        },
        RadioProfile {
            id: profile_ids::TCP,
            name: "TCP gateway".into(),
            region: Region::Simulated,
            modulation: Modulation::Simulated,
            freq_hz: 0,
            bw_hz: 0,
            sf: 0,
            cr: 0,
            tx_power_dbm: 0,
            bitrate: 10_000_000,
            duty_cycle_permille: 0,
            mtu: MAX_PACKET_SIZE_TCP,
            tx_allowed: true,
        },
    ]
}

/// MTU used by the simulated transport.
pub const MAX_PACKET_SIZE_SIM: usize = 400;
/// MTU used by TCP transports.
pub const MAX_PACKET_SIZE_TCP: usize = 512;
/// Largest wire packet one SX1262 LoRa frame can carry.
///
/// The chip's FIFO is 255 bytes; 222 matches the common regional payload
/// cap (EU868/US915) and leaves margin for preamble/header overhead in
/// future wire-format revisions. A UCR packet never fragments across RF
/// frames, so this is the hard ceiling on LoRa links.
pub const RF_LORA_MTU: usize = 222;

/// Look up a built-in profile by id.
pub fn profile_by_id(id: u8) -> Option<RadioProfile> {
    builtin_profiles().into_iter().find(|p| p.id == id)
}
