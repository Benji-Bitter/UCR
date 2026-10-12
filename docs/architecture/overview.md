# UCR Architecture

## Layered design

```
┌──────────────────────────────────────────────────────────┐
│ Applications: desktop app, ucr-node CLI, firmware UI     │
├──────────────────────────────────────────────────────────┤
│ ucr-core    Node runtime: sessions, contacts, groups,    │
│             channels, events, store-and-forward, acks    │
├─────────────┬──────────────┬──────────────┬──────────────┤
│ ucr-routing │ ucr-crypto   │ ucr-storage  │ ucr-morse    │
│ neighbours, │ X25519/AEAD  │ redb tables  │ timing/decode│
│ routes, QoS │ handshake    │              │              │
├─────────────┴──────────────┴──────────────┴──────────────┤
│ ucr-identity  Ed25519 + NodeId binding                   │
├──────────────────────────────────────────────────────────┤
│ ucr-protocol  wire format, profiles, channels (no_std)   │
├──────────────────────────────────────────────────────────┤
│ ucr-transport Transport trait: broadcast send/recv       │
│   ├─ ucr-simulator  deterministic RF medium              │
│   ├─ TcpTransport   point-to-point gateway link          │
│   └─ SerialTransport (feature) → firmware                │
└──────────────────────────────────────────────────────────┘
```

Dependencies point strictly downward — `ucr-protocol` knows nothing
about routing; `ucr-routing` knows nothing about crypto; `ucr-core`
composes them all.

## The `Node` runtime (`ucr-core`)

`Node::start(transport)` spawns three tasks:

- **rx task** — pulls frames from the transport, verifies signatures
  opportunistically (embedded pubkeys, learned keys, KeyRequest on miss),
  feeds the router, executes `RouteAction`s.
- **beacon task** — adaptive presence announcements with LBT-aware jitter.
- **maintenance task** — ack-deadline sweeps, retransmits, stale session
  handshake retries, store-and-forward queue flushing.

App-facing state is delivered two ways: a `broadcast::Receiver<NodeEvent>`
stream (messages, deliveries, sessions, emergencies, topology) and
synchronous query methods (`neighbors()`, `routes()`, `channels()`,
`dm_history()`, …).

### Lock discipline

`Node` uses `parking_lot::Mutex` for synchronous state (`router`,
`sessions`, `pending_acks`, `pending_e2e`, `cfg`, `store` is internally
locked). Hard rules learned from production deadlocks:

- Never evaluate a second `.lock()` while a guard temporary is alive —
  scrutinee expressions (`if`, `match`, `let`-chained inserts) hold
  temporaries for the whole statement.
- Never call a method that may lock `X` while holding `X`'s guard —
  hoist needed values into locals inside a scoped block.
- Lock order where nesting is unavoidable: `router` before
  `pending_acks`/`sessions`; `cfg` is leaf-level only.

## Routing model (`ucr-routing`)

- **Neighbour table** — keyed by `NodeId`; EWMA RSSI/SNR from every
  overheard frame (relay field attribution), rx/tx counters, freshness.
- **Routes** — learned three ways: passive observation (a heard relay is
  a proven next hop toward the originator), beacons' `seen` summaries,
  and on-demand RREQ/RREP accumulation.
- **Forwarding** — floods dedup-and-reflood inside scope+TTL; unicasts
  forward only when named `next_hop`.
- **Store-and-forward** — outbound queue keyed by destination with TTL;
  flushed by maintenance when a route appears.
- **Actions, not side effects** — `Router::handle_rx` returns
  `RouteAction`s (`Transmit`, `Deliver`, `SendAck`, `NeedRoute`,
  `ReplyKey`, `Drop`) that `Node` executes — the router stays pure and
  unit-testable.

## Cryptography (`ucr-identity` + `ucr-crypto`)

- Identity: Ed25519 keypair; `NodeId = BLAKE3(pubkey)[..16]`.
- Packet authenticity: Ed25519 signature over immutable end-to-end fields.
- Sessions: ephemeral X25519 (new keys per handshake → forward secrecy),
  ChaCha20-Poly1305 with `session_id` as AEAD associated data.
- Groups: symmetric group key distributed inside E2E sessions (invite
  payloads), rotated on member removal (`epoch` bumps).

## Persistence (`ucr-storage`)

`redb` embedded database — contacts, per-peer/channel message history,
groups+keys, learned pubkeys, delivery states, meta (read markers).
In-memory backend for tests/sim; file backend for real nodes.

## Transports (`ucr-transport`)

```rust
trait Transport {
    async fn send(&self, frame: &[u8]) -> Result<(), TransportError>;
    async fn recv(&self) -> Result<(Vec<u8>, RxMeta), TransportError>;
    fn mtu(&self) -> usize;
    fn profile_id(&self) -> u8;
    fn kind(&self) -> TransportKind;
    fn may_transmit(&self) -> bool { true }
    fn congestion(&self) -> u8 { 0 }
    fn measures_congestion(&self) -> bool { false }
}
```

A transport is a *broadcast medium*: `send` emits one frame to all
listeners in range; `recv` yields frames the medium decided this node
hears. Point-to-point links (TCP) emulate broadcast trivially; the
simulator emulates it faithfully.

## Simulator (`ucr-simulator`)

Deterministic event-driven medium (`SimWorld` + per-node `SimTransport`):

- Seeded `ChaCha8Rng`; identical seeds ⇒ identical runs.
- Free-space path loss + log-normal shadowing; per-link reception verdict.
- Airtime accounting; half-duplex deafness during own transmissions.
- Collision model: overlapping airtime destroys frames unless one
  dominates by capture margin.
- Listen-before-talk: senders defer while the channel reads busy.
- Duty-cycle budgets per profile; extra loss knob; environmental
  `range_limit_m`.
- Partitions (`block_pair`/`isolate`), obstacles (added loss along a
  segment), mobility patterns.

The whole node stack — beacons, RREQs, sessions, acks, store-and-forward
— runs on it unmodified, which is what the mesh integration suite proves.
