# UCR — Universal Citizens Radio

UCR is an open, decentralised, hardware-independent digital radio
communication platform. Compatible devices exchange signed and encrypted
messages over intelligent multi-hop RF mesh networks — with no central
server, no account, and no internet required.

The primary purpose is general public communication, private messaging,
group communication, and emergency preparedness.

**Status: early development.** The protocol core, mesh router, E2E
cryptography, deterministic RF simulator, and CLI tools are implemented
and tested. The desktop app runs the real node stack over the simulator,
and the ESP32-S3 + SX1262 reference modem firmware compiles for the real
xtensa target. See [ROADMAP.md](ROADMAP.md).

## What works today

- **Self-certifying identity** — Ed25519 keys; `NodeId = BLAKE3(pubkey)[..16]`.
- **E2E encryption** — ephemeral X25519 handshake + ChaCha20-Poly1305
  sessions; signatures authenticate the handshake (forward secrecy).
- **Authenticated packets** — every originator packet is Ed25519-signed;
  signatures survive multi-hop relaying.
- **Multi-hop mesh routing** — scoped flooding, designated-next-hop unicast
  forwarding, passive route learning, RREQ/RREP discovery, dedup,
  store-and-forward, ack tracking with retry.
- **Public channels** — scoped broadcast text channels
  (`ucr.public`, marine, road, community, custom).
- **Encrypted DMs** — with authenticated delivery acknowledgements.
- **Groups** — invite, accept/decline, symmetric group messaging.
- **Emergency broadcasts** — signed, priority-queued, local rate-limited,
  relayed regardless of sender blocking.
- **Digital Morse** — Morse as UCR payloads (not CW), plus a keying decoder
  and playback scheduler for training.
- **Deterministic RF simulator** — seeded medium with free-space path loss,
  shadowing, airtime, half-duplex, collisions, listen-before-talk,
  duty cycle, mobility, obstacles, and partitions. The whole protocol
  stack runs unmodified on it.
- **CLI tools** — `ucr-sim` scenario runner and `ucr-node` interactive
  headless node over the TCP gateway transport.
- **Desktop app** — Tauri 2 shell with a live ASCII globe network view
  (drag to rotate, scroll to zoom, search places/nodes), channels, DMs,
  groups, contacts, emergency, Morse, and a full event log.

## Requirements

| Tool | Why | Install |
|------|-----|---------|
| Rust ≥1.81 (stable) | workspace crates | `rustup` from https://rustup.rs |
| Node ≥18 + pnpm | desktop frontend | `npm i -g pnpm` |
| Tauri prerequisites | native window | see below |
| espup + Xtensa toolchain | firmware builds only | `cargo install espup` |

Tauri system prerequisites (one-time, for the desktop app):

- **macOS** — Xcode Command Line Tools: `xcode-select --install`
- **Linux (Debian/Ubuntu)** —
  `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`
- **Windows** — Microsoft C++ Build Tools + WebView2 (usually preinstalled)

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

# Native app — real Rust node stack + simulator in-process:
pnpm tauri dev                # dev build with hot-reload
pnpm tauri build              # release bundle (.app/.msi/deb)

# Or frontend-only in a browser — runs a faithful JS port of the sim
# (web-sim badge in the header tells you which backend is live):
pnpm dev                      # http://localhost:1420
```

Press `[ run ]` — no radio hardware needed. The simulator spins up a line
of nodes (count / spacing / seed configurable), they discover each other,
and every view is live:

- **network** — ASCII globe (real Natural Earth coastlines + world cities);
  drag rotates, scroll zooms, double-click resets, `>` box searches any
  node or place and flies there. Node pins are coarse city positions —
  precise coordinates are never shown.
- **channels / direct / groups / morse** — real protocol traffic.
- **emergency** — signed broadcasts with verification state.
- Clicking a pin selects it; `[ disable ]` partitions the node mid-mesh.

## CLI tools

```bash
# Deterministic scripted scenario over simulated RF:
cargo run -p ucr-cli --bin ucr-sim -- run --nodes 3 --duration 90 --seed 7

