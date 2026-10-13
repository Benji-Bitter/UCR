//! UCR desktop backend — embeds the real node stack, real IP transports,
//! and a deterministic simulated RF world for development.
//!
//! Every Tauri command operates on real `ucr-core` nodes; the simulator is
//! a transport like any other, so what the UI shows is what the protocol
//! actually does. In real mode the node runs on a `MuxTransport` combining
//! UDP LAN broadcast, TCP gateways, and (optionally) a serial modem.

use std::net::SocketAddr;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use ucr_core::{Node, NodeBuilder, NodeConfig, NodeEvent};
use ucr_protocol::payload::EmergencyCategory;
use ucr_protocol::station::{CoarsePos, StationClass};
use ucr_protocol::{ChannelId, GroupId, NodeId, MAX_PACKET_SIZE};
use ucr_simulator::{Pos, SimConfig, SimWorld};
use ucr_transport::{MuxTransport, TcpTransport, Transport, UdpBroadcastTransport};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct SimHandle {
    world: SimWorld,
    nodes: Vec<Node>,
    driver: JoinHandle<()>,
    event_tasks: Vec<JoinHandle<()>>,
}

/// A live node on real transports (LAN broadcast + TCP gateways + serial).
struct RealHandle {
    node: Node,
    mux: Arc<MuxTransport>,
    /// Announced coarse position (city-level, ~8 km uncertainty radius).
    pos: Option<CoarsePos>,
    link_tasks: Vec<JoinHandle<()>>,
    event_task: Option<JoinHandle<()>>,
}

struct AppState {
    sim: Mutex<Option<SimHandle>>,
    real: Mutex<Option<RealHandle>>,
    /// Dedicated tokio runtime for node tasks — the node stack is async.
    rt: tokio::runtime::Runtime,
}

// ---------------------------------------------------------------------------
// DTOs — serialisable views over non-serde internals
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct NodeVisDto {
    id: usize,
    x: f32,
    y: f32,
    enabled: bool,
    node_id: String,
    callsign: String,
    heard: Vec<(usize, f32)>,
    /// Announced coarse position — present only when the node publishes one.
    lat: Option<f64>,
    lon: Option<f64>,
}

#[derive(Serialize)]
struct NeighborDto {
    node_id: String,
    callsign: String,
    rssi_ewma: f32,
    snr_ewma: f32,
    last_seen_ms: u64,
    rx_count: u32,
    bidirectional: bool,
    quality: u8,
}

#[derive(Serialize)]
struct RouteDto {
    dest: String,
    next_hop: String,
    metric: u32,
    hops: u8,
}

#[derive(Serialize)]
struct StatsDto {
    medium_tx: u64,
    medium_delivered: u64,
    medium_dropped_loss: u64,
    medium_dropped_collision: u64,
    medium_dropped_halfduplex: u64,
    medium_dropped_blocked: u64,
    airtime_ms: u64,
    /// Governing link: smallest MTU + slowest profile across the mux —
    /// the link every packet must fit and the rate that paces airtime.
    link_mtu: usize,
    link_bitrate: u32,
    link_name: String,
}

fn link_profile(mux: &MuxTransport) -> (usize, u32, String) {
    let p = ucr_protocol::profile::profile_by_id(mux.profile_id());
    (
        mux.mtu(),
        p.as_ref().map(|p| p.bitrate).unwrap_or(0),
        p.map(|p| p.name.to_lowercase())
            .unwrap_or_else(|| "unknown".into()),
    )
}

#[derive(Clone, Serialize)]
struct EventDto {
    node: usize,
    event: NodeEvent,
}

#[derive(Serialize)]
struct NodeInfoDto {
    node_id: String,
    callsign: String,
    pubkey: String,
}

fn short_err<E: std::fmt::Debug>(e: E) -> String {
    format!("{e:?}")
}

fn with_node<R>(st: &AppState, idx: usize, f: impl FnOnce(&Node) -> R) -> Result<R, String> {
    if let Some(sim) = st.sim.lock().as_ref() {
        if let Some(node) = sim.nodes.get(idx) {
            return Ok(f(node));
        }
    }
    if idx == 0 {
        if let Some(real) = st.real.lock().as_ref() {
            return Ok(f(&real.node));
        }
    }
    Err("no node running".into())
}

