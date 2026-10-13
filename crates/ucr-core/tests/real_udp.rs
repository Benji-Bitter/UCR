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