# Two interactive nodes over the TCP gateway transport:
cargo run -p ucr-cli --bin ucr-node -- --callsign ALPHA --listen 127.0.0.1:4400
cargo run -p ucr-cli --bin ucr-node -- --callsign BRAVO --connect 127.0.0.1:4400
```

In `ucr-node`, bare text goes to `ucr.public`; `/help` lists commands
(`/dm`, `/morse`, `/emergency`, `/neighbors`, `/routes`, `/stats` …).

## Firmware (ESP32-S3 + SX1262)

```bash
cargo install espup && espup install   # Xtensa Rust toolchain, once
cd firmware/ucr-modem
cargo check                            # compiles for xtensa-esp32s3-none-elf
# with TX enabled (RX-only is the default — see regulatory notes):
cargo check --features tx-enable
```

Wire the SX1262 per `firmware/ucr-modem/README.md` (pin table + SPI map),
flash with `cargo espflash flash --release --features tx-enable` after
completing the regulatory checklist in `docs/hardware/reference.md`.

Host↔modem serial framing (`[0x55 0xAA] [u16 len LE] [payload] [crc8]`)
is shared byte-for-byte between `ucr-transport` and the firmware codec —
cross-implementation tests live in `crates/ucr-transport/tests/serial_framing.rs`.

## Layout

```
crates/
  ucr-protocol    wire format, profiles, channels — no_std (shared with firmware)
  ucr-identity    Ed25519 identity, NodeId binding, packet sign/verify
  ucr-crypto      X25519 session handshake, ChaCha20-Poly1305 sessions
  ucr-transport   broadcast-medium transport trait + TCP/serial transports
  ucr-routing     neighbour table, link quality, routes, dedup, queue
  ucr-simulator   deterministic RF medium (a real transport, not a mock)
  ucr-storage     redb persistence: contacts, messages, groups, keys
  ucr-morse       Morse timing, playback, key decoder
  ucr-core        node runtime — composes everything behind an event API
  ucr-cli         ucr-sim + ucr-node binaries
apps/desktop    Tauri 2 desktop app — React/TS frontend, Rust backend
firmware/ucr-modem  ESP32-S3 + SX1262 reference modem (esp-hal, no_std)
docs/           protocol, architecture, security, hardware, development
```

## Design invariants

- **Nothing requires infrastructure.** Every feature works peer-to-peer;
  gateways are optional transports, not authorities.
- **The wire format is the spec.** `ucr-protocol` is `no_std` — firmware
  and desktop share the exact same bytes.
- **Signatures cover end-to-end fields only** — hop-by-hop header fields
  (`relay`, `next_hop`, `ttl`, `hops`) are mutable and unsigned, so relayed
  packets keep the originator's verifiable signature.
- **Honest radios.** The simulator models airtime, half-duplex deafness,
  collisions, and listen-before-talk deferral — tests that pass here
  survive real RF.

## Documentation

- [docs/protocol/wire-format.md](docs/protocol/wire-format.md) — packet & crypto spec
- [docs/architecture/overview.md](docs/architecture/overview.md) — crate map & data flow
- [docs/security/model.md](docs/security/model.md) — threat model & crypto rationale
- [docs/hardware/reference.md](docs/hardware/reference.md) — firmware plan + HAL porting guide
- [docs/protocol/serial.md](docs/protocol/serial.md) — host↔modem serial protocol
- [firmware/ucr-modem/README.md](firmware/ucr-modem/README.md) — build & flash
- [CONTRIBUTING.md](CONTRIBUTING.md) · [ROADMAP.md](ROADMAP.md)

## License

Apache-2.0 — see [LICENSE](LICENSE).

World-city data: GeoNames dump (CC-BY 4.0). Coastlines: Natural Earth.
