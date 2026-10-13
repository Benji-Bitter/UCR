//! `ucr-node` — a headless, interactive UCR node on real transports.
//!
//! By default it joins the LAN via UDP broadcast (port 4401) — run two
//! instances on the same network and they find each other. TCP gateways and
//! a serial modem can be attached on top:
//!
//!   ucr-node --callsign ALPHA                          # LAN only
//!   ucr-node --callsign ALPHA --listen 0.0.0.0:4402    # + inbound TCP
//!   ucr-node --callsign BRAVO --connect 10.0.0.5:4402  # + dial a peer
//!   ucr-node --callsign VK2X --serial /dev/ttyUSB0     # + real radio
//!
//! Commands (line input):
//!   <text>                  send to the joined channel (default ucr.public)
//!   /dm <nodeid> <text>     encrypted direct message
//!   /morse <nodeid> <text>  digital morse (not CW)
//!   /emergency <cat> <text> signed emergency broadcast
//!   /join <name>            join/create a public channel
//!   /channel <name>         switch the default channel for bare text
//!   /accept <nodeid>        accept a contact request
//!   /block <nodeid>         block a sender
//!   /id  /neighbors  /routes  /contacts  /groups  /channels  /stats
//!   /help  /quit

use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpListener;

use ucr_core::{Node, NodeBuilder, NodeConfig, NodeEvent};
use ucr_protocol::station::{CoarsePos, StationClass};
use ucr_protocol::EmergencyCategory;
use ucr_protocol::{ChannelCategory, ChannelId, NodeId, Scope};
use ucr_storage::Store;
use ucr_transport::tcp::TcpTransport;
use ucr_transport::{MuxTransport, UdpBroadcastTransport, DEFAULT_UDP_PORT};

#[derive(Parser)]
#[command(name = "ucr-node", version, about = "Headless interactive UCR node")]
struct Cli {
    /// Your callsign (max 12 chars).
    #[arg(long)]
    callsign: String,
    /// Node description shown to peers.
    #[arg(long, default_value = "")]
    desc: String,
    /// UDP LAN broadcast port — every node on the LAN shares it.
    #[arg(long, default_value_t = DEFAULT_UDP_PORT)]
    udp_port: u16,
    /// Disable the LAN broadcast transport.
    #[arg(long)]
    no_udp: bool,
    /// Accept inbound TCP gateway connections on this address.
    #[arg(long)]
    listen: Option<SocketAddr>,
    /// Dial a peer's TCP gateway (repeatable).
    #[arg(long)]
    connect: Vec<SocketAddr>,
    /// Serial port of a UCR modem accessory (e.g. /dev/ttyUSB0).
    #[arg(long)]
    serial: Option<String>,
    /// Serial baud rate for the modem link.
    #[arg(long, default_value_t = 115_200)]
    baud: u32,
    /// Coarse position to announce (city-level is plenty).
    #[arg(long)]
    lat: Option<f64>,
    /// Coarse position to announce.
    #[arg(long)]
    lon: Option<f64>,
    /// Keep the GPS fix local — don't announce position in beacons.
    /// The "GPS off" switch on a build that still wants the fix for
    /// emergencies and its own UI.
    #[arg(long)]
    no_share_pos: bool,
    /// Persistent store directory (redb). Omit for an in-memory node.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Do not relay traffic for other nodes.
    #[arg(long)]
    no_relay: bool,
    /// Station class announced in beacons: individual, base, relay,
    /// community, portable, boat, vehicle, truck, volunteer, other.
    #[arg(long, default_value = "individual")]
    class: String,
    /// Default channel to join and send to.
    #[arg(long, default_value = "ucr.public")]
    channel: String,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let cli = Cli::parse();

