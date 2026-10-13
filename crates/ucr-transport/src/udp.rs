//! UDP LAN broadcast transport.
//!
//! The closest honest analogue of "shout into the air" on an IP network:
//! every datagram is one complete UCR wire frame sent to the subnet
//! broadcast address, and every UCR node on the LAN hears it. Zero config,
//! no gateway — open the same port on two machines (or two processes) and
//! they are on the same channel. `SO_REUSEADDR`/`SO_REUSEPORT` allow
//! several nodes per host, like several radios on one desk.
//!
//! There is no RF measurement over IP, so RSSI/SNR report `unmeasured`
//! rather than fabricating values.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use async_trait::async_trait;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use ucr_protocol::profile::profile_ids;
use ucr_protocol::MAX_PACKET_SIZE;

use crate::{RxMeta, Transport, TransportError, TransportKind};

/// Default UDP port for LAN broadcast UCR traffic.
pub const DEFAULT_UDP_PORT: u16 = 4401;

pub struct UdpBroadcastTransport {
    sock: UdpSocket,
    dest: parking_lot::Mutex<SocketAddr>,
    epoch: Instant,
    tx_count: AtomicU64,
    rx_count: AtomicU64,
}

impl std::fmt::Debug for UdpBroadcastTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdpBroadcastTransport")
            .field("dest", &self.dest)
            .field("local", &self.sock.local_addr().ok())
            .finish()
    }
}

impl UdpBroadcastTransport {
    /// Bind `0.0.0.0:port` and broadcast to `255.255.255.255:port`.
    /// Multiple nodes may share the port on one host.
    pub fn bind(port: u16) -> std::io::Result<Self> {
        Self::bind_to(port, ([255, 255, 255, 255], port).into())
    }

    /// Bind and broadcast to a specific destination (e.g. a subnet-directed
    /// or multicast address). Datagrams still carry raw UCR wire frames.
    pub fn bind_to(port: u16, dest: SocketAddr) -> std::io::Result<Self> {
        let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
        sock.set_reuse_address(true)?;
        #[cfg(unix)]
        sock.set_reuse_port(true)?;
        sock.set_broadcast(true)?;
        sock.set_nonblocking(true)?;
        sock.bind(&SocketAddr::from(([0, 0, 0, 0], port)).into())?;
        let sock = UdpSocket::from_std(sock.into())?;
        Ok(Self {
            sock,
            dest: parking_lot::Mutex::new(dest),
            epoch: Instant::now(),
            tx_count: AtomicU64::new(0),
            rx_count: AtomicU64::new(0),
        })
    }

    /// The address this socket actually bound (port is OS-assigned when
    /// created with port 0).
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.sock.local_addr()
    }

    /// Redirect sends — e.g. point at a specific peer's socket for
    /// loopback testing, where broadcast doesn't exist.
    pub fn set_dest(&self, dest: SocketAddr) {
        *self.dest.lock() = dest;
    }

    /// Frames sent/received so far (for stats display).
    pub fn counters(&self) -> (u64, u64) {
        (
            self.tx_count.load(Ordering::Relaxed),
            self.rx_count.load(Ordering::Relaxed),
        )
    }
}

#[async_trait]
impl Transport for UdpBroadcastTransport {
    async fn send(&self, frame: &[u8]) -> Result<(), TransportError> {
        if frame.len() > MAX_PACKET_SIZE {
            return Err(TransportError::TooLarge(frame.len()));
        }
        let dest = *self.dest.lock();
        self.sock.send_to(frame, dest).await?;
        self.tx_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn recv(&self) -> Result<(Vec<u8>, RxMeta), TransportError> {
        let mut buf = vec![0u8; MAX_PACKET_SIZE];
        loop {
            let (n, _src) = self.sock.recv_from(&mut buf).await?;
            if n == 0 {
                continue;
            }
            self.rx_count.fetch_add(1, Ordering::Relaxed);
            return Ok((
                buf[..n].to_vec(),
                RxMeta::unmeasured(self.epoch.elapsed().as_millis() as u64),
            ));
        }
    }

    fn mtu(&self) -> usize {
        MAX_PACKET_SIZE
    }

    fn profile_id(&self) -> u8 {
        profile_ids::TCP
    }

    fn kind(&self) -> TransportKind {
        TransportKind::Udp
    }
}
