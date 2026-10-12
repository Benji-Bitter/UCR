# ucr-modem — reference UCR radio modem

ESP32-S3 + Semtech SX1262 firmware that bridges a UCR host to LoRa RF using
the serial protocol in `docs/protocol/serial.md`. It is deliberately a
*dumb modem*: all UCR protocol logic (routing, identity, sessions,
channels) stays on the host. The modem only frames/de-frames and moves
bytes between USB and RF, annotating received packets with measured
RSSI/SNR.

## Hardware

- ESP32-S3 board with native USB (any dev board; USB-JTAG pins are fixed).
- SX1262-based module (e.g. Wio-SX1262, generic SX1262 breakout).
- Default pin map is in `src/main.rs` — adjust to your carrier board.
- `module_options(tcxo, dio2_rf_switch)` in `main` must match your module;
  nearly all SX1262 modules are `(true, true)`.

## Toolchain

The ESP32-S3 is an Xtensa part — it needs the `esp` Rust toolchain:

```sh
cargo install espup
espup install                      # installs the esp toolchain channel
. ~/export-esp.sh                  # environment (add to shell profile)
cargo install espflash             # flashing tool
```

`rust-toolchain.toml` pins the `esp` channel; `.cargo/config.toml` sets the
`xtensa-esp32s3-none-elf` target and the `espflash` runner.

## Build and flash

```sh
cd firmware/ucr-modem
cargo build --release                  # receive-only build (default)
cargo espflash flash --release         # or just: cargo run --release
cargo run --release --features tx-enable   # transmit-enabled build
```

Then connect the host:

```rust
let transport = SerialTransport::open("/dev/cu.usbmodem*", 921600, 222)?;
```

(`ucr-node --serial <path>` once that flag lands; today any serial-port
terminal shows the framed stream.)

## TX gating — read before enabling

Region profiles in `ucr-protocol` ship `tx_allowed: false` pending
regulatory verification, and this firmware mirrors that: **TX only compiles
in behind `--features tx-enable`**. Enabling it asserts that you have:

- verified the frequency, power and duty-cycle rules for your jurisdiction
  (AU ISM 915–928 at +20 dBm by default — see `docs/hardware/regulatory.md`);
- confirmed your antenna/antenna gain keeps EIRP within limits;
- licensed/authorised yourself where required.

Without the feature the modem is a monitor: it receives, measures RSSI/SNR,
and streams frames upstream — host TX requests are dropped and logged.

## Architecture

```text
main.rs      bridge loop + pin map + TX gating + LBT policy
framing.rs   serial wire codec — byte-identical to host serial.rs
sx1262.rs    blocking SX1262 driver over embedded-hal SPI
```

The loop is single-threaded blocking: poll USB-JTAG RX, poll DIO1, service
whichever fired. There is no RTOS — a modem this simple doesn't need one.
For a production device the same driver works under Embassy by splitting
the loop into IRQ-driven tasks.

TX path runs listen-before-talk first: `SetCad` on 2 symbols, up to 40
retries at 50 ms backoff (~2 s) before dropping — the host router's
store-and-forward retransmits drops anyway, so the modem never blocks the
host on a busy channel.
