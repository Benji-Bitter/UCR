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

GPS feeds the coarse position your node announces — with the kill-switch
every GPS product shares: `set_share_position(false)` keeps the fix local,
`/gps off` on the CLI, `[ gps off ]` in the app (NMEA→`pos` glue is a
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
| *DIY:* 4×4 membrane keypad | $3 | text entry + digital-lock PIN pad |
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
- **Digital lock** — implemented in `ucr-core` today: `Node::lock(pin)`/
  `unlock(pin)` gate every user-originated send (channel text, voice, DMs,
  groups, morse) while reception, relaying, and beacons continue — you
  still hear traffic, and emergency deliberately bypasses the lock like an
  SOS button on a locked phone. The PIN persists as a blake3 hash in the
  node store, so the unit boots locked after a power cycle; `SendError::
  Locked` is the refusal everywhere. Desktop exposes it via
  `lock_node`/`unlock_node`/`clear_lock` + a header PIN entry; CLI has
  `/lock` `/unlock` `/lockstate`. The handheld UI maps the same calls to
  the 4×4 membrane pad.
- **GPS** — announces coarse position; precise fix never leaves the box.
  Ships with the toggle: `set_share_position` gates the beacon announce,
  persists in the store, keeps the fix when off — the keypad's GPS-off
  key maps straight to it.
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
| u-blox GPS module + toggle on the dash | $10–15 |
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

## 13 · The Godfather Handheld — the best one

Not a relay — a *user radio*, flagship of the line. If the Ranger is the
handheld, this is the handheld you hand to the person who asked for the
best: every feature in the catalog, no compromise parts, built to be the
radio that survives being dropped, drenched, and borrowed.

| Part | ~Price | Why |
|------|--------|-----|
| LilyGo T-Deck Plus | $75 | ESP32-S3 + SX1262 + screen + keyboard + **u-blox GPS onboard** + speaker + mic + battery holder — the most integrated base that exists |
| *or full DIY stack:* ESP32-S3 + SX1262 + 3.2" ILI9341 + mech-feel keypad + INMP441 + MAX98357A + NEO-M10 | $90 | bigger screen, better audio chain, your pick of case |
| Tuned SMA whip + proper u.fl→SMA pigtail | $15 | best RF front end a handheld gets — the tuned antenna is worth more range than any other $15 |
| 2× 18650 (or 21700) + battery sled | $10 | all-weekend runtime, field-swappable cells |
| Machined or PETG-CF clamshell + gasket | $20–30 | the enclosure is what makes it *feel* like the best |
| Second SX1262 module (dual-watch) | $12 | monitor `ucr-marine` while camping on `ucr.public` — real radios do this |
| USB-C cable + lanyard + screen protector | $10 | the bits that make it daily-carry |

**~$180–260 all in. What "best" means here, honestly:**

- **Dual-watch** — second SX1262 scanning a second channel; the mux
  transport already fans packets across links, so the firmware path is
  real, not fantasy.
- **Every software feature ships today on this hardware class** — digital
  lock (PIN pad is right there), morse, emergency bypass, groups, DMs,
  voice gating — all `ucr-core`, all already tested.
- **GPS built in** on the T-Deck Plus — coarse position announces with
  zero extra parts, and the same `set_share_position` kill-switch as
  every GPS product: announce off, fix retained.
- **Codec2-ready** — the I2S chain + ESP32-S3 DSP headroom is exactly
  where LoRa-band voice lands.
- **Open end to end** — the BOM is commodity parts, the case is
  printable, the firmware is this repo. "Best on the market" *and* the
  only one you can build, fix, and fork.
- **The honest gap is the same as every handheld** — it wants the
  `ucr-core`-on-ESP32 port to stop tethering. Until then it's the best
  UCR *handset*, host-brain included via USB-C.

## 14 · Marine lineup — UCR on the water

Three vessel builds — the crew handheld, the helm station, and a floating
Godfather relay. Boats are the best UCR platform that exists: free mast
height, free power, flat-water propagation, and a real reason for mesh.
**Not a VHF replacement** — your crew layer alongside Ch.16.

| build | ~cost | what it is |
|-------|-------|------------|
| deckhand | $55 | IP67 crew handheld with GPS — one per crew, floats in a dry box |
| skipper | $110 | helm install: GPS, 12V house power, rail antenna, always-on Pi node |
| admiral | $210 | flagship vessel: dual GPS, arch collinear, mast run, `--class relay` boat |

Full shopping lists, NMEA notes, and marine rules: `marine.md`.

---

## Software each product needs

| product | runs today | needs (roadmap) |
|---------|-----------|-----------------|
| first node, scout, watchtower | tethered modem ✓ | — |
| pocket, field, vehicle, base, don, godfather, go-box, deckhand, skipper, admiral | tethered modem + host app ✓ | GPS→pos glue (GPS units) |
| ranger, godfather handheld | tethered modem ✓ (digital lock ships in ucr-core) | embedded node port, on-device UI, Codec2 |

`ucr-core` on ESP32 is the single unlock that turns the Ranger into a
phone-free radio. It's the project's highest-value contribution; the
firmware is already structured for it (`framing` + `sx1262` are
platform-agnostic, protocol is `no_std`).
