//! `ucr-node --portal` — a tiny captive-portal web UI served by the node.
//!
//! Screenless radios (Scout, bare modems, anything without a display)
//! get a screen through whatever hosts them. Run the node on the host —
//! Pi, laptop, relay — with `--portal 0.0.0.0:80` and any phone/browser
//! that can reach it gets the UI. When the host is also the WiFi AP
//! (hostapd/dnsmasq wildcard-DNS, or the ESP32 AP mode in firmware), the
//! OS captive-portal check lands here too and the page pops open
//! automatically — that's the whole trick.
//!
//! Deliberately dependency-free: HTTP/1.0, three JSON endpoints, one
//! embedded page. Small enough to port to the ESP32 unchanged.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use ucr_core::Node;
use ucr_protocol::ChannelId;

const PAGE: &str = include_str!("../../../assets/portal.html");

/// Serve the portal on `addr` until the process exits.
pub async fn serve(node: Node, channel: ChannelId, channel_name: String, addr: String) {
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("portal bind {addr} failed: {e}");
            return;
        }
    };
    eprintln!("portal on http://{addr} — captive UI served here");
    let channel_name = std::sync::Arc::new(channel_name);
    loop {
        let Ok((mut sock, _)) = listener.accept().await else {
            continue;
        };
        let node = node.clone();
        let channel_name = channel_name.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 8192];
            let Ok(n) = sock.read(&mut buf).await else {
                return;
            };
            let req = String::from_utf8_lossy(&buf[..n]);
            let mut parts = req.split_whitespace();
            let method = parts.next().unwrap_or("");
            let path = parts.next().unwrap_or("/");
            if path == "/api/status" {
                let stats = node.stats();
                let peers: Vec<String> = node
                    .neighbors()
                    .iter()
                    .map(|p| {
                        let rssi = if p.rssi_ewma.is_nan() {
                            "null".to_string()
                        } else {
                            format!("{:.0}", p.rssi_ewma)
                        };
                        format!(
                            "{{\"n\":\"{}\",\"cls\":\"{:?}\",\"rssi\":{}}}",
                            esc(&p.callsign),
                            p.class,
                            rssi
                        )
                    })
                    .collect();
                let pos = if node.share_position() {
                    node.pos()
                        .map(|p| format!("\"lat\":{:.2},\"lon\":{:.2},", p.lat(), p.lon()))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                let body = format!(
                    "{{\"callsign\":\"{}\",\"node_id\":\"{}\",\"channel\":\"{}\",\"uptime_s\":{},\"locked\":{},\"share_pos\":{},{}\"peers\":[{}]}}",
                    esc(&node.callsign()),
                    node.node_id().to_hex(),
                    esc(&channel_name),
                    stats.uptime_ms / 1000,
                    node.is_locked(),
                    node.share_position(),
                    pos,
                    peers.join(",")
                );
                reply(&mut sock, "200 OK", "application/json", &body).await;
            } else if path == "/api/feed" {
                let me = node.node_id();
                let msgs: Vec<String> = node
                    .channel_history(&channel, 30)
                    .iter()
                    .map(|m| {
                        let who = if m.peer == me {
                            "you".to_string()
                        } else {
                            node.neighbors()
                                .iter()
                                .find(|p| p.node_id == m.peer)
                                .map(|p| p.callsign.clone())
                                .unwrap_or_else(|| m.peer.to_hex()[..8].to_string())
                        };
                        format!(
                            "{{\"who\":\"{}\",\"text\":\"{}\",\"me\":{}}}",
                            esc(&who),
                            esc(&m.text),
                            m.peer == me
                        )
                    })
                    .collect();
                reply(
                    &mut sock,
                    "200 OK",
                    "application/json",
                    &format!("{{\"msgs\":[{}]}}", msgs.join(",")),
                )
                .await;
            } else if path == "/api/send" && method == "POST" {
                let body = req.split("\r\n\r\n").nth(1).unwrap_or("").trim();
                let resp = if body.is_empty() {
                    "{\"err\":\"empty\"}"
                } else {
                    match node.send_channel_text(&channel, body) {
                        Ok(_) => "{\"ok\":true}",
                        Err(e) if format!("{e:?}").contains("Locked") => "{\"err\":\"locked\"}",
                        Err(_) => "{\"err\":\"send failed\"}",
                    }
                };
                reply(&mut sock, "200 OK", "application/json", resp).await;
            } else if method == "GET" {
                // Captive behavior: every path — including the OS probe URLs
                // (/generate_204, /hotspot-detect.html, /ncsi.txt) — gets the
                // page, which is what makes phones pop the portal open.
                reply(&mut sock, "200 OK", "text/html", PAGE).await;
            } else {
                reply(&mut sock, "404", "text/plain", "not found").await;
            }
        });
    }
}

async fn reply(sock: &mut tokio::net::TcpStream, status: &str, ctype: &str, body: &str) {
    let head = format!(
        "HTTP/1.0 {status}\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    let _ = sock.write_all(head.as_bytes()).await;
    let _ = sock.write_all(body.as_bytes()).await;
    let _ = sock.shutdown().await;
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "")
}