// ---------------------------------------------------------------------------
// Commands — world lifecycle
// ---------------------------------------------------------------------------

#[tauri::command]
fn sim_start(
    state: State<'_, AppState>,
    count: usize,
    spacing_m: f32,
    seed: u64,
) -> Result<(), String> {
    sim_stop(state.clone());
    real_stop(state.clone());
    let n = count.clamp(2, 12);
    let cfg = SimConfig {
        seed,
        range_limit_m: spacing_m * 1.6,
        ..Default::default()
    };
    let world = SimWorld::new_at(cfg, ucr_core::now_ms());
    let transports: Vec<_> = (0..n)
        .map(|i| Arc::new(world.add_simple(Pos::new(i as f32 * spacing_m, 0.0))))
        .collect();

    let w = world.clone();
    let driver = state.rt.spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            w.run_for(10);
        }
    });

    let nodes = state.rt.block_on(async {
        let mut out = Vec::new();
        for (i, t) in transports.into_iter().enumerate() {
            let node = NodeBuilder::new()
                .config(NodeConfig {
                    callsign: format!("SIM-{:02}", i),
                    desc: "simulated node".into(),
                    class: StationClass::Individual,
                    beacon_base_ms: 1_200,
                    maintenance_ms: 100,
                    ack_timeout_ms: 3_000,
                    emergency_cooldown_ms: 1_000,
                    ..Default::default()
                })
                .pid_seed(1000 * (i as u32 + 1))
                .start(t)
                .await
                .expect("node start");
            out.push(node);
        }
        out
    });

    *state.sim.lock() = Some(SimHandle {
        world,
        nodes,
        driver,
        event_tasks: Vec::new(),
    });
    Ok(())
}

#[tauri::command]
fn sim_stop(state: State<'_, AppState>) {
    if let Some(sim) = state.sim.lock().take() {
        for n in &sim.nodes {
            n.stop();
        }
        for t in sim.event_tasks {
            t.abort();
        }
        sim.driver.abort();
    }
}

#[tauri::command]
fn sim_topology(state: State<'_, AppState>) -> Result<Vec<NodeVisDto>, String> {
    if let Some(sim) = state.sim.lock().as_ref() {
        let vis = sim.world.node_vis();
        return Ok(vis
            .iter()
            .enumerate()
            .map(|(i, v)| NodeVisDto {
                id: v.id,
                x: v.pos.x,
                y: v.pos.y,
                enabled: v.enabled,
                node_id: sim.nodes[i].node_id().to_hex(),
                callsign: sim.nodes[i].callsign(),
                heard: v.heard_rssi.clone(),
                lat: None,
                lon: None,
            })
            .collect());
    }
    // Real mode: self + the neighbor table — a live view of peers the node
    // has actually heard over the wire, positions only where announced.
    if let Some(real) = state.real.lock().as_ref() {
        let n = &real.node;
        let neighbors = n.neighbors();
        let rssi = |v: f32| if v.is_nan() { -90.0 } else { v };
        let mut out = Vec::with_capacity(neighbors.len() + 1);
        out.push(NodeVisDto {
            id: 0,
            x: 0.0,
            y: 0.0,
            enabled: true,
            node_id: n.node_id().to_hex(),
            callsign: n.callsign(),
            heard: neighbors
                .iter()
                .enumerate()
                .map(|(i, nb)| (i + 1, rssi(nb.rssi_ewma)))
                .collect(),
            lat: real.pos.map(|p| p.lat()),
            lon: real.pos.map(|p| p.lon()),
        });
        for (i, nb) in neighbors.iter().enumerate() {
            out.push(NodeVisDto {
                id: i + 1,
                x: 0.0,
                y: 0.0,
                enabled: true,
                node_id: nb.node_id.to_hex(),
                callsign: nb.callsign.clone(),
                heard: vec![(0, rssi(nb.rssi_ewma))],
                lat: nb.pos.map(|p| p.lat()),
                lon: nb.pos.map(|p| p.lon()),
            });
        }
        return Ok(out);
    }
    Err("no node running".into())
}