    // One node, however many links: LAN broadcast + TCP gateways + serial
    // all ride the same mux, so every wire frame reaches every wire.
    let mux = MuxTransport::new();
    // peers may attach after startup, so track configuration, not link count
    let mut configured = false;
    if !cli.no_udp {
        match UdpBroadcastTransport::bind(cli.udp_port) {
            Ok(t) => {
                eprintln!("lan broadcast on udp/{}", cli.udp_port);
                mux.add(Arc::new(t));
                configured = true;
            }
            Err(e) => eprintln!("udp bind :{} failed: {e}", cli.udp_port),
        }
    }
    if let Some(addr) = cli.listen {
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("bind {addr} failed: {e}");
                std::process::exit(1);
            }
        };
        eprintln!("listening for tcp gateways on {addr}");
        configured = true;
        let mux = mux.clone();
        tokio::spawn(async move {
            loop {
                match TcpTransport::accept(&listener, ucr_protocol::MAX_PACKET_SIZE).await {
                    Ok(t) => mux.add(Arc::new(t)),
                    Err(_) => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
                }
            }
        });
    }
    for addr in cli.connect.clone() {
        eprintln!("dialing {addr}…");
        configured = true;
        let mux = mux.clone();
        tokio::spawn(async move {
            loop {
                match TcpTransport::connect(addr, ucr_protocol::MAX_PACKET_SIZE).await {
                    Ok(t) => {
                        mux.add(Arc::new(t));
                        break;
                    }
                    Err(_) => tokio::time::sleep(std::time::Duration::from_secs(3)).await,
                }
            }
        });
    }
    if let Some(path) = &cli.serial {
        match ucr_transport::serial::SerialTransport::open(
            path,
            cli.baud,
            ucr_protocol::profile::RF_LORA_MTU,
        ) {
            Ok(t) => {
                eprintln!("serial modem on {path} @ {} baud", cli.baud);
                mux.add(Arc::new(t));
                configured = true;
            }
            Err(e) => eprintln!("serial {path} failed: {e}"),
        }
    }
    if !configured {
        eprintln!("error: no transports up — enable udp, --listen, --connect, or --serial");
        std::process::exit(2);
    }

    let pos = cli
        .lat
        .zip(cli.lon)
        .map(|(la, lo)| CoarsePos::new(la, lo, 8_000));

    let class = match cli.class.as_str() {
        "base" | "basestation" => StationClass::BaseStation,
        "relay" => StationClass::RelayNode,
        "community" => StationClass::CommunityStation,
        "portable" => StationClass::Portable,
        "boat" => StationClass::Boat,
        "vehicle" => StationClass::Vehicle,
        "truck" => StationClass::Truck,
        "volunteer" | "emergencyvolunteer" => StationClass::EmergencyVolunteer,
        "other" => StationClass::Other,
        _ => StationClass::Individual,
    };
    let mut builder = NodeBuilder::new().config(NodeConfig {
        callsign: cli.callsign.chars().take(12).collect(),
        desc: cli.desc,
        class,
        relay: !cli.no_relay,
        pos,
        share_position: !cli.no_share_pos,
        ..Default::default()
    });
    if let Some(dir) = &cli.data_dir {
        std::fs::create_dir_all(dir).expect("data dir");
        let store = Store::open(&dir.join("node.redb")).expect("open store");
        builder = builder.store(store);
    }

    let node = builder
        .start(mux as Arc<dyn ucr_transport::Transport>)
        .await
        .expect("node start");

    let default_channel =
        node.join_channel(&cli.channel, ChannelCategory::General, Scope::Regional);
    println!(
        "{}",
        [
            "##      ##     ########     ########",
            "##      ##    ##            ##      ##",
            "##      ##    ##            ########",
            "##      ##    ##            ##    ##",
            "##########     ########     ##      ##",
            "",
            "universal citizens radio — node online",
        ]
        .join("\n")
    );
    println!("node id: {}", node.node_id().to_hex());
    println!("callsign: {}", node.callsign());
    println!("channel: {} ({})", cli.channel, default_channel.short());
    println!("type /help for commands");

    // Event printer.
    let mut events = node.events();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(ev) => print_event(&ev),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("(missed {n} events)")
                }
                Err(_) => break,
            }
        }
    });

    // Command REPL.
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut current_channel = default_channel;
    print!("> ");
    std::io::stdout().flush().ok();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            print!("> ");
            std::io::stdout().flush().ok();
            continue;
        }
        if !handle(&node, line, &mut current_channel).await {
            break;
        }
        print!("> ");
        std::io::stdout().flush().ok();
    }
    node.stop();
}

