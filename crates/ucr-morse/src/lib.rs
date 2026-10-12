//! # UCR Morse
//!
//! International Morse code utilities on top of the `ucr-protocol` alphabet:
//!
//! * **Text ↔ elements** — encode/decode per ITU-R M.1677-1;
//! * **Timing** — WPM → element schedule (PARIS standard: dit = 1 unit,
//!   dah = 3, intra-char gap = 1, char gap = 3, word gap = 7);
//! * **Playback schedule** — a list of `(tone_on, duration_ms)` segments a UI
//!   or buzzer can render directly;
//! * **Key input** — feed manual dit/dah durations, get decoded text
//!   (adaptive threshold from observed timings);
//! * **Trainer** — random drills and accuracy scoring.
//!
//! Digital-Morse packets (`PlainApp::Morse`) carry plain text + WPM; the
//! receiver renders audio locally. This is digital Morse over UCR — NOT
//! continuous-wave RF, and not decodable by conventional receivers.

use ucr_protocol::morse::{elements_to_text, text_to_elements, MorseSym};

/// Milliseconds per dit unit at `wpm` words per minute (PARIS = 50 units).
pub fn unit_ms(wpm: u8) -> f32 {
    1200.0 / (wpm.max(1) as f32)
}

/// One audio segment for playback.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    /// Emit the tone when true; silence when false.
    pub on: bool,
    pub dur_ms: f32,
}

/// Build a playback schedule for `text` at `wpm`.
///
/// Timing (PARIS): dit 1u, dah 3u, intra-character gap 1u, character gap 3u,
/// word gap 7u.
pub fn playback_schedule(text: &str, wpm: u8) -> Vec<Segment> {
    let unit = unit_ms(wpm);
    let raw = text_to_elements(text);
    let mut out: Vec<Segment> = Vec::new();
    for (i, e) in raw.iter().enumerate() {
        match e {
            MorseSym::Dit => push(
                &mut out,
                Segment {
                    on: true,
                    dur_ms: unit,
                },
            ),
            MorseSym::Dah => push(
                &mut out,
                Segment {
                    on: true,
                    dur_ms: 3.0 * unit,
                },
            ),
            MorseSym::CharGap => push(
                &mut out,
                Segment {
                    on: false,
                    dur_ms: 3.0 * unit,
                },
            ),
            MorseSym::WordGap => push(
                &mut out,
                Segment {
                    on: false,
                    dur_ms: 7.0 * unit,
                },
            ),
        }
        // Implicit 1-unit silence between tones within the same character.
        if matches!(e, MorseSym::Dit | MorseSym::Dah)
            && matches!(raw.get(i + 1), Some(MorseSym::Dit | MorseSym::Dah))
        {
            push(
                &mut out,
                Segment {
                    on: false,
                    dur_ms: unit,
                },
            );
        }
    }
    out
}

/// Push a segment, merging with the previous when contiguous same-type.
fn push(out: &mut Vec<Segment>, seg: Segment) {
    if let Some(last) = out.last_mut() {
        if last.on == seg.on {
            last.dur_ms += seg.dur_ms;
            return;
        }
    }
    out.push(seg);
}

/// Total playback duration in ms.
pub fn duration_ms(text: &str, wpm: u8) -> f32 {
    playback_schedule(text, wpm).iter().map(|s| s.dur_ms).sum()
}

/// Decoder for manual key input. Feed `(down_ms, up_ms)` durations in order;
/// the decoder adapts the dit/dah threshold to the operator's speed.
#[derive(Default)]
pub struct KeyDecoder {
    events: Vec<(bool, f32)>, // (down?, dur_ms)
    /// All text decoded so far.
    pub text: String,
    syms: Vec<MorseSym>,
}

