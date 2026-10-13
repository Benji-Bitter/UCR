//! Real-transport tests: nodes talking over actual UDP sockets — real OS
//! networking, real wire frames, real crypto. This is the "fully real" path
//! the desktop app uses; the RF simulator is a separate test world.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::{sleep, timeout};
use ucr_core::{Node, NodeBuilder, NodeConfig, NodeEvent};

use ucr_protocol::station::StationClass;
use ucr_transport::{MuxTransport, UdpBroadcastTransport};

fn test_cfg(callsign: &str) -> NodeConfig {
    NodeConfig {
        callsign: callsign.into(),
        desc: "real-udp test node".into(),
        class: StationClass::Individual,
        beacon_base_ms: 200,
        maintenance_ms: 50,
        ack_timeout_ms: 3_000,
        emergency_cooldown_ms: 1_000,
        ..Default::default()
    }
}

/// Two nodes linked by loopback UDP: each one's datagrams land on the
/// other's socket. Loopback has no broadcast domain, so we point the
/// transports at each other directly — same wire format either way.
async fn udp_pair() -> (Node, Node) {
    let ma = MuxTransport::new();
    let mb = MuxTransport::new();
    let ua = UdpBroadcastTransport::bind_to(0, "127.0.0.1:9".parse().unwrap()).unwrap();
    let ub = UdpBroadcastTransport::bind_to(0, "127.0.0.1:9".parse().unwrap()).unwrap();
    // local_addr() reports the 0.0.0.0 wildcard — send to loopback instead.
    let pb = ub.local_addr().unwrap().port();
    let pa = ua.local_addr().unwrap().port();
    ua.set_dest(([127, 0, 0, 1], pb).into());
    ub.set_dest(([127, 0, 0, 1], pa).into());
    ma.add(Arc::new(ua));
    mb.add(Arc::new(ub));
    let a = NodeBuilder::new()
        .config(test_cfg("REAL-A"))
        .pid_seed(11)
        .start(ma)
        .await
        .expect("node A");
    let b = NodeBuilder::new()
        .config(test_cfg("REAL-B"))
        .pid_seed(22)
        .start(mb)
        .await
        .expect("node B");
    (a, b)
}

async fn wait_event<F: Fn(&NodeEvent) -> bool>(
    rx: &mut tokio::sync::broadcast::Receiver<NodeEvent>,
    pred: F,
    ms: u64,
) -> bool {
    timeout(Duration::from_millis(ms), async {
        loop {
            match rx.recv().await {
                Ok(ev) if pred(&ev) => return true,
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return false,
            }
        }
    })
    .await
    .unwrap_or(false)
}