/// Returns false to exit the REPL.
async fn handle(node: &Node, line: &str, current_channel: &mut ChannelId) -> bool {
    let mut parts = line.splitn(3, ' ');
    let cmd = parts.next().unwrap_or("");
    if !cmd.starts_with('/') {
        match node.send_channel_text(current_channel, line) {
            Ok(id) => println!("sent #{id}"),
            Err(e) => println!("send failed: {e:?}"),
        }
        return true;
    }
    let arg1 = parts.next().unwrap_or("").trim();
    let arg2 = parts.next().unwrap_or("").trim();
    match cmd {
        "/quit" | "/exit" => return false,
        "/help" => print_help(),
        "/id" => println!("{} {}", node.node_id().to_hex(), node.callsign()),
        "/dm" => match parse_peer(arg1) {
            Some(peer) => match node.send_dm(peer, arg2) {
                Ok(id) => println!("dm queued #{id}"),
                Err(e) => println!("dm failed: {e:?}"),
            },
            None => println!("usage: /dm <nodeid-hex> <text>"),
        },
        "/morse" => match parse_peer(arg1) {
            Some(peer) => match node.send_morse(peer, arg2, 20) {
                Ok(id) => println!("morse queued #{id}"),
                Err(e) => println!("morse failed: {e:?}"),
            },
            None => println!("usage: /morse <nodeid-hex> <text>"),
        },
        "/emergency" => {
            let (cat, text) = parse_emergency(arg1, arg2);
            match node.send_emergency(cat, text, None) {
                Ok(id) => println!("emergency sent id={id}"),
                Err(e) => println!("emergency failed: {e:?}"),
            }
        }
        "/join" => {
            if arg1.is_empty() {
                println!("usage: /join <name>");
            } else {
                let ch = node.join_channel(arg1, ChannelCategory::General, Scope::Regional);
                println!("joined {} ({})", arg1, ch.short());
            }
        }
        "/channel" => {
            if arg1.is_empty() {
                println!("usage: /channel <name>");
            } else {
                *current_channel =
                    node.join_channel(arg1, ChannelCategory::General, Scope::Regional);
                println!("default channel: {} ({})", arg1, current_channel.short());
            }
        }
        "/accept" => match parse_peer(arg1) {
            Some(peer) => match node.accept_contact(peer) {
                Ok(()) => println!("accepted"),
                Err(e) => println!("accept failed: {e:?}"),
            },
            None => println!("usage: /accept <nodeid-hex>"),
        },
        "/block" => match parse_peer(arg1) {
            Some(peer) => match node.block(peer) {
                Ok(()) => println!("blocked"),
                Err(e) => println!("block failed: {e:?}"),
            },
            None => println!("usage: /block <nodeid-hex>"),
        },
        "/neighbors" => {
            for nb in node.neighbors() {
                println!(
                    "  {} {} rssi={:.1} snr={:.1} heard={}ms",
                    nb.node_id.to_hex(),
                    node.callsign_of(&nb.node_id),
                    nb.rssi_ewma,
                    nb.snr_ewma,
                    nb.last_seen_ms
                );
            }
        }
        "/routes" => {
            for r in node.routes() {
                println!(
                    "  {} via {:?} hops={}",
                    r.dest.to_hex(),
                    r.next_hop.to_hex(),
                    r.hops
                );
            }
        }
        "/contacts" => {
            for c in node.contacts() {
                println!(
                    "  {} {} blocked={} trusted={}",
                    c.node_id.to_hex(),
                    c.callsign,
                    c.blocked,
                    c.trusted
                );
            }
        }
        "/channels" => {
            for ch in node.channels() {
                println!("  {} {} scope={:?}", ch.id.short(), ch.name, ch.scope);
            }
        }
        "/groups" => {
            for g in node.groups() {
                println!("  {} {} members={}", g.group_id, g.name, g.members.len());
            }
        }
        "/lock" => {
            if arg1.is_empty() {
                println!("usage: /lock <pin> — sets the PIN and engages the digital lock");
            } else {
                match node.lock(arg1) {
                    Ok(()) => println!("locked — tx disabled until /unlock <pin>"),
                    Err(e) => println!("lock failed: {e}"),
                }
            }
        }
        "/unlock" => {
            if arg1.is_empty() {
                println!("usage: /unlock <pin>");
            } else if node.unlock(arg1) {
                println!("unlocked — tx enabled");
            } else {
                println!("wrong pin");
            }
        }
        "/lockstate" => {
            if node.is_locked() {
                println!("locked — rx/relay only, sends refused");
            } else {
                println!("unlocked");
            }
        }
        "/gps" => match arg1 {
            "on" => {
                node.set_share_position(true);
                println!("gps sharing on — position announced in beacons");
            }
            "off" => {
                node.set_share_position(false);
                println!("gps sharing off — fix kept locally, beacons carry no position");
            }
            _ => println!(
                "gps sharing {}",
                if node.share_position() { "on" } else { "off" }
            ),
        },
        "/stats" => println!("{:?}", node.stats()),
        _ => println!("unknown command {cmd} — /help"),
    }
    true
}