#[tauri::command]
fn sim_stats(state: State<'_, AppState>) -> Result<StatsDto, String> {
    if let Some(sim) = state.sim.lock().as_ref() {
        let s = sim.world.stats();
        let p = ucr_protocol::profile::profile_by_id(ucr_protocol::profile::profile_ids::SIM);
        return Ok(StatsDto {
            medium_tx: s.tx_scheduled,
            medium_delivered: s.delivered,
            medium_dropped_loss: s.dropped_loss,
            medium_dropped_collision: s.dropped_collision,
            medium_dropped_halfduplex: s.dropped_halfduplex,
            medium_dropped_blocked: s.dropped_blocked,
            airtime_ms: s.airtime_ms,
            link_mtu: p.as_ref().map(|p| p.mtu).unwrap_or(400),
            link_bitrate: p.as_ref().map(|p| p.bitrate).unwrap_or(0),
            link_name: p
                .map(|p| p.name.to_lowercase())
                .unwrap_or_else(|| "sim".into()),
        });
    }
    if let Some(real) = state.real.lock().as_ref() {
        let (tx, rx) = real.mux.counters();
        let (link_mtu, link_bitrate, link_name) = link_profile(&real.mux);
        return Ok(StatsDto {
            medium_tx: tx,
            medium_delivered: rx,
            medium_dropped_loss: 0,
            medium_dropped_collision: 0,
            medium_dropped_halfduplex: 0,
            medium_dropped_blocked: 0,
            airtime_ms: 0,
            link_mtu,
            link_bitrate,
            link_name,
        });
    }
    Err("no node running".into())
}