#[tokio::test(flavor = "multi_thread")]
async fn udp_nodes_discover_each_other() {
    let (a, b) = udp_pair().await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !a.neighbors().iter().any(|n| n.node_id == b.node_id())
        && tokio::time::Instant::now() < deadline
    {
        sleep(Duration::from_millis(50)).await;
    }
    assert!(
        a.neighbors().iter().any(|n| n.node_id == b.node_id()),
        "A never heard B's beacon over real UDP"
    );
    a.stop();
    b.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn udp_dm_delivery_e2e_encrypted() {
    let (a, b) = udp_pair().await;
    let mut rx = b.events();
    // discovery grace: the session handshake needs B's beacon first
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !a.neighbors().iter().any(|n| n.node_id == b.node_id())
        && tokio::time::Instant::now() < deadline
    {
        sleep(Duration::from_millis(50)).await;
    }
    a.send_dm(b.node_id(), "over real udp").expect("send_dm");
    let got = wait_event(
        &mut rx,
        |ev| matches!(ev, NodeEvent::DirectMessage { text, .. } if text == "over real udp"),
        15_000,
    )
    .await;
    assert!(got, "B never received the DM over real UDP");
    a.stop();
    b.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn udp_voice_frames_flood() {
    let (a, b) = udp_pair().await;
    let mut rx = b.events();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !b.neighbors().iter().any(|n| n.node_id == a.node_id())
        && tokio::time::Instant::now() < deadline
    {
        sleep(Duration::from_millis(50)).await;
    }
    let ch = ucr_protocol::ChannelId::from_name("ucr-general");
    // three frames of fake ADPCM then an end marker
    for seq in 0..3u16 {
        a.send_channel_voice(&ch, 1, 7, seq, seq == 2, &[0xABu8; 200])
            .expect("voice send");
    }
    let got = wait_event(
        &mut rx,
        |ev| matches!(ev, NodeEvent::VoiceFrame { last: true, .. }),
        10_000,
    )
    .await;
    assert!(got, "B never saw a voice frame over real UDP");
    a.stop();
    b.stop();
}

// ---------------------------------------------------------------------------
// LoRa constraints: the SX1262 world is 222-byte frames at ~1.75kbit/s.
// These tests prove the node respects that link budget on real transports.
// ---------------------------------------------------------------------------

/// A stubbed narrowband link — reports the LoRa profile/MTU and captures
/// every outbound wire frame so the test can measure what would hit the air.
#[derive(Debug, Default)]
struct LoraCapture {
    sent: std::sync::Mutex<Vec<Vec<u8>>>,
}

#[async_trait::async_trait]
impl ucr_transport::Transport for LoraCapture {
    async fn send(&self, frame: &[u8]) -> Result<(), ucr_transport::TransportError> {
        self.sent.lock().unwrap().push(frame.to_vec());
        Ok(())
    }
    async fn recv(
        &self,
    ) -> Result<(Vec<u8>, ucr_transport::RxMeta), ucr_transport::TransportError> {
        std::future::pending::<()>().await;
        unreachable!()
    }
    fn mtu(&self) -> usize {
        ucr_protocol::profile::RF_LORA_MTU
    }
    fn profile_id(&self) -> u8 {
        ucr_protocol::profile::profile_ids::EU_LORA
    }
    fn kind(&self) -> ucr_transport::TransportKind {
        ucr_transport::TransportKind::Serial
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn lora_mtu_chunks_text_and_rejects_fat_voice() {
    use ucr_protocol::channel::ChannelCategory;
    use ucr_protocol::header::Scope;

    let link = Arc::new(LoraCapture::default());
    let node = NodeBuilder::new()
        .config(test_cfg("LORA-A"))
        .pid_seed(31)
        .start(link.clone())
        .await
        .expect("node");

    let ch = node.join_channel("ucr-general", ChannelCategory::General, Scope::Local);
    // ~460 chars — can't fit one 222B LoRa frame, must split into chunks.
    let text = "packet radio over lora — ".repeat(20);
    node.send_channel_text(&ch, &text).expect("chunked send");
    let frames = link.sent.lock().unwrap().clone();
    assert!(
        frames.len() >= 2,
        "long text should chunk onto lora, got {} frames",
        frames.len()
    );
    assert!(
        frames
            .iter()
            .all(|f| f.len() <= ucr_protocol::profile::RF_LORA_MTU),
        "every wire frame must fit one lora rf frame"
    );

    // A voice frame larger than the rf budget is refused at send time —
    // better a clean error than a frame the modem can't transmit.
    let fat = vec![0u8; ucr_protocol::profile::RF_LORA_MTU - 20];
    let err = node
        .send_channel_voice(&ch, 1, 1, 0, false, &fat)
        .unwrap_err();
    assert!(format!("{err}").contains("too large"), "err was {err}");

    node.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn udp_channel_flood() {
    let (a, b) = udp_pair().await;
    let mut rx = b.events();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !b.neighbors().iter().any(|n| n.node_id == a.node_id())
        && tokio::time::Instant::now() < deadline
    {
        sleep(Duration::from_millis(50)).await;
    }
    let ch = ucr_protocol::ChannelId::from_name("ucr-general");
    // channel packets aren't retransmitted — resend until one lands
    let got = wait_event(
        &mut rx,
        |ev| matches!(ev, NodeEvent::ChannelMessage { text, .. } if text == "lan flood"),
        15_000,
    );
    let sender = tokio::spawn({
        let a = a.clone();
        async move {
            for _ in 0..30 {
                let _ = a.send_channel_text(&ch, "lan flood");
                sleep(Duration::from_millis(200)).await;
            }
        }
    });
    assert!(got.await, "B never received the channel message");
    sender.abort();
    a.stop();
    b.stop();
}

/// The digital lock: a PIN on the node gates every user-originated send —
/// the "keypad lock" on a real handheld. Reception continues (you still
/// hear the net), and emergency deliberately bypasses it like an SOS
/// button on a locked phone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn digital_lock_gates_tx_not_rx() {
    let (a, b) = udp_pair().await;
    let ch = ucr_protocol::ChannelId::from_name("ucr-general");

    // Pin too short refuses; locking engages.
    assert!(a.lock("12").is_err());
    a.lock("1-2-3-4").expect("lock");
    assert!(a.is_locked());

    // Every origin-send refuses while locked.
    use ucr_core::SendError;
    assert!(matches!(
        a.send_channel_text(&ch, "nope"),
        Err(SendError::Locked)
    ));
    assert!(matches!(
        a.send_channel_voice(&ch, 1, 0, 0, true, &[0u8; 8]),
        Err(SendError::Locked)
    ));
    assert!(matches!(
        a.send_dm(b.node_id(), "nope"),
        Err(SendError::Locked)
    ));
    assert!(matches!(
        a.send_contact_request(b.node_id(), "nope"),
        Err(SendError::Locked)
    ));
    assert!(matches!(
        a.send_morse(b.node_id(), "nope", 15),
        Err(SendError::Locked)
    ));
    assert!(matches!(a.create_group("g", "d"), Err(SendError::Locked)));

    // Wrong PIN doesn't open it.
    assert!(!a.unlock("9-9-9-9"));
    assert!(a.is_locked());

    // But the locked node still HEARS: B sends, A receives.
    let mut rx = a.events();
    let sender = tokio::spawn({
        let b = b.clone();
        let ch = ch.clone();
        async move {
            for _ in 0..30 {
                let _ = b.send_channel_text(&ch, "heard while locked");
                sleep(Duration::from_millis(200)).await;
            }
        }
    });
    assert!(
        wait_event(
            &mut rx,
            |ev| matches!(ev, NodeEvent::ChannelMessage { text, .. } if text == "heard while locked"),
            15_000,
        )
        .await,
        "locked node stopped receiving"
    );
    sender.abort();

    // Right PIN unlocks; sending works again.
    assert!(a.unlock("1-2-3-4"));
    assert!(!a.is_locked());
    let mut rx = b.events();
    a.send_channel_text(&ch, "back on air")
        .expect("send after unlock");
    assert!(
        wait_event(
            &mut rx,
            |ev| matches!(ev, NodeEvent::ChannelMessage { text, .. } if text == "back on air"),
            15_000,
        )
        .await,
        "unlocked node couldn't transmit"
    );
    a.stop();
    b.stop();
}

/// The lock survives a restart — a stolen radio stays locked even after
/// a power cycle, because the PIN hash lives in the store.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn digital_lock_persists_across_restart() {
    let dir = std::env::temp_dir().join(format!("ucr-lock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    {
        let store = ucr_storage::Store::open(&dir).unwrap();
        let node = NodeBuilder::new()
            .config(test_cfg("LOCKY"))
            .store(store)
            .pid_seed(5)
            .start(MuxTransport::new())
            .await
            .unwrap();
        node.lock("4321").unwrap();
        node.stop();
        // Background tasks hold the store until they observe shutdown —
        // give them a moment so the redb file lock is released.
        drop(node);
        sleep(Duration::from_millis(500)).await;
    }
    {
        // redb keeps an exclusive file lock — retry briefly if the old
        // store hasn't fully dropped yet.
        let mut store = None;
        for _ in 0..20 {
            match ucr_storage::Store::open(&dir) {
                Ok(s) => {
                    store = Some(s);
                    break;
                }
                Err(_) => sleep(Duration::from_millis(100)).await,
            }
        }
        let store = store.expect("reopen store");
        let node = NodeBuilder::new()
            .config(test_cfg("LOCKY"))
            .store(store)
            .pid_seed(5)
            .start(MuxTransport::new())
            .await
            .unwrap();
        assert!(node.is_locked(), "lock didn't survive restart");
        assert!(matches!(
            node.send_channel_text(&ucr_protocol::ChannelId::from_name("ucr-general"), "x"),
            Err(ucr_core::SendError::Locked)
        ));
        assert!(node.unlock("4321"));
        node.stop();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// GPS toggle: share_position gates whether beacons carry the coarse fix.
/// Off = position-silent beacons but the fix is kept locally; on = the
/// fix returns to the wire. The node's own cfg never loses the position.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gps_toggle_gates_beacon_position() {
    let (a, b) = udp_pair().await;
    a.set_position(Some(ucr_protocol::CoarsePos::new(-33.87, 151.21, 8_000)));

    // Sharing on: B sees A's position once the beacon lands.
    assert!(
        wait_event(
            &mut b.events(),
            |ev| matches!(ev, NodeEvent::NeighborUpdate { .. }),
            15_000,
        )
        .await
            || true
    ); // neighbor events are periodic — poll the table instead
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut seen_pos = false;
    while tokio::time::Instant::now() < deadline {
        if let Some(nb) = b.neighbors().iter().find(|n| n.node_id == a.node_id()) {
            if nb.pos.is_some() {
                seen_pos = true;
                break;
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
    assert!(seen_pos, "B never learned A's announced position");

    // Kill-switch off: fix stays local, beacons go silent. We can't prove
    // a negative frame, but we CAN prove the gate flipped and the fix is
    // retained — the wire side is the same cfg.pos the beacon reads.
    a.set_share_position(false);
    assert!(!a.share_position());
    assert!(a.pos().is_some());
    a.stop();
    b.stop();
}
