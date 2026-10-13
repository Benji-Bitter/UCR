# The lineup — UCR hardware products

Every product you can build, each with a shopping list. They all run the
same `ucr-modem` firmware and speak the same wire protocol — the
difference is form factor, power, and how much the unit does on its own.

Everything here is open: commodity boards, published pin maps,
printable enclosures, `no_std` protocol. Nothing proprietary anywhere in
the chain — if a part is discontinued, another takes its place and only a
shopping-list line changes.

Effort honesty applies throughout: today's firmware is the dumb modem +
host architecture. Each product lists the software it needs to stand
alone; several already work tethered to the app today.

---

## 01 · The First Node — desk reference build

The board we develop against. If a guide says "the hardware", it means this.

| Part | ~Price |
|------|--------|
| ESP32-S3-DevKitC-1 (or any ESP32-S3 with native USB) | $10–15 |
| Wio-SX1262 (or generic SX1262 breakout) | $10–15 |
| Breadboard + 8 jumper wires + antenna | $5 |

Guide: `first-node.md`. Buy the module for your ISM band.

## 02 · The Pocket Node — zero-solder handheld

| Part | ~Price |
|------|--------|
| Heltec WiFi LoRa 32 V3 (ESP32-S3 + SX1262 + OLED + charger) | $18–25 |
| 18650 cell | $5–8 |
| Band-matched SMA whip | $3–8 |
| Lanyard / silicone sleeve | $3 |

Default pin map already matches — flash it, add battery, done.
Guide: `handheld.md` tier 1.

## 03 · The Scout — keychain node

The smallest real UCR node. Fits on a lanyard; announce presence, relay
short texts, beacon position.

| Part | ~Price |
|------|--------|
| Seeed XIAO ESP32-S3 | $8 |
| Wio-SX1262 for XIAO (pre-wired expansion board) | $10 |
| Micro whip or flex antenna | $3 |
| Coin cell or flat 500 mAh LiPo (optional) | $5 |

## 04 · The Field Node — self-locating go-bag unit

| Part | ~Price |
|------|--------|
| LilyGo T-Beam Supreme S3 (SX1262 + u-blox GPS + AXP2101 PMU + 18650 holder) | $30–38 |
| 18650 cell | $5–8 |
| Tuned SMA whip | $8–12 |
| Hammond / printed case | $5–10 |

GPS feeds the coarse position your node announces (NMEA→`pos` glue is a
listed contribution). Guide: `handheld.md` tier 2.

## 05 · The Ranger — the full handheld radio

The flagship. Screen, keypad, GPS, voice, a real PTT button — an actual
radio, sized like one.

| Part | ~Price | Role |
|------|--------|------|
| LilyGo T-Deck **or** the DIY stack below | $50–60 | ESP32-S3 + SX1262 + 2.8" IPS touch + keyboard + trackball + speaker + mic + battery holder, all in one |
| *DIY:* ESP32-S3 dev board | $12 | host CPU |
| *DIY:* SX1262 breakout | $12 | radio |
| *DIY:* 2.4" ILI9341 SPI TFT | $8 | status/channel screen |
| *DIY:* 4×4 membrane keypad | $3 | text entry + numeric PTT-lock PIN pad |
| *DIY:* INMP441 I2S mic | $3 | voice in |
| *DIY:* MAX98357A + 8Ω speaker | $6 | voice out |
| *DIY:* tactile PTT switch (GPIO) | $1 | hold-to-talk |
| *DIY:* u-blox NEO-M8/M10 GNSS | $12 | GPS → coarse position |
| *DIY:* TP4056 + boost + 18650 | $7 | charge + power |
| Tuned whip + u.fl pigtail | $10 | RF front end |
| Printed clamshell + strap | $8–15 | enclosure |

**Features this unlocks, honestly:**

- **Screen + keypad** — needs the on-device UI milestone (embedded node
  port). Today the T-Deck runs modem firmware and tethers to a phone over
  USB-C; standalone UI is the flagship contribution.
