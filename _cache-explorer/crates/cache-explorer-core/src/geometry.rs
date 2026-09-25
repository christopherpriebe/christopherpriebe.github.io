//! The shape of a valid configuration: sets, ways, lines and the widths of the
//! tag, index and offset fields, the checked constructors for the indices the
//! shape bounds, and the decoding of an address into its fields.

use serde::Serialize;
use thiserror::Error;

use crate::format;
use crate::units::{Addr, LineAddr, SetIndex, Tag, WayIndex, WordIndex};

/// The shape of a valid configuration, as `Config::validate` computes it.
///
/// Only `Config::validate` builds a `Geometry`, so its counts are non-zero
/// powers of two and its widths add up to `address_bits` with at least one tag
/// bit; the core divides and shifts by them on that basis. Its fields are
/// private to the core and read through the methods of the same names, so code
/// outside the core can neither build a `Geometry` nor change one:
///
/// ```compile_fail,E0616
/// let mut geometry = cache_explorer_core::Config::default().validate().unwrap();
/// geometry.address_bits = 200;
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Geometry {
    /// Width of a byte address.
    pub(crate) address_bits: u8,
    /// Bytes per word.
    pub(crate) word_bytes: u32,
    /// Bytes per line.
    pub(crate) line_bytes: u32,
    /// Ways per set: the associativity.
    pub(crate) ways: u32,
    /// Sets in the cache.
    pub(crate) sets: u32,
    /// Lines in the cache: `sets × ways`.
    pub(crate) lines: u32,
    /// Words per line.
    pub(crate) words_per_line: u32,
    /// Width of the offset field: `log2(line_bytes)`.
    pub(crate) offset_bits: u8,
    /// Width of the index field: `log2(sets)`, 0 when fully associative.
    pub(crate) index_bits: u8,
    /// Width of the tag field: the address bits left over; at least 1.
    pub(crate) tag_bits: u8,
    /// How lines are placed.
    pub(crate) placement: Placement,
}

/// The bits of one address field, inclusive at both ends: bit `hi` down to
/// bit `lo`, `width` bits in all. An empty field has no range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
pub struct BitRange {
    /// The most significant bit of the field.
    pub hi: u8,
    /// The least significant bit of the field.
    pub lo: u8,
    /// Bits in the field: `hi − lo + 1`.
    pub width: u8,
}

/// Where each field lies in an address. The tag is never empty; the index is
/// absent when the cache is fully associative, and the offset when a line is
/// one byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
pub struct BitRanges {
    /// The tag's bits, the most significant ones.
    pub tag: BitRange,
    /// The index's bits, between the tag and the offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<BitRange>,
    /// The offset's bits, the least significant ones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<BitRange>,
}

/// Which field of an address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    /// The tag, compared with the tags a set holds.
    Tag,
    /// The index, which picks the set.
    Index,
    /// The offset, which picks the byte within the line.
    Offset,
}

/// One field of a decoded address as the address strip shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct AddressField {
    /// Which field this is.
    pub field: FieldKind,
    /// The field's bits, most significant first, one character per bit.
    pub bits: String,
    /// The field's value as the strip labels it: the tag in hex (`0x01`), the
    /// index and the offset in decimal (`2`, `4`).
    pub value_text: String,
    /// The note under the field: `5 bits`, `3 bits → set 2`,
    /// `4 bits → word 1, byte 0`.
    pub note: String,
    /// Where the field lies in the address.
    pub range: BitRange,
}

