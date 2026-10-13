# Marine builds — UCR on the water

A boat is where UCR earns its keep: past the headland there is no cell
tower, marina Wi-Fi dies at the breakwater, and a handheld VHF can't send
text, can't encrypt, and can't mesh. A UCR boat rig keeps crew, tenders,
and rafted flotillas talking — text, positions, emergencies — over its own
mesh, and every other boat with a UCR node extends the net for free.

**Honest scope note, once, up front:** UCR is *not* a marine VHF and never
replaces one. VHF Ch.16 is the law of the sea — distress calls, coast
guard, bridge-to-bridge. UCR is your *crew* radio: the layer VHF never
gave you (texts, DMs, position sharing, store-and-forward when the other
boat is over the horizon, mesh through the harbor). Carry both.

## Why boats are the best UCR platform

- **Height is free.** A mast puts the antenna 10–20m up — the single
  biggest link-budget win that exists. A masthead UCR easily talks
  boat-to-boat over a whole anchorage and reaches shore relays.
- **Power is free.** House battery bank + alternator + solar = an
  always-on node that never thinks about sleep.
- **The sea is flat.** Nothing beats open water for RF propagation —
  expect the top end of every range estimate.
- **Boats anchor together.** A rafted flotilla is a natural mesh; an
  emergency at anchor hits every hull in one flood.

## What marine adds over land builds

| concern | land | marine |
|---------|------|--------|
| enclosure | printed PETG | gasket box, IP65+ minimum, salt air eats everything |
| power | USB/18650 | 12V house bank → fused 5V buck |
| antenna | desk whip | rail/arch/mast mount, coax run below deck |
| GPS | handheld GNSS | boat already has it — NMEA 0183 out of the chartplotter |
| corrosion | — | dielectric grease on every connector, conformal coat inside |

---

## Tier 1 · The Deckhand — ~$45 — the crew handheld

A Pocket Node in a dry box. One per crew member — comms deck-to-deck,
boat-to-tender, "dinner's ready" to the anchorage.

| Part | ~Price | Why |
|------|--------|-----|
| Heltec LoRa 32 V3 (your ISM band) | $20 | board + radio + OLED + charger, zero soldering |
| Short SMA whip, tuned | $10 | range |
| u.fl→SMA pigtail | $3 | antenna feed |
| IP67 polycarbonate box (~120×70×30mm) | $8 | salt water is the enemy |
| Cable gland + wrist lanyard | $4 | sealing + don't drop it overboard |

**Build:** same flash as every build. Board lives in the gasket box; whip
exits through the gland. USB-C charge port gets a sealed cap — every hole
is a leak.

**Use it for:** anchor watch net (`ucr-marine` channel), tender runs,
hail-the-harbor without touching VHF. Float it with a foam insert and it
survives the dinghy drop.

## Tier 2 · The Skipper — ~$100 — the helm station

A permanent nav-station install: the boat's own UCR node, powered off the
house bank, feeding the rail antenna, meshing with every boat in the bay
while you sleep.

| Part | ~Price | Why |
|------|--------|-----|
| ESP32-S3-DevKit + Wio-SX1262 (or Heltec V3) | $25 | the modem |
| Pi Zero 2 W (+ SD) | $20 | always-on host — `ucr-node` runs the mesh |
| Fused 12V→5V buck converter | $8 | clean power off the house bank |
| IP65 junction box, nav-station size | $15 | behind the panel |
| Rail/arch antenna bracket + tuned whip | $15 | stern rail or solar arch mount |
| RG-58 run, glands, dielectric grease | $15 | below-deck feed |
| USB-C OTG cable (Pi↔modem) | $5 | wired link |

**Build:** modem + Pi in the junction box at the nav station, antenna on
the stern rail or arch. `ucr-node --callsign VESSEL-NAME --class base` on
the Pi, serial to the modem — it boots with the boat and meshes the
anchorage as infrastructure, quietly relaying everyone's traffic.

**Optional NMEA feed:** your chartplotter already outputs position on
NMEA 0183 ($5 USB-RS422 dongle). The GPS→`ucr-node --lat/--lon` glue is
a friendly first contribution — today you set the coarse position once
per passage.

## Tier 3 · The Admiral — ~$200 — the flagship vessel install

A floating Godfather: mastheight antenna, house-bank power, always on,
the harbor's relay. Every boat that joins the mesh gets routed through
you; you hear and forward everything, like a lighthouse that reads
envelopes.

| Part | ~Price | Why |
|------|--------|-----|
| ESP32-S3 + SX1262 pair | $25 | modem |
| Pi 4 (or a Pi you own) | $55 | host + future AIS/NMEA gateway |
| 5–8dBi collinear, marine mount | $45 | arch or mizzen-mast top — height × gain |
| LMR-200 coax (10m) + glands | $25 | low-loss mast run |
| IP65 enclosure + sealed panel | $20 | the electronics bay |
| Fused buck + inline fuse + master switch | $12 | proper marine DC |
| NMEA 0183 USB bridge (RS422↔USB) | $8 | plotter position → coarse pos |
| Dielectric grease, conformal coat, tinned wire | $15 | it lives in salt air |

**Build:** same Don/Godfather config, vessel-grade materials.
`ucr-node --callsign MV-ADMIRAL --class relay` on the Pi — the boat is
permanently infrastructure. At anchor in a busy bay you are the reason
everyone's messages route.

**Stretch goals (documented contributions, not shipped):**
- **AIS-adjacent** — read the boat's own AIS/NMEA stream, announce your
  vessel position into the mesh (coarse only — privacy at anchor is real).
- **Dual-node rig** — second modem on a second profile for channel
  diversity; the mux already fans packets across both.
- **Shore link** — a Godfather on a coastal hill + the Admiral bridges
  harbor traffic onto the land mesh.

## Marine rules

1. **VHF still rules.** UCR supplements Ch.16, never substitutes.
2. **Coarse position only.** A boat's anchorage is its bedroom — the
   protocol announces city-level position by design; never wire a precise
   fix into beacons.
3. **Antenna before transmit, always.** Salt-air connectors corrode —
   check SWR/contact before blaming the code.
4. **Your ISM band, your rules.** 915MHz in AU/US, 868MHz in EU — buy the
   module for your waters, and remember vessels in EU waters still obey
   EU duty-cycle law.
5. **Every boat a relay.** `--class relay` on anything house-powered —
   the anchorage mesh is only as good as its powered-on nodes.

*Everything in this file is buildable with today's firmware — tethered
modem + host is the shipped architecture. The NMEA position glue and the
on-device port are tracked contributions.*
