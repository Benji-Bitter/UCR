<div align="center">

<pre>
██    ██   ██████   ██████
██    ██  ██        ██   ██
██    ██  ██        ██████
██    ██  ██        ██ ██
 ██████    ██████   ██  ██
</pre>

<h3>universal citizens radio</h3>

<p><em>open, decentralised, hardware-independent digital radio<br>
no servers &middot; no accounts &middot; no internet required</em></p>

<p>
<a href="https://github.com/Benji-Bitter/UCR/actions/workflows/ci.yml"><img src="https://github.com/Benji-Bitter/UCR/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
<a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-black?style=flat-square" alt="Apache-2.0"></a>
<a href="#requirements"><img src="https://img.shields.io/badge/rust-%3E%3D1.81-black?style=flat-square&logo=rust&logoColor=white" alt="rust >= 1.81"></a>
<a href="docs/hardware/devices.md"><img src="https://img.shields.io/badge/lora-sx1262-black?style=flat-square" alt="lora sx1262"></a>
<a href="crates/ucr-protocol"><img src="https://img.shields.io/badge/wire--format-no__std-black?style=flat-square" alt="no_std wire format"></a>
<a href="docs/hardware/handheld.md"><img src="https://img.shields.io/badge/handheld-tiers-black?style=flat-square" alt="handheld tiers"></a>
<a href="CONTRIBUTING.md"><img src="https://img.shields.io/badge/PRs-welcome-black?style=flat-square" alt="PRs welcome"></a>
<a href="https://github.com/Benji-Bitter/UCR/stargazers"><img src="https://img.shields.io/github/stars/Benji-Bitter/UCR?style=flat-square&color=black&labelColor=black" alt="stars"></a>
</p>

</div>

UCR is an open, decentralised, hardware-independent digital radio
communication platform. Compatible devices exchange signed and encrypted
messages over intelligent multi-hop RF mesh networks. No central server,
no account, no internet required.

The primary purpose is general public communication, private messaging,
group communication, and emergency preparedness.

## Start here

