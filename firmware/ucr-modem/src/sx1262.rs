//! Minimal blocking Semtech SX1262 driver over `embedded-hal` SPI.
//!
//! Covers the subset the UCR modem needs: LoRa init, continuous RX with
//! IRQ polling on DIO1, blocking TX with timeout, channel-activity
//! detection for listen-before-talk, and packet-status RSSI/SNR readback.
//!
//! SPI protocol (DS.SX1261-2.W.APP §4): NSS is held low for the entire
//! transaction — command byte, optional address/args, then NOP-clocked
//! status + data bytes. BUSY must be low before every transaction.

use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::SpiDevice;

// Register/command maps — kept complete; TX-path entries are only used
// in `tx-enable` builds.
#[allow(dead_code)]
mod cmd {
    pub const SET_STANDBY: u8 = 0x80;
    pub const SET_TX: u8 = 0x83;
    pub const SET_RX: u8 = 0x82;
    pub const SET_PACKET_TYPE: u8 = 0x8A;
    pub const SET_RF_FREQUENCY: u8 = 0x86;
    pub const SET_PA_CONFIG: u8 = 0x95;
    pub const SET_TX_PARAMS: u8 = 0x8E;
    pub const SET_MODULATION_PARAMS: u8 = 0x8B;
    pub const SET_PACKET_PARAMS: u8 = 0x8C;
    pub const SET_DIO_IRQ_PARAMS: u8 = 0x08;
    pub const SET_DIO2_RF_SWITCH: u8 = 0x9D;
    pub const SET_DIO3_TCXO: u8 = 0x97;
    pub const SET_BUFFER_BASE: u8 = 0x8F;
    pub const SET_CAD_PARAMS: u8 = 0x88;
    pub const SET_CAD: u8 = 0xC5;
    pub const SET_REGULATOR_MODE: u8 = 0x96;
    pub const WRITE_BUFFER: u8 = 0x0E;
    pub const READ_BUFFER: u8 = 0x1E;
    pub const GET_STATUS: u8 = 0xC0;
    pub const GET_RX_BUFFER_STATUS: u8 = 0x13;
    pub const GET_PACKET_STATUS: u8 = 0x14;
    pub const GET_IRQ_STATUS: u8 = 0x12;
    pub const CLEAR_IRQ_STATUS: u8 = 0x02;
    pub const CALIBRATE: u8 = 0x89;
    pub const GET_DEVICE_ERRORS: u8 = 0x17;
    pub const CLEAR_DEVICE_ERRORS: u8 = 0x07;
}

#[allow(dead_code)]
pub mod irq {
    pub const TX_DONE: u16 = 0x0001;
    pub const RX_DONE: u16 = 0x0002;
    pub const HEADER_ERR: u16 = 0x0020;
    pub const CRC_ERR: u16 = 0x0040;
    pub const CAD_DONE: u16 = 0x0080;
    pub const CAD_DETECTED: u16 = 0x0100;
    pub const TIMEOUT: u16 = 0x0200;
    pub const ALL: u16 = 0x03FF;
}

#[allow(dead_code)] // TX-path variants only constructed in tx-enable builds
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RadioError<E> {
    Spi(E),
    BusyTimeout,
    TxTimeout,
    CadTimeout,
    TooLong,
}

/// LoRa physical-layer configuration.
#[derive(Debug, Clone, Copy)]
pub struct LoRaConfig {
    /// Centre frequency in Hz.
    pub freq_hz: u32,
    /// Bandwidth: 0x04=125k, 0x05=250k, 0x06=500k.
    pub bw: u8,
    /// Spreading factor 5..=12.
    pub sf: u8,
    /// Coding rate: 1=4/5, 2=4/6, 3=4/7, 4=4/8.
    pub cr: u8,
    /// Preamble length in symbols.
    pub preamble: u16,
    /// TX output power in dBm (-9..=22 for SX1262).
    pub power_dbm: i8,
}

#[allow(dead_code)] // alternate regional profiles, selected per build
impl LoRaConfig {
    /// UCR AU ISM 915 profile (`profile_ids::AU_LORA`): 916 MHz, SF9/BW125,
    /// CR 4/5, +20 dBm. Must stay in sync with
    /// `ucr_protocol::profile::builtin_profiles()`.
    pub const UCR_AU: Self = Self {
        freq_hz: 916_000_000,
        bw: 0x04,
        sf: 9,
        cr: 1,
        preamble: 8,
        power_dbm: 20,
    };
    /// UCR EU ISM 868 profile.
    pub const UCR_EU: Self = Self {
        freq_hz: 868_100_000,
        bw: 0x04,
        sf: 9,
        cr: 1,
        preamble: 8,
        power_dbm: 14,
    };
    /// UCR US ISM 915 profile.
    pub const UCR_US: Self = Self {
        freq_hz: 903_900_000,
        bw: 0x04,
        sf: 9,
        cr: 1,
        preamble: 8,
        power_dbm: 22,
    };
}

