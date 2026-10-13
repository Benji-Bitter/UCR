//! USB serial transport for UCR radio accessories.
//!
//! Wire framing (host ↔ accessory): `[0x55 0xAA] [u16 len LE] [payload] [crc8]`.
//! The accessory firmware (see `firmware/esp32-s3`) terminates this framing
//! and relays payloads over RF. RSSI/SNR metadata arrives inside the
//! accessory's frame format (see `docs/protocol/serial.md`); frames that
//! carry no metadata report `RxMeta::unmeasured` — never fabricated.
//!
//! A dedicated blocking-reader thread feeds an async channel because
//! `serialport` is a synchronous API.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serialport::SerialPort;
use tokio::sync::{mpsc, Mutex};

use crate::{RxMeta, Transport, TransportError, TransportKind};

const SYNC: [u8; 2] = [0x55, 0xAA];
const MAX_FRAME: usize = ucr_protocol::MAX_PACKET_SIZE;
/// Accessory→host frame carrying link metadata:
/// `payload = frame || rssi_i16_le || snr_i8 || rssi_valid`
const META_TRAILER: usize = 4;

fn crc8(data: &[u8]) -> u8 {
    // CRC-8/SMBUS (poly 0x07)
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

/// Serial transport to a UCR accessory device.
pub struct SerialTransport {
    port: Mutex<Box<dyn SerialPort>>,
    rx: Mutex<mpsc::Receiver<(Vec<u8>, RxMeta)>>,
    alive: Arc<AtomicBool>,
    mtu: usize,
    port_name: String,
}

impl std::fmt::Debug for SerialTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SerialTransport")
            .field("port", &self.port_name)
            .finish()
    }
}

impl SerialTransport {
    /// Open `path` (e.g. `/dev/cu.usbserial-*`) at `baud` and start the
    /// reader thread.
    pub fn open(path: &str, baud: u32, mtu: usize) -> std::io::Result<Self> {
        let mut port = serialport::new(path, baud)
            .timeout(Duration::from_millis(50))
            .open()?;
        port.set_timeout(Duration::from_millis(50))?;
        let reader_port = port.try_clone()?;
        let (tx, rx) = mpsc::channel(256);
        let alive = Arc::new(AtomicBool::new(true));
        let alive2 = alive.clone();
        std::thread::Builder::new()
            .name("ucr-serial-rx".into())
            .spawn(move || reader_loop(reader_port, tx, alive2))
            .map_err(std::io::Error::other)?;
        Ok(Self {
            port: Mutex::new(port),
            rx: Mutex::new(rx),
            alive,
            mtu,
            port_name: path.to_string(),
        })
    }

    /// Enumerate candidate serial ports.
    pub fn list_ports() -> Vec<String> {
        serialport::available_ports()
            .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
            .unwrap_or_default()
    }
}

fn reader_loop(
    mut port: Box<dyn SerialPort>,
    tx: mpsc::Sender<(Vec<u8>, RxMeta)>,
    alive: Arc<AtomicBool>,
) {
    let mut buf = Vec::with_capacity(1024);
    let mut tmp = [0u8; 512];
    while alive.load(Ordering::Relaxed) {
        match port.read(&mut tmp) {
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                drain_frames(&mut buf, &tx);
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => {
                alive.store(false, Ordering::Relaxed);
                break;
            }
        }
    }
    let _ = tx.blocking_send((Vec::new(), RxMeta::unmeasured(0)));
}

fn drain_frames(buf: &mut Vec<u8>, tx: &mpsc::Sender<(Vec<u8>, RxMeta)>) {
    loop {
        // resync
        while buf.len() >= 2 && (buf[0] != SYNC[0] || buf[1] != SYNC[1]) {
            buf.remove(0);
        }
        if buf.len() < 5 {
            return;
        }
        let len = u16::from_le_bytes([buf[2], buf[3]]) as usize;
        if len == 0 || len > MAX_FRAME + META_TRAILER {
            buf.remove(0);
            continue;
        }
        if buf.len() < 4 + len + 1 {
            return;
        }
        let payload = &buf[4..4 + len];
        let crc = buf[4 + len];
        if crc8(payload) != crc {
            buf.drain(..2);
            continue;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        // Frame may carry an RSSI/SNR trailer appended by the accessory.
        let (frame, meta) = if len > META_TRAILER {
            let trailer_at = len - META_TRAILER;
            let rssi_raw = i16::from_le_bytes([payload[trailer_at], payload[trailer_at + 1]]);
            let snr_raw = payload[trailer_at + 2] as i8;
            let valid = payload[trailer_at + 3];
            if valid == 0xA5 && rssi_raw > -200 && rssi_raw < 0 {
                (
                    payload[..trailer_at].to_vec(),
                    RxMeta::measured(rssi_raw as f32, snr_raw as f32, now),
                )
            } else {
                (payload.to_vec(), RxMeta::unmeasured(now))
            }
        } else {
            (payload.to_vec(), RxMeta::unmeasured(now))
        };
        if tx.blocking_send((frame, meta)).is_err() {
            return;
        }
        buf.drain(..4 + len + 1);
    }
}

#[async_trait]
impl Transport for SerialTransport {
    async fn send(&self, frame: &[u8]) -> Result<(), TransportError> {
        if frame.len() > self.mtu() {
            return Err(TransportError::TooLarge(frame.len()));
        }
        if !self.alive.load(Ordering::Relaxed) {
            return Err(TransportError::Closed);
        }
        let mut pkt = Vec::with_capacity(frame.len() + 5);
        pkt.extend_from_slice(&SYNC);
        pkt.extend_from_slice(&(frame.len() as u16).to_le_bytes());
        pkt.extend_from_slice(frame);
        pkt.push(crc8(frame));
        let mut port = self.port.lock().await;
        port.write_all(&pkt)?;
        port.flush()?;
        Ok(())
    }

    async fn recv(&self) -> Result<(Vec<u8>, RxMeta), TransportError> {
        let mut rx = self.rx.lock().await;
        match rx.recv().await {
            Some((f, m)) if !f.is_empty() => Ok((f, m)),
            _ => Err(TransportError::Closed),
        }
    }

    fn mtu(&self) -> usize {
        // Serial links carry a UCR modem: a wire packet must fit one RF
        // frame, so the LoRa FIFO budget caps whatever the caller asked for.
        self.mtu
            .min(MAX_FRAME)
            .min(ucr_protocol::profile::RF_LORA_MTU)
    }

    fn profile_id(&self) -> u8 {
        ucr_protocol::profile::profile_ids::AU_LORA
    }

    fn kind(&self) -> TransportKind {
        TransportKind::Serial
    }
}