#[tauri::command]
fn sim_set_enabled(state: State<'_, AppState>, idx: usize, enabled: bool) -> Result<(), String> {
    let sim = state.sim.lock();
    let sim = sim.as_ref().ok_or("no simulator running")?;
    sim.world.set_enabled(idx, enabled);
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands — real mode lifecycle (LAN broadcast + TCP gateways + serial)
// ---------------------------------------------------------------------------

#[tauri::command]
#[allow(clippy::too_many_arguments)] // tauri commands take flat arg lists
fn real_start(
    app: AppHandle,
    state: State<'_, AppState>,
    callsign: String,
    desc: String,
    udp_port: u16,
    tcp_port: u16,
    connect: Vec<String>,
    serial_path: Option<String>,
    serial_baud: u32,
    lat: Option<f64>,
    lon: Option<f64>,
) -> Result<(), String> {
    real_stop(state.clone());
    sim_stop(state.clone());

    let mux = MuxTransport::new();
    let mut link_tasks: Vec<JoinHandle<()>> = Vec::new();

    // LAN broadcast — zero-config discovery on the local network segment.
    // Socket setup + mux forwarder spawn need the tokio reactor; tauri
    // commands run on plain threads, so enter the runtime explicitly.
    let udp = state
        .rt
        .block_on(async { UdpBroadcastTransport::bind(udp_port) })
        .map_err(short_err)?;
    state.rt.block_on(async {
        mux.add(Arc::new(udp));
    });

    // TCP listener — remote peers dial in from anywhere reachable.
    let listener = state
        .rt
        .block_on(TcpListener::bind(("0.0.0.0", tcp_port)))
        .map_err(short_err)?;
    {
        let mux = mux.clone();
        link_tasks.push(state.rt.spawn(async move {
            loop {
                match TcpTransport::accept(&listener, MAX_PACKET_SIZE).await {
                    Ok(t) => mux.add(Arc::new(t)),
                    Err(_) => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
                }
            }
        }));
    }

    // TCP dial-out — retry until connected or the node is stopped.
    for addr in connect
        .iter()
        .filter_map(|a| a.trim().parse::<SocketAddr>().ok())
    {
        let mux = mux.clone();
        link_tasks.push(state.rt.spawn(async move {
            loop {
                match TcpTransport::connect(addr, MAX_PACKET_SIZE).await {
                    Ok(t) => {
                        mux.add(Arc::new(t));
                        break;
                    }
                    Err(_) => tokio::time::sleep(std::time::Duration::from_secs(3)).await,
                }
            }
        }));
    }

    // Serial modem — an attached SX1262-style radio accessory.
    if let Some(path) = serial_path.as_deref().filter(|p| !p.trim().is_empty()) {
        let t = ucr_transport::serial::SerialTransport::open(
            path,
            serial_baud,
            ucr_protocol::profile::RF_LORA_MTU,
        )
        .map_err(short_err)?;
        let mux = mux.clone();
        state.rt.block_on(async move {
            mux.add(Arc::new(t));
        });
    }

    let pos = lat.zip(lon).map(|(la, lo)| CoarsePos::new(la, lo, 8_000));

    // Persistent store — a real node keeps its identity between launches.
    let store = app
        .path()
        .app_data_dir()
        .ok()
        .and_then(|dir| {
            std::fs::create_dir_all(&dir).ok()?;
            ucr_storage::Store::open(&dir.join("ucr.redb")).ok()
        })
        .unwrap_or_else(|| ucr_storage::Store::in_memory().expect("in-memory store"));

    let node = state
        .rt
        .block_on(async {
            NodeBuilder::new()
                .config(NodeConfig {
                    callsign: if callsign.is_empty() {
                        "UCR-NODE".into()
                    } else {
                        callsign
                    },
                    desc,
                    class: StationClass::Individual,
                    pos,
                    beacon_base_ms: 3_000,
                    ..Default::default()
                })
                .store(store)
                .start(mux.clone() as Arc<dyn ucr_transport::Transport>)
                .await
        })
        .map_err(short_err)?;

    *state.real.lock() = Some(RealHandle {
        node,
        mux,
        pos,
        link_tasks,
        event_task: None,
    });
    Ok(())
}

#[tauri::command]
fn real_stop(state: State<'_, AppState>) {
    if let Some(real) = state.real.lock().take() {
        real.node.stop();
        if let Some(t) = real.event_task {
            t.abort();
        }
        for t in real.link_tasks {
            t.abort();
        }
        drop(real.mux); // aborts member forwarders
    }
}

// ---------------------------------------------------------------------------
// Commands — node queries
// ---------------------------------------------------------------------------

#[tauri::command]
fn node_info(state: State<'_, AppState>, idx: usize) -> Result<NodeInfoDto, String> {
    with_node(&state, idx, |n| NodeInfoDto {
        node_id: n.node_id().to_hex(),
        callsign: n.callsign(),
        pubkey: hex(&n.public_key()),
    })
}

#[tauri::command]
fn node_neighbors(state: State<'_, AppState>, idx: usize) -> Result<Vec<NeighborDto>, String> {
    with_node(&state, idx, |n| {
        n.neighbors()
            .iter()
            .map(|x| NeighborDto {
                node_id: x.node_id.to_hex(),
                callsign: x.callsign.clone(),
                rssi_ewma: x.rssi_ewma,
                snr_ewma: x.snr_ewma,
                last_seen_ms: x.last_seen_ms,
                rx_count: x.rx_count,
                bidirectional: x.bidirectional,
                quality: x.quality_score(),
            })
            .collect()
    })
}

#[tauri::command]
fn node_routes(state: State<'_, AppState>, idx: usize) -> Result<Vec<RouteDto>, String> {
    with_node(&state, idx, |n| {
        n.routes()
            .iter()
            .map(|r| RouteDto {
                dest: r.dest.to_hex(),
                next_hop: r.next_hop.to_hex(),
                metric: r.metric,
                hops: r.hops,
            })
            .collect()
    })
}

#[tauri::command]
fn node_channels(state: State<'_, AppState>, idx: usize) -> Result<Vec<serde_json::Value>, String> {
    with_node(&state, idx, |n| {
        n.channels()
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id.short(),
                    "name": c.name,
                    "category": format!("{:?}", c.category),
                    "scope": format!("{:?}", c.scope),
                })
            })
            .collect()
    })
}

#[tauri::command]
fn node_contacts(state: State<'_, AppState>, idx: usize) -> Result<Vec<serde_json::Value>, String> {
    with_node(&state, idx, |n| {
        n.contacts()
            .iter()
            .map(|c| {
                serde_json::json!({
                    "node_id": c.node_id.to_hex(),
                    "callsign": c.callsign,
                    "desc": c.desc,
                    "blocked": c.blocked,
                    "trusted": c.trusted,
                    "verified": c.verified,
                    "favourite": c.favourite,
                })
            })
            .collect()
    })
}

#[tauri::command]
fn node_groups(state: State<'_, AppState>, idx: usize) -> Result<Vec<serde_json::Value>, String> {
    with_node(&state, idx, |n| {
        n.groups()
            .iter()
            .map(|g| {
                serde_json::json!({
                    "group_id": g.group_id.to_string(),
                    "name": g.name,
                    "desc": g.desc,
                    "epoch": g.epoch,
                    "members": g.members.len(),
                })
            })
            .collect()
    })
}

