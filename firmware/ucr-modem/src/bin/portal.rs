//! UCR captive-portal firmware — ESP32-S3 SoftAP.
//!
//! The radio becomes its own Wi-Fi network: SSID `UCR-XXXX`, gateway
//! 192.168.4.1. A minimal DHCP responder hands phones an address, a wildcard
//! DNS answers every lookup with our IP, and an HTTP server on :80 serves the
//! portal page — so phones show the captive-portal popup and every URL lands
//! on the radio's own UI. The phone is the screen.
//!
//! Build:  cargo build --features wifi-portal --bin portal --release

#![no_std]
#![no_main]

use core::fmt::Write as _;
use core::str;

use embassy_executor::Spawner;
use embassy_net::{
    tcp::TcpSocket,
    udp::{PacketMetadata, UdpSocket},
    Config as NetConfig, IpAddress, IpEndpoint, IpListenEndpoint, Ipv4Address, Ipv4Cidr, Runner,
    Stack, StackResources, StaticConfigV4,
};
use embassy_time::{Duration, Instant, Timer};
use esp_backtrace as _;
use esp_hal::{efuse::base_mac_address, rng::Rng, timer::timg::TimerGroup};
use esp_println::println;
use esp_radio::wifi::{
    ap::AccessPointConfig, Config as WifiMode, ControllerConfig, Interface, Ssid, WifiController,
};
use heapless::String;
use static_cell::StaticCell;

const AP_IP: [u8; 4] = [192, 168, 4, 1];
const PAGE: &[u8] = include_bytes!("../../../../assets/portal.html");
const DNS_ANSWER: [u8; 16] = [
    0xc0, 0x0c, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x3c, 0x00, 0x04, 192, 168, 4, 1,
];

#[esp_rtos::main]
async fn main(spawner: Spawner) {
    esp_alloc::heap_allocator!(size: 128 * 1024);

    let p = esp_hal::init(esp_hal::Config::default());
    let timg0 = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timg0.timer0, p.FROM_CPU_INTR0);

    let mac = base_mac_address();
    let mac = mac.as_bytes();
    static SSID_CELL: StaticCell<String<32>> = StaticCell::new();
    let ssid = SSID_CELL.init(String::<32>::new());
    write!(ssid, "UCR-{:02X}{:02X}", mac[4], mac[5]).unwrap();

    let ap_cfg = AccessPointConfig::default()
        .with_ssid(Ssid::try_from(ssid.as_str()).unwrap());
    let controller_cfg =
        ControllerConfig::default().with_initial_config(WifiMode::AccessPoint(ap_cfg));
    let _controller = WifiController::new(p.WIFI, controller_cfg).unwrap();
    let ap = Interface::access_point();

    let net_cfg = NetConfig::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(Ipv4Address::new(AP_IP[0], AP_IP[1], AP_IP[2], AP_IP[3]), 24),
        gateway: Some(Ipv4Address::new(AP_IP[0], AP_IP[1], AP_IP[2], AP_IP[3])),
        dns_servers: Default::default(),
    });
    let seed = Rng::new().random() as u64;
    static RES: StaticCell<StackResources<6>> = StaticCell::new();
    let (stack, runner) =
        embassy_net::new(ap, net_cfg, RES.init(StackResources::<6>::new()), seed);

    println!();
    println!("##      ##     ########     ########");
    println!("##      ##    ##            ##      ##");
    println!("##      ##    ##            ########");
    println!("##      ##    ##            ##    ##");
    println!("##########     ########     ##      ##");
    println!();
    println!("ucr-modem portal: ap {} up — http://{}.{}.{}.{}", ssid, AP_IP[0], AP_IP[1], AP_IP[2], AP_IP[3]);

    spawner.spawn(run_task(runner).unwrap());
    spawner.spawn(dhcp_task(stack).unwrap());
    spawner.spawn(dns_task(stack).unwrap());
    spawner.spawn(http_task(stack, ssid.as_str()).unwrap());

    loop {
        Timer::after(Duration::from_secs(3600)).await;
    }
}

/// Runner::run polls the device AND the socket state machine — the only
/// background task the stack needs.
#[embassy_executor::task]
async fn run_task(mut runner: Runner<'static, Interface>) {
    runner.run().await
}

/// Minimal BOOTP/DHCP responder — one fixed lease table is overkill for a
/// captive AP, so the offer is derived deterministically from the client MAC.
#[embassy_executor::task]
async fn dhcp_task(stack: Stack<'static>) {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0; 1024];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buf = [0; 1024];
    let mut sock = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if sock.bind(IpListenEndpoint { addr: None, port: 67 }).is_err() {
        println!("dhcp: bind :67 failed");
        return;
    }
    let mut req = [0u8; 600];
    loop {
        let (n, _from) = match sock.recv_from(&mut req).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        if n < 240 || req[0] != 1 || req[236..240] != [0x63, 0x82, 0x53, 0x63] {
            continue;
        }
        let mtype = dhcp_msg_type(&req[..n]);
        if mtype != 1 && mtype != 3 {
            continue; // only DISCOVER / REQUEST get an answer
        }
        let mut rep = [0u8; 300];
        rep[..240].copy_from_slice(&req[..240]);
        rep[0] = 2; // BOOTREPLY
        rep[16] = 192;
        rep[17] = 168;
        rep[18] = 4;
        rep[19] = 20 + (req[33] % 200); // yiaddr — from client MAC byte
        rep[20..24].copy_from_slice(&AP_IP); // siaddr
        let mut o = 240usize;
        rep[o] = 53;
        rep[o + 1] = 1;
        rep[o + 2] = if mtype == 1 { 2 } else { 5 }; // OFFER / ACK
        o += 3;
        rep[o] = 54; // server id
        rep[o + 1] = 4;
        rep[o + 2..o + 6].copy_from_slice(&AP_IP);
        o += 6;
        rep[o] = 51; // lease 3600s
        rep[o + 1] = 4;
        rep[o + 2..o + 6].copy_from_slice(&[0, 0, 0x0e, 0x10]);
        o += 6;
        rep[o] = 1; // netmask /24
        rep[o + 1] = 4;
        rep[o + 2..o + 6].copy_from_slice(&[255, 255, 255, 0]);
        o += 6;
        rep[o] = 3; // router
        rep[o + 1] = 4;
        rep[o + 2..o + 6].copy_from_slice(&AP_IP);
        o += 6;
        rep[o] = 6; // dns = us (wildcard answers below)
        rep[o + 1] = 4;
        rep[o + 2..o + 6].copy_from_slice(&AP_IP);
        o += 6;
        rep[o] = 255;
        o += 1;
        let bcast = IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)), 68);
        let _ = sock.send_to(&rep[..o], bcast).await;
    }
}

