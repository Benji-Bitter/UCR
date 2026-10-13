//! Hardware detection for UCR accessories.
//!
//! The host can't trust a USB port's name — a UCR modem identifies itself
//! by answering a `PING` control frame with `IDENT_PREFIX + info`, a
//! handshake that stays on the serial link (never transmitted over RF) and
//! works even on receive-only firmware builds.
//!
//! `serial_candidates` is cheap enumeration only. `probe_modem` opens a
//! port and runs the handshake; `detect_serial_devices` combines both.

use std::io::Read;
use std::time::{Duration, Instant};

use serialport::SerialPortType;

use crate::serial::{write_frame, IDENT_PREFIX, PING};

/// One candidate serial device.
#[derive(Clone, Debug)]
pub struct SerialCandidate {
    pub path: String,
    /// Human description — USB product/manufacturer, or port type.
    pub label: String,
    pub vid: Option<u16>,
    pub pid: Option<String>,
    /// True only when the device answered the UCR modem handshake.
    pub is_modem: bool,
    /// The modem's info string, e.g. `ucr-modem/0.1 sf9 bw125`.
    pub modem_info: Option<String>,
}

/// Enumerate serial ports worth probing. Skips Bluetooth and other
/// non-USB ports that can never be a UCR modem.
pub fn serial_candidates() -> Vec<SerialCandidate> {
    let ports = match serialport::available_ports() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    ports
        .into_iter()
        .filter_map(|p| {
            let (label, vid, pid) = match &p.port_type {
                SerialPortType::UsbPort(u) => {
                    let product = u
                        .product
                        .clone()
                        .or_else(|| u.manufacturer.clone())
                        .unwrap_or_else(|| "usb serial".into());
                    (product, Some(u.vid), Some(format!("{:04x}", u.pid)))
                }
                SerialPortType::PciPort => return None,
                SerialPortType::BluetoothPort => return None,
                SerialPortType::Unknown => ("serial".to_string(), None, None),
            };
            // macOS: /dev/tty.* entries are incoming stubs — only cu.* is
            // for opening. Skip BT/debug leftovers either way.
            if p.port_name.contains("Bluetooth") || p.port_name.contains("debug") {
                return None;
            }
            Some(SerialCandidate {
                path: p.port_name,
                label,
                vid,
                pid,
                is_modem: false,
                modem_info: None,
            })
        })
        .collect()
}

/// Open `path`, send `UCRPING`, and wait up to `timeout` for an
/// `UCRMODEM <info>` reply. Returns the info string on success.
///
/// Opens and closes the port itself — safe to run while nothing else
/// holds it. A non-UCR device just sees 9 harmless bytes.
pub fn probe_modem(path: &str, baud: u32, timeout: Duration) -> Option<String> {
    let mut port = serialport::new(path, baud)
        .timeout(Duration::from_millis(50))
        .open()
        .ok()?;
    if write_frame(&mut port, PING).is_err() {
        return None;
    }
    let deadline = Instant::now() + timeout;
    let mut buf = Vec::with_capacity(256);
    let mut chunk = [0u8; 256];
    while Instant::now() < deadline {
        match port.read(&mut chunk) {
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(info) = extract_ident(&buf) {
                    return Some(info);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => return None,
        }
    }
    None
}

/// Enumerate + probe. Ports that answer the handshake are flagged
/// `is_modem` with their info string attached.
pub fn detect_serial_devices(baud: u32, timeout: Duration) -> Vec<SerialCandidate> {
    let mut devs = serial_candidates();
    for d in &mut devs {
        if let Some(info) = probe_modem(&d.path, baud, timeout) {
            d.is_modem = true;
            d.modem_info = Some(info);
        }
    }
    devs
}

/// Scan accumulated bytes for a frame whose payload is an ident reply.
fn extract_ident(buf: &[u8]) -> Option<String> {
    let mut i = 0;
    while i + 4 <= buf.len() {
        if buf[i] == 0x55 && buf[i + 1] == 0xAA {
            let len = u16::from_le_bytes([buf[i + 2], buf[i + 3]]) as usize;
            let need = 4 + len + 1;
            if i + need > buf.len() {
                break; // incomplete frame — wait for more bytes
            }
            let payload = &buf[i + 4..i + 4 + len];
            if payload.starts_with(IDENT_PREFIX) {
                return Some(String::from_utf8_lossy(&payload[IDENT_PREFIX.len()..]).into_owned());
            }
            i += need; // skip this frame, keep scanning
        } else {
            i += 1; // hunt for sync
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn framed(payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0x55, 0xAA];
        v.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        v.extend_from_slice(payload);
        v.push(0); // crc value irrelevant to extract_ident
        v
    }

    #[test]
    fn ident_frame_decodes() {
        let buf = framed(b"UCRMODEM ucr-modem/0.1 sf9");
        assert_eq!(extract_ident(&buf).as_deref(), Some("ucr-modem/0.1 sf9"));
    }

    #[test]
    fn ident_survives_leading_junk() {
        let mut buf = vec![0xDE, 0xAD, 0x00];
        buf.extend(framed(b"UCRPING")); // a stray non-ident frame first
        buf.extend(framed(b"UCRMODEM test-fw"));
        assert_eq!(extract_ident(&buf).as_deref(), Some("test-fw"));
    }

    #[test]
    fn truncated_frame_waits_for_more() {
        let buf = framed(b"UCRMODEM x");
        assert_eq!(extract_ident(&buf[..buf.len() - 3]), None);
    }
}
