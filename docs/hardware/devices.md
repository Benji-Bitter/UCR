# Device catalog

Every board someone might turn into a UCR radio, sorted by how much work
stands between you and `radio ready`. The firmware reference is
`firmware/ucr-modem` (ESP32-S3 + SX1262); the modem contract for ports is
in `first-node.md` and `serial.md`.

Effort legend:

- **drop-in** — flashes today, no code changes
- **pin-map** — same chips, edit the ~7 GPIO lines at the top of `main()`
- **hal port** — different MCU; `sx1262.rs` is platform-agnostic, you
  rewrite the peripherals half of `main.rs` (table in `reference.md`)
- **radio swap** — not an SX1262; `framing.rs` survives, the driver doesn't

> Honesty note: only the reference build is compile-verified on the xtensa
> target. Nothing below has been validated on real silicon yet — when you
> run one of these on actual hardware, your pin map and result are exactly
> the kind of PR this repo wants.

---

## Drop-in

| Board | Radio | ~Price | Notes |
|-------|-------|--------|-------|
| ESP32-S3-DevKitC + Wio-SX1262 | SX1262 | $12–25 | **The reference.** Wire per `first-node.md`. |
| Heltec WiFi LoRa 32 V3 | SX1262 | $18–25 | OLED + charger onboard. The default pin map in `main.rs` is the Heltec V3 LoRa wiring — flashes unmodified. Buy the 868 or 915 SKU for your region. |

## Pin-map only

Same two chips, different GPIO routing. Change the pin block in `main()`
(seven lines) and, where noted, `module_options`.

| Board | What you get | ~Price | Notes |
|-------|--------------|--------|-------|
| LilyGo T-Beam Supreme (S3) | + u-blox GPS, AXP2101 PMU, 18650 holder | $30–38 | Field unit. GPS→`pos` glue is a known gap. See `handheld.md` tier 2. |
| LilyGo T-Deck | + 2.8" IPS, keyboard, trackball, mic, speaker | $50–60 | The standalone-radio body. See `handheld.md` tier 3. |
| LilyGo T-Beam S3 (v1.x SX1262 builds) | + GPS, 18650 | $28–35 | Older sibling of the Supreme; several revisions exist — match your board's schematic. |
| Seeed XIAO ESP32-S3 + Wio-SX1262 kit | tiny footprint | ~$18 | The expansion board is pre-wired; map Seeed's documented pin names onto our table. |
| Heltec Wireless Stick V3 / HTIT | bare-minimum V3 | $15–20 | V3 wiring family — likely drop-in, but verify against the board schematic before flashing. |
| Heltec HT-CT62 tracker | compact tracker | ~$15 | ESP32-S3 + SX1262; check schematic for routing. |

## HAL port — new MCU, same SX1262

`sx1262.rs` speaks `embedded-hal` traits, not esp-hal types. You supply
your platform's SPI/GPIO/delay; the driver and framing don't change.
Realistic ports, best-first:

| Board | MCU | ~Price | Why it's interesting |
|-------|-----|--------|----------------------|
| LilyGo T-Echo | nRF52840 + SX1262 | $55–65 | e-ink, GPS, Qi charging — the best-looking portable hardware. nrf-hal is mature Rust. |
| Raspberry Pi Pico + SX1262 breakout | RP2040 | $10–15 | Cheapest port target; rp-hal + a Pico make a $15 modem. |
| RAKwireless WisBlock (nRF52 core) | nRF52840 + SX1262 | $30–50 | Modular industrial ecosystem, proper enclosures. |
| XIAO ESP32-C3 + Wio-SX1262 | ESP32-C3 (RISC-V) | ~$15 | Tiny; still esp-hal so it's a target flag, not a rewrite — but no USB-JTAG, so the host link becomes UART. |
| STM32 Nucleo + Semtech shield | STM32L4 | $30–60 | For the STM32 crowd; embassy-stm32 is mature. |
| Arduino-class host | AVR/RP2040/other | varies | Don't port the Rust — write the *serial contract* (6 lines, `first-node.md`) in your stack and be a modem. |

## Radio swap — keep the framing, write a driver

Everything in `framing.rs` and the host serial protocol survives; only
`sx1262.rs` is replaced. The honest pitch: the installed base below is
bigger than SX1262's, so these ports matter.

| Radio | Found in | Effort |
|-------|----------|--------|
| SX1276/77/78/79 | RFM95W, older Heltec V1/V2, TTGO LoRa32 v1.x, Adafruit RFM9x FeatherWing | New LoRa driver — no CAD-DETECT nuance changes, different register map. Huge install base. |
| LR1121 | newer multi-band modules | Same SX126x family — smaller delta than SX1276. |
| EByte E22 UART modules | E22-900M30S etc. | Not a driver at all — it has its own UART protocol. Write a transport shim (serial frame ↔ E22 AT/binary) and skip `sx1262.rs` entirely. |
| RFM69/FSK radios | RFM69HCW | GFSK profile already exists in `profile.rs`; different modulation path. |
| nRF24L01+ | everywhere | 2.4 GHz, short range — fine for a bench mesh, not the citizen-radio target. |

## Infrastructure builds

- **[The Godfather Node](godfather.md)** — a dedicated always-on relay:
  no mic, no screen, no user. Any SX1262 board + a Pi Zero 2 W mounted
  high, declaring `RelayNode`, bridging RF↔LAN↔TCP in one command.
  Two builds priced: the attic "Don" (~$45) and the mast-mounted
  Godfather (~$120–150).

## No-radio nodes (still real UCR)

- **Any laptop/RPi** running `ucr-node` — UDP LAN + TCP gateways are real
  links; a Pi Zero 2 W makes a $15 always-on gateway.
- **Two machines, one LAN** — zero-hardware mesh for development.

---

## Adding your board

Pin-map PRs are the friendliest contribution going:

```text
1. flash the reference build (rx-only) — does the serial link work?
2. if `radio init FAILED`: your pin map differs → edit the 7-line GPIO
   block, note module_options (tcxo? rf switch?)
3. `espflash monitor` until `radio ready`
4. PR: your board, its exact pin map, what worked, what didn't
```

The pin-map-as-data refactor (a `boards/` table so boards select at
flash-time instead of edit-time) is an open contribution — talk to an
issue before starting.