fn dhcp_msg_type(pkt: &[u8]) -> u8 {
    let mut i = 240;
    while i < pkt.len() {
        let tag = pkt[i];
        if tag == 255 {
            break;
        }
        if tag == 0 {
            i += 1;
            continue;
        }
        if i + 1 >= pkt.len() {
            break;
        }
        let len = pkt[i + 1] as usize;
        if tag == 53 && i + 2 < pkt.len() {
            return pkt[i + 2];
        }
        i += 2 + len;
    }
    0
}

/// Wildcard DNS — every A query resolves to the radio. This is what makes
/// captive-portal detection redirect any domain to the portal page.
#[embassy_executor::task]
async fn dns_task(stack: Stack<'static>) {
    let mut rx_meta = [PacketMetadata::EMPTY; 8];
    let mut rx_buf = [0; 512];
    let mut tx_meta = [PacketMetadata::EMPTY; 8];
    let mut tx_buf = [0; 512];
    let mut sock = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);
    if sock.bind(IpListenEndpoint { addr: None, port: 53 }).is_err() {
        println!("dns: bind :53 failed");
        return;
    }
    let mut q = [0u8; 512];
    loop {
        let (n, from) = match sock.recv_from(&mut q).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        if n < 13 {
            continue;
        }
        // walk the first qname: labels until a 0-length label or pointer
        let mut i = 12usize;
        while i < n {
            let l = q[i] as usize;
            if l == 0 {
                i += 1;
                break;
            }
            if l & 0xc0 == 0xc0 {
                i += 2;
                break;
            }
            i += 1 + l;
        }
        i += 4; // qtype + qclass
        if i > n || i + 16 > 600 {
            continue;
        }
        let mut rep = [0u8; 600];
        rep[..i].copy_from_slice(&q[..i]);
        rep[2] = 0x81;
        rep[3] = 0x80; // standard response, no error
        rep[6] = 0;
        rep[7] = 1; // ancount = 1
        rep[8] = 0;
        rep[9] = 0;
        rep[10] = 0;
        rep[11] = 0;
        rep[i..i + 16].copy_from_slice(&DNS_ANSWER);
        let _ = sock.send_to(&rep[..i + 16], from).await;
    }
}

#[embassy_executor::task]
async fn http_task(stack: Stack<'static>, ssid: &'static str) {
    let mut rx_buf = [0; 2048];
    let mut tx_buf = [0; 4096];
    loop {
        let mut sock = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        sock.set_timeout(Some(Duration::from_secs(30)));
        if sock
            .accept(IpListenEndpoint { addr: None, port: 80 })
            .await
            .is_err()
        {
            Timer::after(Duration::from_millis(200)).await;
            continue;
        }
        let mut req = [0u8; 1536];
        let mut used = 0usize;
        loop {
            match sock.read(&mut req[used..]).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    used += n;
                    if used >= req.len() || req[..used].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
            }
        }
        let path = req_path(&req[..used]);
        if path == "/api/status" {
            let mut json = String::<384>::new();
            let _ = write!(
                json,
                "{{\"ssid\":\"{}\",\"ip\":\"192.168.4.1\",\"radio\":\"sx1262\",\"link\":\"lora 1750\",\"firmware\":\"ucr-modem 0.1\",\"uptime_ms\":{}}}",
                ssid,
                Instant::now().as_millis()
            );
            send_response(&mut sock, "application/json", json.as_bytes()).await;
        } else {
            // every other path serves the portal — captive-probe URLs included
            send_response(&mut sock, "text/html", PAGE).await;
        }
        sock.close();
    }
}

async fn send_response(sock: &mut TcpSocket<'_>, content_type: &str, body: &[u8]) {
    let mut hdr = String::<192>::new();
    let _ = write!(
        hdr,
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        content_type,
        body.len()
    );
    write_all(sock, hdr.as_bytes()).await;
    write_all(sock, body).await;
}

/// embassy-net 0.7 sockets do partial writes; loop until the buffer's out.
async fn write_all(sock: &mut TcpSocket<'_>, mut buf: &[u8]) {
    while !buf.is_empty() {
        match sock.write(buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf = &buf[n..],
        }
    }
}

fn req_path(buf: &[u8]) -> &str {
    str::from_utf8(buf)
        .ok()
        .and_then(|s| s.strip_prefix("GET "))
        .and_then(|s| s.split(' ').next())
        .unwrap_or("/")
}