impl KeyDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Estimated dit unit from observed tones (first quartile of tone
    /// durations approximates the operator's dit length).
    fn est_unit(&self) -> f32 {
        let mut durs: Vec<f32> = self
            .events
            .iter()
            .filter(|(down, _)| *down)
            .map(|(_, d)| *d)
            .collect();
        if durs.is_empty() {
            return 100.0;
        }
        durs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let q = (durs.len() / 4).min(durs.len() - 1);
        durs[q]
    }

    /// Feed one key event. `down=true` is a tone duration, `false` a pause.
    /// Returns newly-decoded text appended to `self.text` (may be empty).
    pub fn feed(&mut self, down: bool, dur_ms: f32) -> String {
        self.events.push((down, dur_ms));
        let unit = self.est_unit();
        if down {
            self.syms.push(if dur_ms < 2.0 * unit {
                MorseSym::Dit
            } else {
                MorseSym::Dah
            });
            return String::new();
        }
        if dur_ms >= 6.0 * unit {
            self.syms.push(MorseSym::WordGap);
        } else if dur_ms >= 2.5 * unit {
            self.syms.push(MorseSym::CharGap);
        } else {
            return String::new();
        }
        self.decode_ready()
    }

    /// Decode symbols up to the last gap boundary (trailing partial
    /// characters stay buffered for more input or `finish`).
    fn decode_ready(&mut self) -> String {
        let mut cut = None;
        for (i, s) in self.syms.iter().enumerate() {
            if matches!(s, MorseSym::CharGap | MorseSym::WordGap) {
                cut = Some(i + 1);
            }
        }
        let Some(cut) = cut else { return String::new() };
        let ready: Vec<MorseSym> = self.syms.drain(..cut).collect();
        let t = elements_to_text(&ready);
        self.text.push_str(&t);
        t
    }

    /// Force-decode everything remaining (end of transmission).
    pub fn finish(&mut self) -> String {
        let t = elements_to_text(&self.syms);
        self.syms.clear();
        self.text.push_str(&t);
        t
    }
}

/// Training drills and scoring.
pub mod trainer {
    use rand::seq::SliceRandom;
    use rand::Rng;

    const GROUPS: &[&str] = &[
        "AEIOU", "BCDFG", "HJKLMN", "PQRST", "VWXYZ", "01234", "56789",
    ];

    /// Random N-character drill drawn from progressive character groups.
    /// `level` 1..=7 selects how many groups are in the pool (Koch-style
    /// progression).
    pub fn drill(len: usize, level: usize, rng: &mut impl Rng) -> String {
        let level = level.clamp(1, GROUPS.len());
        let pool: String = GROUPS[..level].concat();
        let chars: Vec<char> = pool.chars().collect();
        (0..len).map(|_| *chars.choose(rng).unwrap()).collect()
    }

    /// Score an attempt vs expected text: accuracy 0..=1 and the position of
    /// the first error.
    pub fn score(expected: &str, got: &str) -> (f32, Option<usize>) {
        let e: Vec<char> = expected.chars().collect();
        let g: Vec<char> = got.chars().collect();
        let mut correct = 0;
        let mut first_err = None;
        for i in 0..e.len().max(g.len()) {
            match (e.get(i), g.get(i)) {
                (Some(a), Some(b)) if a == b => correct += 1,
                _ => {
                    if first_err.is_none() {
                        first_err = Some(i);
                    }
                }
            }
        }
        (correct as f32 / e.len().max(1) as f32, first_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wpm_timing() {
        assert!((unit_ms(20) - 60.0).abs() < 0.01);
        // "E" = dit → one 60 ms tone.
        let s = playback_schedule("E", 20);
        assert_eq!(
            s,
            vec![Segment {
                on: true,
                dur_ms: 60.0
            }]
        );
    }

    #[test]
    fn sos_timing() {
        // "SOS" = S(...) CharGap O(---) CharGap S(...):
        // tones 12u + intra-char gaps 4u + char gaps 6u + final char 5u = 27u
        let total = duration_ms("SOS", 20);
        assert!((total - 27.0 * 60.0).abs() < 0.5);
    }

    #[test]
    fn roundtrip() {
        let elems = ucr_protocol::morse::text_to_elements("SOS 123");
        assert_eq!(elements_to_text(&elems), "SOS 123");
    }

    #[test]
    fn key_decoder() {
        let mut kd = KeyDecoder::new();
        let unit = 80.0;
        // "S" = dit dit dit
        for _ in 0..3 {
            kd.feed(true, unit);
            kd.feed(false, unit);
        }
        kd.feed(false, 3.0 * unit); // char gap → decode "S"
                                    // "O" = dah dah dah
        for _ in 0..3 {
            kd.feed(true, 3.0 * unit);
            kd.feed(false, unit);
        }
        let mut out = kd.feed(false, 7.0 * unit); // word gap → decode "O "
        out.push_str(&kd.finish());
        assert!(kd.text.starts_with("SO"));
        let _ = out;
    }
}
