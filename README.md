# UCR — Universal Citizens Radio

UCR is an open, decentralised, hardware-independent digital radio
communication platform. Compatible devices exchange signed and encrypted
messages over intelligent multi-hop RF mesh networks — with no central
server, no account, and no internet required.

The primary purpose is general public communication, private messaging,
group communication, and emergency preparedness.

**Status: early development.** The protocol core, mesh router, E2E
cryptography, deterministic RF simulator, and CLI tools are implemented
and tested. The desktop app runs the real node stack over the
simulator, and the ESP32-S3 + SX1262 reference modem firmware compiles
for the real xtensa target.
See [ROADMAP.md](ROADMAP.md).

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
apps/desktop    Tauri 2 desktop application (in progress)
firmware/ucr-modem  ESP32-S3 + SX1262 reference modem (esp-hal, no_std)
docs/           protocol, architecture, security, hardware, development
```

## Quick start

```bash
cargo build --workspace

# Run the full end-to-end mesh test suite (13 scenarios):
cargo test -p ucr-core --test mesh

# Run a scripted 3-node line scenario over simulated RF:
cargo run -p ucr-cli --bin ucr-sim -- run --nodes 3 --duration 90 --seed 7

# Chat between two real nodes over the TCP gateway transport:
cargo run -p ucr-cli --bin ucr-node -- --callsign ALPHA --listen 127.0.0.1:4400
cargo run -p ucr-cli --bin ucr-node -- --callsign BRAVO --connect 127.0.0.1:4400
```

In `ucr-node`, bare text goes to `ucr.public`; `/help` lists commands
(`/dm`, `/morse`, `/emergency`, `/neighbors`, `/routes`, `/stats` …).

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
