# The lineup — UCR hardware products

Every product you can build, each with a shopping list. They all run the
same `ucr-modem` firmware and speak the same wire protocol — the
difference is form factor, power, and how much the unit does on its own.

Everything here is open: commodity boards, published pin maps,
printable enclosures, `no_std` protocol. Nothing proprietary anywhere in
the chain — if a part is discontinued, another takes its place and only a
shopping-list line changes.

**Part links** point at the manufacturer's page or a supplier search —
they're starting points, not affiliate links; buy wherever your region
ships cheapest. **Wiring:** every product's schematic and pin breakout is
in [`schematics.md`](schematics.md).

## The software map — what runs on what

Every product is the same two-piece architecture:

| layer | code | runs on |
|-------|------|---------|
| modem firmware | [`firmware/ucr-modem`](../../firmware/ucr-modem) (`main.rs` + `framing.rs` + `sx1262.rs`) | the ESP32-S3 — SPI to the SX1262, framed serial to the host |
| node brain | [`crates/ucr-core`](../../crates/ucr-core) (`node.rs` — routing, crypto, storage, lock, GPS toggle) | the host: desktop app, `ucr-node` CLI, or a Pi |
| transports | [`crates/ucr-transport`](../../crates/ucr-transport) (`serial.rs`, `mux.rs`, `detect.rs`) | serial↔radio bridge + LAN/TCP links on the same node |
| host apps | [`apps/desktop`](../../apps/desktop) · [`crates/ucr-cli`](../../crates/ucr-cli) | Tauri app / headless `ucr-node` |

Each product below lists what runs today and what it still needs —
several already work tethered to the app *today*.

---

## 01 · The First Node — desk reference build

The board we develop against. If a guide says "the hardware", it means this.

