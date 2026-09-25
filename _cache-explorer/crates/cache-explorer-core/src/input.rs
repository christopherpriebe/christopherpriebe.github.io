//! Everything the student types that the core reads as a number: an address
//! in the address box, a value in the Value box.
//!
//! The UI never turns typed text into a number. It passes the text here on
//! every keystroke and draws what comes back, so the rules for what may be
//! typed, and the messages for text that does not fit the configuration, are
//! the core's. An address is read against the displayed run's geometry, which
//! gives its width; a value against the same geometry's word size.

use serde::Serialize;
use thiserror::Error;

use crate::format;
use crate::geometry::{last_address, AccessError, Decoded, Geometry};
use crate::units::Word;

/// A typed address, read against a geometry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AddressInput {
    /// Nothing is typed.
    Empty,
    /// A word-aligned address within the address width.
    Valid {
        /// The address, decoded.
        decoded: Decoded,
    },
    /// An address within the width that is not word-aligned. It decodes, so
    /// the address strip follows it, but it cannot be issued.
    Misaligned {
        /// The address, decoded.
        decoded: Decoded,
        /// Why it cannot be issued, naming the aligned addresses either side:
        /// `0x0A6 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8.`
        message: String,
    },
    /// Text that is not a hex address within the width.
    Invalid {
        /// What to type instead: `Type a 12-bit hex address, 0x000 to 0xFFF.`
        message: String,
    },
}

/// A typed data value, read against a word size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WordInput {
    /// Nothing is typed.
    Empty,
    /// A number the word can hold.
    Valid {
        /// The word as it would be stored: a negative number as its two's
        /// complement in the word's width.
        value: Word,
        /// The stored word as the log prints it: `7`, and `253` for `-3` in a
        /// 1-byte word.
        text: String,
    },
    /// Text that is not a number, or a number the word cannot hold.
    Invalid {
        /// What is wrong: `300 does not fit a 1-byte word (−128 to 255).`, or
        /// for text that is not a number, `Type a whole number for a 1-byte
        /// word, −128 to 255.`
        message: String,
    },
}

/// Why typed text is not a number the core can use. `Display` is the message
/// the student reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
enum InputError {
    /// Text that is not a hex address within the width, whether it is not hex
    /// at all or too large.
    #[error(
        "Type {} {}-bit hex address, {} to {}.",
        format::a_or_an(*.address_bits),
        format::setting(*.address_bits),
        format::address(0, *.address_bits),
        format::address(last_address(*.address_bits), *.address_bits)
    )]
    NotAnAddress {
        /// The address width.
        address_bits: u8,
    },
    /// Text that is not a number.
    #[error(
        "Type a whole number for {} {}-byte word, {} to {}.",
        format::a_or_an(*.word_bytes),
        format::setting(*.word_bytes),
        format::negative_setting(lowest_magnitude(*.word_bytes)),
        format::setting(highest(*.word_bytes))
    )]
    NotAWord {
        /// The word size, in bytes.
        word_bytes: u32,
    },
    /// A number the word cannot hold, signed or unsigned.
    #[error(
        "{} does not fit {} {}-byte word ({} to {}).",
        format::signed_data(*.negative, *.magnitude),
        format::a_or_an(*.word_bytes),
        format::setting(*.word_bytes),
        format::negative_setting(lowest_magnitude(*.word_bytes)),
        format::setting(highest(*.word_bytes))
    )]
    WordTooWide {
        /// Whether the number typed is negative.
        negative: bool,
        /// The number typed, without its sign.
        magnitude: u64,
        /// The word size, in bytes.
        word_bytes: u32,
    },
}