#[tauri::command]
fn node_dm_history(
    state: State<'_, AppState>,
    idx: usize,
    peer: String,
) -> Result<Vec<serde_json::Value>, String> {
    let peer = NodeId::from_hex(&peer).ok_or("bad node id")?;
    with_node(&state, idx, |n| {
        n.dm_history(&peer, 200).iter().map(msg_json).collect()
    })
}

#[tauri::command]
fn node_channel_history(
    state: State<'_, AppState>,
    idx: usize,
    channel: String,
) -> Result<Vec<serde_json::Value>, String> {
    let cid = ChannelId::from_name(&channel);
    with_node(&state, idx, |n| {
        n.channel_history(&cid, 200).iter().map(msg_json).collect()
    })
}

fn msg_json(m: &ucr_storage::StoredMessage) -> serde_json::Value {
    serde_json::json!({
        "msg_id": m.msg_id,
        "peer": m.peer.to_hex(),
        "direction": format!("{:?}", m.direction),
        "text": m.text,
        "at_ms": m.at_ms,
        "delivery": format!("{:?}", m.delivery),
    })
}

// ---------------------------------------------------------------------------
// Commands — actions
// ---------------------------------------------------------------------------

#[tauri::command]
fn join_channel(state: State<'_, AppState>, idx: usize, name: String) -> Result<String, String> {
    use ucr_protocol::channel::ChannelCategory;
    use ucr_protocol::header::Scope;
    with_node(&state, idx, |n| {
        n.join_channel(&name, ChannelCategory::General, Scope::Regional)
            .short()
    })
}

#[tauri::command]
fn send_channel_text(
    state: State<'_, AppState>,
    idx: usize,
    channel: String,
    text: String,
) -> Result<u64, String> {
    let cid = ChannelId::from_name(&channel);
    with_node(&state, idx, |n| {
        n.send_channel_text(&cid, &text).map_err(short_err)
    })?
}

