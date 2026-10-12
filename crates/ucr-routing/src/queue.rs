//! Bounded store-and-forward queue for unreachable destinations.

use std::collections::VecDeque;
use ucr_protocol::{NodeId, Packet};

use crate::{OUTBOUND_QUEUE_MAX, QUEUE_EXPIRY_MS};

/// A packet waiting for a route/liveness to its destination.
#[derive(Clone, Debug)]
pub struct QueuedPacket {
    pub packet: Packet,
    pub dest: NodeId,
    pub enqueued_ms: u64,
    pub expires_ms: u64,
    /// Delivery attempts so far (RREQs sent).
    pub attempts: u8,
    /// Don't retry before this time (controlled backoff).
    pub next_retry_ms: u64,
}

/// FIFO store-and-forward queue with bounded capacity.
#[derive(Debug)]
pub struct OutboundQueue {
    q: VecDeque<QueuedPacket>,
    cap: usize,
}

impl Default for OutboundQueue {
    fn default() -> Self {
        Self::new(OUTBOUND_QUEUE_MAX)
    }
}

impl OutboundQueue {
    pub fn new(cap: usize) -> Self {
        Self {
            q: VecDeque::new(),
            cap,
        }
    }

    /// Enqueue a packet. Returns `Err(packet)` if the queue is full — the
    /// caller reports queue-full to the app instead of silently dropping.
    pub fn enqueue(
        &mut self,
        packet: Packet,
        dest: NodeId,
        now: u64,
        ttl_ms: u64,
    ) -> Result<(), Box<Packet>> {
        if self.q.len() >= self.cap {
            return Err(Box::new(packet));
        }
        self.q.push_back(QueuedPacket {
            packet,
            dest,
            enqueued_ms: now,
            expires_ms: now + ttl_ms.min(QUEUE_EXPIRY_MS),
            attempts: 0,
            next_retry_ms: now,
        });
        Ok(())
    }

    /// Packets eligible for another delivery attempt now.
    pub fn due(&mut self, now: u64) -> Vec<QueuedPacket> {
        let mut out = Vec::new();
        let mut keep = VecDeque::with_capacity(self.q.len());
        while let Some(qp) = self.q.pop_front() {
            if now >= qp.expires_ms {
                // caller drains expired separately via expire()
                keep.push_back(qp);
            } else if now >= qp.next_retry_ms {
                out.push(qp);
            } else {
                keep.push_back(qp);
            }
        }
        self.q = keep;
        out
    }

    /// Put a due-packet back (e.g. RREQ sent, waiting for reply) with a new
    /// backoff.
    pub fn requeue(&mut self, mut qp: QueuedPacket, backoff_ms: u64) {
        qp.attempts = qp.attempts.saturating_add(1);
        qp.next_retry_ms = qp
            .next_retry_ms
            .saturating_add(backoff_ms)
            .min(qp.expires_ms);
        if self.q.len() < self.cap {
            self.q.push_back(qp);
        }
    }

    /// Remove and return expired entries (for delivery-failure reporting).
    pub fn expire(&mut self, now: u64) -> Vec<QueuedPacket> {
        let mut out = Vec::new();
        self.q.retain(|qp| {
            if now >= qp.expires_ms {
                out.push(qp.clone());
                false
            } else {
                true
            }
        });
        out
    }

    /// Flush queued traffic for `dest` — e.g. a route just came up. Returns
    /// packets ready to send immediately.
    pub fn flush_for(&mut self, dest: &NodeId) -> Vec<QueuedPacket> {
        let mut out = Vec::new();
        self.q.retain(|qp| {
            if &qp.dest == dest {
                out.push(qp.clone());
                false
            } else {
                true
            }
        });
        out
    }

    pub fn len(&self) -> usize {
        self.q.len()
    }

    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }

    /// Snapshot for diagnostics/UI.
    pub fn snapshot(&self) -> Vec<QueuedPacket> {
        self.q.iter().cloned().collect()
    }
}
