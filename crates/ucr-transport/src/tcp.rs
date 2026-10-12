//! TCP gateway transport.
//!
//! Wire framing: `[u16 len BE] [frame bytes]`. Used by internet-assisted
//! gateway nodes and for multi-process testing. One TCP connection = one
//! point-to-point link; gateways that fan out over RF run a full UCR node
//! and re-broadcast on the radio transport per the normal relay rules.
//!
//! The protocol version is exchanged in a 4-byte hello on connect:
//! `[0x55, ver, profile_id, mtu_hi, mtu_lo]`. Mismatched versions are
//! rejected.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use ucr_protocol::profile::profile_ids;

use crate::{RxMeta, Transport, TransportError, TransportKind};

pub const TCP_PROTOCOL_VERSION: u8 = 1;
const MAX_FRAME: usize = ucr_protocol::MAX_PACKET_SIZE;

/// A point-to-point TCP transport. Construct via `connect`/`accept`.
pub struct TcpTransport {
    reader: Mutex<OwnedReadHalf>,
    writer: Mutex<OwnedWriteHalf>,
    mtu: usize,
    clock_offset_ms: Arc<AtomicU64>,
    peer: SocketAddr,
}

impl std::fmt::Debug for TcpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TcpTransport")
            .field("peer", &self.peer)
            .field("mtu", &self.mtu)
            .finish()
    }
}

impl TcpTransport {
    /// Outbound connection to `addr` (a UCR gateway).
    pub async fn connect(addr: SocketAddr, mtu: usize) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        Self::handshake(stream, mtu).await
    }

    /// Listen for a single inbound gateway connection.
    pub async fn accept(listener: &TcpListener, mtu: usize) -> std::io::Result<Self> {
        let (stream, _) = listener.accept().await?;
        Self::handshake(stream, mtu).await
    }

    async fn handshake(stream: TcpStream, mtu: usize) -> std::io::Result<Self> {
        let peer = stream.peer_addr()?;
        let (mut r, mut w) = stream.into_split();
        // symmetric hello — both sides write then read
        let mtu_c = mtu.min(MAX_FRAME) as u16;
        w.write_all(&[0x55, TCP_PROTOCOL_VERSION, profile_ids::TCP])
            .await?;
        w.write_all(&mtu_c.to_be_bytes()).await?;
        let mut hello = [0u8; 5];
        r.read_exact(&mut hello).await?;
        if hello[0] != 0x55 || hello[1] != TCP_PROTOCOL_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ucr tcp handshake failed (version mismatch?)",
            ));
        }
        let peer_mtu = u16::from_be_bytes([hello[3], hello[4]]) as usize;
        Ok(Self {
            reader: Mutex::new(r),
            writer: Mutex::new(w),
            mtu: (mtu_c as usize).min(peer_mtu),
            clock_offset_ms: Arc::new(AtomicU64::new(0)),
            peer,
        })
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn send(&self, frame: &[u8]) -> Result<(), TransportError> {
        if frame.len() > self.mtu {
            return Err(TransportError::TooLarge(frame.len()));
        }
        let mut w = self.writer.lock().await;
        w.write_all(&(frame.len() as u16).to_be_bytes()).await?;
        w.write_all(frame).await?;
        w.flush().await?;
        Ok(())
    }

    async fn recv(&self) -> Result<(Vec<u8>, RxMeta), TransportError> {
        let mut r = self.reader.lock().await;
        let mut lenb = [0u8; 2];
        r.read_exact(&mut lenb).await?;
        let len = u16::from_be_bytes(lenb) as usize;
        if len == 0 || len > MAX_FRAME {
            return Err(TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bad frame length",
            )));
        }
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf).await?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
            + self.clock_offset_ms.load(Ordering::Relaxed);
        Ok((buf, RxMeta::unmeasured(now)))
    }

    fn mtu(&self) -> usize {
        self.mtu
    }

    fn profile_id(&self) -> u8 {
        profile_ids::TCP
    }

    fn kind(&self) -> TransportKind {
        TransportKind::Tcp
    }
}
