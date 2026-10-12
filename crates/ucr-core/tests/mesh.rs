//! End-to-end mesh tests: real nodes, real routing, real crypto — over the
//! deterministic simulated medium. These are the tests that prove UCR works.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::{sleep, timeout};
use ucr_core::{Node, NodeBuilder, NodeConfig, NodeEvent};

use ucr_protocol::payload::EmergencyCategory;
use ucr_protocol::station::StationClass;
use ucr_simulator::{Pos, SimConfig, SimWorld};
use ucr_storage::DeliveryState;

/// Test node config: fast beacons/maintenance for ms-scale runs.
fn test_cfg(callsign: &str) -> NodeConfig {
    NodeConfig {
        callsign: callsign.into(),
        desc: "test node".into(),
        class: StationClass::Individual,
        beacon_base_ms: 1_200,
        maintenance_ms: 100,
        ack_timeout_ms: 3_000,
        emergency_cooldown_ms: 1_000,
        ..Default::default()
    }
}

/// A simulated network of UCR nodes in a line at `spacing` metres.
struct SimNet {
    world: SimWorld,
    nodes: Vec<Node>,
    _driver: tokio::task::JoinHandle<()>,
}

async fn line_net(n: usize, spacing: f32, seed: u64) -> SimNet {
    let cfg = SimConfig {
        seed,
        range_limit_m: spacing * 1.6,
        ..Default::default()
    };
    let world = SimWorld::new_at(cfg, ucr_core::now_ms());
    let mut transports = Vec::new();
    for i in 0..n {
        transports.push(Arc::new(
            world.add_simple(Pos::new(i as f32 * spacing, 0.0)),
        ));
    }
    // Real-time pacing driver: advances the medium ~1:1 with wall time.
    let w = world.clone();
    let driver = tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(10)).await;
            w.run_for(10);
        }
    });
    let mut nodes = Vec::new();
    for (i, t) in transports.into_iter().enumerate() {
        let node = NodeBuilder::new()
            .config(test_cfg(&format!("TEST-{:02}", i)))
            .pid_seed(1000 * (i as u32 + 1))
            .start(t)
            .await
            .expect("node start");
        nodes.push(node);
    }
    SimNet {
        world,
        nodes,
        _driver: driver,
    }
}

/// Wait until `pred` on a poll loop or timeout. Returns false on timeout.
async fn until<F: FnMut() -> bool>(mut pred: F, ms: u64) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    while tokio::time::Instant::now() < deadline {
        if pred() {
            return true;
        }
        sleep(Duration::from_millis(25)).await;
    }
    pred()
}

/// Collect events from a receiver until `pred` matches or timeout.
async fn wait_event<F: Fn(&NodeEvent) -> bool>(
    rx: &mut tokio::sync::broadcast::Receiver<NodeEvent>,
    pred: F,
    ms: u64,
) -> bool {
    let res = timeout(Duration::from_millis(ms), async {
        loop {
            match rx.recv().await {
                Ok(ev) if pred(&ev) => return true,
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return false,
            }
        }
    })
    .await;
    res.unwrap_or(false)
}

