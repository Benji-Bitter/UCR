//! Composite transport — several real links behind one `Transport` face.
//!
//! A real node rides whatever it has: LAN broadcast, one or more TCP
//! gateways, a serial modem. `MuxTransport` fans `send` out to every member
//! and merges all inbound frames into a single queue, so the node core
//! never cares which link a packet arrived on.

use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use ucr_protocol::profile::profile_ids;
use ucr_protocol::MAX_PACKET_SIZE;

use crate::{RxMeta, Transport, TransportError, TransportKind};

pub struct MuxTransport {
    members: Mutex<Vec<Arc<dyn Transport>>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    out_tx: mpsc::UnboundedSender<(Vec<u8>, RxMeta)>,
    in_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<(Vec<u8>, RxMeta)>>,
    tx_count: Arc<AtomicU64>,
    rx_count: Arc<AtomicU64>,
}

impl Debug for MuxTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MuxTransport")
            .field("members", &self.members.lock().len())
            .finish()
    }
}

impl MuxTransport {
    pub fn new() -> Arc<Self> {
        let (out_tx, in_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            members: Mutex::new(Vec::new()),
            tasks: Mutex::new(Vec::new()),
            out_tx,
            in_rx: tokio::sync::Mutex::new(in_rx),
            tx_count: Arc::new(AtomicU64::new(0)),
            rx_count: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Attach a link. Spawns a forwarder pumping its `recv` into the shared
    /// inbound queue. Safe to call after the node is already running —
    /// e.g. when a TCP peer connects or a serial modem is plugged in.
    pub fn add(self: &Arc<Self>, t: Arc<dyn Transport>) {
        self.members.lock().push(t.clone());
        let tx = self.out_tx.clone();
        let rx_count = self.rx_count.clone();
        let task = tokio::spawn(async move {
            // forwarder retires when the member link dies or the mux closes
            while let Ok((frame, meta)) = t.recv().await {
                rx_count.fetch_add(1, Ordering::Relaxed);
                if tx.send((frame, meta)).is_err() {
                    break;
                }
            }
        });
        self.tasks.lock().push(task);
    }

    /// Aggregate frame counters across members.
    pub fn counters(&self) -> (u64, u64) {
        (
            self.tx_count.load(Ordering::Relaxed),
            self.rx_count.load(Ordering::Relaxed),
        )
    }

    /// Number of attached links.
    pub fn link_count(&self) -> usize {
        self.members.lock().len()
    }
}

impl Drop for MuxTransport {
    fn drop(&mut self) {
        for t in self.tasks.lock().drain(..) {
            t.abort();
        }
    }
}

#[async_trait]
impl Transport for MuxTransport {
    async fn send(&self, frame: &[u8]) -> Result<(), TransportError> {
        let members = self.members.lock().clone();
        if members.is_empty() {
            return Ok(()); // nowhere to send — like shouting into dead air
        }
        for t in &members {
            let _ = t.send(frame).await; // per-link failures are expected
        }
        self.tx_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn recv(&self) -> Result<(Vec<u8>, RxMeta), TransportError> {
        self.in_rx
            .lock()
            .await
            .recv()
            .await
            .ok_or(TransportError::Closed)
    }

    fn mtu(&self) -> usize {
        let m = self.members.lock();
        m.iter().map(|t| t.mtu()).min().unwrap_or(MAX_PACKET_SIZE)
    }

    fn profile_id(&self) -> u8 {
        self.members
            .lock()
            .first()
            .map(|t| t.profile_id())
            .unwrap_or(profile_ids::TCP)
    }

    fn kind(&self) -> TransportKind {
        TransportKind::Mux
    }
}
