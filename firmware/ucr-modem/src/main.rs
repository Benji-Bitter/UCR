//! UCR modem firmware — ESP32-S3 + Semtech SX1262.
//!
//! Bridges the UCR host (laptop/desktop running `ucr-transport`'s
//! `SerialTransport`) to the SX1262 LoRa radio:
//!
//! ```text
//! host ──USB serial──▶ [frame decoder] ──▶ [LBT/CAD] ──▶ SX1262 TX
//! host ◀──USB serial── [frame encoder + RSSI/SNR trailer] ◀── SX1262 RX
//! ```
//!
//! Wire framing is byte-identical to `crates/ucr-transport/src/serial.rs`:
//! `[0x55 0xAA] [u16 len LE] [payload] [crc8]`.
//!
//! ## Pin map (adjust `pins` below for your board)
//!
//! The defaults follow common ESP32-S3 SX1262 carrier boards:
//!
//! | SX1262 | ESP32-S3 |
//! |--------|----------|
//! | SCK    | GPIO9    |
//! | MOSI   | GPIO10   |
//! | MISO   | GPIO11   |
//! | NSS    | GPIO8    |
//! | RST    | GPIO12   |
//! | BUSY   | GPIO13   |
//! | DIO1   | GPIO14   |
//!
//! DIO2/3 RF-switch and TCXO are handled inside the radio by
//! `SetDio2AsRfSwitchCtrl` / `SetDio3AsTcxoCtrl` — see `module_options`.
//!
//! ## TX gating
//!
//! Region profiles in `ucr-protocol` ship receive-only pending regulatory
//! verification (`tx_allowed: false`). The firmware mirrors that: transmit
//! requires the `tx-enable` cargo feature. Without it, host TX frames are
//! dropped and logged — RX works unconditionally.

#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    spi::{master::Spi, Mode as SpiMode},
    time::Instant,
    usb::usb_serial_jtag::UsbSerialJtag,
};
use esp_println::println;

mod framing;
mod sx1262;

use sx1262::{irq, LoRaConfig, Sx1262};

/// Which regional UCR profile this build transmits on. Keep in sync with
/// `ucr_protocol::profile::builtin_profiles()`.
const LORA: LoRaConfig = LoRaConfig::UCR_AU;
/// SPI clock for the SX1262 — datasheet max 18 MHz, 8 MHz is conservative.
const SPI_HZ: u32 = 8_000_000;
/// CAD retries before a TX frame is dropped (host/router retries anyway).
#[cfg(feature = "tx-enable")]
const LBT_ATTEMPTS: u8 = 40;
/// Backoff between CAD retries, ms.
#[cfg(feature = "tx-enable")]
const LBT_BACKOFF_MS: u32 = 50;
/// Radio-side TX timeout, ms (SF9/BW125 max airtime ~1.1 s at 222 B).
#[cfg(feature = "tx-enable")]
const TX_TIMEOUT_MS: u32 = 2_000;

fn now_ms() -> u64 {
    Instant::now().duration_since_epoch().as_millis()
}