| Part | ~Price |
|------|--------|
| [ESP32-S3-DevKitC-1](https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32s3/) (or any ESP32-S3 with native USB) | $10–15 |
| [Wio-SX1262](https://www.seeedstudio.com/catalogsearch/result/?q=Wio-SX1262) (or generic SX1262 breakout) | $10–15 |
| [Breadboard + 8 jumper wires + antenna](https://www.amazon.com/s?k=esp32+breadboard+jumper+wire+kit) | $5 |

**Software:** flash `firmware/ucr-modem` (`cargo espflash flash --release`),
plug in — the app auto-detects it via the `UCRPING` handshake in
`transport/detect.rs`, clamps MTU to 222, paces beacons by airtime, gates
voice. Wire contract for writing your own firmware: `docs/protocol/serial.md`.
Guide: `first-node.md`. Buy the module for your ISM band.

## 02 · The Pocket Node — zero-solder handheld

| Part | ~Price |
|------|--------|
| [Heltec WiFi LoRa 32 V3](https://heltec.org/project/wifi-lora-32-v3/) (ESP32-S3 + SX1262 + OLED + charger) | $18–25 |
| [18650 cell](https://www.amazon.com/s?k=18650+battery+flat+top) | $5–8 |
| [Band-matched SMA whip](https://www.amazon.com/s?k=915mhz+lora+antenna+sma) | $3–8 |
| Lanyard / silicone sleeve | $3 |

Default pin map already matches — flash it, add battery, done.
**Software:** same `ucr-modem` firmware; the onboard OLED shows nothing
today (display task is a listed contribution in `first-node.md`).
Guide: `handheld.md` tier 1.

## 03 · The Scout — keychain node

The smallest real UCR node. Fits on a lanyard; announce presence, relay
short texts, beacon position.

| Part | ~Price |
|------|--------|
| [Seeed XIAO ESP32-S3](https://www.seeedstudio.com/catalogsearch/result/?q=XIAO-ESP32S3) | $8 |
| [Wio-SX1262 for XIAO](https://wiki.seeedstudio.com/wio_sx1262_with_xiao_esp32s3_for_meshtastic/) (pre-wired expansion board) | $10 |
| Micro whip or flex antenna | $3 |
| Coin cell or flat 500 mAh LiPo (optional) | $5 |

**Software:** same firmware, new pin map — the XIAO's GPIO routing differs,
so `main.rs`'s pin block needs editing (recipe in `devices.md`). Smallest
HAL-port target in the catalog.

## 04 · The Field Node — self-locating go-bag unit

| Part | ~Price |
|------|--------|
| [LilyGo T-Beam Supreme S3](https://www.aliexpress.com/wholesale?SearchText=lilygo+t-beam+supreme+s3) (SX1262 + u-blox GPS + AXP2101 PMU + 18650 holder) | $30–38 |
| [18650 cell](https://www.amazon.com/s?k=18650+battery+flat+top) | $5–8 |
| [Tuned SMA whip](https://www.amazon.com/s?k=915mhz+lora+antenna+sma) | $8–12 |
| [Hammond / printed case](https://www.amazon.com/s?k=hammond+abs+enclosure+box) | $5–10 |

GPS feeds the coarse position your node announces — with the kill-switch
every GPS product shares: `set_share_position(false)` keeps the fix local,
`/gps off` on the CLI, `[ gps off ]` in the app (NMEA→`pos` glue is a
listed contribution). Guide: `handheld.md` tier 2.

**Software:** `ucr-modem` + `ucr-node --lat/--lon` host-side. GPS→NMEA→
`NodeConfig.pos` glue is a first contribution — the node side is one
`set_position()` call away (`crates/ucr-core/src/node.rs`).

## 05 · The Ranger — the full handheld radio

The flagship. Screen, keypad, GPS, voice, a real PTT button — an actual
radio, sized like one.

| Part | ~Price | Role |
|------|--------|------|
| [LilyGo T-Deck](https://lilygo.cc/products/t-deck) **or** the DIY stack below | $50–60 | ESP32-S3 + SX1262 + 2.8" IPS touch + keyboard + trackball + speaker + mic + battery holder, all in one |
| *DIY:* [ESP32-S3 dev board](https://docs.espressif.com/projects/esp-dev-kits/en/latest/esp32s3/) | $12 | host CPU |
| *DIY:* [SX1262 breakout](https://www.amazon.com/s?k=sx1262+module+868+915) | $12 | radio |
| *DIY:* [2.4" ILI9341 SPI TFT](https://www.amazon.com/s?k=ili9341+2.4+spi+tft) | $8 | status/channel screen |
| *DIY:* [4×4 membrane keypad](https://www.amazon.com/s?k=4x4+membrane+keypad) | $3 | text entry + digital-lock PIN pad |
| *DIY:* [INMP441 I2S mic](https://www.amazon.com/s?k=inmp441+microphone+module) | $3 | voice in |
| *DIY:* [MAX98357A + 8Ω speaker](https://www.amazon.com/s?k=max98357a+i2s+amplifier) | $6 | voice out |
| *DIY:* tactile PTT switch (GPIO) | $1 | hold-to-talk |
| *DIY:* [u-blox NEO-M8/M10 GNSS](https://www.u-blox.com/en/product/neo-m8-series) | $12 | GPS → coarse position |
| *DIY:* [TP4056 + boost + 18650](https://www.amazon.com/s?k=tp4056+module+18650) | $7 | charge + power |
| [Tuned whip + u.fl pigtail](https://www.amazon.com/s?k=915mhz+antenna+ufl+sma+pigtail) | $10 | RF front end |
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
  voice codec that fits 1.75kbit/s LoRa). Wired voice already works over
  LAN/TCP hosts today (`send_voice` + `audio.ts` PTT).

## 06 · The Base Station — home node

| Part | ~Price |
|------|--------|
| Any ESP32-S3 + SX1262 combo (or [Heltec V3](https://heltec.org/project/wifi-lora-32-v3/)) | $20–25 |
| [Pi Zero 2 W](https://www.raspberrypi.com/products/raspberry-pi-zero-2-w/) or any always-on box you own | $15 |
| [5 dBi desktop antenna on a short feeder](https://www.amazon.com/s?k=915mhz+5dbi+antenna+sma) | $15 |
| Wall PSU | owned |

The set-and-forget house node: LAN bridge + RF in one `ucr-node` process.
Better antenna = the entire upgrade.

**Software:** `ucr-node --callsign HOME --class base --serial /dev/ttyUSB0`
on the Pi — the mux transport (`transport/mux.rs`) fans every packet
across serial + UDP + TCP at once, so it's a LAN↔RF bridge for free.

## 07 · The Vehicle Node

| Part | ~Price |
|------|--------|
| ESP32-S3 + SX1262 board | $20 |
| [Mag-mount whip (roof) + coax](https://www.amazon.com/s?k=magnet+mount+antenna+915mhz) | $15–25 |
| [12 V→5 V buck converter](https://www.amazon.com/s?k=12v+to+5v+buck+converter+usb) | $5 |
| [u-blox GPS module](https://www.u-blox.com/en/product/neo-m8-series) + toggle on the dash | $10–15 |
| Dash or trunk enclosure | $5 |

Car-powered, roof antenna, GPS-positioned: the best mobile node money
barely buys. Ignition-sense power so it sleeps with the car; a metal roof
under a mag-mount is a free ground plane — real range gains. The dash
toggle wires to the same `set_share_position` kill-switch — `/gps off`
in hardware form.

**Software:** `ucr-node --lat/--lon` + `--class vehicle`. Same GPS→pos
glue contribution as the field node.

## 08 · The Don — attic relay

| Part | ~Price |
|------|--------|
| Heltec V3 or any SX1262 board | $20 |
| [Pi Zero 2 W](https://www.raspberrypi.com/products/raspberry-pi-zero-2-w/) + SD + PSU | $25 |
| Band-matched antenna | $8 |

The cheap always-on relay. Full build: `godfather.md`.

**Software:** `ucr-node --class relay` — `StationClass::RelayNode` in the
beacon, relay + store-and-forward on (`routing/` handles the mesh work).
No screen, no mic, no input — it just forwards.

## 09 · The Godfather — mast relay

The community-grade always-on relay: collinear antenna, IP65 box, solar
option, RF↔LAN↔TCP gateway in one command. Full build + BOM:
`godfather.md` (~$120–150).

**Software:** `ucr-node --callsign X --class relay --serial … --listen
0.0.0.0:4402 --lat … --lon …` — the same `ucr-core` every product runs,
configured to be infrastructure.

## 10 · The Watchtower — listening post

| Part | ~Price |
|------|--------|
| Cheapest ESP32-S3 + SX1262 combo | $15–18 |
| Any antenna | $3 |

Flashes the **receive-only** build (default, no `tx-enable`): fully legal
everywhere, hears everything, reports real RSSI/SNR to the host. Use it
for coverage mapping, monitoring your own mesh, or a radio you can hand
anyone with zero regulatory exposure.

**Software:** `ucr-modem` unmodified — receive-only is the *default* TX
gate in `main.rs`; the host app reads RSSI/SNR from the serial trailer
(`framing.rs`). Zero code changes needed — it's the out-of-box behavior.

## 11 · The Go-Box — emergency kit

Emergency preparedness is the project's stated purpose; this is the
physical form of it.

| Part | ~Price |
|------|--------|
| [Pelican-style 1200 case](https://www.amazon.com/s?k=pelican+1200+case) (or ammo can) | $15–25 |
| ESP32-S3 + SX1262 board | $20 |
| [20000 mAh power bank](https://www.amazon.com/s?k=20000mah+power+bank+usb+c) + cable | $20–25 |
| Telescopic whip on panel mount + pigtail | $15 |
| Laminated quick-start card (channels, PTT, emergency) | $1 |
| Optional: foldable solar panel | $30+ |

Everything inside the case: open lid, power bank on, node up in seconds.
Runs receive-only out of the box so it can't make your day worse.

**Software:** `ucr-node` + the emergency path — `send_emergency` bypasses
the digital lock (the SOS button on a locked radio), floods `Regional`
scope, and every relay forwards it regardless of blocking (`node.rs`).

## 12 · The Dev Rig — contributor's bench

| Part | ~Price |
|------|--------|
| 2× ESP32-S3 + SX1262 sets | $25–40 |
| [USB power meter](https://www.amazon.com/s?k=usb+c+power+meter) | $8 |
| Spare antennas, jumper wires, headers | $10 |

Two radios on one desk lets you close the loop yourself: real frames, real
RSSI, real airtime — no second location needed. This is how firmware PRs
get made.

**Software:** two `ucr-node` instances + `cargo test -p ucr-core --test
real_udp` for the socket-level suite. When you're porting, the board
bring-up recipe in `devices.md` is the path.

## 13 · The Godfather Handheld — the best one

Not a relay — a *user radio*, flagship of the line. If the Ranger is the
handheld, this is the handheld you hand to the person who asked for the
best: every feature in the catalog, no compromise parts, built to be the
radio that survives being dropped, drenched, and borrowed.

| Part | ~Price | Why |
|------|--------|-----|
| [LilyGo T-Deck Plus](https://lilygo.cc/products/t-deck-plus) | $75 | ESP32-S3 + SX1262 + screen + keyboard + **u-blox GPS onboard** + speaker + mic + battery holder — the most integrated base that exists |
| *or full DIY stack:* ESP32-S3 + SX1262 + 3.2" ILI9341 + mech-feel keypad + INMP441 + MAX98357A + NEO-M10 | $90 | bigger screen, better audio chain, your pick of case |
| [Tuned SMA whip + proper u.fl→SMA pigtail](https://www.amazon.com/s?k=915mhz+sma+antenna+ufl+pigtail) | $15 | best RF front end a handheld gets — the tuned antenna is worth more range than any other $15 |
| [2× 18650 (or 21700) + battery sled](https://www.amazon.com/s?k=18650+battery+sled+holder) | $10 | all-weekend runtime, field-swappable cells |
| Machined or PETG-CF clamshell + gasket | $20–30 | the enclosure is what makes it *feel* like the best |
| [Second SX1262 module](https://www.amazon.com/s?k=sx1262+module+868+915) (dual-watch) | $12 | monitor `ucr-marine` while camping on `ucr.public` — real radios do this |
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

Full shopping lists with links, NMEA notes, the GPS kill-switch, and
marine rules: [`marine.md`](marine.md).

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
