//! Everything the student types that the core reads as a number: an address
//! in the address box, a value in the Value box, and a hit time or miss
//! penalty in the configuration bar.
//!
//! The UI never turns typed text into a number. It passes the text here on
//! every keystroke and draws what comes back, so the rules for what may be
//! typed, and the messages for text that does not fit the configuration, are
//! the core's. An address is read against the displayed run's geometry, which
//! gives its width; a value against the same geometry's word size. The
//! address and value boxes together are an issue draft: the op an Issue press
//! would append, and the typed address's set for the cache table, which shows
//! it while no access is at the cursor.

use serde::Serialize;
use thiserror::Error;

use crate::config::{Config, ConfigError, ConfigNumberField, Timing, FALLBACK_TIMING};
use crate::explain;
use crate::format;
use crate::geometry::{highest_word, last_address, AccessError, Decoded, Geometry};
use crate::limits::{MAX_CYCLES, MIN_CYCLES};
use crate::op::{AccessKind, Op};
use crate::session::Session;
use crate::snapshot::WayCompare;
use crate::units::{SetIndex, Word};

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

/// A typed hit time or miss penalty, applied to a configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConfigNumberInput {
    /// A number of cycles in range.
    Valid {
        /// The configuration with the number in its timing, which is then
        /// on.
        config: Config,
    },
    /// Text that is not a number of cycles in range.
    Invalid {
        /// What is wrong: `Hit time must be between 1 and 1,000 cycles; got
        /// 0.`, or for text that is not a whole number, `Type a whole number
        /// of cycles, 1 to 1,000.`
        message: String,
    },
}

/// What the address and value boxes would issue, and what the address strip
/// and the cache table show of the typed address while no access is at the
/// cursor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct IssueDraft {
    /// The typed address, read.
    pub address: AddressInput,
    /// The typed value, read, for a store; absent for a load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<WordInput>,
    /// The op an Issue press appends, when the address, and a store's
    /// value, are valid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<Op>,
    /// The request lane's label for the typed address: "Load 0x0A4", or "No
    /// request" when it does not decode.
    pub request_text: String,
    /// The typed address's set, when it decodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_set: Option<SetIndex>,
    /// One `=?` comparator per way of that set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compare: Option<Vec<WayCompare>>,
}

/// Why typed text is not a number the core can use. `Display` is the message
/// the student reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub(crate) enum InputError {
    /// Text that is not a hex address within the width, whether it is not hex
    /// at all or too large.
    #[error(
        "Type {} {}-bit hex address, {} to {}.",
        format::a_or_an(*.address_bits),
        format::setting(*.address_bits),
        format::address(0_u32, *.address_bits),
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
        format::setting(highest_word(*.word_bytes))
    )]
    NotAWord {
        /// The word size, in bytes.
        word_bytes: u32,
    },
    /// Text that is not a whole number of cycles.
    #[error(
        "Type a whole number of cycles, {} to {}.",
        format::setting(MIN_CYCLES),
        format::setting(MAX_CYCLES)
    )]
    NotCycles,
    /// A number the word cannot hold, signed or unsigned.
    #[error(
        "{} does not fit {} {}-byte word ({} to {}).",
        format::signed_data(*.negative, *.magnitude),
        format::a_or_an(*.word_bytes),
        format::setting(*.word_bytes),
        format::negative_setting(lowest_magnitude(*.word_bytes)),
        format::setting(highest_word(*.word_bytes))
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
                    address_bits: self.address_bits(),
                }
                .to_string(),
            },
            Some(decoded) if decoded.aligned => AddressInput::Valid { decoded },
            Some(decoded) => AddressInput::Misaligned {
                message: AccessError::Misaligned {
                    addr: decoded.addr,
                    address_bits: self.address_bits(),
                    word_bytes: self.word_bytes(),
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
        let word_bytes = self.word_bytes();
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
        match fit_word(negative, magnitude, word_bytes) {
            Ok(value) => WordInput::Valid {
                value,
                text: format::data(value.get()),
            },
            Err(error) => WordInput::Invalid {
                message: error.to_string(),
            },
        }
    }

    /// What the address and value boxes would issue: the typed address read
    /// as [`Geometry::read_address`] reads it, a store's value as
    /// [`Geometry::read_word`] does, and the op an Issue press appends when
    /// both are valid. An address that decodes, aligned or not, has its
    /// request text and its set, with a pending comparator on each way.
    pub fn draft_issue(&self, kind: AccessKind, addr_text: &str, value_text: &str) -> IssueDraft {
        let address = self.read_address(addr_text);
        let value = (kind == AccessKind::Write).then(|| self.read_word(value_text));
        let decoded = match &address {
            AddressInput::Valid { decoded } | AddressInput::Misaligned { decoded, .. } => {
                Some(decoded)
            }
            AddressInput::Empty | AddressInput::Invalid { .. } => None,
        };
        let op = match (&address, &value) {
            (AddressInput::Valid { decoded }, None) => Some(Op::Issue {
                kind,
                addr: decoded.addr,
                value: None,
            }),
            (AddressInput::Valid { decoded }, Some(WordInput::Valid { value, .. })) => {
                Some(Op::Issue {
                    kind,
                    addr: decoded.addr,
                    value: Some(*value),
                })
            }
            _ => None,
        };
        IssueDraft {
            request_text: decoded.map_or_else(explain::no_request, |decoded| {
                explain::typed_request(self, kind, decoded.addr)
            }),
            selected_set: decoded.map(|decoded| decoded.index),
            compare: decoded.map(|_| (0..self.ways()).map(|_| WayCompare::Pending).collect()),
            address,
            value,
            op,
        }
    }
}

