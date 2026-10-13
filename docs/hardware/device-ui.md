# On-device UI — the screen a UCR radio deserves

The design language of the desktop app, shrunk onto real displays: pure
monochrome, monospace grid, square edges, `[ bracketed ]` actions, dim
labels, inverted selection. A 1-bit OLED forces the discipline the
aesthetic already chose — this spec makes the smallest screen feel like
the same product, not a demoted version.

Two display classes, one language:

| display | where | grid (6×8px font) | feel |
|---------|-------|--------------------|------|
| 128×64 OLED (0.96") | pocket nodes, V3/V4, go-box | 21 cols × 8 rows | pager — dense, glanceable |
| 320×240 TFT (2.4–3.2") | ranger, godfather handheld, T-Deck | 40 cols × 30 rows | radio — full views |

Everything below is drawn at real proportions. The 8-row OLED layouts are
not miniatures of the TFT — they're redesigned for the space.

## The one rule

**The status bar is sacred.** Every screen keeps row 0:

```
NODE-9F2A  #general      ●92%
live lora1750 gps lock
```

callsign left · channel · battery right; sub-row: mode, link profile,
the `[ gps ]`/`[ lock ]` states exactly as the app header shows them.
A user learns one strip of chrome and it follows them everywhere.

## The boot screen — first thing the display ever says

Every UCR unit wakes the same way: the mark, the name, the self-test.
Drawn at OLED proportions — the TFT version keeps the same layout,
breathed wider:

```
┌─────────────────────┐
│                     │
│  ##  ## ###### #####│
│  ##  ## ##     ## ##│
│  ##  ## ##     #####│
│  ##  ## ##     ## ##│
│  ###### ###### ## ##│
│  universal citizens │
│  radio · boot v0.1  │
└─────────────────────┘
```

then the POST crawl replaces the center — same lines the host console
prints (`identity … ok`, `radio sx1262 … ok`, `host link … up`), and a
final beat on `ready`. A stuck line *is* the diagnostic: `radio init
FAILED` freezes the splash so a dead wire announces itself on the screen,
not just a log. The splash is a screen like any other in the state
machine — first state on the ring, exits to `idle` on done/timeout/press.

## Navigation — three hardware classes

| input | devices | model |
|-------|---------|-------|
| 1 button (+PRG) | V3/V4 OLED | short-press cycles views on a ring; long-press = select/enter |
| keypad (4×4) | ranger, godfather handheld | arrows/jog + OK + back + digits; hardware keys: PTT, [gps], [lock] |
| trackball + keys | T-Deck | cursor + scroll; same views, richer hit areas |

The view ring (short-press order) is fixed everywhere so muscle memory
transfers: **idle → channel → inbox → peers → menu**. Modal screens
(lock PIN, emergency arm) sit off the ring — hardware keys summon them.

---

## OLED layouts — 21×8

### idle (the face of the radio)

```
┌─────────────────────┐
│RANGER-3   #general 92%│
│live  lora  gps·lock  │
│─────────────────────│
│                     │
│     standing by     │
│                     │
│ peers 5   air 0.9%  │
│ [menu]  ptt to talk │
└─────────────────────┘
```

### channel (message feed)

```
│#ucr-marine      ●live│
│SKIPPER-7 ·2hop ·sig✓│
│ anchored off the pt │
│GODFTHR ·1hop ·sig✓  │
│ relaying you shore  │
│◉ DECKHAND on the air│
│                     │
│ [tx] [type] [back]  │
```

### PIN lock (the digital lock, on-device)

```
│   digital lock      │
│                     │
│   pin: ■ ■ ■ _      │
│                     │
│  enter to unlock    │
│  tx gated · rx open │
│                     │
│  [clear]  [cancel]  │
```

### PTT transmit (hold = on air)

```
│████████████ tx █████│
│  ◉ ON AIR           │
│  codec2 1400bps     │
│  #ucr-marine        │
│  00:03   f#7        │
│  release to end     │
│                     │
│█████████████████████│
```

Inverted = you are the thing on the air. Unmissable at arm's length.

### emergency arm (off the ring, deliberate)

```
│  ⚠ EMERGENCY        │
│                     │
│  hold PTT 3s        │
│  to broadcast       │
│                     │
│  medical  other     │
│  (bypasses lock)    │
│                     │
```

Two-step on purpose — the SOS bypasses the PIN lock, so it must be
unmistakable and unfumbleable.

### peers

```
│peers · heard 14s    │
│SKIPPER-7  boat -87  │
│GODFTHR    relay -71 │
│ALPHA      ind  NaN·ip│
│DECKHAND   port -92  │
│                     │
│ [dm] [info] [back]  │
```

---

## TFT layouts — the full views

The 40×30 grid gets the app's real views, not stretched OLED ones:

```
┌──────────────────────────────────────────┐
│GODFTHR-HH  #ucr-marine        live   ●97%│
│lora 1750bps mtu222  gps on   unlocked    │
├──────────┬───────────────────────────────┤
│CHANNELS  │ # ucr-marine        5 peers   │
│ucr.public│                               │
│ucr-marine│ SKIPPER-7 ·2hop ·sig✓ ·14:02  │
│uhf-local │  anchored off the point       │
│+ join    │                               │
│          │ GODFTHR ·1hop ·sig✓ ·14:03    │
│──────────│  relaying you to shore net    │
│INBOX (2) │                               │
│          │ ◉ DECKHAND on the air — c2    │
│SETTINGS  │                               │
│          ├───────────────────────────────┤
│          │ [ hold to talk ]   [ type ]   │
│          │ txt…  _                       │
├──────────┴───────────────────────────────┤
│ [lock] [gps] [emerg] [morse] [peers]    │
└──────────────────────────────────────────┘
```

Sidebar ring mirrors the app's left rail — channels, inbox, settings —
same information hierarchy, same monospace honesty.

### menu (keypad-driven)

```
│ settings              ▲               │
│▸callsign   GODFTHR-HH                 │
│ channel    #ucr-marine                │
│ gps        announce on   [ toggle ]   │
│ lock       unlocked      [ set pin ]  │
│ beacon     auto · ~37s   [ change ]   │
│ link       lora 1750 mtu 222          │
│ brightness ████████░░                 │
│ about      ucr-modem 0.1 · f3a9       │
│                                       │
│ ok:edit  back:exit                    │
```

### morse keyer

```
│ morse · 15wpm              key: PTT   │
│ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─  │
│  ..- -.-. .-.   =  UCR               │
│  _ _ _                                │
│ send → ALPHA   [tx] [clear] [back]   │
```

---

## Interaction details that make it feel finished

- **Inverted = active.** The screen inverts exactly where something is
  *happening*: PTT frame, the selected row, an on-air peer's name. On a
  1-bit OLED that reads instantly at arm's length — no color crutch.
- **`◉` is voice, always.** On-air peer, your own TX, the talker ID —
  one glyph means "someone is speaking," consistent with the app.
- **Signature truth inline.** `sig✓`/`sig✗` on every message — the wire's
  crypto status is a first-class citizen, not a settings page.
- **Dim is a vocabulary.** Second-class info (hops, ages, ids) sits in
  dim — never bold the background noise.
- **Lock screen is a door, not a wall.** It says what still works:
  `tx gated · rx open` — like the real keypad lock on a handheld.
- **Emergency is off the ring.** You can't stumble into it cycling
  views; it's a two-step deliberate act that outranks the PIN.
- **No dead ends.** Every screen's bottom row is its way out — OLED says
  `[back]`, TFT maps hardware keys. You never hunt for exit.
- **The glyph set is tiny on purpose:** `◉` on-air, `●` link/running,
  `⚠` emergency, `✓`/`✗` sig state, `■` PIN digit, `_` cursor,
  `▸` menu cursor, `─│┌┐└┘` frames. Anything else is noise.

## Implementation — how this lands on the chips

| piece | crate / approach | notes |
|-------|------------------|-------|
| framebuffer | `embedded-graphics` + a 1-bit `BinaryColor` canvas | OLED is natively 1-bit; TFT renders the same mono theme |
| font | embedded bitmap font (6×8 / 8×12) | the cell grid above is the layout unit — everything snaps to it |
| layout | text-grid composer, not pixel widgets | status bar + body + action row; screens declare rows |
| input | GPIO poll task → key event queue | button ring / keypad matrix (PCF8574) / trackball |
| state | the embedded `ucr-core` port feeds views | the UI reads `Node` state — same calls as desktop/CLI (`lock`, `unlock`, `set_share_position`, `send_*`) |
| where | `firmware/ucr-modem` UI task (or the node port) | display task is a listed contribution — this doc is its spec |

The dependency that matters: this UI wants **the `ucr-core`-on-ESP32
port** — the same unlock the Ranger needs to stop tethering. Until then,
the OLED/TFT can still render host-fed views over the serial contract
(the host streams `NodeEvent`s; the display task draws them).

*This is the spec the handheld firmware builds against. When the display
task ships, it should match these screens pixel-for-grid — the language
is settled; only the code is open.*