impl Geometry {
    /// Reads a typed address: hex digits in either case, with or without
    /// `0x` (`0x0A4`, `0A4`, `a4`), spaces around them ignored. Addresses are
    /// hex everywhere in the tool, so `164` reads as `0x164`.
    ///
    /// An address within the width decodes, aligned or not; anything else,
    /// too large or not hex at all, gets one message generated from the width.
    pub fn read_address(&self, text: &str) -> AddressInput {
        let text = text.trim();
        if text.is_empty() {
            return AddressInput::Empty;
        }
        let digits = strip_hex_prefix(text).unwrap_or(text);
        let decoded = parse_digits(digits, 16)
            .and_then(|value| u32::try_from(value).ok())
            .and_then(|value| self.decode(value).ok());
        match decoded {
            None => AddressInput::Invalid {
                message: InputError::NotAnAddress {
                    address_bits: self.address_bits,
                }
                .to_string(),
            },
            Some(decoded) if decoded.aligned => AddressInput::Valid { decoded },
            Some(decoded) => AddressInput::Misaligned {
                message: AccessError::Misaligned {
                    addr: decoded.addr,
                    address_bits: self.address_bits,
                    word_bytes: self.word_bytes,
                }
                .to_string(),
                decoded,
            },
        }
    }

    /// Reads a typed data value: decimal (`7`), or hex after `0x` (`0x2A`),
    /// either one after a minus sign (`-3`, or the typographic `−` the
    /// messages print), spaces around it ignored.
    ///
    /// A number fits a word of `w` bytes if it does as a signed or as an
    /// unsigned number, from −2^(8w−1) to 2^(8w) − 1, as a store's immediate
    /// does; a negative one is stored as its two's complement, so `-3` in a
    /// 1-byte word is 253.
    pub fn read_word(&self, text: &str) -> WordInput {
        let text = text.trim();
        if text.is_empty() {
            return WordInput::Empty;
        }
        let word_bytes = self.word_bytes;
        let (negative, unsigned) = match text.strip_prefix(['-', '\u{2212}']) {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let magnitude = match strip_hex_prefix(unsigned) {
            Some(digits) => parse_digits(digits, 16),
            None => parse_digits(unsigned, 10),
        };
        let Some(magnitude) = magnitude else {
            return WordInput::Invalid {
                message: InputError::NotAWord { word_bytes }.to_string(),
            };
        };
        let fits = if negative {
            magnitude <= lowest_magnitude(word_bytes)
        } else {
            magnitude <= highest(word_bytes)
        };
        if !fits {
            return WordInput::Invalid {
                message: InputError::WordTooWide {
                    negative,
                    magnitude,
                    word_bytes,
                }
                .to_string(),
            };
        }
        let value = if negative {
            magnitude.wrapping_neg() & highest(word_bytes)
        } else {
            magnitude
        };
        WordInput::Valid {
            value: Word::new_unchecked(value),
            text: format::data(value),
        }
    }
}

/// `text` after a `0x` or `0X` prefix, if it has one.
fn strip_hex_prefix(text: &str) -> Option<&str> {
    text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"))
}

/// The number `digits` spell in `radix` (10 or 16); `None` if there are no
/// digits, if a character is not a digit of the radix, or if the number does
/// not fit 64 bits.
fn parse_digits(digits: &str, radix: u32) -> Option<u64> {
    if digits.is_empty() {
        return None;
    }
    digits.chars().try_fold(0_u64, |number, digit| {
        number
            .checked_mul(u64::from(radix))?
            .checked_add(u64::from(digit.to_digit(radix)?))
    })
}

/// The largest unsigned value a word of `word_bytes` bytes holds: 2^(8w) − 1.
fn highest(word_bytes: u32) -> u64 {
    u64::MAX
        .checked_shr(u64::BITS.saturating_sub(word_bytes.saturating_mul(u8::BITS)))
        .unwrap_or(0)
}

/// The magnitude of the most negative signed value a word of `word_bytes`
/// bytes holds: 2^(8w − 1).
fn lowest_magnitude(word_bytes: u32) -> u64 {
    1_u64
        .checked_shl(word_bytes.saturating_mul(u8::BITS).saturating_sub(1))
        .unwrap_or(0)
}
