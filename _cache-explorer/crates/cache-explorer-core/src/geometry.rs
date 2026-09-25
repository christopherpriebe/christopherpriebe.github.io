//! The shape of a valid configuration: sets, ways, lines and the widths of the
//! tag, index and offset fields, and the checked constructors for the indices
//! the shape bounds.

use serde::Serialize;

use crate::units::{Addr, SetIndex, Tag, WayIndex, WordIndex};

/// The shape of a valid configuration, as `Config::validate` computes it.
///
/// A `Geometry` exists only for a configuration that validated, so its counts
/// are non-zero powers of two and its widths add up to `address_bits` with at
/// least one tag bit. Code outside the core cannot build one by hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Geometry {
    /// Width of a byte address.
    pub address_bits: u8,
    /// Bytes per word.
    pub word_bytes: u32,
    /// Bytes per line.
    pub line_bytes: u32,
    /// Ways per set: the associativity.
    pub ways: u32,
    /// Sets in the cache.
    pub sets: u32,
    /// Lines in the cache: `sets × ways`.
    pub lines: u32,
    /// Words per line.
    pub words_per_line: u32,
    /// Width of the offset field: `log2(line_bytes)`.
    pub offset_bits: u8,
    /// Width of the index field: `log2(sets)`, 0 when fully associative.
    pub index_bits: u8,
    /// Width of the tag field: the address bits left over; at least 1.
    pub tag_bits: u8,
    /// How lines are placed.
    pub placement: Placement,
}

/// How a cache places lines. A one-line cache is both direct-mapped and fully
/// associative by definition; it is `FullyAssociative`, so it has no index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum Placement {
    /// One way per set, and more than one set.
    DirectMapped,
    /// Several ways per set, and more than one set.
    SetAssociative,
    /// One set holding every line.
    FullyAssociative,
}

impl Placement {
    /// The placement of a cache with `sets` sets of `ways` ways.
    pub(crate) const fn of(sets: u32, ways: u32) -> Self {
        if sets == 1 {
            Self::FullyAssociative
        } else if ways == 1 {
            Self::DirectMapped
        } else {
            Self::SetAssociative
        }
    }
}

impl Geometry {
    /// `value` as an address, if it fits `address_bits`.
    pub fn addr(&self, value: u32) -> Option<Addr> {
        (u64::from(value) < 1_u64 << self.address_bits).then(|| Addr::new_unchecked(value))
    }

    /// `value` as a set index, if the cache has that set.
    pub fn set_index(&self, value: u32) -> Option<SetIndex> {
        (value < self.sets).then(|| SetIndex::new_unchecked(value))
    }

    /// `value` as a way index, if each set has that way.
    pub fn way_index(&self, value: u32) -> Option<WayIndex> {
        (value < self.ways).then(|| WayIndex::new_unchecked(value))
    }

    /// `value` as a word index, if each line has that word.
    pub fn word_index(&self, value: u32) -> Option<WordIndex> {
        (value < self.words_per_line).then(|| WordIndex::new_unchecked(value))
    }

    /// `value` as a tag, if it fits `tag_bits`.
    pub fn tag(&self, value: u32) -> Option<Tag> {
        (u64::from(value) < 1_u64 << self.tag_bits).then(|| Tag::new_unchecked(value))
    }
}
