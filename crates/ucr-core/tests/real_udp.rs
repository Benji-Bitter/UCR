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