| I want to… | go to |
|-----------|-------|
| run the app in 60 seconds | [Quickstart](#quickstart) |
| build my first node (~$15) | [first-node.md](docs/hardware/first-node.md) |
| pick a handheld to carry | [handheld.md](docs/hardware/handheld.md) — 3 tiers |
| put one on my boat | [marine.md](docs/hardware/marine.md) — 3 tiers |
| run an always-on relay | [godfather.md](docs/hardware/godfather.md) |
| see all 16 builds + shopping lists | [products.md](docs/hardware/products.md) |
| check if my board works | [devices.md](docs/hardware/devices.md) |
| flash the ESP32 firmware | [firmware/ucr-modem](firmware/ucr-modem/README.md) |
| write my own modem firmware | [protocol/serial.md](docs/protocol/serial.md) — the 6-line contract |
| read the protocol spec | [wire-format.md](docs/protocol/wire-format.md) |
| run it headless | [CLI tools](#cli-tools) — `ucr-node` |
| contribute | [CONTRIBUTING.md](CONTRIBUTING.md) · [ROADMAP.md](ROADMAP.md) |

**Status: early development.** The protocol core, mesh router, E2E
cryptography, real IP transports (UDP LAN broadcast and TCP gateways), a
serial modem transport, a deterministic RF simulator, and CLI tools are
implemented and tested. The desktop app runs live nodes over LAN/TCP or
the simulator, and the ESP32-S3 + SX1262 reference modem firmware compiles
for the real xtensa target. See [ROADMAP.md](ROADMAP.md).

## What works today

- **Self-certifying identity** using Ed25519 keys. `NodeId = BLAKE3(pubkey)[..16]`.
- **E2E encryption** via ephemeral X25519 handshake plus ChaCha20-Poly1305
  sessions. Signatures authenticate the handshake for forward secrecy.
- **Authenticated packets.** Every originator packet is Ed25519-signed,
  and the signature survives multi-hop relaying.
- **Multi-hop mesh routing** with scoped flooding, designated-next-hop
  unicast forwarding, passive route learning, RREQ/RREP discovery, dedup,
  store-and-forward, and ack tracking with retry.
- **Packet voice (push-to-talk).** Channels carry voice, not just text:
  hold to transmit sends signed IMA-ADPCM frames (8kHz mono, ~32kbit/s)
  flooded to every listener. Real-time by design: loss is skipped, never
  replayed. ADPCM needs broadband links (LAN/TCP); on LoRa the link budget
  gates it off and Codec2 voice is the modem-firmware path. Codec ids are
  registered (`ucr-protocol::payload::voice_codecs`) so new codecs slot in
  without breaking older nodes.
- **Public channels** for scoped broadcast text
  (`ucr.public`, marine, road, community, custom).
- **Encrypted DMs** with authenticated delivery acknowledgements.
- **Groups** with invites, accept/decline, and symmetric group messaging.
- **Emergency broadcasts** that are signed, priority-queued, locally
  rate-limited, and relayed regardless of sender blocking.
- **Digital Morse** as UCR payloads (not CW), plus a keying decoder and
  playback scheduler for training.
- **Digital lock** — a PIN gates every user-originated send (text, voice,
  DMs, groups, morse) while receive/relay keep working, like a real
  handheld's keypad lock. Emergency deliberately bypasses it; the PIN
  persists as a blake3 hash so a locked node stays locked across restarts.
- **Deterministic RF simulator** with a seeded medium: free-space path
  loss, shadowing, airtime, half-duplex, collisions, listen-before-talk,
  duty cycle, mobility, obstacles, and partitions. The whole protocol
  stack runs unmodified on it.
- **Real transports.** UDP subnet broadcast for zero-config LAN meshing,
  TCP gateways for point links across networks, and USB serial for the
  SX1262 modem. A mux transport lets one node ride all of them at once.
- **Built for the SX1262 link budget.** Wire packets on RF links are capped
  at 222 bytes — one LoRa frame — and the node measures real signed-packet
  overhead before sending. Long channel text splits APRS-style into
  multiple frames, oversized payloads fail with `TooLarge` instead of
  dying mid-air, and beacon pacing is airtime-aware: at SF9 (~1.75kbit/s)
  discovery beacons pace to ~37s so a node never eats the channel. The
  UI shows the governing link's profile, bitrate, and MTU live.
- **CLI tools**: `ucr-sim` scenario runner and `ucr-node` interactive
  headless node over real transports.
- **Desktop app** on Tauri 2 with a live ASCII globe network view
  (drag to rotate, scroll to zoom, search places/nodes), channels, DMs,
  groups, contacts, emergency, Morse, and a full event log.

## The builds

Sixteen products, one firmware, one wire protocol. The full catalog with
per-part shopping lists is **[docs/hardware/products.md](docs/hardware/products.md)**.

| product | ~cost | what it is |
|---------|-------|------------|
| **the first node** | $15–25 | ESP32-S3 + SX1262 on your desk — the reference build → [guide](docs/hardware/first-node.md) |
| **the pocket node** | $20 | Heltec V3 + battery + whip — zero-solder handheld |
| **the scout** | $18 | XIAO ESP32-S3 + Wio-SX1262 — keychain-sized node |
| **the field node** | $50 | T-Beam Supreme — own battery, own GPS, cased |
| **the ranger** | $80–120 | the flagship handheld: screen, keypad, digital lock, GPS, mic, speaker, real PTT |
| **the godfather handheld** | $180–260 | the best handheld — dual-watch, every feature, no-compromise parts |
| **the base station** | $40 | set-and-forget home node — LAN↔RF bridge |
| **the vehicle node** | $45 | 12 V, mag-mount roof whip, GPS — the mobile unit |
| **the deckhand** | $55 | IP67 crew handheld with GPS → [marine.md](docs/hardware/marine.md) |
| **the skipper** | $110 | helm install: GPS + house power + rail antenna → [marine.md](docs/hardware/marine.md) |
| **the admiral** | $210 | flagship vessel — floating Godfather relay → [marine.md](docs/hardware/marine.md) |
| **the don** | $45 | attic always-on relay → [godfather.md](docs/hardware/godfather.md) |
| **the godfather** | $120–150 | mast-mounted community relay + gateway → [godfather.md](docs/hardware/godfather.md) |
| **the watchtower** | $18 | receive-only listening post — legal everywhere, hears everything |
| **the go-box** | $90 | sealed emergency kit — open the lid, you're a node |
| **the dev rig** | $40 | two radios on one bench — how firmware PRs get made |

Everything is open: commodity boards, published pin maps, printable
enclosures, `no_std` protocol. No proprietary part anywhere in the chain.

Board shopping? [devices.md](docs/hardware/devices.md) catalogs ~20 boards
by effort tier: drop-in, pin-map edit, HAL port, or driver swap.
Pin-map PRs for new boards are the friendliest contribution going.

## Quickstart

```bash
git clone https://github.com/Benji-Bitter/UCR.git && cd UCR
cargo test --workspace              # prove the stack end-to-end (~3 min)
cd apps/desktop && pnpm install && pnpm tauri dev
```

The window that opens is the whole product: `[ go online ]` puts a real
node on LAN broadcast + TCP + whatever serial hardware it autodetects.
`[ run sim ]` is the labelled test world. Channels are voice-first, DMs
are encrypted end to end, the globe shows live topology.

## Requirements

| Tool | Why | Install |
|------|-----|---------|
| Rust >=1.81 (stable) | workspace crates | `rustup` from https://rustup.rs |
| Node >=18 + pnpm | desktop frontend | `npm i -g pnpm` |
| Tauri prerequisites | native window | see below |
| espup + Xtensa toolchain | firmware builds only | `cargo install espup` |

Tauri system prerequisites (one-time, for the desktop app):

- **macOS**: Xcode Command Line Tools via `xcode-select --install`
- **Linux (Debian/Ubuntu)**:
  `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`
- **Windows**: Microsoft C++ Build Tools plus WebView2 (usually preinstalled)

## Setup

```bash
git clone https://github.com/Benji-Bitter/UCR.git
cd UCR

# Rust workspace: protocol, identity, crypto, transport, routing,
# simulator, storage, morse, core, CLI
cargo build --workspace
cargo test --workspace        # unit + 13-scenario mesh suite
```

## Desktop app

```bash
cd apps/desktop
pnpm install

# Native app: real Rust node stack + simulator in-process
pnpm tauri dev                # dev build with hot-reload
pnpm tauri build              # release bundle (.app/.msi/deb)

# Or frontend-only in a browser. Runs a faithful JS port of the sim
# (the web-sim badge in the header tells you which backend is live):
pnpm dev                      # http://localhost:1420
```

Press `[ go online ]`. That starts a real node: UDP broadcast on the LAN
port you pick (default 4401), a TCP listener for gateways (default 4402),
an optional dial-out peer (`host:port`), and an optional serial modem.

**Hardware autodetect.** While the online form is open the app scans USB
serial continuously and probes every candidate with a `UCRPING` handshake —
a `ucr-modem` firmware answers with its ident string, gets marked `◉`, and
is auto-selected when it's the only one. Attach it and the whole stack
retunes: wire packets cap at 222 bytes (one SX1262 frame), beacons pace to
the LoRa airtime budget (~37s at SF9), the header shows
`au ism 915 lora 1750bps · mtu 222`, and voice gates itself off because
ADPCM can't fit a 1.75kbit/s link. Plug nothing in and you get the IP
profile: fast beacons, PTT voice, wide frames. Ports that don't answer
the handshake are still listed, marked unprobed, in case your firmware
predates the ping reply. Push-to-talk also checks for actual microphone
hardware before offering itself.

`[ run sim ]` instead spins up the deterministic test world: a line of
simulated nodes (count, spacing, seed configurable) that discover each
other and traffic every view. No hardware needed, and it is labelled
`sim` in the header so the modes are never confused.

- **network** is an ASCII globe built on real Natural Earth coastlines and
  world cities. Drag rotates, scroll zooms, double-click resets, and the
  `>` box searches any node or place and flies there. Node pins use coarse
  city positions; precise coordinates are never shown.
- **channels / direct / groups / morse** carry real protocol traffic.
  Channels are voice-first: `[ hold to talk ]` sends ADPCM voice frames;
  text sits underneath.
- **emergency** shows signed broadcasts with verification state.
- Clicking a pin selects it. `[ disable ]` partitions the node mid-mesh.

## CLI tools

```bash
# Deterministic scripted scenario over simulated RF:
cargo run -p ucr-cli --bin ucr-sim -- run --nodes 3 --duration 90 --seed 7

# Interactive nodes. With no flags they mesh over LAN broadcast; two
# machines on the same network find each other with zero config:
cargo run -p ucr-cli --bin ucr-node -- --callsign ALPHA
cargo run -p ucr-cli --bin ucr-node -- --callsign BRAVO

# Two nodes on one machine: use a TCP link between them (same-host
# broadcast sharing varies by OS):

cargo run -p ucr-cli --bin ucr-node -- --callsign ALPHA --listen 0.0.0.0:4402
cargo run -p ucr-cli --bin ucr-node -- --callsign BRAVO --connect 127.0.0.1:4402 --no-udp

# TCP gateways also link across networks (repeatable --connect):

# A real SX1262 modem on USB serial, plus LAN on the side:
cargo run -p ucr-cli --bin ucr-node -- --callsign VK2X --serial /dev/ttyUSB0

# Useful flags: --udp-port, --no-udp, --lat/--lon (coarse position to
# announce), --data-dir (persistent identity), --baud, --no-relay,
# --class (individual | base | relay | community | ...).
```

In `ucr-node`, bare text goes to `ucr.public`. `/help` lists commands
(`/dm`, `/morse`, `/emergency`, `/neighbors`, `/routes`, `/stats` ...).

## Firmware (ESP32-S3 + SX1262)

**New to the hardware side? Start with
[docs/hardware/first-node.md](docs/hardware/first-node.md)** — shopping
list, wiring table, flashing, first contact, and the modem contract for
writing your own firmware in any language. Building something handheld?
[docs/hardware/handheld.md](docs/hardware/handheld.md) prices three tiers,
from a $20 leashed node to a standalone radio with mic, speaker, and
screen. A different board? [docs/hardware/devices.md](docs/hardware/devices.md)
catalogs what drops in, what needs a pin map, and what needs a port.

```bash
cargo install espup && espup install   # Xtensa Rust toolchain, once
cd firmware/ucr-modem
cargo check                            # compiles for xtensa-esp32s3-none-elf
# with TX enabled (RX-only is the default, see regulatory notes):
cargo check --features tx-enable
```

Wire the SX1262 per `firmware/ucr-modem/README.md` (pin table + SPI map),
then flash with `cargo espflash flash --release --features tx-enable`
after completing the regulatory checklist in `docs/hardware/regulatory.md`.

Host-to-modem serial framing (`[0x55 0xAA] [u16 len LE] [payload] [crc8]`)
is shared byte-for-byte between `ucr-transport` and the firmware codec.
Cross-implementation tests live in `crates/ucr-transport/tests/serial_framing.rs`.

## Layout

```
crates/
  ucr-protocol    wire format, profiles, channels (no_std, shared with firmware)
  ucr-identity    Ed25519 identity, NodeId binding, packet sign/verify
  ucr-crypto      X25519 session handshake, ChaCha20-Poly1305 sessions
  ucr-transport   broadcast-medium transport trait; UDP/TCP/serial + mux
  ucr-routing     neighbour table, link quality, routes, dedup, queue
  ucr-simulator   deterministic RF medium (a real transport, not a mock)
  ucr-storage     redb persistence: contacts, messages, groups, keys
  ucr-morse       Morse timing, playback, key decoder
  ucr-core        node runtime; composes everything behind an event API
  ucr-cli         ucr-sim + ucr-node binaries
apps/desktop    Tauri 2 desktop app: React/TS frontend, Rust backend
firmware/ucr-modem  ESP32-S3 + SX1262 reference modem (esp-hal, no_std)
docs/           protocol, architecture, security, hardware, development
```

## Design invariants

- **Nothing requires infrastructure.** Every feature works peer-to-peer.
  Gateways are optional transports, not authorities.
- **The wire format is the spec.** `ucr-protocol` is `no_std`, so firmware
  and desktop share the exact same bytes.
- **Signatures cover end-to-end fields only.** Hop-by-hop header fields
  (`relay`, `next_hop`, `ttl`, `hops`) are mutable and unsigned, so
  relayed packets keep the originator's verifiable signature.
- **Honest radios.** The simulator models airtime, half-duplex deafness,
  collisions, and listen-before-talk deferral. Tests that pass here
  survive real RF.

## Documentation

- [docs/protocol/wire-format.md](docs/protocol/wire-format.md): packet & crypto spec
- [docs/architecture/overview.md](docs/architecture/overview.md): crate map & data flow
- [docs/security/model.md](docs/security/model.md): threat model & crypto rationale
- [docs/hardware/products.md](docs/hardware/products.md): the product lineup + shopping lists
- [docs/hardware/first-node.md](docs/hardware/first-node.md): build your first node
- [docs/hardware/handheld.md](docs/hardware/handheld.md): three handheld tiers
- [docs/hardware/godfather.md](docs/hardware/godfather.md): the always-on relay
- [docs/hardware/marine.md](docs/hardware/marine.md): marine builds — crew handheld → floating relay
- [docs/hardware/devices.md](docs/hardware/devices.md): supported-boards catalog
- [docs/hardware/reference.md](docs/hardware/reference.md): firmware plan + HAL porting guide
- [docs/protocol/serial.md](docs/protocol/serial.md): host-to-modem serial protocol
- [firmware/ucr-modem/README.md](firmware/ucr-modem/README.md): build & flash
- [CONTRIBUTING.md](CONTRIBUTING.md) · [ROADMAP.md](ROADMAP.md)

## License

Apache-2.0. See [LICENSE](LICENSE).

World-city data: GeoNames dump (CC-BY 4.0). Coastlines: Natural Earth.
