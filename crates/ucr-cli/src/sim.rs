//! `ucr-sim` — deterministic UCR mesh scenario runner.
//!
//! Runs real UCR nodes over the simulated RF medium with a seeded world and
//! reports deliveries, drops and stats as JSON lines. Exit code is 0 when the
//! scenario's expected traffic was delivered, 1 otherwise (unless --observe).

use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;
use tokio::time::{sleep, timeout};

use ucr_core::{Node, NodeBuilder, NodeConfig, NodeEvent};
use ucr_protocol::station::StationClass;
use ucr_protocol::{ChannelCategory, Scope};
use ucr_simulator::{Pos, SimConfig, SimWorld};
use ucr_storage::DeliveryState;

#[derive(Parser)]
#[command(name = "ucr-sim", version, about = "Deterministic UCR mesh simulation")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a scripted scenario and report the outcome.
    Run {
        /// Topology shape.
        #[arg(long, value_enum, default_value = "line")]
        topology: Topology,
        /// Number of nodes.
        #[arg(long, default_value_t = 3)]
        nodes: usize,
        /// Distance between adjacent nodes in metres.
        #[arg(long, default_value_t = 300.0)]
        spacing: f32,
        /// Environmental range ceiling in metres (0 = derive from spacing).
        #[arg(long, default_value_t = 0.0)]
        range: f32,
        /// Simulation seed.
        #[arg(long, default_value_t = 42)]
        seed: u64,
        /// Scenario duration in seconds (wall time).
        #[arg(long, default_value_t = 120)]
        duration: u64,
        /// Extra random frame loss 0..1 applied by the medium.
        #[arg(long, default_value_t = 0.0)]
        extra_loss: f32,
        /// Emit every node event as a JSON line.
        #[arg(long)]
        json: bool,
        /// Only observe — always exit 0 regardless of delivery outcome.
        #[arg(long)]
        observe: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Topology {
    /// Nodes in a line `spacing` apart — forces multi-hop when
    /// range < (nodes-1)*spacing.
    Line,
    /// Roughly square grid with `spacing` cell pitch.
    Grid,
}

fn node_cfg(i: usize) -> NodeConfig {
    NodeConfig {
        callsign: format!("SIM-{:02}", i),
        desc: "ucr-sim node".into(),
        class: StationClass::Individual,
        beacon_base_ms: 1_200,
        maintenance_ms: 100,
        ack_timeout_ms: 3_000,
        emergency_cooldown_ms: 1_000,
        ..Default::default()
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Run {
            topology,
            nodes,
            spacing,
            range,
            seed,
            duration,
            extra_loss,
            json,
            observe,
        } => {
            let code = run(&Scenario {
                topology,
                n: nodes,
                spacing,
                range,
                seed,
                duration,
                extra_loss,
                json,
            })
            .await;
            std::process::exit(if observe { 0 } else { code });
        }
    }
}

struct Scenario {
    topology: Topology,
    n: usize,
    spacing: f32,
    range: f32,
    seed: u64,
    duration: u64,
    extra_loss: f32,
    json: bool,
}

async fn run(sc: &Scenario) -> i32 {
    let Scenario {
        topology,
        n,
        spacing,
        range,
        seed,
        duration,
        extra_loss,
        json,
    } = *sc;
    let n = n.max(2);
    let range_limit_m = if range > 0.0 { range } else { spacing * 1.6 };
    let world = SimWorld::new_at(
        SimConfig {
            seed,
            range_limit_m,
            extra_loss,
            ..Default::default()
        },
        ucr_core::now_ms(),
    );

    let positions: Vec<Pos> = match topology {
        Topology::Line => (0..n).map(|i| Pos::new(i as f32 * spacing, 0.0)).collect(),
        Topology::Grid => {
            let side = (n as f32).sqrt().ceil() as usize;
            (0..n)
                .map(|i| Pos::new((i % side) as f32 * spacing, (i / side) as f32 * spacing))
                .collect()
        }
    };

    let transports: Vec<_> = positions
        .iter()
        .map(|p| Arc::new(world.add_simple(*p)))
        .collect();

    // Real-time pacing driver.
    let w = world.clone();
    let driver = tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(10)).await;
            w.run_for(10);
        }
    });

    let mut nodes: Vec<Node> = Vec::new();
    for (i, t) in transports.iter().enumerate() {
        let node = NodeBuilder::new()
            .config(node_cfg(i))
            .pid_seed(1000 * (i as u32 + 1))
            .start(t.clone())
            .await
            .expect("node start");
        nodes.push(node);
    }

    let mut rxs: Vec<_> = nodes.iter().map(|nd| nd.events()).collect();

    // Event printer task.
    let n_nodes = n;
    let printer = tokio::spawn(async move {
        loop {
            for (i, rx) in rxs.iter_mut().enumerate() {
                while let Ok(ev) = rx.try_recv() {
                    if json {
                        println!("{}", json!({ "node": i, "event": format!("{ev:?}") }));
                    } else {
                        println!("[n{i}] {ev:?}");
                    }
                }
            }
            sleep(Duration::from_millis(50)).await;
        }
    });

    // Discovery phase.
    let edge_hops = match topology {
        Topology::Line => n_nodes - 1,
        Topology::Grid => 1, // grid is dense — direct/near-direct likely
    };
    let _ = edge_hops;
    let discovered = wait_until(|| nodes.iter().all(|nd| !nd.neighbors().is_empty()), 30_000).await;
    if !discovered {
        eprintln!("warn: not all nodes discovered a neighbour within 30s");
    }

    // Traffic phase: channel flood + end-to-end DM from first to last node.
    let channel = nodes[0].join_channel("ucr.public", ChannelCategory::General, Scope::Regional);
    let dm_target = nodes[n_nodes - 1].node_id();

    nodes[0]
        .send_channel_text(&channel, "ucr-sim channel beacon")
        .expect("channel send");
    nodes[0]
        .send_dm(dm_target, "ucr-sim end-to-end probe")
        .expect("dm send");

    // Wait for the DM ack on node 0 and the channel message on the last node.
    let mut rx0 = nodes[0].events();
    let mut rx_last = nodes[n_nodes - 1].events();
    let mut got_dm_ack = false;
    let mut got_channel = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(duration);
    while tokio::time::Instant::now() < deadline && !(got_dm_ack && got_channel) {
        if !got_dm_ack {
            if let Ok(Ok(NodeEvent::Delivery {
                state: DeliveryState::Delivered,
                ..
            })) = timeout(Duration::from_millis(100), rx0.recv()).await
            {
                got_dm_ack = true;
            }
        }
        if !got_channel {
            if let Ok(Ok(ev)) = timeout(Duration::from_millis(100), rx_last.recv()).await {
                if matches!(ev, NodeEvent::ChannelMessage { .. }) {
                    got_channel = true;
                }
            }
        }
    }

    let stats: Vec<_> = nodes.iter().map(|nd| nd.stats()).collect();
    let world_stats = world.stats();
    println!("---");
    println!(
        "{}",
        json!({
            "dm_delivered": got_dm_ack,
            "channel_received": got_channel,
            "medium": format!("{world_stats:?}"),
            "nodes": stats.iter().enumerate().map(|(i, s)| json!({
                "node": i,
                "neighbors": nodes[i].neighbors().len(),
                "router": format!("{:?}", s.router),
            })).collect::<Vec<_>>(),
        })
    );

    printer.abort();
    driver.abort();
    if got_dm_ack && got_channel {
        0
    } else {
        1
    }
}

async fn wait_until<F: FnMut() -> bool>(mut pred: F, ms: u64) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
    while tokio::time::Instant::now() < deadline {
        if pred() {
            return true;
        }
        sleep(Duration::from_millis(25)).await;
    }
    pred()
}
