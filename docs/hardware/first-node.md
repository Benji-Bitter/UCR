# Your first UCR node

From a bare board on your desk to a real node on the mesh. No prior
radio experience needed — the firmware is written so the risky parts
(radio legality, channel access) are already handled.

## Shopping list

| Option | Cost | Notes |
|--------|------|-------|
| Generic ESP32-S3 dev board + Wio-SX1262 (or any SX1262 breakout) | ~$12–25 | The reference build. Wire it yourself with the table below. |
| Heltec WiFi LoRa 32 V3 | ~$25 | ESP32-S3 + SX1262 on one board; adjust the pin map to the board's fixed wiring. |
| LilyGo T-Beam Supreme | ~$30 | ESP32-S3 + SX1262 + GPS + battery. Same deal: check its pin map. |
| **Nothing** | free | `ucr-node` meshes over LAN UDP/TCP with zero radio hardware — same protocol, same app. |

The radio module must be an **SX1262** (or SX1268/SX1261 — same driver).
Buy the variant matching your region's ISM band: 433, 868, or 915 MHz
modules are not interchangeable — the antenna and matching network differ.

Building something you carry? **[handheld.md](handheld.md)** prices out
three handheld tiers — a $20 leashed node, a self-locating field unit,
and a full standalone radio with mic, speaker, and screen.

## Wiring (generic ESP32-S3 + SX1262 breakout)

```text
 SX1262      ESP32-S3     purpose
 --------    ---------    ---------------------------------
 VCC         3V3          power — 3.3 V only, never 5 V
 GND         GND          ground
 SCK         GPIO9        SPI clock
 MOSI        GPIO10       SPI host → radio
 MISO        GPIO11       SPI radio → host
 NSS         GPIO8        SPI chip select
 RST         GPIO12       radio reset
 BUSY        GPIO13       radio busy flag (must be an input)
 DIO1        GPIO14       radio interrupt (RX done / TX done / CAD)
 ANT         (antenna)    screw/solder the antenna BEFORE powering on TX
```

All seven signal pins are assigned in one block at the top of `main()` in
`firmware/ucr-modem/src/main.rs` — if your board's LoRa chip is wired to
different GPIOs (Heltec and T-Beam both are), change the numbers there.
`module_options(tcxo_on_dio3, dio2_rf_switch)` in the same place must match
your module; nearly every SX1262 module on the market is `(true, true)`.

## Flash it

```sh
# one-time toolchain setup
cargo install espup && espup install
. ~/export-esp.sh                 # or add it to your shell profile
cargo install espflash

# build + flash (receive-only by default — legal everywhere, listens and
# reports what it hears but cannot transmit)
cd firmware/ucr-modem
cargo espflash flash --release    # or: cargo run --release
```

The boot log (USB serial monitor, `espflash monitor`) should print:

```text
ucr-modem: init sx1262 @ 916000000 Hz
ucr-modem: radio ready, mode=StbyRc
ucr-modem: receive-only build (tx-enable feature off)
```

If `radio init FAILED` appears instead, your pin map or module options are
wrong — check the wiring table and `module_options` first.

## Connect it

Plug the board into USB and open the desktop app. While the online form
is open it scans USB serial continuously: a UCR modem answers the
`UCRPING` handshake and shows up as `◉ ucr-modem/0.1 esp32-sx1262`.
Pick it (a lone modem picks itself) and press `[ go online ]`.

The header switches to the LoRa profile — `lora 1750bps · mtu 222` —
and the node retunes automatically: RF-sized packets, airtime-paced
beacons, chunked text. No configuration to get wrong.

## Transmit: the legal gate

The firmware cannot transmit until you build with `--features tx-enable`,
and that is a deliberate legal checkpoint, not friction:

```sh
cargo espflash flash --release --features tx-enable
```

Enabling TX asserts that **you** have verified your jurisdiction's rules
for the frequency, power, and duty cycle you are running — the checklist
is in `docs/hardware/regulatory.md`. Default is the AU 915 MHz profile at
+20 dBm; EU/US profiles ship receive-only pending verification.

Until then the modem is a fully-functional *monitor*: it hears packets,
measures real RSSI/SNR, answers probes, and streams everything to the
host. Your host node just never gets frames relayed to air.

## Write your own firmware

The contract a device must satisfy to be a UCR modem — implement it in
any language on any MCU:

```text
serial frame   [0x55 0xAA] [u16 len LE] [payload] [crc8/SMBUS poly 0x07]
control        answer payload "UCRPING" with "UCRMODEM <your ident>"
host → radio   payload = one raw UCR wire packet; transmit it verbatim
radio → host   payload = packet || rssi_i16_le || snr_i8 || 0xA5
sizes          wire packet ≤ 222 bytes (one SX1262 LoRa frame)
discipline     CAD listen-before-talk before every transmit
```

Everything else — identity, signatures, encryption, routing, channels,
voice framing — lives on the host in `ucr-core`. The modem is intentionally
dumb; if your firmware moves bytes and respects the air, it is a UCR modem.

For porting inside this repo instead (STM32, nRF52, different radio):
`framing.rs` and `sx1262.rs` are platform-agnostic; you only rewrite the
peripherals half of `main.rs`. The trait-to-pin table is in
`reference.md`.

## The honest state of things

- Compiles for `xtensa-esp32s3-none-elf`; the driver, framing, LBT and the
  probe handshake are implemented. 
- It has not yet been validated on physical silicon — first-on-air testing
  is exactly the kind of contribution this project needs. Found a bug on
  real hardware? Open an issue with your board + pin map.
- Host fallback while you wait for parts: `--serial` accepts any framed
  port, and two machines on a LAN mesh over UDP with no radio at all.
