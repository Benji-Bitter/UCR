//! UCR desktop backend — embeds the real node stack plus a deterministic
//! simulated RF world so the full app is usable with no radio hardware.
//!
//! Every Tauri command operates on real `ucr-core` nodes; the simulator is
//! a transport like any other, so what the UI shows is what the protocol
//! actually does.

use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::task::JoinHandle;

use ucr_core::{Node, NodeBuilder, NodeConfig, NodeEvent};
use ucr_protocol::payload::EmergencyCategory;
use ucr_protocol::station::StationClass;
use ucr_protocol::{ChannelId, GroupId, NodeId};
use ucr_simulator::{Pos, SimConfig, SimWorld};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct SimHandle {
    world: SimWorld,
    nodes: Vec<Node>,
    driver: JoinHandle<()>,
    event_tasks: Vec<JoinHandle<()>>,
}

struct AppState {
    sim: Mutex<Option<SimHandle>>,
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
    let sim = st.sim.lock();
    let sim = sim.as_ref().ok_or("simulator not running")?;
    let node = sim.nodes.get(idx).ok_or("no such node")?;
    Ok(f(node))
}

// ---------------------------------------------------------------------------
// Commands — world lifecycle
// ---------------------------------------------------------------------------

#[tauri::command]
fn sim_start(state: State<'_, AppState>, count: usize, spacing_m: f32, seed: u64) -> Result<(), String> {
    sim_stop(state.clone());
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

    let nodes = state
        .rt
        .block_on(async {
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
    let sim = state.sim.lock();
    let sim = sim.as_ref().ok_or("simulator not running")?;
    let vis = sim.world.node_vis();
    Ok(vis.iter()
        .enumerate()
        .map(|(i, v)| NodeVisDto {
            id: v.id,
            x: v.pos.x,
            y: v.pos.y,
            enabled: v.enabled,
            node_id: sim.nodes[i].node_id().to_hex(),
            callsign: sim.nodes[i].callsign(),
            heard: v.heard_rssi.clone(),
        })
        .collect())
}

#[tauri::command]
fn sim_stats(state: State<'_, AppState>) -> Result<StatsDto, String> {
    let sim = state.sim.lock();
    let sim = sim.as_ref().ok_or("simulator not running")?;
    let s = sim.world.stats();
    Ok(StatsDto {
        medium_tx: s.tx_scheduled,
        medium_delivered: s.delivered,
        medium_dropped_loss: s.dropped_loss,
        medium_dropped_collision: s.dropped_collision,
        medium_dropped_halfduplex: s.dropped_halfduplex,
        medium_dropped_blocked: s.dropped_blocked,
        airtime_ms: s.airtime_ms,
    })
}

#[tauri::command]
fn sim_set_enabled(state: State<'_, AppState>, idx: usize, enabled: bool) -> Result<(), String> {
    let sim = state.sim.lock();
    let sim = sim.as_ref().ok_or("simulator not running")?;
    sim.world.set_enabled(idx, enabled);
    Ok(())
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
        n.dm_history(&peer, 200)
            .iter()
            .map(msg_json)
            .collect()
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
fn send_dm(state: State<'_, AppState>, idx: usize, peer: String, text: String) -> Result<u64, String> {
    let peer = NodeId::from_hex(&peer).ok_or("bad node id")?;
    with_node(&state, idx, |n| {
        n.send_dm(peer, &text).map_err(short_err)
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
fn create_group(state: State<'_, AppState>, idx: usize, name: String, desc: String) -> Result<String, String> {
    with_node(&state, idx, |n| {
        n.create_group(&name, &desc).map(|g| g.to_string()).map_err(short_err)
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
    let mut sim = state.sim.lock();
    let sim = sim.as_mut().ok_or("simulator not running")?;
    if !sim.event_tasks.is_empty() {
        return Ok(()); // already bridged
    }
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
    Ok(())
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
            rt,
        })
        .invoke_handler(tauri::generate_handler![
            sim_start,
            sim_stop,
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