fn parse_peer(s: &str) -> Option<NodeId> {
    let id = NodeId::from_hex(s);
    if id.is_none() {
        println!("bad node id (want 32 hex chars): {s:?}");
    }
    id
}

fn parse_emergency<'a>(cat: &'a str, text: &'a str) -> (EmergencyCategory, &'a str) {
    let c = match cat.to_ascii_lowercase().as_str() {
        "medical" | "med" => EmergencyCategory::Medical,
        "fire" => EmergencyCategory::Fire,
        "flood" => EmergencyCategory::Flood,
        "accident" => EmergencyCategory::Accident,
        "vessel" => EmergencyCategory::VesselDistress,
        "missing" => EmergencyCategory::PersonMissing,
        _ => EmergencyCategory::Other,
    };
    (c, if text.is_empty() { cat } else { text })
}

fn print_help() {
    println!(
        "commands:\n  <text>                   send to current channel\n  /dm <nodeid> <text>      encrypted DM\n  /morse <nodeid> <text>   digital morse\n  /emergency <cat> <text>  categories: medical fire flood accident vessel missing other\n  /join <name>             join a channel\n  /channel <name>          set default channel\n  /accept <nodeid>         accept contact request\n  /block <nodeid>\n  /lock <pin>              engage digital lock (tx gated; pin stored hashed)\n  /unlock <pin>            disengage lock\n  /lockstate               show lock state\n  /gps [on|off]            toggle position announce (fix kept locally when off)\n  /id /neighbors /routes /contacts /channels /groups /stats\n  /quit"
    );
}

fn print_event(ev: &NodeEvent) {
    match ev {
        NodeEvent::ChannelMessage {
            channel_name,
            callsign,
            text,
            hops,
            verified,
            ..
        } => println!(
            "\n[#{}] {}: {} (hops={} verified={})",
            channel_name, callsign, text, hops, verified
        ),
        NodeEvent::DirectMessage {
            from,
            callsign,
            text,
            ..
        } => {
            println!("\n[dm {} {}] {}", callsign, from.to_hex(), text)
        }
        NodeEvent::Delivery { state, msg_id, .. } => {
            println!("\n[delivery {:?} msg={:?}]", state, msg_id)
        }
        NodeEvent::Emergency {
            callsign,
            category,
            text,
            verified,
            ..
        } => println!(
            "\n[EMERGENCY {:?} {}] {} (verified={})",
            category, callsign, text, verified
        ),
        NodeEvent::Morse {
            callsign,
            text,
            wpm,
            ..
        } => {
            println!("\n[morse {} @{}wpm] {}", callsign, wpm, text)
        }
        NodeEvent::SessionEstablished { peer } => {
            println!("\n[session established {}]", peer.to_hex())
        }
        NodeEvent::NeighborUpdate => println!("\n[neighbors changed]"),
        NodeEvent::RouteUpdate { dest, hops } => {
            println!("\n[route {} hops={}]", dest.to_hex(), hops)
        }
        NodeEvent::ContactRequest {
            from,
            callsign,
            text,
            ..
        } => println!(
            "\n[contact request {} {}] {} — /accept {}",
            callsign,
            from.to_hex(),
            text,
            from.to_hex()
        ),
        NodeEvent::GroupInvite {
            group_id,
            name,
            callsign,
            ..
        } => {
            println!(
                "\n[group invite {} \"{}\" from {}]",
                group_id, name, callsign
            )
        }
        NodeEvent::GroupMessage {
            group_id,
            callsign,
            text,
            ..
        } => {
            println!("\n[group {} {}] {}", group_id, callsign, text)
        }
        other => println!("\n{other:?}"),
    }
}
