//! International Morse code alphabet (ITU-R M.1677-1).
//!
//! Only the alphabet and element model live here so firmware can share them.
//! Timing, playback scheduling and key input live in the `ucr-morse` crate.
//!
//! UCR carries *digital* Morse — symbols encoded in data packets. It is not
//! continuous-wave RF and cannot be decoded by conventional receivers.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// One Morse element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MorseSym {
    /// `.`
    Dit,
    /// `-`
    Dah,
    /// Gap between characters (3 dit units).
    CharGap,
    /// Gap between words (7 dit units).
    WordGap,
}

/// Encode text as a Morse element stream. Unknown characters are skipped;
/// `'` (prosign `<SK>` etc. are not encoded — plain ITU alphabet only).
pub fn text_to_elements(text: &str) -> Vec<MorseSym> {
    let mut out = Vec::new();
    let mut first_char = true;
    for ch in text.chars() {
        if ch == ' ' {
            if !first_char {
                out.push(MorseSym::WordGap);
            }
            first_char = true;
            continue;
        }
        let Some(code) = lookup(ch) else { continue };
        if !first_char {
            out.push(MorseSym::CharGap);
        }
        first_char = false;
        for c in code.chars() {
            out.push(match c {
                '.' => MorseSym::Dit,
                _ => MorseSym::Dah,
            });
        }
    }
    out
}

/// Decode a Morse element stream back to text. Best-effort: malformed gaps
/// are tolerated, unknown sequences are dropped.
pub fn elements_to_text(elements: &[MorseSym]) -> String {
    let mut out = String::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut String| {
        if !cur.is_empty() {
            if let Some(ch) = lookup_rev(cur) {
                out.push(ch);
            }
            cur.clear();
        }
    };
    for &el in elements {
        match el {
            MorseSym::Dit => cur.push('.'),
            MorseSym::Dah => cur.push('-'),
            MorseSym::CharGap => flush(&mut cur, &mut out),
            MorseSym::WordGap => {
                flush(&mut cur, &mut out);
                if !out.ends_with(' ') && !out.is_empty() {
                    out.push(' ');
                }
            }
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// ITU-R M.1677-1 international Morse table.
pub fn lookup(ch: char) -> Option<&'static str> {
    Some(match ch.to_ascii_uppercase() {
        'A' => ".-",
        'B' => "-...",
        'C' => "-.-.",
        'D' => "-..",
        'E' => ".",
        'F' => "..-.",
        'G' => "--.",
        'H' => "....",
        'I' => "..",
        'J' => ".---",
        'K' => "-.-",
        'L' => ".-..",
        'M' => "--",
        'N' => "-.",
        'O' => "---",
        'P' => ".--.",
        'Q' => "--.-",
        'R' => ".-.",
        'S' => "...",
        'T' => "-",
        'U' => "..-",
        'V' => "...-",
        'W' => ".--",
        'X' => "-..-",
        'Y' => "-.--",
        'Z' => "--..",
        '0' => "-----",
        '1' => ".----",
        '2' => "..---",
        '3' => "...--",
        '4' => "....-",
        '5' => ".....",
        '6' => "-....",
        '7' => "--...",
        '8' => "---..",
        '9' => "----.",
        '.' => ".-.-.-",
        ',' => "--..--",
        '?' => "..--..",
        '\'' => ".----.",
        '!' => "-.-.--",
        '/' => "-..-.",
        '(' => "-.--.",
        ')' => "-.--.-",
        '&' => ".-...",
        ':' => "---...",
        ';' => "-.-.-.",
        '=' => "-...-",
        '+' => ".-.-.",
        '-' => "-....-",
        '_' => "..--.-",
        '"' => ".-..-.",
        '$' => "...-..-",
        '@' => ".--.-.",
        _ => return None,
    })
}

fn lookup_rev(code: &str) -> Option<char> {
    // Small table — linear scan is fine and avoids a static map dependency.
    const CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.,?'!/()&:;=+-_\"$@";
    CHARS.chars().find(|&ch| lookup(ch) == Some(code))
}

/// Pack an element stream into bytes (2 bits per symbol: 00 dit, 01 dah,
/// 10 chargap, 11 wordgap). Compact form for over-the-air Morse key streams.
pub fn pack_elements(elements: &[MorseSym]) -> Vec<u8> {
    let mut out = Vec::with_capacity(elements.len() / 4 + 1);
    let mut byte = 0u8;
    let mut shift = 0u8;
    for &el in elements {
        let v = match el {
            MorseSym::Dit => 0u8,
            MorseSym::Dah => 1,
            MorseSym::CharGap => 2,
            MorseSym::WordGap => 3,
        };
        byte |= v << shift;
        shift += 2;
        if shift == 8 {
            out.push(byte);
            byte = 0;
            shift = 0;
        }
    }
    if shift > 0 {
        out.push(byte);
    }
    out
}

/// Unpack `pack_elements` output. `count` = number of 2-bit symbols.
pub fn unpack_elements(packed: &[u8], count: usize) -> Vec<MorseSym> {
    let mut out = Vec::with_capacity(count);
    'outer: for &byte in packed {
        for shift in (0..8).step_by(2) {
            if out.len() == count {
                break 'outer;
            }
            out.push(match (byte >> shift) & 0b11 {
                0 => MorseSym::Dit,
                1 => MorseSym::Dah,
                2 => MorseSym::CharGap,
                _ => MorseSym::WordGap,
            });
        }
    }
    out
}
