# Schematics + breakouts — every product

Every UCR product is **one core circuit plus optional breakout blocks**.
This file is the wiring truth: the core schematic every radio shares, the
block library (GPS, display, audio, keypad, PTT, second radio, power), and
a per-product diagram showing exactly which blocks connect.

Pin numbers are the **canonical map** matching `firmware/ucr-modem/src/main.rs`
on an ESP32-S3-DevKitC-1. All-in-one boards (Heltec V3, T-Beam, T-Deck,
XIAO) wire the radio at the factory — for those, the "schematic" is the
onboard path and a pin-map note instead of jumper wires.

**Before soldering:** check your board's PSRAM type. ESP32-S3 variants with
**octal PSRAM (N8R8/N16R8)** strap GPIO33–37 — those pins are NOT free.
The map below avoids them anyway; GPIO19/20 (native USB) and GPIO0/3/45/46
(strapping) are deliberately unused by UCR.

---

## The core — every product shares this

```
                 ┌────────────────────┐
                 │   ESP32-S3 DevKit  │
                 │                    │
                 │  GPIO9  (SCK)  ────┼──────┐
                 │  GPIO10 (MOSI) ────┼───┐  │
                 │  GPIO11 (MISO) ────┼─┐ │  │
                 │  GPIO8  (NSS)  ────┼┐ │ │  │
                 │  GPIO12 (RST)  ────┼┼┐│ │  │
                 │  GPIO13 (BUSY) ────┼┼┼┼┐│  │
                 │  GPIO14 (DIO1) ────┼┼┼┼┼┼┐ │
                 │                    ││││││││
                 │  3V3 ──────────────┼┼┼┼┼┼┼┼┐
                 │  GND ──────────────┼┼┼┼┼┼┼┼┼┐
                 └────────────────────┘│││││││││
                                        │││││││││
                 ┌────────────────────┐│││││││││
                 │   SX1262 module    │││││││││
                 │   (Wio / breakout) │││││││││
                 │                    │││││││││
                 │  SCK  ◄────────────┼┘│││││││
                 │  MOSI ◄────────────┼──┘││││││
                 │  MISO ────────────►│───┘││││
                 │  NSS  ◄────────────┼────┘││││
                 │  RST  ◄────────────┼─────┘│││
                 │  BUSY ────────────►│──────┘││
                 │  DIO1 ────────────►│───────┘│
                 │  3V3  ◄────────────┼────────┘
                 │  GND  ◄────────────┼─────────
                 │                    │
                 │  ANT ──────────────┼──► antenna (REQUIRED before TX)
                 └────────────────────┘
```

| signal | ESP32-S3 | SX1262 | direction | notes |
|--------|----------|--------|-----------|-------|
| SCK    | GPIO9    | SCK    | →        | SPI clock |
| MOSI   | GPIO10   | MOSI   | →        | |
| MISO   | GPIO11   | MISO   | ←        | |
| NSS    | GPIO8    | NSS    | →        | chip select, idle high |
| RST    | GPIO12   | RST    | →        | radio reset |
| BUSY   | GPIO13   | BUSY   | ←        | radio busy flag — must read |
| DIO1   | GPIO14   | DIO1   | ←        | IRQ: TX done / RX done / CAD |
| power  | 3V3+GND  | 3V3+GND | →       | **never 5V** — SX1262 is 3.3V only |
| ANT    | —        | ANT    | —        | antenna before any transmit |

Host link is USB-C: the ESP32-S3's native USB-Serial/JTAG presents a CDC
serial port; the `UCRPING`/framed-packet contract runs on it
(`docs/protocol/serial.md`).

---

## The breakout library

### A · GPS — u-blox NEO-M8N/M10N on UART1

```
ESP32-S3                NEO-M8N module
GPIO17 (TX1) ────────► RX
GPIO18 (RX1) ◄──────── TX
3V3/5V*      ────────► VCC   *module-dependent — check yours
GND          ───────── GND
```

NMEA 0183 sentences at 9600 baud → `ucr-node --lat/--lon` (glue is a
listed contribution). Kill-switch: `Node::set_share_position` —
`[/gps off]` / `[ gps off ]` keeps the fix local.

### B · Display — ILI9341 2.4" SPI TFT (shares the radio's SPI bus)

```
ESP32-S3                ILI9341
GPIO9  (SCK)  ────────► SCK      (shared with radio)
GPIO10 (MOSI) ────────► SDI/MOSI (shared)
GPIO38        ────────► CS       (own select — this is what makes sharing safe)
GPIO39        ────────► D/C
GPIO40        ────────► RES
GPIO41        ────────► LED/BL   (via ~220Ω resistor)
3V3 + GND     ───────── VCC/GND
```

Rule of SPI sharing: same clock/data lines, **one CS per device** — the
radio keeps NSS=GPIO8, display gets CS=GPIO38.

### C · Mic — INMP441 I2S (voice in)