#[esp_hal::main]
fn main() -> ! {
    let p = esp_hal::init(esp_hal::Config::default());
    // ucr-protocol links `alloc`; give it a heap. The modem itself never
    // allocates — this only satisfies the shared crate's code paths.
    esp_alloc::heap_allocator!(size: 32 * 1024);
    let delay = Delay::new();

    // ---------- Host link: USB-SERIAL-JTAG (native USB port) ----------
    // Logs go to UART0 (see esp-println config), keeping this stream clean.
    let mut usb = UsbSerialJtag::new(p.USB_DEVICE);

    // ---------- Radio SPI + control pins ----------
    let spi = Spi::new(
        p.SPI2,
        esp_hal::spi::master::Config::default()
            .with_frequency(esp_hal::time::Rate::from_hz(SPI_HZ))
            .with_mode(SpiMode::_0),
    )
    .expect("spi")
    .with_sck(p.GPIO9)
    .with_mosi(p.GPIO10)
    .with_miso(p.GPIO11);

    let nss = Output::new(p.GPIO8, Level::High, OutputConfig::default());
    let reset = Output::new(p.GPIO12, Level::High, OutputConfig::default());
    let busy = Input::new(p.GPIO13, InputConfig::default().with_pull(Pull::Down));
    let dio1 = Input::new(p.GPIO14, InputConfig::default().with_pull(Pull::Down));

    let spi_dev = embedded_hal_bus::spi::ExclusiveDevice::new(spi, nss, delay).expect("spi device");

    let mut radio = Sx1262::new(spi_dev, busy, reset, dio1, now_ms)
        // Most SX1262 modules (Wio-SX1262, RFM95-style 1262 boards) have a
        // TCXO on DIO3 and the RF switch on DIO2. Set per your module.
        .module_options(true, true);

    println!("ucr-modem: init sx1262 @ {} Hz", LORA.freq_hz);
    match radio.init(LORA) {
        Ok(()) => println!("ucr-modem: radio ready, mode={:?}", radio.chip_mode()),
        Err(_) => println!("ucr-modem: radio init FAILED"),
    }
    let _ = radio.rx_continuous();

    #[cfg(feature = "tx-enable")]
    println!("ucr-modem: TX ENABLED (regulatory checklist acknowledged)");
    #[cfg(not(feature = "tx-enable"))]
    println!("ucr-modem: receive-only build (tx-enable feature off)");

    // ---------- Main bridge loop ----------
    let mut dec = framing::Decoder::new();
    let mut rx_buf = [0u8; 255];
    let mut tx_frame = [0u8; framing::MAX_FRAME + framing::META_TRAILER + 8];

    loop {
        // -- host → radio --------------------------------------------------
        while let Ok(b) = usb.read_byte() {
            if let Some(len) = dec.feed(b) {
                #[cfg(feature = "tx-enable")]
                {
                    tx_with_lbt(&mut radio, dec.payload(len), &delay);
                }
                #[cfg(not(feature = "tx-enable"))]
                {
                    let _ = len;
                    println!("ucr-modem: TX frame dropped (receive-only build)");
                }
                dec.reset();
            }
        }

        // -- radio → host --------------------------------------------------
        if radio.irq_pending() {
            match radio.irq_status() {
                Ok(flags) => {
                    let _ = radio.clear_irq(flags);
                    if flags & irq::RX_DONE != 0 {
                        match radio.read_packet(&mut rx_buf) {
                            Ok((len, st)) => {
                                let n = framing::encode(
                                    &rx_buf[..len],
                                    Some((st.rssi_dbm, st.snr_db)),
                                    &mut tx_frame,
                                );
                                write_all(&mut usb, &tx_frame[..n]);
                            }
                            Err(_) => println!("ucr-modem: rx read failed"),
                        }
                    }
                    if flags & (irq::CRC_ERR | irq::HEADER_ERR) != 0 {
                        println!("ucr-modem: rx pkt err flags={:04x}", flags);
                    }
                }
                Err(_) => println!("ucr-modem: irq read failed"),
            }
            // Ensure RX continues after any event.
            let _ = radio.rx_continuous();
        }
    }
}

/// Listen-before-talk via CAD, then transmit. Drops after LBT_ATTEMPTS.
#[cfg(feature = "tx-enable")]
fn tx_with_lbt<SPI, BUSY, RESET, DIO1, E>(
    radio: &mut Sx1262<SPI, BUSY, RESET, DIO1>,
    payload: &[u8],
    delay: &Delay,
) where
    SPI: embedded_hal::spi::SpiDevice<Error = E>,
    BUSY: embedded_hal::digital::InputPin,
    RESET: embedded_hal::digital::OutputPin,
    DIO1: embedded_hal::digital::InputPin,
{
    let mut free = false;
    for _ in 0..LBT_ATTEMPTS {
        match radio.channel_busy(2) {
            Ok(busy) if !busy => {
                free = true;
                break;
            }
            Ok(_) | Err(_) => delay.delay_millis(LBT_BACKOFF_MS),
        }
    }
    if !free {
        println!("ucr-modem: TX dropped, channel busy ({} B)", payload.len());
        let _ = radio.rx_continuous();
        return;
    }
    match radio.transmit(payload, TX_TIMEOUT_MS) {
        Ok(()) => println!("ucr-modem: tx {} B", payload.len()),
        Err(sx1262::RadioError::TxTimeout) => println!("ucr-modem: tx timeout"),
        Err(_) => println!("ucr-modem: tx failed"),
    }
    let _ = radio.rx_continuous();
}

fn write_all(usb: &mut UsbSerialJtag<'_, esp_hal::Blocking>, data: &[u8]) {
    let _ = usb.write(data); // 64-byte chunked blocking write
    let _ = usb.flush_tx();
}
