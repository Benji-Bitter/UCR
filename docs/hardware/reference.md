# Reference Hardware & Firmware Plan

Target: **ESP32-S3 + Semtech SX1262** — the cheapest widely-available
LoRa combination (e.g. Heltec LoRa 32 V3, LilyGo T-Beam, generic
SX1262 modules). Nothing in the design is ESP32-specific; the transport
trait accepts any radio.

## Firmware architecture

```
┌─────────────────────────────────────────────┐
│ ucr-firmware (no_std Rust, esp-hal)         │
│  ├─ ucr-protocol (shared — same wire bytes) │
│  ├─ SX1262 driver (SPI)                     │
│  ├─ radio task: CAD/LBT → TX/RX             │
│  └─ host link: framed serial protocol       │
└──────────────┬──────────────────────────────┘
               │ USB serial (or SPI direct)
┌──────────────▼──────────────────────────────┐
│ Host (desktop / ucr-node)                   │
│   SerialTransport ⇄ full UCR node stack     │
└─────────────────────────────────────────────┘
```

The firmware is a *modem*: it does channel access (CAD listen-before-
talk), airtime/duty enforcement, and frame passthrough. All routing,
crypto, and app logic stays in `ucr-core` — one implementation, every
platform. A future standalone mode can run `ucr-core` on-device for
nodes without a host.

## Serial framing (docs/protocol/serial.md — draft)

Length-prefixed frames: `0x55, proto_ver, profile_id, mtu(2B)` hello at
open (same handshake as `TcpTransport`), then `len(2B) + frame` in both
directions. RX metadata frames carry `{rssi_dbm, snr_db}` when the radio
reports them; otherwise `RxMeta::unmeasured`.

## Radio bring-up checklist

1. SPI init + SX1262 reset/NSS/DIO1 mapping per board.
2. SetPacketType(LoRa), frequency per `RadioProfile`.
3. Modulation params: SF/BW/CR from profile; LDRO as required.
4. PA config + TX power clamp per regulatory table.
5. RX continuous + CAD before every TX (LBT).
6. Airtime computation (Semtech formula) → duty budget accounting.
7. Frame pipe: serial RX → air TX; air RX → serial TX + RSSI/SNR.

## Why SX1262

- Sub-GHz ISM globally (433/868/915 MHz variants).
- CAD lets us do real LBT — the same behaviour the simulator models.
- ~2 km+ practical links at legal power; better with elevation.
- $8–25 hardware; huge hobbyist supply chain.

## Alternative radios (transport-compatible)

- **SX1276** (older LoRa) — same class, driver tweak only.
- **nRF52840 + external PA** — BLE + sub-GHz companion.
- **WiFi-aware / ESP-NOW** — unlicensed 2.4 GHz mesh transport where
  sub-GHz is impractical.

Anything implementing `Transport { send, recv, mtu, profile_id }` is a
UCR radio.