#[tauri::command]
fn send_dm(
    state: State<'_, AppState>,
    idx: usize,
    peer: String,
    text: String,
) -> Result<u64, String> {
    let peer = NodeId::from_hex(&peer).ok_or("bad node id")?;
    with_node(&state, idx, |n| n.send_dm(peer, &text).map_err(short_err))?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // tauri commands take flat arg lists
fn send_voice(
    state: State<'_, AppState>,
    idx: usize,
    channel: String,
    codec: u8,
    tx_id: u16,
    seq: u16,
    last: bool,
    data: Vec<u8>,
) -> Result<u64, String> {
    let cid = ChannelId::from_name(&channel);
    with_node(&state, idx, |n| {
        n.send_channel_voice(&cid, codec, tx_id, seq, last, &data)
            .map_err(short_err)
    })?
}

#[tauri::command]
fn send_morse(
    state: State<'_, AppState>,
    idx: usize,
    peer: String,
    text: String,
    wpm: u8,
) -> Result<u64, String> {
    let peer = NodeId::from_hex(&peer).ok_or("bad node id")?;
    with_node(&state, idx, |n| {
        n.send_morse(peer, &text, wpm).map_err(short_err)
    })?
}

#[tauri::command]
fn send_emergency(
    state: State<'_, AppState>,
    idx: usize,
    category: String,
    text: String,
) -> Result<u64, String> {
    let cat = match category.to_ascii_lowercase().as_str() {
        "medical" => EmergencyCategory::Medical,
        "fire" => EmergencyCategory::Fire,
        "flood" => EmergencyCategory::Flood,
        "accident" => EmergencyCategory::Accident,
        "vessel" => EmergencyCategory::VesselDistress,
        "missing" => EmergencyCategory::PersonMissing,
        _ => EmergencyCategory::Other,
    };
    with_node(&state, idx, |n| {
        n.send_emergency(cat, &text, None).map_err(short_err)
    })?
}

#[tauri::command]
fn create_group(
    state: State<'_, AppState>,
    idx: usize,
    name: String,
    desc: String,
) -> Result<String, String> {
    with_node(&state, idx, |n| {
        n.create_group(&name, &desc)
            .map(|g| g.to_string())
            .map_err(short_err)
    })?
}

#[tauri::command]
fn invite_group(
    state: State<'_, AppState>,
    idx: usize,
    group_id: String,
    peer: String,
) -> Result<(), String> {
    let gid = parse_group(&group_id)?;
    let peer = NodeId::from_hex(&peer).ok_or("bad node id")?;
    with_node(&state, idx, |n| {
        n.invite_to_group(gid, peer).map_err(short_err)
    })?
}

#[tauri::command]
fn accept_group(state: State<'_, AppState>, idx: usize, group_id: String) -> Result<(), String> {
    let gid = parse_group(&group_id)?;
    with_node(&state, idx, |n| n.accept_group(gid).map_err(short_err))?
}

#[tauri::command]
fn send_group_text(
    state: State<'_, AppState>,
    idx: usize,
    group_id: String,
    text: String,
) -> Result<u64, String> {
    let gid = parse_group(&group_id)?;
    with_node(&state, idx, |n| {
        n.send_group_text(gid, &text).map_err(short_err)
    })?
}

#[tauri::command]
fn accept_contact(state: State<'_, AppState>, idx: usize, peer: String) -> Result<(), String> {
    let peer = NodeId::from_hex(&peer).ok_or("bad node id")?;
    with_node(&state, idx, |n| n.accept_contact(peer).map_err(short_err))?
}

#[tauri::command]
fn block_contact(state: State<'_, AppState>, idx: usize, peer: String) -> Result<(), String> {
    let peer = NodeId::from_hex(&peer).ok_or("bad node id")?;
    with_node(&state, idx, |n| n.block(peer).map_err(short_err))?
}

#[tauri::command]
fn node_stats(state: State<'_, AppState>, idx: usize) -> Result<serde_json::Value, String> {
    with_node(&state, idx, |n| {
        let s = n.stats();
        serde_json::json!({ "router": format!("{:?}", s.router) })
    })
}

// ---------------------------------------------------------------------------
// Event bridge — real NodeEvents forwarded to the frontend
// ---------------------------------------------------------------------------

#[tauri::command]
fn subscribe_events(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(sim) = state.sim.lock().as_mut() {
        if sim.event_tasks.is_empty() {
            for (i, n) in sim.nodes.iter().enumerate() {
                let mut rx = n.events();
                let app = app.clone();
                sim.event_tasks.push(state.rt.spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(event) => {
                                let _ = app.emit("ucr:event", EventDto { node: i, event });
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(_) => break,
                        }
                    }
                }));
            }
        }
        return Ok(());
    }
    if let Some(real) = state.real.lock().as_mut() {
        if real.event_task.is_none() {
            let mut rx = real.node.events();
            let app = app.clone();
            real.event_task = Some(state.rt.spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(event) => {
                            let _ = app.emit("ucr:event", EventDto { node: 0, event });
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            }));
        }
        return Ok(());
    }
    Err("no node running".into())
}

// ---------------------------------------------------------------------------

fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn parse_group(s: &str) -> Result<GroupId, String> {
    // GroupId Display is 8-hex; accept that or 16-hex.
    let s = s.trim_start_matches("GroupId(").trim_end_matches(')');
    let bytes: Vec<u8> = (0..s.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect();
    if bytes.len() != 8 {
        return Err("bad group id".into());
    }
    let mut out = [0u8; 8];
    out.copy_from_slice(&bytes);
    Ok(GroupId(out))
}

pub fn run() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("tokio runtime");

    tauri::Builder::default()
        .manage(AppState {
            sim: Mutex::new(None),
            real: Mutex::new(None),
            rt,
        })
        .invoke_handler(tauri::generate_handler![
            sim_start,
            sim_stop,
            real_start,
            real_stop,
            sim_topology,
            sim_stats,
            sim_set_enabled,
            node_info,
            node_neighbors,
            node_routes,
            node_channels,
            node_contacts,
            node_groups,
            node_dm_history,
            node_channel_history,
            join_channel,
            send_channel_text,
            send_voice,
            send_dm,
            send_morse,
            send_emergency,
            create_group,
            invite_group,
            accept_group,
            send_group_text,
            accept_contact,
            block_contact,
            node_stats,
            subscribe_events,
        ])
        .run(tauri::generate_context!())
        .expect("error running UCR");
}
