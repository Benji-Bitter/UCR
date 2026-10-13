# Handheld builds — three tiers

A UCR node you can carry. Every tier runs the same `ucr-modem` firmware
and speaks the same serial protocol; the difference is how self-contained
the thing in your hand is.

**The honest architecture first.** Today the board is a *radio modem* —
it frames UCR packets and moves them between air and serial. The brain
(identity, crypto, channels, UI) is `ucr-core` on a host: laptop over
USB, a Raspberry Pi in your bag, eventually the ESP32 itself once the
embedded node port lands. So tier 1 and 2 are *tethered* radios; tier 3
is where the compute moves into your hand.

| | basic | mediocre | advanced |
|---|-------|----------|----------|
| cost | ~$20 | ~$50 | ~$80–120 |
| soldering | none | a little | real |
| self-contained | tethered to host | tethered, own power+GPS | standalone radio |
| good for | first contact | field day, go-bag | the real thing |

---

## Tier 1 — Basic: "the leashed node" (~$20)

One board does almost everything; you add power and antenna.

| Part | Price | Notes |
|------|-------|-------|
| Heltec WiFi LoRa 32 V3 | $18–25 | ESP32-S3 + SX1262 + 0.96" OLED + battery charger + JST lead, all onboard. **The default pin map in `main.rs` already matches V3's LoRa wiring** — flash it and it works. |
| Band-matched antenna | $3–8 | SMA whip for *your* ISM band (868 or 915 MHz — not interchangeable). Never transmit without it: an open feedpoint cooks the PA. |
| 18650 cell | $5–8 | V3 has a battery connector + charge circuit. Any quality cell works; don't buy counterfeits for something carried on-body. |
| USB-C cable | owned | to the laptop running the app. |

That's it — four line items, zero soldering. Flash per
`first-node.md`, plug into the host, and you're a walking node.

**What you get:** RX ~15–25 h per charge, real RSSI/SNR on incoming
packets, the OLED free for whoever writes a display task (firmware
contribution opportunity). Range like any SF9 LoRa link: ~1–3 km urban,
10 km+ line-of-sight.

## Tier 2 — Mediocre: "self-locating field node" (~$50)

For go-bags and field days: own power, own position fix, weather-proofed.

| Part | Price | Notes |
|------|-------|-------|
| LilyGo T-Beam Supreme (S3) | $30–38 | ESP32-S3 + SX1262 + u-blox GPS + AXP2101 power management + 18650 holder onboard. |
| 18650 cell | $5–8 | slotted into the board's holder. |
| Tuned SMA whip | $8–12 | a proper 915/868 antenna is the single best range upgrade — a $3 stub throws away half your link budget. |
| Enclosure | $5–10 | Hammond 1551-size or 3D-printed; the T-Beam has case files on the LilyGo repo. |

**Pin map:** T-Beam wiring differs from V3 — adjust the GPIO block at the
top of `main()` (it's seven lines, all in one place) and the AXP2101
needs an I2C init for battery rails. Both are known-good patterns; search
existing issues before reinventing.

**GPS → coarse position, honestly:** the T-Beam's GNSS can feed the coarse
`lat/lon` your node announces (city-level on the globe, never exact).
Wiring NMEA parsing into the modem firmware is a real known gap — today
you set the position manually in the app, and wiring `gps → NodeConfig.pos`
is one of the friendliest first contributions. Whatever feeds it, the
announce carries a kill-switch: `Node::set_share_position(false)` keeps
the fix local and stops it reaching beacons — `/gps off` on the CLI,
`[ gps off ]` in the app, a latching toggle on the handheld.

## Tier 3 — Advanced: "the standalone radio" (~$80–120)

Compute moves into the handset: screen, keyboard, mic, speaker — a real
radio, not a peripheral.

**Option A — the shortcut:** LilyGo T-Deck (~$50–60 + battery). ESP32-S3,
SX1262, 2.8" IPS touchscreen, keyboard, trackball, speaker *and* mic,
battery holder. The closest thing to a shrink-wrapped UCR handheld that
exists — but note honestly: it still needs the **embedded node port**
(`ucr-core` on ESP32) to run standalone. Today it can run the modem
firmware and tether to a phone over USB-C.

**Option B — the build:**

| Part | Price | Role |
|------|-------|------|
| ESP32-S3 dev board (PTH or devkit) | $10–15 | host CPU |
| Wio-SX1262 / SX1262 breakout | $10–15 | radio |
| INMP441 (I2S mic module) | $3 | voice capture |
| MAX98357A + 8Ω speaker | $6 | voice playback |
| Tactile switch on a GPIO | $1 | real PTT button — hold-to-talk hardware |
| AXP2101 or TP4056 + boost | $5 | charge + power management |
| 18650 + holder | $6 | power |
| GNSS module (u-blox NEO or the T-Beam trick) | $10–15 | position |
| Tuned SMA antenna + u.fl pigtail | $10 | the RF front end |
| Hammond box or printed shell | $8–15 | enclosure |
| misc: perfboard, headers, strap | $5 | |

**Why this tier matters for UCR specifically:** the I2S mic + speaker
pair is exactly where Codec2 voice lands — the ESP32-S3 has the DSP
headroom to run the 700–3200 bit/s vocoder on-device (FreeDV proved it on
this silicon), which is the only way voice works over a 1.75 kbit/s LoRa
link. The PTT button maps straight onto the same `ChannelVoice` flood the
desktop app sends today.

## Rules that apply at every tier

- **Antenna before power.** Transmitting into nothing reflects energy into
  the PA. Match the band; a wrong-band whip is nearly as bad as none.
- **Legality is yours.** `tx-enable` exists precisely because bands, power,
  and duty cycle are jurisdiction-specific. The checklist lives in
  `regulatory.md` — read it before enabling TX, and a receive-only build
  is a real, useful node.
- **Coarse positions only.** Announce the city-level `lat/lon` — the
  protocol and UI are built to never leak precise location. Keep it that
  way even though the GPS knows better.

## What's genuinely left to build (real contribution paths)

- `ucr-core` on ESP32 (`no_std`/embassy port) — turns tier 3 into a
  phone-free standalone node. The big one.
- Codec2 encode/decode on-device — voice on the actual RF link.
- NMEA → `NodeConfig.pos` glue in the modem — auto-locating nodes.
- OLED/e-ink status task — channel activity, RSSI, battery on the V3's
  screen.
- Board pin maps as data, not code — a `boards/` table so Heltec/T-Beam/
  T-Deck flash with one flag instead of editing `main.rs`.

These are all open doors, not blockers — pick one and the handheld tiers
get better for everyone.
