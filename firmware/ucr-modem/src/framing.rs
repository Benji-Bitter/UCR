//! Serial wire framing — host ↔ modem.
//!
//! Must stay byte-identical with `crates/ucr-transport/src/serial.rs`:
//! `[0x55 0xAA] [u16 len LE] [payload] [crc8]` in both directions.
//! Downstream (modem → host) payloads append the link metadata trailer:
//! `frame || rssi_i16_le || snr_i8 || 0xA5`.

pub const SYNC: [u8; 2] = [0x55, 0xAA];
pub const META_VALID: u8 = 0xA5;
pub const META_TRAILER: usize = 4;
/// Shared protocol bound — `ucr_protocol::MAX_PACKET_SIZE` (512).
/// Wire MTU for LoRa profiles is smaller (222) and enforced host-side.
pub const MAX_FRAME: usize = ucr_protocol::MAX_PACKET_SIZE;

/// CRC-8/SMBUS (poly 0x07) — identical to the host side.
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Streaming frame decoder — feed bytes, get complete payloads out.
pub struct Decoder {
    buf: [u8; MAX_FRAME + 8],
    len: usize,
}

impl Decoder {
    pub const fn new() -> Self {
        Self {
            buf: [0; MAX_FRAME + 8],
            len: 0,
        }
    }

    /// Feed one byte. Returns `Some(len)` with the payload in `payload()`
    /// when a complete valid frame arrived.
    pub fn feed(&mut self, byte: u8) -> Option<usize> {
        // Resync: drop everything until a SYNC pair starts the buffer.
        if self.len == 0 && byte != SYNC[0] {
            return None;
        }
        if self.len == 1 && byte != SYNC[1] {
            self.len = if byte == SYNC[0] { 1 } else { 0 };
            return None;
        }
        if self.len >= self.buf.len() {
            self.len = 0;
            return None;
        }
        self.buf[self.len] = byte;
        self.len += 1;
        if self.len < 4 {
            return None;
        }
        let want = u16::from_le_bytes([self.buf[2], self.buf[3]]) as usize;
        if want == 0 || want > MAX_FRAME {
            self.len = 0;
            return None;
        }
        if self.len < 4 + want + 1 {
            return None;
        }
        let payload = &self.buf[4..4 + want];
        if crc8(payload) != self.buf[4 + want] {
            self.len = 0;
            return None;
        }
        Some(want)
    }

    #[allow(dead_code)] // used only in tx-enable builds
    pub fn payload(&self, len: usize) -> &[u8] {
        &self.buf[4..4 + len]
    }

    pub fn reset(&mut self) {
        self.len = 0;
    }
}

/// Encode `payload` (+ optional RSSI/SNR trailer) into `out`; returns len.
pub fn encode(payload: &[u8], rssi_snr: Option<(i16, i8)>, out: &mut [u8]) -> usize {
    let body_len = payload.len() + if rssi_snr.is_some() { META_TRAILER } else { 0 };
    out[0] = SYNC[0];
    out[1] = SYNC[1];
    out[2..4].copy_from_slice(&(body_len as u16).to_le_bytes());
    out[4..4 + payload.len()].copy_from_slice(payload);
    let mut end = 4 + payload.len();
    if let Some((rssi, snr)) = rssi_snr {
        out[end..end + 2].copy_from_slice(&rssi.to_le_bytes());
        out[end + 2] = snr as u8;
        out[end + 3] = META_VALID;
        end += META_TRAILER;
    }
    out[end] = crc8(&out[4..end]);
    end + 1
}