```
ESP32-S3                INMP441
GPIO4  (BCK)  ────────► SCK
GPIO5  (WS)   ────────► WS
GPIO6  (SD)   ◄──────── SD
GND           ───────── L/R   (tie to GND = left channel)
3V3 + GND     ───────── VDD/GND
```

### D · Speaker — MAX98357A I2S (voice out, shares mic clocks)

```
ESP32-S3                MAX98357A
GPIO4  (BCK)  ────────► BCLK   (shared with mic)
GPIO5  (WS)   ────────► LRC    (shared)
GPIO7  (DIN)  ────────► DIN
5V   + GND    ───────── VIN/GND
                        +── 8Ω speaker
```

RX and TX on one I2S clock pair — mic reads SD-in, amp drives DIN-out.
This is the exact chain Codec2 lands on.

### E · Keypad — 4×4 membrane via PCF8574 I2C backpack

```
ESP32-S3                PCF8574 board              4×4 keypad
GPIO1  (SDA)  ────────► SDA
GPIO2  (SCL)  ────────► SCL
3V3 + GND     ───────── VCC/GND
                        P0–P7 ────────────────► R1–R4, C1–C4
```

The 8-wire matrix would eat eight GPIOs — the $1 I2C backpack costs two.
Text entry + the digital-lock PIN pad (`Node::lock`/`unlock`).

### F · PTT — one tactile button

```
ESP32-S3                switch
GPIO21 ────────┬─────── (one leg)
               │   GPIO input_pullup
GND      ──────┴─────── (other leg)
```