/// An address split into its tag, index and offset under one geometry, with
/// the texts the address strip and the log show for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Decoded {
    /// The address.
    pub addr: Addr,
    /// The address in hex, padded to the address width: `0x0A4`.
    pub addr_text: String,
    /// The address of the line holding it: the offset bits cleared.
    pub line: LineAddr,
    /// The line address in hex, padded to the address width: `0x0A0`.
    pub line_text: String,
    /// The tag.
    pub tag: Tag,
    /// The tag in hex, padded to the tag's width and to at least two digits:
    /// `0x01`.
    pub tag_text: String,
    /// The set the address maps to; 0 when the cache is fully associative.
    pub index: SetIndex,
    /// The byte within the line.
    pub offset: u32,
    /// The word within the line.
    pub word: WordIndex,
    /// The byte within the word; 0 for an aligned address.
    pub byte_in_word: u32,
    /// Whether the address is a multiple of the word size. A misaligned
    /// address decodes, so the strip follows it while the student types, but
    /// cannot be issued.
    pub aligned: bool,
    /// Where each field lies in the address.
    pub bit_ranges: BitRanges,
    /// The address in binary, one character per address bit, most significant
    /// first.
    pub bits: String,
    /// The non-empty fields, most significant first: tag, index, offset.
    pub fields: Vec<AddressField>,
}

/// Why an address cannot be accessed. `Display` is the message the student
/// reads.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum AccessError {
    /// The address does not fit the address width.
    #[error(
        "{} does not fit {} {}-bit address ({}\u{2013}{}).",
        format::address(*.addr, *.address_bits),
        format::a_or_an(*.address_bits),
        format::setting(*.address_bits),
        format::address(0, *.address_bits),
        format::address(last_address(*.address_bits), *.address_bits)
    )]
    OutOfRange {
        /// The address given.
        addr: u32,
        /// The width it does not fit.
        address_bits: u8,
    },
    /// The address is not a multiple of the word size.
    #[error(
        "{} is not aligned to {} {}-byte word; use {}.",
        format::address(.addr.get(), *.address_bits),
        format::a_or_an(*.word_bytes),
        format::setting(*.word_bytes),
        aligned_neighbours(*.addr, *.address_bits, *.word_bytes)
    )]
    Misaligned {
        /// The address given.
        addr: Addr,
        /// The address width, which sets how addresses are written.
        address_bits: u8,
        /// The word size, in bytes.
        word_bytes: u32,
    },
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
    /// Width of a byte address.
    pub const fn address_bits(&self) -> u8 {
        self.address_bits
    }

    /// Bytes per word.
    pub const fn word_bytes(&self) -> u32 {
        self.word_bytes
    }

    /// Bytes per line.
    pub const fn line_bytes(&self) -> u32 {
        self.line_bytes
    }

    /// Ways per set: the associativity.
    pub const fn ways(&self) -> u32 {
        self.ways
    }

    /// Sets in the cache.
    pub const fn sets(&self) -> u32 {
        self.sets
    }

    /// Lines in the cache: `sets × ways`.
    pub const fn lines(&self) -> u32 {
        self.lines
    }

    /// Words per line.
    pub const fn words_per_line(&self) -> u32 {
        self.words_per_line
    }

    /// Width of the offset field: `log2(line_bytes)`.
    pub const fn offset_bits(&self) -> u8 {
        self.offset_bits
    }

    /// Width of the index field: `log2(sets)`, 0 when fully associative.
    pub const fn index_bits(&self) -> u8 {
        self.index_bits
    }

    /// Width of the tag field: the address bits left over; at least 1.
    pub const fn tag_bits(&self) -> u8 {
        self.tag_bits
    }

    /// How lines are placed.
    pub const fn placement(&self) -> Placement {
        self.placement
    }

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

    /// Splits `addr` into its tag, index and offset. A misaligned address
    /// decodes, with `aligned` false.
    ///
    /// # Errors
    ///
    /// [`AccessError::OutOfRange`] if `addr` does not fit `address_bits`; such
    /// an address has no decoding.
    pub fn decode(&self, addr: u32) -> Result<Decoded, AccessError> {
        let addr = self.addr(addr).ok_or(AccessError::OutOfRange {
            addr,
            address_bits: self.address_bits,
        })?;
        let value = addr.get();
        // The counts are non-zero powers of two and the tag keeps at least one
        // bit, so these masks and shifts stay within the address.
        let offset = value & (self.line_bytes - 1);
        let index = (value >> self.offset_bits) & (self.sets - 1);
        let tag = value >> (self.offset_bits + self.index_bits);
        let word = offset / self.word_bytes;
        let byte_in_word = offset % self.word_bytes;
        let bit_ranges = BitRanges {
            tag: BitRange {
                hi: self.address_bits - 1,
                lo: self.offset_bits + self.index_bits,
                width: self.tag_bits,
            },
            index: BitRange::from_lo(self.offset_bits, self.index_bits),
            offset: BitRange::from_lo(0, self.offset_bits),
        };
        let tag_text = format::tag(tag, self.tag_bits);
        let mut fields = vec![AddressField {
            field: FieldKind::Tag,
            bits: format::bits(tag, self.tag_bits),
            value_text: tag_text.clone(),
            note: bit_count(self.tag_bits),
            range: bit_ranges.tag,
        }];
        if let Some(range) = bit_ranges.index {
            fields.push(AddressField {
                field: FieldKind::Index,
                bits: format::bits(index, self.index_bits),
                value_text: format::data(index),
                note: format!(
                    "{} \u{2192} set {}",
                    bit_count(self.index_bits),
                    format::data(index)
                ),
                range,
            });
        }
        if let Some(range) = bit_ranges.offset {
            fields.push(AddressField {
                field: FieldKind::Offset,
                bits: format::bits(offset, self.offset_bits),
                value_text: format::data(offset),
                note: format!(
                    "{} \u{2192} word {}, byte {}",
                    bit_count(self.offset_bits),
                    format::data(word),
                    format::data(byte_in_word)
                ),
                range,
            });
        }
        Ok(Decoded {
            addr,
            addr_text: format::address(value, self.address_bits),
            line: LineAddr::new_unchecked(value - offset),
            line_text: format::address(value - offset, self.address_bits),
            tag: Tag::new_unchecked(tag),
            tag_text,
            index: SetIndex::new_unchecked(index),
            offset,
            word: WordIndex::new_unchecked(word),
            byte_in_word,
            aligned: byte_in_word == 0,
            bit_ranges,
            bits: format::bits(value, self.address_bits),
            fields,
        })
    }

    /// The address of the line with tag `tag` in set `set`: the two fields put
    /// back in place, with a zero offset.
    pub fn line_addr(&self, tag: Tag, set: SetIndex) -> LineAddr {
        LineAddr::new_unchecked(
            (tag.get() << (self.offset_bits + self.index_bits)) | (set.get() << self.offset_bits),
        )
    }
}

