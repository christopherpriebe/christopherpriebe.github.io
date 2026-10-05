//! The record of an error in a source and the arithmetic of places in it:
//! where the error is, for the program drawer to underline, and which line
//! and column a byte of the source falls on.
//!
//! The record holds the message and the message with its place as the
//! instruction set words them; this module has no sentence of its own.

use serde::Serialize;

/// One error in a program or trace, and where it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct ParseError {
    /// The source line, from 1.
    pub line: u32,
    /// The column where the problem starts, from 1, counted in Unicode
    /// scalar values.
    pub column: u32,
    /// How many characters to underline from there; 0 when there is nothing
    /// to underline, as for an empty source or an empty line past the line
    /// limit.
    pub length: u32,
    /// What is wrong: ``unknown instruction `lw`; use ld, st, add or bnez.``
    pub message: String,
    /// The message with its place:
    /// ``Line 3, column 5: unknown instruction `lw`; use ld, st, add or bnez.``
    pub text: String,
}

/// The number of characters in `text`, as a column count.
#[must_use]
pub fn chars(text: &str) -> u32 {
    u32::try_from(text.chars().count()).unwrap_or(u32::MAX)
}

/// The line and column of the character holding byte `byte` of `source`,
/// both from 1: where a source too large for the limit crosses it.
#[must_use]
pub fn place_of_byte(source: &str, byte: u32) -> (u32, u32) {
    let byte = usize::try_from(byte)
        .unwrap_or(usize::MAX)
        .min(source.len());
    let start = (0..=byte)
        .rev()
        .find(|&at| source.is_char_boundary(at))
        .unwrap_or(0);
    let before = source.get(..start).unwrap_or_default();
    let line = u32::try_from(before.matches('\n').count())
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    let column = chars(before.rsplit('\n').next().unwrap_or_default()).saturating_add(1);
    (line, column)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    #[test]
    fn the_place_of_a_byte_counts_lines_and_characters() {
        assert_eq!(place_of_byte("abc", 0), (1, 1));
        assert_eq!(place_of_byte("abc\ndef", 5), (2, 2));
        // Byte 2 is inside "é", which starts at byte 1.
        assert_eq!(place_of_byte("a\u{e9}b", 2), (1, 2));
        assert_eq!(place_of_byte("ab", 99), (1, 3));
    }

    #[test]
    fn characters_are_counted_not_bytes() {
        assert_eq!(chars(""), 0);
        assert_eq!(chars("ld r1"), 5);
        assert_eq!(chars("\u{e9}\u{e9}"), 2);
    }
}
