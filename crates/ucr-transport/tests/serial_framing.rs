//! Cross-implementation test: the firmware's framing codec
//! (`firmware/ucr-modem/src/framing.rs`) must be byte-compatible with
//! `SerialTransport`'s wire format — same sync, same CRC, same trailer.

#[allow(dead_code)] // not every decoder method is exercised by these tests
#[path = "../../../firmware/ucr-modem/src/framing.rs"]
mod fw;

fn host_crc8(data: &[u8]) -> u8 {
    // CRC-8/SMBUS (poly 0x07) — duplicated from serial.rs, which keeps it
    // private. Any drift between the two breaks this test.
    let mut crc = 0u8;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn host_frame(payload: &[u8]) -> Vec<u8> {
    // Exact replication of SerialTransport::send's encoding.
    let mut pkt = Vec::with_capacity(payload.len() + 5);
    pkt.extend_from_slice(&[0x55, 0xAA]);
    pkt.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    pkt.extend_from_slice(payload);
    pkt.push(host_crc8(payload));
    pkt
}

#[test]
fn firmware_crc_matches_host() {
    let vectors: &[&[u8]] = &[b"", b"\x00", b"UCR", &[0xDE, 0xAD, 0xBE, 0xEF]];
    for v in vectors {
        assert_eq!(fw::crc8(v), host_crc8(v), "crc mismatch for {v:?}");
    }
}

#[test]
fn firmware_decodes_host_frame() {
    let payload = b"hello mesh";
    let frame = host_frame(payload);
    let mut dec = fw::Decoder::new();
    let mut got = None;
    for &b in &frame {
        got = dec.feed(b).or(got);
    }
    let len = got.expect("decoder did not emit a frame");
    assert_eq!(dec.payload(len), payload);
}

#[test]
fn firmware_decoder_resyncs_after_garbage() {
    let frame = host_frame(b"pkt");
    let mut stream = vec![0x00, 0xFF, 0x55, 0x13]; // garbage + false sync start
    stream.extend_from_slice(&frame);
    let mut dec = fw::Decoder::new();
    let mut got = None;
    for &b in &stream {
        got = dec.feed(b).or(got);
    }
    assert_eq!(dec.payload(got.unwrap()), b"pkt");
}

#[test]
fn firmware_encoder_matches_host_parse() {
    // Modem→host: encode with RSSI/SNR trailer, verify the host's
    // META_TRAILER layout: frame || rssi_i16_le || snr_i8 || 0xA5.
    let frame = b"radio-frame";
    let mut out = [0u8; 600];
    let _n = fw::encode(frame, Some((-73i16, 9i8)), &mut out);

    assert_eq!(&out[..2], &[0x55, 0xAA]);
    let len = u16::from_le_bytes([out[2], out[3]]) as usize;
    assert_eq!(len, frame.len() + 4);
    let body = &out[4..4 + len];
    assert_eq!(&body[..frame.len()], frame);
    let t = frame.len();
    assert_eq!(i16::from_le_bytes([body[t], body[t + 1]]), -73);
    assert_eq!(body[t + 2] as i8, 9);
    assert_eq!(body[t + 3], fw::META_VALID);
    assert_eq!(out[4 + len], host_crc8(body));
}

#[test]
fn host_to_firmware_plain_roundtrip() {
    // No metadata → payload is just the frame.
    let frame = b"meta-free";
    let mut out = [0u8; 600];
    let n = fw::encode(frame, None, &mut out);
    let len = u16::from_le_bytes([out[2], out[3]]) as usize;
    assert_eq!(&out[4..4 + len], frame);
    assert_eq!(out[4 + len], host_crc8(frame));
    let _ = n;
}