/// A typed hit time or miss penalty, `field`, applied to `shown`: a whole
/// number from [`MIN_CYCLES`] to [`MAX_CYCLES`], spaces around it ignored.
/// The result's timing is on, with the other number from `shown`'s timing,
/// or while `shown` is untimed from `recent`, the most recent timing in the
/// log, or from the fallback timing.
pub(crate) fn read_config_number(
    field: ConfigNumberField,
    text: &str,
    shown: &Config,
    recent: Option<Timing>,
) -> ConfigNumberInput {
    let text = text.trim();
    let got = (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse::<u32>().ok())
        .flatten();
    let Some(got) = got else {
        return ConfigNumberInput::Invalid {
            message: InputError::NotCycles.to_string(),
        };
    };
    if !(MIN_CYCLES..=MAX_CYCLES).contains(&got) {
        return ConfigNumberInput::Invalid {
            message: ConfigError::Timing { field, got }.to_string(),
        };
    }
    let base = shown.timing.or(recent).unwrap_or(FALLBACK_TIMING);
    let timing = match field {
        ConfigNumberField::HitCycles => Timing {
            hit_cycles: got,
            ..base
        },
        ConfigNumberField::MissCycles => Timing {
            miss_cycles: got,
            ..base
        },
    };
    ConfigNumberInput::Valid {
        config: Config {
            timing: Some(timing),
            ..shown.clone()
        },
    }
}

impl Session {
    /// A typed address, read against the displayed run's geometry, as
    /// [`Geometry::read_address`] reads it.
    pub fn read_address(&self, text: &str) -> AddressInput {
        self.machine().geometry.read_address(text)
    }

    /// A typed value, read against the displayed run's word size, as
    /// [`Geometry::read_word`] reads it.
    pub fn read_word(&self, text: &str) -> WordInput {
        self.machine().geometry.read_word(text)
    }

    /// What the address and value boxes would issue on the displayed run, as
    /// [`Geometry::draft_issue`] reads them.
    pub fn draft_issue(&self, kind: AccessKind, addr_text: &str, value_text: &str) -> IssueDraft {
        self.machine()
            .geometry
            .draft_issue(kind, addr_text, value_text)
    }

    /// A typed hit time or miss penalty, applied to `shown`, the pending
    /// configuration or the displayed run's: the configuration with that
    /// number in its timing, which is then on, and the other number as the
    /// timing fields show it; or, for a number out of range, the timing's
    /// range message (`Hit time must be between 1 and 1,000 cycles; got
    /// 0.`), and for anything else a prompt to type a whole number of
    /// cycles, so nothing the student types reaches a configuration as a
    /// malformed timing.
    pub fn read_config_number(
        &self,
        field: ConfigNumberField,
        text: &str,
        shown: &Config,
    ) -> ConfigNumberInput {
        read_config_number(field, text, shown, self.recent_timing())
    }
}

/// A number, `magnitude` after a minus sign when `negative`, as a word of
/// `word_bytes` bytes. It fits if it does as a signed or as an unsigned
/// number, from −2^(8w−1) to 2^(8w) − 1, and a negative one becomes its two's
/// complement in the word's width, so −3 in a 1-byte word is 253. The Value
/// box and a `st` immediate read their numbers so, and an `add` immediate
/// does with the eight bytes of a register. A trace's `W` value has no sign
/// and is checked against the unsigned range alone.
///
/// # Errors
///
/// The word-fit message, for a number the word cannot hold: `300 does not
/// fit a 1-byte word (−128 to 255).`
pub(crate) fn fit_word(
    negative: bool,
    magnitude: u64,
    word_bytes: u32,
) -> Result<Word, InputError> {
    let fits = if negative {
        magnitude <= lowest_magnitude(word_bytes)
    } else {
        magnitude <= highest_word(word_bytes)
    };
    if !fits {
        return Err(InputError::WordTooWide {
            negative,
            magnitude,
            word_bytes,
        });
    }
    let value = if negative {
        magnitude.wrapping_neg() & highest_word(word_bytes)
    } else {
        magnitude
    };
    Ok(Word::new_unchecked(value))
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

/// The magnitude of the most negative signed value a word of `word_bytes`
/// bytes holds: 2^(8w − 1).
fn lowest_magnitude(word_bytes: u32) -> u64 {
    1_u64
        .checked_shl(word_bytes.saturating_mul(u8::BITS).saturating_sub(1))
        .unwrap_or(0)
}