impl BitRange {
    /// The `width` bits from bit `lo` up; `None` when `width` is 0, since an
    /// empty field has no range.
    const fn from_lo(lo: u8, width: u8) -> Option<BitRange> {
        match width.checked_sub(1) {
            Some(above_lo) => Some(BitRange {
                hi: lo + above_lo,
                lo,
                width,
            }),
            None => None,
        }
    }
}

/// The highest address `address_bits` bits can hold.
pub(crate) fn last_address(address_bits: u8) -> u32 {
    u32::MAX
        .checked_shr(u32::BITS.saturating_sub(u32::from(address_bits)))
        .unwrap_or(0)
}

/// A field's width as its note starts: `1 bit`, `5 bits`.
fn bit_count(bits: u8) -> String {
    format!(
        "{} {}",
        format::setting(bits),
        format::plural(bits, "bit", "bits")
    )
}

/// The aligned addresses either side of a misaligned one, as its message
/// offers them: `0x0A4 or 0x0A8`, or only the lower one when the upper one
/// would not fit the address width.
fn aligned_neighbours(addr: Addr, address_bits: u8, word_bytes: u32) -> String {
    let below = addr.get() - addr.get().checked_rem(word_bytes).unwrap_or(0);
    let below_text = format::address(below, address_bits);
    match below.checked_add(word_bytes) {
        Some(above) if above <= last_address(address_bits) => {
            format!("{below_text} or {}", format::address(above, address_bits))
        }
        _ => below_text,
    }
}