/// Channel floods are not retransmitted by the protocol — a lost packet
/// stays lost. Real senders resend; do the same until `rx` sees it.
async fn send_channel_until(
    node: &Node,
    ch: &ucr_protocol::ChannelId,
    text: &str,
    rx: &mut tokio::sync::broadcast::Receiver<NodeEvent>,
    min_hops: u8,
) -> bool {
    for _ in 0..6 {
        node.send_channel_text(ch, text).expect("send");
        let got = wait_event(
            rx,
            |e| {
                matches!(e, NodeEvent::ChannelMessage { text: t, hops, .. }
                if t == text && *hops >= min_hops)
            },
            2_500,
        )
        .await;
        if got {
            return true;
        }
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn neighbour_discovery() {
    let net = line_net(2, 300.0, 1).await;
    let ok = until(
        || {
            net.nodes[0]
                .neighbors()
                .iter()
                .any(|n| n.node_id == net.nodes[1].node_id() && n.bidirectional)
        },
        45_000,
    )
    .await;
    assert!(ok, "nodes failed to discover each other bidirectionally");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn channel_text_direct() {
    let net = line_net(2, 300.0, 2).await;
    assert!(until(|| !net.nodes[0].neighbors().is_empty(), 30_000).await);
    let ch = net.nodes[0].join_channel(
        "ucr-general",
        ucr_protocol::channel::ChannelCategory::General,
        ucr_protocol::header::Scope::Local,
    );
    let mut rx = net.nodes[1].events();
    assert!(
        send_channel_until(&net.nodes[0], &ch, "hello channel", &mut rx, 0).await,
        "channel message not received"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn channel_text_multihop_flood() {
    // A — B — C: C must receive A's channel text via B's relay.
    let net = line_net(3, 300.0, 3).await;
    assert!(until(|| net.nodes[1].neighbors().len() >= 2, 30_000).await);
    let ch = net.nodes[0].join_channel(
        "ucr-general",
        ucr_protocol::channel::ChannelCategory::General,
        ucr_protocol::header::Scope::Regional,
    );
    let mut rx = net.nodes[2].events();
    assert!(
        send_channel_until(&net.nodes[0], &ch, "flooding works", &mut rx, 1).await,
        "multi-hop channel message not received"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn encrypted_dm_multihop_with_ack() {
    // A — B — C: A→C encrypted DM over two hops + authenticated ack.
    let net = line_net(3, 300.0, 4).await;
    assert!(until(|| net.nodes[1].neighbors().len() >= 2, 30_000).await);
    // Let A also hear C indirectly / gather topology.
    sleep(Duration::from_millis(500)).await;

    let mut rx_c = net.nodes[2].events();
    let mut rx_a = net.nodes[0].events();
    net.nodes[0]
        .send_dm(net.nodes[2].node_id(), "secret over two hops")
        .expect("dm");

    // C should see a session + the decrypted message.
    let got_dm = wait_event(
        &mut rx_c,
        |e| matches!(e, NodeEvent::DirectMessage { text, .. } if text == "secret over two hops"),
        30_000,
    )
    .await;
    assert!(got_dm, "C did not receive/decrypt the DM");

    // A should observe a verified ack → Delivered.
    let got_ack = wait_event(
        &mut rx_a,
        |e| matches!(e, NodeEvent::Delivery { state, .. } if *state == DeliveryState::Delivered),
        15_000,
    )
    .await;
    assert!(got_ack, "A did not receive an authenticated delivery ack");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn store_and_forward_on_reconnect() {
    // C starts disabled; A queues a DM; C enables → message delivered.
    let net = line_net(3, 300.0, 5).await;
    net.world.set_enabled(2, false);
    assert!(until(|| !net.nodes[0].neighbors().is_empty(), 30_000).await);

    let c_id = net.nodes[2].node_id();
    net.nodes[0]
        .send_dm(c_id, "queued until you're back")
        .expect("dm");

    let mut rx_c = net.nodes[2].events();
    sleep(Duration::from_millis(400)).await;
    net.world.set_enabled(2, true);

    assert!(
        wait_event(
            &mut rx_c,
            |e| matches!(e, NodeEvent::DirectMessage { text, .. } if text == "queued until you're back"),
            40_000
        )
        .await,
        "store-and-forward delivery failed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplicate_packets_suppressed() {
    let net = line_net(3, 300.0, 6).await;
    assert!(until(|| net.nodes[1].neighbors().len() >= 2, 30_000).await);
    let ch = net.nodes[0].join_channel(
        "ucr-general",
        ucr_protocol::channel::ChannelCategory::General,
        ucr_protocol::header::Scope::Regional,
    );
    let mut rx = net.nodes[2].events();
    assert!(
        send_channel_until(&net.nodes[0], &ch, "dedup me", &mut rx, 0).await,
        "message not delivered"
    );
    // The re-flood from C returns to B, whose router must suppress the
    // duplicate (a line topology gives C only one copy).
    let dup = until(|| net.nodes[1].stats().router.dropped_dup >= 1, 8_000).await;
    assert!(dup, "no duplicate suppression observed at the relay");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn partition_blocks_then_recovers() {
    let net = line_net(3, 300.0, 7).await;
    assert!(until(|| !net.nodes[0].neighbors().is_empty(), 30_000).await);
    // Partition A from B — A cannot reach C through B.
    net.world.block_pair(0, 1);
    let ch = net.nodes[0].join_channel(
        "ucr-general",
        ucr_protocol::channel::ChannelCategory::General,
        ucr_protocol::header::Scope::Regional,
    );
    let mut rx = net.nodes[2].events();
    net.nodes[0]
        .send_channel_text(&ch, "through the wall?")
        .expect("send");
    let got = wait_event(
        &mut rx,
        |e| matches!(e, NodeEvent::ChannelMessage { text, .. } if text == "through the wall?"),
        4_000,
    )
    .await;
    assert!(!got, "message leaked through a hard partition");
    net.world.unblock_pair(0, 1);
    // Floods are single-shot — a lost frame isn't retried at the protocol
    // layer, so retry the application send until one copy gets through.
    let mut got = false;
    for _ in 0..10 {
        net.nodes[0]
            .send_channel_text(&ch, "partition healed")
            .expect("send");
        got = wait_event(
            &mut rx,
            |e| matches!(e, NodeEvent::ChannelMessage { text, .. } if text == "partition healed"),
            2_500,
        )
        .await;
        if got {
            break;
        }
    }
    assert!(got, "network did not recover after partition healed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn emergency_propagates_with_signature() {
    let net = line_net(3, 300.0, 8).await;
    assert!(until(|| !net.nodes[1].neighbors().is_empty(), 30_000).await);
    let mut rx = net.nodes[2].events();
    // Floods are not retransmitted — retry within the emergency cooldown
    // window like a real sender would.
    let mut got = false;
    for _ in 0..6 {
        net.nodes[0]
            .send_emergency(EmergencyCategory::Medical, "need assistance", None)
            .expect("emergency");
        got = wait_event(
            &mut rx,
            |e| matches!(e, NodeEvent::Emergency { verified, .. } if *verified),
            2_500,
        )
        .await;
        if got {
            break;
        }
        sleep(Duration::from_millis(1_100)).await; // emergency cooldown
    }
    assert!(got, "verified emergency not received");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn group_invite_and_message() {
    let net = line_net(2, 300.0, 9).await;
    assert!(until(|| !net.nodes[0].neighbors().is_empty(), 30_000).await);

    let gid = net.nodes[0]
        .create_group("crew", "test group")
        .expect("create");
    let mut rx_b = net.nodes[1].events();
    net.nodes[0]
        .invite_to_group(gid, net.nodes[1].node_id())
        .expect("invite");
    let invited = wait_event(
        &mut rx_b,
        |e| matches!(e, NodeEvent::GroupInvite { group_id, .. } if *group_id == gid),
        20_000,
    )
    .await;
    assert!(invited, "B never got the invite");
    net.nodes[1].accept_group(gid).expect("accept");
    sleep(Duration::from_millis(300)).await;
    // Group messages are flooded without protocol retransmission — retry.
    let mut got = false;
    for _ in 0..6 {
        net.nodes[0]
            .send_group_text(gid, "group hello")
            .expect("group send");
        got = wait_event(
            &mut rx_b,
            |e| matches!(e, NodeEvent::GroupMessage { text, .. } if text == "group hello"),
            2_500,
        )
        .await;
        if got {
            break;
        }
    }
    assert!(got, "B did not decrypt the group message");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blocked_identity_drops_app_delivery() {
    let net = line_net(2, 300.0, 10).await;
    assert!(until(|| !net.nodes[0].neighbors().is_empty(), 30_000).await);
    net.nodes[1].block(net.nodes[0].node_id()).expect("block");
    let mut rx_b = net.nodes[1].events();
    net.nodes[0]
        .send_dm(net.nodes[1].node_id(), "you won't see this")
        .expect("dm");
    let got = wait_event(
        &mut rx_b,
        |e| matches!(e, NodeEvent::DirectMessage { .. }),
        15_000,
    )
    .await;
    assert!(!got, "blocked sender's DM was delivered to the app");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn morse_digital_delivery() {
    let net = line_net(2, 300.0, 11).await;
    assert!(until(|| !net.nodes[0].neighbors().is_empty(), 30_000).await);
    let mut rx = net.nodes[1].events();
    net.nodes[0]
        .send_morse(net.nodes[1].node_id(), "SOS", 20)
        .expect("morse");
    assert!(
        wait_event(
            &mut rx,
            |e| matches!(e, NodeEvent::Morse { text, wpm, .. } if text == "SOS" && *wpm == 20),
            15_000
        )
        .await,
        "digital morse not delivered"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn emergency_history_records_received() {
    let net = line_net(2, 300.0, 21).await;
    assert!(until(|| !net.nodes[0].neighbors().is_empty(), 30_000).await);
    net.nodes[0]
        .send_emergency(EmergencyCategory::Other, "test emergency", None)
        .expect("emergency");
    assert!(
        until(|| !net.nodes[1].emergency_history().is_empty(), 15_000).await,
        "emergency not recorded in history"
    );
    let hist = net.nodes[1].emergency_history();
    assert_eq!(hist[0].from, net.nodes[0].node_id());
}
