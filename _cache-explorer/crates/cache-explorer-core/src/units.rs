//! Newtypes for the numbers the machine deals in, so an address, a set, a way
//! and a data value can never be mixed up.
//!
//! Each serializes as its bare number and has a public `get`. The raw
//! constructor, `new_unchecked`, is crate-private: code outside the core
//! obtains range-checked values from a [`Geometry`](crate::Geometry), and the
//! core itself creates them only from a `Geometry` or from values it has
//! already checked. A value deserialized from JavaScript or JSON is unchecked
//! until it is validated against the geometry in force.
//!
//! `Display` prints the width-free form: hex for addresses and tags, padded to
//! the two digits the narrowest configuration shows, and decimal for the rest.
//! Text shown to the student is padded to the configuration's widths by the
//! formatting code, which knows them.

use std::fmt;

use serde::{Deserialize, Serialize, Serializer};

use crate::format;

/// Hex digits of the width-free form of an address or a tag.
const HEX_DIGITS: usize = 2;

/// A byte address, below `2^address_bits` of the configuration in force.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct Addr(u32);

/// The address of a line: an [`Addr`] with its offset bits cleared.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct LineAddr(u32);

/// A tag, below `2^tag_bits` of the geometry in force.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct Tag(u32);

/// A set, below the geometry's number of sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct SetIndex(u32);

/// A way within a set, below the geometry's number of ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct WayIndex(u32);

/// A word within a line, below the geometry's words per line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct WordIndex(u32);

/// A cycle count within a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct Cycle(u64);

/// A position in the session's log of micro-events: 0 is the state before the
/// first event, and k the state after the k-th event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct EventPos(u32);

/// An index into the session's ops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct OpIndex(u32);

/// A data value, zero-extended from the configuration's word size.
///
/// It serializes as a 128-bit integer: `serde_json` writes that as a plain
/// number, exact to 2^64 − 1, and serde-wasm-bindgen hands it to JavaScript
/// as a `BigInt`, since a JavaScript number cannot hold every 8-byte word. It
/// deserializes from any unsigned 64-bit integer, so JSON numbers, JavaScript
/// numbers that are safe integers, and `BigInt`s are all accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify), tsify(type = "bigint"))]
#[serde(transparent)]
pub struct Word(u64);

/// One of the eight 64-bit registers. `r0` reads 0, and writes to it are
/// dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum Register {
    /// `r0`, which always reads 0.
    R0 = 0,
    /// `r1`, which hand-issued loads and trace reads write.
    R1 = 1,
    /// `r2`.
    R2 = 2,
    /// `r3`.
    R3 = 3,
    /// `r4`.
    R4 = 4,
    /// `r5`.
    R5 = 5,
    /// `r6`.
    R6 = 6,
    /// `r7`.
    R7 = 7,
}

impl Addr {
    /// Wraps an address the caller has already checked against the geometry.
    pub(crate) const fn new_unchecked(value: u32) -> Self {
        Self(value)
    }

    /// The address as a number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl LineAddr {
    /// Wraps a line address the caller has already checked against the geometry.
    pub(crate) const fn new_unchecked(value: u32) -> Self {
        Self(value)
    }

    /// The line address as a number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Tag {
    /// Wraps a tag the caller has already checked against the geometry.
    pub(crate) const fn new_unchecked(value: u32) -> Self {
        Self(value)
    }

    /// The tag as a number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl SetIndex {
    /// Wraps a set index the caller has already checked against the geometry.
    pub(crate) const fn new_unchecked(value: u32) -> Self {
        Self(value)
    }

    /// The set index as a number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl WayIndex {
    /// Wraps a way index the caller has already checked against the geometry.
    pub(crate) const fn new_unchecked(value: u32) -> Self {
        Self(value)
    }

    /// The way index as a number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl WordIndex {
    /// Wraps a word index the caller has already checked against the geometry.
    pub(crate) const fn new_unchecked(value: u32) -> Self {
        Self(value)
    }

    /// The word index as a number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Cycle {
    /// Wraps a cycle count the engine has computed.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the engine counts cycles in timed mode")
    )]
    pub(crate) const fn new_unchecked(value: u64) -> Self {
        Self(value)
    }

    /// The cycle count as a number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl EventPos {
    /// Wraps a position the session has checked against its log.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the session numbers event positions")
    )]
    pub(crate) const fn new_unchecked(value: u32) -> Self {
        Self(value)
    }

    /// The position as a number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl OpIndex {
    /// Wraps an op index the session has checked against its log.
    #[cfg_attr(not(test), expect(dead_code, reason = "the session numbers ops"))]
    pub(crate) const fn new_unchecked(value: u32) -> Self {
        Self(value)
    }

    /// The op index as a number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl Word {
    /// Wraps a value the caller has already fitted to the word size.
    pub(crate) const fn new_unchecked(value: u64) -> Self {
        Self(value)
    }

    /// The value as a number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl Register {
    /// The register's number, 0 to 7.
    pub const fn get(self) -> u8 {
        self as u8
    }

    /// The register's name, `r0` to `r7`, as the assembler reads it.
    const fn name(self) -> &'static str {
        match self {
            Register::R0 => "r0",
            Register::R1 => "r1",
            Register::R2 => "r2",
            Register::R3 => "r3",
            Register::R4 => "r4",
            Register::R5 => "r5",
            Register::R6 => "r6",
            Register::R7 => "r7",
        }
    }
}

impl Serialize for Word {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u128(u128::from(self.0))
    }
}

impl fmt::Display for Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format::hex(self.0, HEX_DIGITS))
    }
}

