# UCR Wire Format

Version: 0.1 (draft — expect breaking changes until 1.0)

All multi-byte integers are little-endian via `postcard` compact encoding
over `serde`. `ucr-protocol` is `no_std`; the same bytes are produced by
firmware, desktop, and the simulator.

## Packet layout

```
+---------------------+----------------+------------------+
| Header (fixed)      | Payload (var)  | Signature (64B)  |
| postcard-encoded    | postcard       | Ed25519, optional|
+---------------------+----------------+------------------+
```

### Header

| Field       | Type    | Notes |
|-------------|---------|-------|
| `packet_id` | u32     | `(src, packet_id)` deduplicates the mesh |
| `src`       | NodeId  | originator identity (16 B) |
| `dst`       | NodeId  | recipient, or `BROADCAST` for floods |
| `relay`     | NodeId  | last-hop transmitter; rewritten per forward |
| `next_hop`  | NodeId  | designated forwarder (unicast); `BROADCAST` on floods |
| `ttl`       | u8      | decremented per forward; 0 ⇒ deliver only |
| `scope`     | enum    | originator-declared distribution bound |
| `hops`      | u8      | forward counter (diagnostics) |
| `prio`      | enum    | queue priority — does not bypass airtime limits |
| `want_ack`  | bool    | request authenticated end-to-end ack |

`NodeId` is 16 bytes: `BLAKE3(ed25519_pubkey)[..16]` — self-certifying;
`check_binding(pubkey, node_id)` proves a pubkey owns an id without any
directory. `BROADCAST` is the all-ones id.

### Scope → TTL

| Scope    | TTL |
|----------|-----|
| Direct   | 1   |
| Local    | 4   |
| Regional | 8   |
| Extended | 16  |

Intermediate nodes never extend a packet beyond its declared scope.

### Signature coverage

The signature covers the **immutable end-to-end portion** only:
`packet_id, src, dst, scope, prio, want_ack` + payload. Hop-by-hop fields
(`relay`, `next_hop`, `ttl`, `hops`) are unsigned so relays can rewrite
them without invalidating the originator's signature.

Packets that mutate the payload in flight (RREQ/RREP metric accumulation)
travel unsigned — they are control traffic whose validity is established
by processing rules, not signatures.

### Payloads (`Payload` enum)

- `Beacon` — periodic presence: pubkey, callsign, description, station
  class, capabilities, profile id, congestion, battery, coarse position,
  tone id, neighbour summary.
- `Data` → `AppData`:
  - `ChannelText` — named public channel message (flooded, signed).
  - `Plain` — signed cleartext apps (e.g. `PlainApp::Morse`).
  - `Encrypted` — `{session_id, nonce, ct}` (ChaCha20-Poly1305).
- `Ack` — authenticated delivery receipt `{acked_id, hops_taken}`.
- `SessionInit` / `SessionAccept` — signed X25519 handshake.
- `KeyRequest` / `KeyReply` — pubkey exchange for signature verification.
- `RouteRequest` / `RouteReply` — RREQ/RREP with accumulated metrics.
- `Emergency` — signed distress `{category, text, pos, emergency_id}`.
- `Tone` — identification-tone definition.
- `GroupInvite` / `GroupAccept` / `GroupDecline` / `GroupMessage` /
  `GroupKeyRotate` — group membership + symmetric-key traffic.

## E2E session handshake

1. A→B `SessionInit {pubkey_A, eph_pub_A, ts}` (signed packet).
2. B verifies the packet signature, checks `pubkey_A` binds to `src`, and
   `|now − ts| ≤ 10 min`; replies `SessionAccept {pubkey_B, eph_pub_B,
   session_id}` (signed packet).
3. Both derive `shared = X25519(eph_A, eph_B)` and
   `key, session_id = HKDF(shared, ids, eph pubs, ts)`.
4. A confirms `session_id` matches; session established.

Retransmitted `SessionInit`s are byte-identical (the timestamp is bound
into the session id) and retried on a paced schedule until an accept
arrives or the pending handshake ages out.

AEAD associated data is the `session_id` — identical on both ends.

## Forwarding rules (summary)

- Flood packets: dedup on `(src, packet_id)`; forward if `ttl > 0` and
  within scope; rewrite `relay`/`next_hop`/`ttl`/`hops`.
- Unicast packets: only the node named by `next_hop` forwards, choosing
  the fresh best route; everyone else treats them as overheard traffic.
- `want_ack` unicasts trigger a signed `Ack` from the destination;
  originators retry up to `ack_retries` times.
- Packets failing signature policy are never relayed.

## Radio profiles

Profiles describe a physical layer — frequency, bandwidth, spreading
factor, bitrate, duty-cycle budget, MTU, and a `tx_allowed` gate that
stays `false` until the regulatory checklist for that band is complete
(see `docs/hardware/regulatory.md`). The profile id rides in beacons so
mixed-profile nodes never attempt interoperation they can't complete.
