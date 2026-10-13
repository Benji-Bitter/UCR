# The Godfather Node

You don't talk to it. You talk *through* it.

A Godfather is a dedicated always-on relay: no mic, no screen, no user,
no personality — just a station bolted somewhere high that hears every
packet in reach and sends each one faithfully onward. It's the
infrastructure that turns a handful of handhelds into an actual mesh.

**What makes it different from a normal node:** nothing in software —
and that's the point. A UCR node relays by nature. The Godfather is the
same `ucr-node` binary you'd run on a laptop, but configured as
`--class relay`, powered forever, positioned for coverage, and left
alone. Store-and-forward means it holds queued traffic for up to two
hours, so messages that hit it at 3am still complete their journey when
the destination comes back in range.

## Two builds

| | The Don | The Full Godfather |
|---|---------|--------------------|
| role | attic/bedroom relay | mast-mounted community relay |
| radio | any SX1262 board (Heltec V3 is fine) | SX1262 in an IP65 box, or a 1W-PA module like E22-900M30S |
| antenna | indoor whip on a windowsill | 5–8 dBi outdoor collinear on a mast |
| host | Pi Zero 2 W | Pi Zero 2 W or Pi 3/4 |
| cost | ~$45 | ~$120–150 |
| range out | ~3–8 km | ~15–30 km line-of-sight |

## The Don (~$45)

| Part | Price |
|------|-------|
| Heltec LoRa 32 V3 (or any SX1262 board) | $20 |
| Raspberry Pi Zero 2 W + SD + PSU | $25 |
| Band-matched antenna | $8 |

The Pi runs `ucr-node` headless; the board plugs into it over USB and
just relays. Stick the whole thing high — top shelf, attic, skylight.
Height is worth more than power at every price point.

## The Full Godfather (~$120–150)

| Part | Price | Notes |
|------|-------|-------|
| ESP32-S3 + SX1262 in a weatherproof box | $25 | or E22-900M30S (1 W PA) — see legal notes |
| Pi Zero 2 W / Pi 3 | $15–30 | the brain — runs the node, bridges LAN↔RF |
| 5–8 dBi collinear fiberglass antenna | $25–40 | mounted as high as legal |
| LMR-200 feedline, kept short | $10 | coax loss eats antenna gain — 3–5 m max at 915 MHz |
| IP65 enclosure + glands | $15 | everything RF-external lives inside |
| Mains 5 V PSU (or 20 W solar + LiFePO4) | $15–30 | hilltop sites run on solar fine — a relay sips power |
| Mast/pole, mounting strap | $10–20 | chimney straps work |

## Bring it up

```bash
# on the pi — modem on usb serial, lan bridge, tcp gateway for remote
# links, declared a RelayNode, announcing its real coarse position
ucr-node --callsign GODFTHR \
  --class relay \
  --desc "always-on relay - messages find a way through" \
  --serial /dev/ttyUSB0 \
  --listen 0.0.0.0:4402 \
  --lat -33.87 --lon 151.21 \
  --data-dir /var/lib/ucr
```

- `--class relay` — beacons announce it as a `RelayNode`, not a person.
- `--serial` + the defaults — UDP LAN and the serial modem join the same
  mux, so **RF traffic lands on the LAN and LAN traffic hits the air**.
  That's the whole trick: one command and the Godfather is a real
  RF↔IP gateway, no glue code.
- `--lat/--lon` — announce its *real* coarse position. A relay's location
  is a public fact (it's infrastructure, not a person), and known
  positions help the mesh reason about paths.
- `--data-dir` — persistent identity. The same callsign+keypair survives
  reboots, which is how peers learn to trust it.
- `relay` and `emergency_relay` default on — it forwards everything in
  scope without being asked, including emergency traffic.

## Honest notes

- **Height beats watts.** Moving a node from desk to mast gains more
  link budget than any PA. A $30 antenna on a chimney outperforms a 1 W
  module on a shelf, and it's the legal-friendly upgrade.
- **Duty cycle is real.** A popular relay can brush against EU-style 1%
  airtime limits — the node's airtime pacing paces *its own* beacons, but
  relayed traffic doesn't shrink. Busy communities should plan a relay
  profile, not a bigger amplifier.
- **Fixed-station rules differ.** In some jurisdictions a permanently
  installed station has different limits than a handheld. The
  `tx-enable` checklist (`regulatory.md`) applies doubly here — a
  receive-only Godfather is still a useful node (it monitors, and the
  app shows what it hears).
- **1 W PA modules** (E22-900M30S and friends) fit the same UART-style
  module footprint but need their own driver path — see `devices.md`
  under radio swaps. Legal in fewer places than you think.
- **It keeps secrets it can't read.** DMs relay through it fully
  encrypted — the Godfather forwards ciphertext it cannot decrypt. Being
  trusted infrastructure doesn't mean seeing anyone's traffic.
