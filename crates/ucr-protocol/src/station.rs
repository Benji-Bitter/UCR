//! Station classification and capability flags.

use serde::{Deserialize, Serialize};

/// Self-declared station classification.
///
/// This is advisory metadata for display and routing heuristics. It does NOT
/// confer any trusted authority: `EmergencyVolunteer` (or any other class)
/// must never be treated as verified emergency-services identity. Verifiable
/// organisation credentials are a planned future extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StationClass {
    Individual,
    Boat,
    Vehicle,
    Truck,
    BaseStation,
    Portable,
    CommunityStation,
    RelayNode,
    EmergencyVolunteer,
    Other,
}

impl StationClass {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Individual => "Individual",
            Self::Boat => "Boat",
            Self::Vehicle => "Vehicle",
            Self::Truck => "Truck",
            Self::BaseStation => "Base Station",
            Self::Portable => "Portable",
            Self::CommunityStation => "Community Station",
            Self::RelayNode => "Relay Node",
            Self::EmergencyVolunteer => "Emergency Volunteer",
            Self::Other => "Other",
        }
    }

    pub fn all() -> &'static [StationClass] {
        &[
            Self::Individual,
            Self::Boat,
            Self::Vehicle,
            Self::Truck,
            Self::BaseStation,
            Self::Portable,
            Self::CommunityStation,
            Self::RelayNode,
            Self::EmergencyVolunteer,
            Self::Other,
        ]
    }
}

/// Node capability bitflags advertised in beacons.
pub mod caps {
    /// Node is willing to relay packets for others.
    pub const RELAY: u16 = 1 << 0;
    /// Node bridges to another transport (e.g. internet gateway, second band).
    pub const GATEWAY: u16 = 1 << 1;
    /// Node has trusted position information.
    pub const POSITIONED: u16 = 1 << 2;
    /// Node can receive digital voice frames.
    pub const VOICE_RX: u16 = 1 << 3;
    /// Node can transmit digital voice frames.
    pub const VOICE_TX: u16 = 1 << 4;
    /// Node agrees to relay priority emergency traffic.
    pub const EMERGENCY_RELAY: u16 = 1 << 5;
    /// Node understands digital Morse payloads.
    pub const MORSE: u16 = 1 << 6;
    /// Node is solar / hard-wired powered (good relay candidate).
    pub const POWERED: u16 = 1 << 7;
    /// Node persists messages for store-and-forward.
    pub const STORE_FORWARD: u16 = 1 << 8;
    /// Node supports low duty-cycle regions without dropping traffic.
    pub const DUTY_CYCLE_AWARE: u16 = 1 << 9;
}

/// Coarse geographic position, optionally attached to beacons and emergency
/// traffic. Coordinates are degrees × 1e5 (~1.1 m resolution); senders SHOULD
/// quantise and set `precision_m` to reflect how much accuracy is disclosed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoarsePos {
    pub lat_e5: i32,
    pub lon_e5: i32,
    /// Radius of uncertainty in metres. `u16::MAX` = unspecified.
    pub precision_m: u16,
}

impl CoarsePos {
    pub fn new(lat: f64, lon: f64, precision_m: u16) -> Self {
        Self {
            lat_e5: (lat * 1e5) as i32,
            lon_e5: (lon * 1e5) as i32,
            precision_m,
        }
    }

    pub fn lat(&self) -> f64 {
        self.lat_e5 as f64 / 1e5
    }

    pub fn lon(&self) -> f64 {
        self.lon_e5 as f64 / 1e5
    }

    /// Approximate great-circle distance to another position in metres.
    pub fn distance_m(&self, other: &CoarsePos) -> f64 {
        let r = 6_371_000.0_f64;
        let d_lat = to_radians(other.lat() - self.lat());
        let d_lon = to_radians(other.lon() - self.lon());
        let a = powi(sin(d_lat / 2.0), 2)
            + cos(to_radians(self.lat()))
                * cos(to_radians(other.lat()))
                * powi(sin(d_lon / 2.0), 2);
        2.0 * r * asin(sqrt(a))
    }
}

#[cfg(feature = "std")]
#[inline]
fn to_radians(d: f64) -> f64 {
    d.to_radians()
}
#[cfg(not(feature = "std"))]
#[inline]
fn to_radians(d: f64) -> f64 {
    d * (core::f64::consts::PI / 180.0)
}
#[cfg(feature = "std")]
#[inline]
fn sin(x: f64) -> f64 {
    x.sin()
}
#[cfg(not(feature = "std"))]
#[inline]
fn sin(x: f64) -> f64 {
    libm::sin(x)
}
#[cfg(feature = "std")]
#[inline]
fn cos(x: f64) -> f64 {
    x.cos()
}
#[cfg(not(feature = "std"))]
#[inline]
fn cos(x: f64) -> f64 {
    libm::cos(x)
}
#[cfg(feature = "std")]
#[inline]
fn asin(x: f64) -> f64 {
    x.asin()
}
#[cfg(not(feature = "std"))]
#[inline]
fn asin(x: f64) -> f64 {
    libm::asin(x)
}
#[cfg(feature = "std")]
#[inline]
fn sqrt(x: f64) -> f64 {
    x.sqrt()
}
#[cfg(not(feature = "std"))]
#[inline]
fn sqrt(x: f64) -> f64 {
    libm::sqrt(x)
}
#[cfg(feature = "std")]
#[inline]
fn powi(x: f64, n: i32) -> f64 {
    x.powi(n)
}
#[cfg(not(feature = "std"))]
#[inline]
fn powi(x: f64, n: i32) -> f64 {
    libm::pow(x, n as f64)
}