/// RSSI/SNR measured for the most recent received packet.
#[derive(Debug, Clone, Copy)]
pub struct PacketStatus {
    /// RSSI of the packet, dBm.
    pub rssi_dbm: i16,
    /// SNR of the packet, dB (signed — can be negative below the noise floor).
    pub snr_db: i8,
}

pub struct Sx1262<SPI, BUSY, RESET, DIO1> {
    spi: SPI,
    busy: BUSY,
    reset: RESET,
    dio1: DIO1,
    /// True if the module uses a TCXO on DIO3 (most SX1262 modules do).
    has_tcxo: bool,
    /// True if the module wires the RF switch to DIO2 (most do).
    dio2_rf_switch: bool,
    /// Millisecond clock used for blocking timeouts.
    now_ms: fn() -> u64,
    /// Preamble from the init-time LoRa config, reused per-TX.
    preamble: u16,
}

impl<SPI, BUSY, RESET, DIO1, E> Sx1262<SPI, BUSY, RESET, DIO1>
where
    SPI: SpiDevice<Error = E>,
    BUSY: InputPin,
    RESET: OutputPin,
    DIO1: InputPin,
{
    pub fn new(spi: SPI, busy: BUSY, reset: RESET, dio1: DIO1, now_ms: fn() -> u64) -> Self {
        Self {
            spi,
            busy,
            reset,
            dio1,
            has_tcxo: true,
            dio2_rf_switch: true,
            now_ms,
            preamble: 8,
        }
    }

    /// Configure for modules without a TCXO and/or without a DIO2 RF switch.
    pub fn module_options(mut self, has_tcxo: bool, dio2_rf_switch: bool) -> Self {
        self.has_tcxo = has_tcxo;
        self.dio2_rf_switch = dio2_rf_switch;
        self
    }

    fn millis(&self) -> u64 {
        (self.now_ms)()
    }

    /// Block until BUSY goes low (radio ready for a new command).
    fn wait_busy(&mut self) -> Result<(), RadioError<E>> {
        let deadline = self.millis() + 100;
        while self.busy.is_high().unwrap_or(false) {
            if self.millis() > deadline {
                return Err(RadioError::BusyTimeout);
            }
        }
        Ok(())
    }

    /// Command with args only — single NSS-low transaction.
    fn cmd(&mut self, op: u8, args: &[u8]) -> Result<(), RadioError<E>> {
        self.wait_busy()?;
        let mut ops = [
            embedded_hal::spi::Operation::Write(&[op]),
            embedded_hal::spi::Operation::Write(args),
        ];
        self.spi
            .transaction(&mut ops[..1 + usize::from(!args.is_empty())])
            .map_err(RadioError::Spi)
    }

    /// Command + args, then a NOP-clocked chip-status byte (discarded) and
    /// `out.len()` data bytes — all inside one NSS-low transaction.
    fn cmd_read(&mut self, op: u8, args: &[u8], out: &mut [u8]) -> Result<(), RadioError<E>> {
        self.wait_busy()?;
        let mut status = [0u8];
        let opbuf = [op];
        let mut ops: heapless::Vec<embedded_hal::spi::Operation<'_, u8>, 4> = heapless::Vec::new();
        let _ = ops.push(embedded_hal::spi::Operation::Write(&opbuf));
        if !args.is_empty() {
            let _ = ops.push(embedded_hal::spi::Operation::Write(args));
        }
        if !out.is_empty() {
            let _ = ops.push(embedded_hal::spi::Operation::Transfer(&mut status, &[0x00]));
            let _ = ops.push(embedded_hal::spi::Operation::TransferInPlace(out));
        }
        self.spi.transaction(&mut ops).map_err(RadioError::Spi)
    }

    /// Read-only command returning status bytes directly (GetStatus) —
    /// no preceding status-NOP phase unlike the other Get* commands.
    fn raw_read(&mut self, op: u8, out: &mut [u8]) -> Result<(), RadioError<E>> {
        self.wait_busy()?;
        let mut ops = [
            embedded_hal::spi::Operation::Write(&[op]),
            embedded_hal::spi::Operation::TransferInPlace(out),
        ];
        self.spi.transaction(&mut ops).map_err(RadioError::Spi)
    }

    /// Hard reset then full radio configuration for `cfg`.
    pub fn init(&mut self, cfg: LoRaConfig) -> Result<(), RadioError<E>> {
        let _ = self.reset.set_low();
        delay_cycles(240_000); // ~1 ms at 240 MHz
        let _ = self.reset.set_high();
        delay_cycles(1_200_000); // ~5 ms
        self.wait_busy()?;

        // Standby on RC, then switch to the crystal/TCXO and DC-DC.
        self.cmd(cmd::SET_STANDBY, &[0x00])?;
        self.cmd(cmd::SET_REGULATOR_MODE, &[0x01])?; // DC-DC (ignored if absent)
        if self.has_tcxo {
            // DIO3 = TCXO supply, 1.8 V, ~5 ms startup delay.
            self.cmd(cmd::SET_DIO3_TCXO, &[0x02, 0x00, 0x00, 0x40])?;
        }
        self.cmd(cmd::SET_STANDBY, &[0x01])?; // STBY_XOSC
        if self.dio2_rf_switch {
            self.cmd(cmd::SET_DIO2_RF_SWITCH, &[0x01])?;
        }

        self.cmd(cmd::SET_PACKET_TYPE, &[0x01])?; // LoRa

        // Frequency register = freq_hz * 2^25 / 32 MHz.
        let reg = (cfg.freq_hz as u64 * (1 << 25) / 32_000_000) as u32;
        self.cmd(cmd::SET_RF_FREQUENCY, &reg.to_be_bytes())?;

        self.cmd(cmd::SET_BUFFER_BASE, &[0x00, 0x00])?;
        // LDRO is mandatory at SF11/SF12 with BW125; off otherwise.
        let ldro: u8 = if cfg.sf >= 11 { 1 } else { 0 };
        self.cmd(cmd::SET_MODULATION_PARAMS, &[cfg.sf, cfg.bw, cfg.cr, ldro])?;
        self.set_packet_params(cfg.preamble, 0xFF)?;
        self.preamble = cfg.preamble;

        // PA config for SX1262 (deviceSel=0x00). hpMax=0x07 supports +22 dBm.
        self.cmd(cmd::SET_PA_CONFIG, &[0x04, 0x07, 0x00, 0x01])?;
        self.cmd(
            cmd::SET_TX_PARAMS,
            &[
                cfg.power_dbm as u8,
                0x02, // 40 µs ramp
            ],
        )?;

        // All IRQ sources routed to DIO1.
        self.cmd(
            cmd::SET_DIO_IRQ_PARAMS,
            &[
                0x03, 0xFF, // global mask
                0x03, 0xFF, // DIO1 map
                0x00, 0x00, // DIO2 map
                0x00, 0x00, // DIO3 map
            ],
        )?;

        self.cmd(cmd::CALIBRATE, &[0x7F])?;
        self.cmd(cmd::CLEAR_IRQ_STATUS, &irq::ALL.to_be_bytes())?;
        self.cmd(cmd::CLEAR_DEVICE_ERRORS, &[0x00])?;
        Ok(())
    }

    fn set_packet_params(&mut self, preamble: u16, len: u8) -> Result<(), RadioError<E>> {
        self.cmd(
            cmd::SET_PACKET_PARAMS,
            &[
                (preamble >> 8) as u8,
                preamble as u8,
                0x00, // explicit header
                len,
                0x01, // CRC on
                0x00, // standard IQ
            ],
        )
    }

    /// Put the radio in continuous receive (0xFFFFFF timeout).
    pub fn rx_continuous(&mut self) -> Result<(), RadioError<E>> {
        self.cmd(cmd::SET_RX, &[0xFF, 0xFF, 0xFF])
    }

    /// Non-blocking poll: did DIO1 signal an event?
    pub fn irq_pending(&mut self) -> bool {
        self.dio1.is_high().unwrap_or(false)
    }

    pub fn irq_status(&mut self) -> Result<u16, RadioError<E>> {
        let mut b = [0u8; 2];
        self.cmd_read(cmd::GET_IRQ_STATUS, &[], &mut b)?;
        Ok(u16::from_be_bytes(b))
    }

    pub fn clear_irq(&mut self, mask: u16) -> Result<(), RadioError<E>> {
        self.cmd(cmd::CLEAR_IRQ_STATUS, &mask.to_be_bytes())
    }

    /// Read the received packet after RX_DONE. Returns (len, link status).
    pub fn read_packet(&mut self, buf: &mut [u8]) -> Result<(usize, PacketStatus), RadioError<E>> {
        let mut pos = [0u8; 2];
        self.cmd_read(cmd::GET_RX_BUFFER_STATUS, &[], &mut pos)?;
        let len = (pos[0] as usize).min(buf.len());

        let mut data = [0u8; 255];
        self.cmd_read(cmd::READ_BUFFER, &[pos[1]], &mut data[..len])?;
        buf[..len].copy_from_slice(&data[..len]);

        let mut st = [0u8; 3];
        self.cmd_read(cmd::GET_PACKET_STATUS, &[], &mut st)?;
        Ok((
            len,
            PacketStatus {
                rssi_dbm: -(st[0] as i16) / 2,
                snr_db: (st[1] as i8) / 4,
            },
        ))
    }

    /// Blocking transmit. `timeout_ms` bounds the radio-side timeout.
    #[allow(dead_code)] // used only in tx-enable builds
    pub fn transmit(&mut self, data: &[u8], timeout_ms: u32) -> Result<(), RadioError<E>> {
        if data.len() > 255 {
            return Err(RadioError::TooLong);
        }
        // WRITE_BUFFER at base offset 0x00.
        self.wait_busy()?;
        let mut ops = [
            embedded_hal::spi::Operation::Write(&[cmd::WRITE_BUFFER, 0x00]),
            embedded_hal::spi::Operation::Write(data),
        ];
        self.spi.transaction(&mut ops).map_err(RadioError::Spi)?;

        self.set_packet_params(self.preamble, data.len() as u8)?;
        self.clear_irq(irq::ALL)?;
        // Timeout in 15.625 µs units, 24-bit.
        let t = (timeout_ms * 15_625 / 1_000).min(0xFF_FFFF);
        self.cmd(
            cmd::SET_TX,
            &[
                ((t >> 16) & 0xFF) as u8,
                ((t >> 8) & 0xFF) as u8,
                (t & 0xFF) as u8,
            ],
        )?;

        let deadline = self.millis() + u64::from(timeout_ms) + 50;
        loop {
            let flags = self.irq_status()?;
            if flags & irq::TX_DONE != 0 {
                self.cmd(cmd::SET_STANDBY, &[0x01])?;
                return Ok(());
            }
            if flags & irq::TIMEOUT != 0 || self.millis() > deadline {
                self.cmd(cmd::SET_STANDBY, &[0x01])?;
                return Err(RadioError::TxTimeout);
            }
        }
    }

    /// Single channel-activity check for listen-before-talk.
    /// Returns Ok(true) when energy/a LoRa preamble was detected.
    #[allow(dead_code)] // used only in tx-enable builds
    pub fn channel_busy(&mut self, cad_symbols: u8) -> Result<bool, RadioError<E>> {
        self.cmd(
            cmd::SET_CAD_PARAMS,
            &[
                cad_symbols, // symbols to correlate (1..4)
                22,          // detPeak for SF9
                10,          // detMin
                0x00,        // exit mode: standby after CAD
                0x00,
                0x00,
                0x00, // timeout (unused in this mode)
            ],
        )?;
        self.clear_irq(irq::ALL)?;
        self.cmd(cmd::SET_CAD, &[])?;
        let deadline = self.millis() + 50;
        loop {
            let flags = self.irq_status()?;
            if flags & irq::CAD_DONE != 0 {
                return Ok(flags & irq::CAD_DETECTED != 0);
            }
            if self.millis() > deadline {
                return Err(RadioError::CadTimeout);
            }
        }
    }

    /// Chip mode from GetStatus (2=STBY_RC, 3=STBY_XOSC, 5=RX, 6=TX).
    pub fn chip_mode(&mut self) -> Result<u8, RadioError<E>> {
        let mut b = [0u8; 1];
        self.raw_read(cmd::GET_STATUS, &mut b)?;
        Ok((b[0] >> 4) & 0x07)
    }

    /// Device error flags (bit 8 = XOSC/TCXO start failed).
    #[allow(dead_code)] // diagnostics
    pub fn device_errors(&mut self) -> Result<u16, RadioError<E>> {
        let mut b = [0u8; 2];
        self.cmd_read(cmd::GET_DEVICE_ERRORS, &[], &mut b)?;
        Ok(u16::from_be_bytes(b))
    }
}

/// Crude cycle delay used only during one-shot reset timing.
fn delay_cycles(cycles: u32) {
    for _ in 0..cycles / 4 {
        core::hint::spin_loop();
    }
}
