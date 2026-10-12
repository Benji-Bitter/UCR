# UCR host↔modem serial protocol

Byte-level contract between a UCR host (`ucr-transport::SerialTransport`) and
a UCR radio modem (`firmware/ucr-modem`). Both directions use identical
framing; only the payload differs.

## Frame layout

```text
+--------+--------+---------+---------+===================+--------+
| 0x55   | 0xAA   | len lo  | len hi  | payload (len B)   | crc8   |
+--------+--------+---------+---------+===================+--------+
```

- **Sync** — `0x55 0xAA`, two bytes. Receivers resynchronise by scanning for
  this pair.
- **len** — u16 little-endian payload length, `1..=512+4`.
- **payload** — see direction sections below.
- **crc8** — CRC-8/SMBUS (poly `0x07`, init `0x00`, no reflection) over the
  payload bytes only.

A frame failing sync, length, or CRC is dropped and the receiver resyncs at
the next byte. There is no escaping and no length-extension mechanism.

## Host → modem: transmit request

`payload` = one raw UCR wire-format packet (`docs/protocol/wire-format.md`)
to transmit verbatim. The host enforces the regional profile MTU (222 B for
LoRa profiles); the modem additionally rejects payloads > 255 B.

Receive-only modem builds (the default — see `docs/hardware/regulatory.md`)
drop transmit requests and log the drop.

## Modem → host: received packet

`payload` = `frame || rssi_i16_le || snr_i8 || 0xA5`

- `frame` — the raw UCR wire-format packet as received over the air.
- `rssi_i16_le` — packet RSSI in dBm.
- `snr_i8` — packet SNR in dB (may be negative).
- `0xA5` — metadata-valid marker. The host only trusts the trailer when the
  marker is present and RSSI is in `(-200, 0)`.

Packets delivered without a trailer (e.g. a non-LoRa modem without RF
measurement) report `RxMeta::unmeasured`; the host never fabricates values.

## Transport parameters

- Serial: USB-SERIAL-JTAG native USB on the reference modem; any UART works.
  Baud is host-configured (`SerialTransport::open(path, baud, mtu)`).
- Flow control: none. The modem is always faster to drain than the link can
  fill (915 MHz LoRa airtime dominates everything).
- The framed stream must not share the port with log output — the reference
  firmware logs to UART0 and bridges on the USB-JTAG port for this reason.

## Implementations

- Host: `crates/ucr-transport/src/serial.rs`
- Modem: `firmware/ucr-modem/src/framing.rs`