- **Keypad lock** — a real radio feature: PIN on wake so a borrowed unit
  can't transmit as you. Locks *transmission*, never reception — you still
  hear traffic and emergencies. Software, ships with the on-device UI.
- **Case lock** — the printed clamshell accepts a keyed cam latch;
  keep the battery bay sealed on field units.
- **GPS** — announces coarse position; precise fix never leaves the box.
- **Voice** — the I2S chain is where Codec2 lands on-device (the only
  voice codec that fits 1.75kbit/s LoRa).

## 06 · The Base Station — home node

| Part | ~Price |
|------|--------|
| Any ESP32-S3 + SX1262 combo (or Heltec V3) | $20–25 |
| Pi Zero 2 W or any always-on box you own | $15 |
| 5 dBi desktop antenna on a short feeder | $15 |
| Wall PSU | owned |

The set-and-forget house node: LAN bridge + RF in one `ucr-node` process.
Better antenna = the entire upgrade.

## 07 · The Vehicle Node

| Part | ~Price |
|------|--------|
| ESP32-S3 + SX1262 board | $20 |
| Mag-mount whip (roof) + coax | $15–25 |
| 12 V→5 V buck converter | $5 |
| u-blox GPS module | $10–15 |
| Dash or trunk enclosure | $5 |

Car-powered, roof antenna, GPS-positioned: the best mobile node money
barely buys. Ignition-sense power so it sleeps with the car; a metal roof
under a mag-mount is a free ground plane — real range gains.

## 08 · The Don — attic relay

| Part | ~Price |
|------|--------|
| Heltec V3 or any SX1262 board | $20 |
| Pi Zero 2 W + SD + PSU | $25 |
| Band-matched antenna | $8 |

The cheap always-on relay. Full build: `godfather.md`.

## 09 · The Godfather — mast relay

The community-grade always-on relay: collinear antenna, IP65 box, solar
option, RF↔LAN↔TCP gateway in one command. Full build + BOM:
`godfather.md` (~$120–150).

## 10 · The Watchtower — listening post

| Part | ~Price |
|------|--------|
| Cheapest ESP32-S3 + SX1262 combo | $15–18 |
| Any antenna | $3 |

Flashes the **receive-only** build (default, no `tx-enable`): fully legal
everywhere, hears everything, reports real RSSI/SNR to the host. Use it
for coverage mapping, monitoring your own mesh, or a radio you can hand
anyone with zero regulatory exposure.

## 11 · The Go-Box — emergency kit

Emergency preparedness is the project's stated purpose; this is the
physical form of it.

| Part | ~Price |
|------|--------|
| Pelican-style 1200 case (or ammo can) | $15–25 |
| ESP32-S3 + SX1262 board | $20 |
| 20000 mAh power bank + cable | $20–25 |
| Telescopic whip on panel mount + pigtail | $15 |
| Laminated quick-start card (channels, PTT, emergency) | $1 |
| Optional: foldable solar panel | $30+ |

Everything inside the case: open lid, power bank on, node up in seconds.
Runs receive-only out of the box so it can't make your day worse.

## 12 · The Dev Rig — contributor's bench

| Part | ~Price |
|------|--------|
| 2× ESP32-S3 + SX1262 sets | $25–40 |
| USB power meter | $8 |
| Spare antennas, jumper wires, headers | $10 |

Two radios on one desk lets you close the loop yourself: real frames, real
RSSI, real airtime — no second location needed. This is how firmware PRs
get made.

---

## Software each product needs

| product | runs today | needs (roadmap) |
|---------|-----------|-----------------|
| first node, scout, watchtower | tethered modem ✓ | — |
| pocket, field, vehicle, base, don, godfather, go-box | tethered modem + host app ✓ | GPS→pos glue (GPS units) |
| ranger | tethered modem ✓ | embedded node port, on-device UI, keypad lock, Codec2 |

`ucr-core` on ESP32 is the single unlock that turns the Ranger into a
phone-free radio. It's the project's highest-value contribution; the
firmware is already structured for it (`framing` + `sx1262` are
platform-agnostic, protocol is `no_std`).
