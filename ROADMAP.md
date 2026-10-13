# UCR Roadmap

## Phase 1 — Protocol core ✅ (current)

- [x] no_std wire format, NodeId identity, Ed25519 packet signatures
- [x] X25519+ChaCha20-Poly1305 E2E sessions with signed handshake
- [x] Mesh router: neighbours, link quality, scoped floods, unicast
      forwarding, RREQ/RREP, dedup, store-and-forward, acks+retry
- [x] Deterministic RF simulator (path loss, airtime, half-duplex,
      collisions, LBT, duty cycle, mobility, partitions)
- [x] Node runtime: channels, encrypted DMs, groups, emergency, morse
- [x] 13-scenario mesh integration suite
- [x] CLI: `ucr-sim` scenario runner, `ucr-node` interactive node

## Phase 2 — Usable application

- [x] Tauri desktop app (all sections, ASCII globe, live/sim modes)
- [x] Real transports in-app: UDP LAN broadcast, TCP gateways, serial
      modem via `MuxTransport`; `ucr-node` meshes zero-config on a LAN
- [ ] Persistent profiles/identity management UX
- [ ] Voice/tone announcements polish; morse training tools in UI
- [ ] Onboarding: channel presets, contact verification flow (SAS emoji)

## Phase 3 — Real radios

- [x] ESP32-S3 + SX1262 reference firmware (serial modem mode,
      `firmware/ucr-modem` — compiles for xtensa; RX + gated TX)
- [x] `SerialTransport` host driver (framing verified against the
      firmware codec; auto-detect pending hardware)
- [ ] Region profiles flipped to `tx_allowed` per
      `docs/hardware/regulatory.md` sign-off
- [ ] Field-test reports: range, multi-hop reliability, interference

## Phase 4 — Hardening & scale

- [ ] Adaptive beaconing by density/movement
- [ ] Route metric refinement (ETX-style, hysteresis)
- [ ] Encrypted local store; identity backup/rotate UX
- [ ] Congestion-aware scheduling beyond LBT
- [ ] Property/fuzz tests on codec + router; interop test vectors
- [ ] Spec 1.0: freeze wire format, conformance suite

## Ideas backlog

- Onion routing / metadata reduction research track
- Store-and-forward "post office" role for high sites
- APRS/SMS gateway transports (community-maintained)
- SDR transport via SoapySDR