impl fmt::Display for LineAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format::hex(self.0, HEX_DIGITS))
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format::hex(self.0, HEX_DIGITS))
    }
}

impl fmt::Display for SetIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format::data(self.0))
    }
}

impl fmt::Display for WayIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format::data(self.0))
    }
}

impl fmt::Display for WordIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format::data(self.0))
    }
}

impl fmt::Display for Cycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format::data(self.0))
    }
}

impl fmt::Display for EventPos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format::data(self.0))
    }
}

impl fmt::Display for OpIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format::data(self.0))
    }
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format::data(self.0))
    }
}

impl fmt::Display for Register {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    #[test]
    fn display_uses_the_width_free_forms() {
        assert_eq!(Addr::new_unchecked(0xA4).to_string(), "0xA4");
        assert_eq!(Addr::new_unchecked(0x0FFF).to_string(), "0xFFF");
        assert_eq!(LineAddr::new_unchecked(0x0A0).to_string(), "0xA0");
        assert_eq!(Tag::new_unchecked(0x5).to_string(), "0x05");
        assert_eq!(SetIndex::new_unchecked(2).to_string(), "2");
        assert_eq!(WayIndex::new_unchecked(1).to_string(), "1");
        assert_eq!(WordIndex::new_unchecked(3).to_string(), "3");
        assert_eq!(Cycle::new_unchecked(105).to_string(), "105");
        assert_eq!(Cycle::new_unchecked(20_000).to_string(), "20,000");
        assert_eq!(EventPos::new_unchecked(9).to_string(), "9");
        assert_eq!(OpIndex::new_unchecked(4).to_string(), "4");
        assert_eq!(Word::new_unchecked(4_096).to_string(), "4096");
        assert_eq!(Word::new_unchecked(10_000).to_string(), "10,000");
        assert_eq!(Register::R3.to_string(), "r3");
    }

    #[test]
    fn get_returns_the_raw_value() {
        assert_eq!(Addr::new_unchecked(164).get(), 164);
        assert_eq!(LineAddr::new_unchecked(160).get(), 160);
        assert_eq!(Tag::new_unchecked(1).get(), 1);
        assert_eq!(SetIndex::new_unchecked(2).get(), 2);
        assert_eq!(WayIndex::new_unchecked(3).get(), 3);
        assert_eq!(WordIndex::new_unchecked(1).get(), 1);
        assert_eq!(Cycle::new_unchecked(u64::MAX).get(), u64::MAX);
        assert_eq!(EventPos::new_unchecked(7).get(), 7);
        assert_eq!(OpIndex::new_unchecked(8).get(), 8);
        assert_eq!(Word::new_unchecked(u64::MAX).get(), u64::MAX);
        assert_eq!(Register::R0.get(), 0);
        assert_eq!(Register::R7.get(), 7);
    }

    #[test]
    fn newtypes_serialize_as_bare_numbers() {
        let json = serde_json::to_string(&(
            Addr::new_unchecked(164),
            LineAddr::new_unchecked(160),
            Tag::new_unchecked(1),
            SetIndex::new_unchecked(2),
            WayIndex::new_unchecked(0),
            WordIndex::new_unchecked(1),
            Cycle::new_unchecked(105),
            EventPos::new_unchecked(9),
            OpIndex::new_unchecked(4),
        ))
        .unwrap();
        assert_eq!(json, "[164,160,1,2,0,1,105,9,4]");
    }

    #[test]
    fn input_newtypes_deserialize_from_bare_numbers() {
        let (addr, line, tag, set, way, pos, op): (
            Addr,
            LineAddr,
            Tag,
            SetIndex,
            WayIndex,
            EventPos,
            OpIndex,
        ) = serde_json::from_str("[164,160,1,2,3,9,4]").unwrap();
        assert_eq!(addr, Addr::new_unchecked(164));
        assert_eq!(line, LineAddr::new_unchecked(160));
        assert_eq!(tag, Tag::new_unchecked(1));
        assert_eq!(set, SetIndex::new_unchecked(2));
        assert_eq!(way, WayIndex::new_unchecked(3));
        assert_eq!(pos, EventPos::new_unchecked(9));
        assert_eq!(op, OpIndex::new_unchecked(4));
        assert!(serde_json::from_str::<Addr>("-1").is_err());
        assert!(serde_json::from_str::<Addr>("4294967296").is_err());
    }

    #[test]
    fn words_are_exact_to_64_bits_in_json() {
        for value in [0, 7, u64::MAX] {
            let json = serde_json::to_string(&Word::new_unchecked(value)).unwrap();
            assert_eq!(json, value.to_string());
            assert_eq!(
                serde_json::from_str::<Word>(&json).unwrap(),
                Word::new_unchecked(value)
            );
        }
        assert!(serde_json::from_str::<Word>("18446744073709551616").is_err());
        assert!(serde_json::from_str::<Word>("-3").is_err());
    }

    #[test]
    fn registers_serialize_as_lower_case_names() {
        let all = [
            Register::R0,
            Register::R1,
            Register::R2,
            Register::R3,
            Register::R4,
            Register::R5,
            Register::R6,
            Register::R7,
        ];
        for register in all {
            let json = serde_json::to_string(&register).unwrap();
            assert_eq!(json, format!("\"{register}\""));
            assert_eq!(serde_json::from_str::<Register>(&json).unwrap(), register);
        }
        assert!(serde_json::from_str::<Register>("\"r8\"").is_err());
        assert!(serde_json::from_str::<Register>("\"R1\"").is_err());
    }
}
