//! `SimTransport` — a `Transport` backed by the simulated medium.

use async_trait::async_trait;
use tokio::sync::{mpsc, Mutex};
use ucr_transport::{RxMeta, Transport, TransportError, TransportKind};

use crate::medium::{Delivery, SimWorld};

/// A node's radio in the simulated mesh. Dropped frames, airtime, RSSI/SNR
/// are all products of the medium — nothing is simulated at this layer.
pub struct SimTransport {
    /// Index of this node inside the world (sim node id, NOT a UCR NodeId).
    pub sim_id: usize,
    world: SimWorld,
    rx: Mutex<mpsc::Receiver<Delivery>>,
    mtu: usize,
}

impl std::fmt::Debug for SimTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimTransport")
            .field("sim_id", &self.sim_id)
            .finish()
    }
}

impl SimTransport {
    pub(crate) fn new(sim_id: usize, world: SimWorld, rx: mpsc::Receiver<Delivery>) -> Self {
        Self {
            sim_id,
            world,
            rx: Mutex::new(rx),
            mtu: ucr_protocol::profile::MAX_PACKET_SIZE_SIM,
        }
    }

    pub fn world(&self) -> SimWorld {
        self.world.clone()
    }
}

#[async_trait]
impl Transport for SimTransport {
    async fn send(&self, frame: &[u8]) -> Result<(), TransportError> {
        if frame.len() > self.mtu {
            return Err(TransportError::TooLarge(frame.len()));
        }
        self.world
            .transmit(self.sim_id, frame.to_vec())
            .map(|_| ())
            .map_err(|_| TransportError::Closed)
    }

    async fn recv(&self) -> Result<(Vec<u8>, RxMeta), TransportError> {
        self.rx
            .lock()
            .await
            .recv()
            .await
            .map(|d| (d.frame, d.meta))
            .ok_or(TransportError::Closed)
    }

    fn mtu(&self) -> usize {
        self.mtu
    }

    fn profile_id(&self) -> u8 {
        ucr_protocol::profile::profile_ids::SIM
    }

    fn kind(&self) -> TransportKind {
        TransportKind::SimulatedRf
    }

    fn congestion(&self) -> u8 {
        self.world.congestion(self.sim_id)
    }

    fn measures_congestion(&self) -> bool {
        true
    }
}