Hold = transmit (`send_channel_voice` / the app's pointer PTT path).
Active-low, debounced in firmware.

### G · Second radio — dual-watch SX1262 (shares SPI)

```
shared:   SCK=9  MOSI=10  MISO=11  (both modules listen to the same bus)
radio 2:  NSS2=GPIO15  RST2=GPIO16  BUSY2=GPIO17*  DIO1-2=GPIO18*
3V3 + GND, own antenna — dual-band if you pair 868+915 modules
```

(*GPS moves to the free UART pads if it shares the build — the pin budget
table at the bottom resolves every collision.)

### H · Power — three real-world rails

```
handheld:  18650 ─► TP4056 charge ─► MT3608 boost ─► 5V pin (or 3V3 rail w/ LDO)
vehicle:   12V fused tap ─► buck 5V ─► USB-C in     (+ ignition-sense relay)
base/mast: wall PSU / solar+SLA ─► buck 5V ─► USB-C in
```

Never feed a bare board from unregulated 12V; never run the SX1262 on 5V.

### I · Host link — how a modem talks to its brain

```
ESP32-S3 (native USB) ══ USB-C cable ══ host (laptop / Pi Zero / Pi 4)
  framed serial 115200 8N1:  [55 AA][u16 len][payload][CRC8]
  host→modem: wire packet / UCRPING
  modem→host: wire packet + (RSSI i16 | SNR i8 | 0xA5) trailer
```

---

## Per-product schematics

### 01 · First Node — `CORE` alone

```
[ PC/laptop ]═USB-C═[ ESP32-S3 ]═SPI+GPIO═[ SX1262 ]═ANT
```
The reference. `ucr-modem` flashes, host app drives it.

### 02 · Pocket Node — factory-wired board

```
[ Heltec V3 ]  (onboard: ESP32-S3→SX1262 + OLED + charge IC)
   + 18650 + SMA whip
```
Zero wiring. Pin map = Heltec's factory routing — firmware's default map
already matches (that's why it's the zero-solder pick).

### 03 · Scout — factory-wired pair

```
[ XIAO ESP32-S3 ]═shield headers═[ Wio-SX1262 for XIAO ]═flex ant
```
Pre-wired expansion board — no jumper wires, but `main.rs`'s pin block
needs the XIAO GPIO numbers (`devices.md` recipe).

### 04 · Field Node — factory-wired board + cell

```
[ T-Beam Supreme ]  (onboard: ESP32-S3→SX1262, u-blox GPS on UART,
                     AXP2101 PMU, 18650 sled)
   + whip + case
```
GPS path is onboard — the missing piece is only the NMEA→`pos` software glue.

### 05 · Ranger — `CORE` + B + C + D + E + F + A + H

```
            ┌────────────────────────────────────────────┐
            │              ESP32-S3 DevKit               │
            │                                            │
  SPI bus ──┼── 9/10/11 ──┬──────────────┬───────────┐   │
            │             │              │           │   │
   control ─┼─ NSS8 RST12 BUSY13 DIO1-14 │           │   │
            │             │        CS38 DC39 RES40 BL41  │
            │        [ SX1262 ]    [ ILI9341 screen ]    │
            │                                            │
   I2S in ──┼─ BCK4 WS5 SD6 ──[ INMP441 mic ]            │
   I2S out ─┼─ BCK4 WS5 DIN7 ─[ MAX98357A+speaker ]      │
   PTT ─────┼─ GPIO21 ────────[ PTT button ]             │
   keypad ──┼─ SDA1 SCL2 ─────[ PCF8574 → 4×4 pad ]      │
   GPS ─────┼─ TX17 RX18 ─────[ NEO-M8N ]                │
            │                                            │
   power: TP4056+boost+18650 → 5V rail                   │
            └────────────────────────────────────────────┘
```

### 06 · Base Station — `CORE` + I + better antenna

```
[ Pi Zero (ucr-node --class base) ]═USB═[ ESP32-S3+SX1262 ]═5dBi desktop ant
```
LAN↔RF bridge: the mux transport fans packets across serial+UDP+TCP.

### 07 · Vehicle Node — `CORE` + A + H(12V)

```
12V fused → buck 5V → [ ESP32-S3+SX1262 ]═mag-mount roof whip
                      GPIO17/18 ← NEO-M8N (dash toggle → set_share_position)
```

### 08 · The Don — `CORE` + I + shelf antenna

```
[ Pi Zero: ucr-node --class relay ]═USB═[ modem ]═attic antenna
```

### 09 · The Godfather — `CORE` + I + mast hardware

```
mast: [ collinear ]═LMR-200↓═[ IP65 box: Pi+modem ]← solar/PSU
        --listen 0.0.0.0:4402 --class relay
```

### 10 · Watchtower — `CORE`, receive-only

Same wires — the difference is firmware config, not copper: default
`tx-enable`-absent build hears everything, transmits nothing.

### 11 · Go-Box — `CORE` + H(power bank)

```
[ power bank ]═USB-C═[ ESP32-S3+SX1262 ]═telescopic whip   (all in case)
```

### 12 · Dev Rig — `CORE` ×2

Two first nodes, one desk — each its own `ucr-node` + serial port.

### 13 · Godfather Handheld — `CORE` + all handheld blocks + G

```
      ┌────────────────────────────────────────────────┐
      │                  ESP32-S3                      │
      │  SPI 9/10/11 ──┬───────────────┬───────────────┤
      │   radio 1: NSS8 RST12 BUSY13 DIO14   [SX1262 #1]
      │   radio 2: NSS15 RST16 BUSY* DIO*    [SX1262 #2]  dual-watch
      │   display: CS38 DC39 RES40 BL41      [3.2" ILI9341]
      │   I2S: BCK4 WS5 SD6(mic) DIN7(amp)   [INMP441][MAX98357A]
      │   keypad: SDA1 SCL2                  [PCF8574→4×4]
      │   PTT: GPIO21  GPS: UART17/18        [NEO-M10N]
      │   power: 2×18650 + sled + TP4056     │          │
      └────────────────────────────────────────────────┘
```

(*when radio #2 + GPS share the build, GPS moves to UART2 on GPIO43/44 —
see the pin budget.)

### 14 · Deckhand — `CORE`(Heltec) + A + IP67 box

Heltec V3 + NEO-M8N on its spare UART + whip through a cable gland.
Position announces on `ucr-marine`; `/gps off` at anchor.

### 15 · Skipper — `CORE` + I + A + H(12V)

```
house 12V→buck→[ IP65 box: Pi Zero(ucr-node --class base) + modem ]
                 USB↕                       GPIO17/18↕ NEO-M8N
        ── coax ──► rail/arch whip
```

### 16 · Admiral — `CORE` + I + A×2 + H(marine DC)

```
mizzen/arch: [ 5–8dBi collinear ]═LMR-200↓
nav bay: [ Pi 4 (ucr-node --class relay) ]═USB═[ ESP32-S3+SX1262 ]
           USB↕ NMEA 0183 bridge ← chartplotter (primary GPS)
           GPIO↕ NEO-M10N (backup fix)
house bank → fused buck → 5V
```

---

## The canonical pin budget — one page to resolve every collision

| GPIO | job | free for |
|------|-----|----------|
| 8–14 | radio 1 (NSS/RST/BUSY/DIO1/SPI) | fixed — the core |
| 9,10,11 | SPI bus | shared by display + radio 2 |
| 1,2 | I2C SDA/SCL | keypad backpack, OLED, sensors |
| 4–7 | I2S audio (BCK/WS/SD/DIN) | mic + amp share clocks |
| 15–18 | radio 2 controls / GPS UART | dual-watch OR GPS UART |
| 17,18 | GPS UART1 (when radio 2 absent) | |
| 21 | PTT button | any single input |
| 38–41 | display CS/DC/RES/BL | second CS must be unique |
| 43,44 | UART0 pads (free when console=USB-JTAG) | GPS UART2 on dual-radio builds |
| 19,20 | native USB | **reserved** — host link |
| 0,3,45,46 | strapping | avoid |
| 33–37 | octal PSRAM (N8R8/N16R8) | avoid on R8 variants |

Collisions resolve themselves if you follow the budget: radios share SPI
but each needs its own NSS/RST/BUSY/DIO1; displays share SPI but each
needs its own CS; UARTs are per-peripheral; I2C addresses multiplex.

*Boards differ — `devices.md` maps this budget onto each board's real
GPIO routing. When a build works, a pin-map PR teaches the next builder.*
