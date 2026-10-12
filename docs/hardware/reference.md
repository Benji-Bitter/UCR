# Reference Hardware & Firmware Plan

Target: **ESP32-S3 + Semtech SX1262** — the cheapest widely-available
LoRa combination (e.g. Heltec LoRa 32 V3, LilyGo T-Beam, generic
SX1262 modules). Nothing in the design is ESP32-specific; the transport
trait accepts any radio.

## Firmware architecture

```
┌─────────────────────────────────────────────┐
│ ucr-modem (no_std Rust, esp-hal)            │
│  ├─ ucr-protocol (shared — same wire bytes) │
│  ├─ SX1262 driver (embedded-hal SPI)        │
│  ├─ radio task: CAD/LBT → TX/RX             │
│  └─ host link: framed serial protocol       │
└──────────────┬──────────────────────────────┘
               │ USB serial
┌──────────────▼──────────────────────────────┐
│ Host (desktop / ucr-node)                   │
│   SerialTransport ⇄ full UCR node stack     │
└─────────────────────────────────────────────┘
```

The firmware is a *modem*: it does channel access (CAD listen-before-
talk) and frame passthrough. All routing, crypto, and app logic stays in
`ucr-core` — one implementation, every platform. A future standalone mode
can run `ucr-core` on-device for nodes without a host.

Implementation: `firmware/ucr-modem/` — see its README for toolchain,
pin map, and flashing.

## Serial framing

Implemented and documented in `docs/protocol/serial.md`:
`[0x55 0xAA] [u16 len LE] [payload] [crc8]` both directions; downstream
payloads append `rssi_i16_le || snr_i8 || 0xA5`. No open-time handshake —
the modem is stateless.

## Radio bring-up checklist

1. SPI init + SX1262 reset/NSS/DIO1/BUSY mapping per board (`main.rs`).
2. Module options: TCXO on DIO3? RF switch on DIO2? (`module_options`).
3. SetPacketType(LoRa), frequency per `RadioProfile`.
4. Modulation params: SF/BW/CR from profile; LDRO at SF11+.
5. PA config + TX power per regulatory table — **TX is feature-gated
   (`tx-enable`) until `docs/hardware/regulatory.md` is complete**.
6. RX continuous + CAD before every TX (LBT).
7. Frame pipe: serial RX → air TX; air RX → serial TX + RSSI/SNR trailer.

## Porting the modem (HAL guide)

`sx1262.rs` is written against `embedded-hal` traits, not esp-hal types —
porting means supplying your platform's HAL, not rewriting the driver:

| Driver needs         | embedded-hal trait | ESP32-S3 impl        |
|----------------------|--------------------|----------------------|
| SPI + chip select    | `SpiDevice`        | `ExclusiveDevice`    |
| BUSY line            | `InputPin`         | `gpio::Input`        |
| RESET line           | `OutputPin`        | `gpio::Output`       |
| DIO1 IRQ line        | `InputPin`         | `gpio::Input`        |
| timeouts             | `fn() -> u64` ms   | `Instant` clock      |
| host link            | byte read/write    | `UsbSerialJtag`      |

To port to e.g. STM32 + SX1262 or nRF52840: swap the `spi`, `busy`,
`reset`, `dio1`, `now_ms`, and host-link halves of `main.rs`. `framing.rs`
and `sx1262.rs` are platform-agnostic already.

To use a different radio entirely (SX1276, custom): keep `framing.rs`
byte-identical and replace `sx1262.rs` — the host never sees the radio,
only the serial contract.

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
