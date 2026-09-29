//! Geometry and decoding: the geometry a valid configuration yields with its
//! checked constructors, the decoding of addresses under every placement,
//! line size and address width, typed addresses and values with their
//! messages, and the serialized shapes of a geometry, its repair and what an
//! address decodes to.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::limits::{
    MAX_ADDRESS_BITS, MAX_LINES, MAX_WORDS_PER_LINE, MIN_ADDRESS_BITS,
};
use std::collections::BTreeSet;

use cache_explorer_core::{
    presets, Addr, AddressInput, BitRange, CacheConfig, Config, Decoded, FieldKind, Geometry,
    Placement, SetIndex, Tag, WayIndex, WordBytes, WordIndex, WordInput,
};
use serde_json::json;

use common::with_cache;
use support::{
    check_address_split, check_misaligned_typed_address, check_not_a_number, check_out_of_range,
    check_table_row,
};

#[cfg(test)]
mod common;

/// The tests' helpers. They sit in a module compiled for tests only, where a
/// failed `unwrap` is a failed test, as in the tests themselves.
#[cfg(test)]
mod support {
    use cache_explorer_core::{AccessError, AddressInput, Config, Placement, WordBytes, WordInput};

    use super::common::with_cache;
    use super::{config_of, table_row, words_of};

    /// Checks that a cache of `size` bytes in `line`-byte lines of
    /// `associativity` ways, with `address_bits`-bit addresses and
    /// `word_bytes`-byte words, splits an address into `expected`: the bits of
    /// its tag, index and offset, and its placement; and that its counts of
    /// lines, sets, ways and words agree.
    pub(crate) fn check_address_split(
        address_bits: u8,
        word_bytes: WordBytes,
        size: u32,
        line: u32,
        associativity: u32,
        expected: (u8, u8, u8, Placement),
    ) {
        let config = Config {
            address_bits,
            word_bytes,
            ..with_cache(size, line, associativity)
        };
        let geometry = config.validate().unwrap();
        assert_eq!(
            (
                geometry.tag_bits(),
                geometry.index_bits(),
                geometry.offset_bits(),
                geometry.placement()
            ),
            expected,
            "{config:?}"
        );
        assert_eq!(geometry.lines(), size / line, "{config:?}");
        assert_eq!(
            geometry.sets() * geometry.ways(),
            geometry.lines(),
            "{config:?}"
        );
        assert_eq!(
            geometry.words_per_line(),
            line / word_bytes.bytes(),
            "{config:?}"
        );
        assert_eq!(
            geometry.tag_bits() + geometry.index_bits() + geometry.offset_bits(),
            address_bits,
            "{config:?}"
        );
    }

    /// Checks that `addr` under `config` decodes to the row of the address
    /// table given by its bits, its tag (range and text), and its index and
    /// offset (each a range and a value, `None` for an empty field).
    pub(crate) fn check_table_row(
        config: Config,
        addr: u32,
        bits: &str,
        tag: (u8, u8, &str),
        index: Option<(u8, u8, u32)>,
        offset: Option<(u8, u8, u32)>,
    ) {
        let decoded = config.validate().unwrap().decode(addr).unwrap();
        let (tag_hi, tag_lo, tag_text) = tag;
        assert_eq!(
            table_row(&decoded),
            (
                bits.to_owned(),
                (tag_hi, tag_lo, tag_text.to_owned()),
                index,
                offset
            ),
            "{config:?}"
        );
        assert_eq!(decoded.bits, bits.replace(' ', ""), "{config:?}");
    }

    /// Checks that `addr` does not decode under `address_bits`-bit addresses,
    /// with the error saying `message`, while the last address that fits does.
    pub(crate) fn check_out_of_range(address_bits: u8, addr: u32, message: &str) {
        let geometry = config_of(address_bits, WordBytes::Four, 128, 16, 1)
            .validate()
            .unwrap();
        let error = geometry.decode(addr).unwrap_err();
        assert_eq!(
            error,
            AccessError::OutOfRange {
                addr: u64::from(addr),
                address_bits
            }
        );
        assert_eq!(error.to_string(), message);
        let last = u32::try_from((1_u64 << address_bits) - 1).unwrap();
        assert!(geometry.decode(last).is_ok(), "{address_bits} bits");
    }

    /// Checks that the typed address `addr` is read as misaligned to
    /// `word_bytes`-byte words under `address_bits`-bit addresses, decoded
    /// all the same, and that the error for it says `message`.
    pub(crate) fn check_misaligned_typed_address(
        address_bits: u8,
        word_bytes: WordBytes,
        addr: u32,
        message: &str,
    ) {
        let geometry = config_of(address_bits, word_bytes, 64, 16, 1)
            .validate()
            .unwrap();
        let text = format!("{addr:x}");
        assert_eq!(
            geometry.read_address(&text),
            AddressInput::Misaligned {
                decoded: geometry.decode(addr).unwrap(),
                message: message.to_owned(),
            },
            "{text}"
        );
        let error = AccessError::Misaligned {
            addr: geometry.addr(addr).unwrap(),
            address_bits,
            word_bytes: word_bytes.bytes(),
        };
        assert_eq!(error.to_string(), message, "{text}");
    }

    /// Checks that a word of `word_bytes` bytes reads "seven" as invalid with
    /// `message`.
    pub(crate) fn check_not_a_number(word_bytes: WordBytes, message: &str) {
        let geometry = words_of(word_bytes).validate().unwrap();
        assert_eq!(
            geometry.read_word("seven"),
            WordInput::Invalid {
                message: message.to_owned()
            }
        );
    }
}

// Geometry

cases! {
    check_address_split;
    address_split_of_a_12_bit_direct_mapped_cache =>
        (12, WordBytes::Four, 128, 16, 1, (5, 3, 4, Placement::DirectMapped)),
    address_split_of_a_16_bit_direct_mapped_cache =>
        (16, WordBytes::Four, 128, 16, 1, (9, 3, 4, Placement::DirectMapped)),
    address_split_of_a_12_bit_two_way_cache =>
        (12, WordBytes::Four, 128, 16, 2, (6, 2, 4, Placement::SetAssociative)),
    address_split_of_a_12_bit_four_way_cache =>
        (12, WordBytes::Four, 256, 16, 4, (6, 2, 4, Placement::SetAssociative)),
    address_split_of_a_12_bit_fully_associative_cache =>
        (12, WordBytes::Four, 128, 16, 8, (8, 0, 4, Placement::FullyAssociative)),
    address_split_of_an_8_bit_cache_of_one_byte_lines =>
        (8, WordBytes::One, 16, 1, 1, (4, 4, 0, Placement::DirectMapped)),
    address_split_of_a_32_bit_cache_of_eight_byte_words =>
        (32, WordBytes::Eight, 1_024, 64, 4, (24, 2, 6, Placement::SetAssociative)),
}

#[test]
fn geometry_follows_the_address_split_in_every_named_configuration() {
    for preset in presets() {
        let config = preset.config();
        let geometry = config.validate().unwrap();
        assert_eq!(
            geometry.sets() * geometry.ways(),
            geometry.lines(),
            "{preset:?}"
        );
        assert_eq!(
            geometry.words_per_line(),
            geometry.line_bytes() / geometry.word_bytes(),
            "{preset:?}"
        );
        assert_eq!(
            geometry.tag_bits() + geometry.index_bits() + geometry.offset_bits(),
            geometry.address_bits(),
            "{preset:?}"
        );
    }
}

#[test]
fn one_line_cache_is_fully_associative() {
    let geometry = with_cache(16, 16, 1).validate().unwrap();
    assert_eq!(
        (geometry.sets(), geometry.ways(), geometry.lines()),
        (1, 1, 1)
    );
    assert_eq!(geometry.index_bits(), 0);
    assert_eq!(geometry.placement(), Placement::FullyAssociative);
}

#[test]
fn fully_associative_caches_up_to_max_lines_ways_validate() {
    let config = Config::default();
    let line_counts = std::iter::successors(Some(1_u32), |&lines| lines.checked_mul(2))
        .take_while(|&lines| lines <= MAX_LINES);
    for lines in line_counts {
        let geometry = Config {
            cache: with_cache(lines * 16, 16, lines).cache,
            ..config
        }
        .validate()
        .unwrap();
        assert_eq!(
            (geometry.sets(), geometry.ways(), geometry.index_bits()),
            (1, lines, 0),
            "{lines} lines"
        );
        assert_eq!(
            geometry.tag_bits(),
            config.address_bits - 4,
            "{lines} lines"
        );
        assert_eq!(
            geometry.placement(),
            Placement::FullyAssociative,
            "{lines} lines"
        );
    }
    // Even the widest lines at the narrowest addresses keep a tag bit.
    let widest_line = (MAX_WORDS_PER_LINE + 1).next_power_of_two() / 2 * 8;
    let most_lines = (MAX_LINES + 1).next_power_of_two() / 2;
    let widest = Config {
        address_bits: MIN_ADDRESS_BITS,
        word_bytes: WordBytes::Eight,
        ..with_cache(widest_line * most_lines, widest_line, most_lines)
    };
    let geometry = widest.validate().unwrap();
    assert!(geometry.tag_bits() >= 1);
    assert_eq!(geometry.placement(), Placement::FullyAssociative);
    // Its addresses decode, the tag's bit splitting the address space in two.
    let last = (1_u32 << MIN_ADDRESS_BITS) - 1;
    for addr in [0, last / 2, last / 2 + 1, last] {
        check_decoded(&geometry, addr, &geometry.decode(addr).unwrap());
    }
}

#[test]
fn checked_constructors_accept_only_values_in_range() {
    let geometry = Config::default().validate().unwrap();
    let address_space = 1_u32 << geometry.address_bits();
    assert_eq!(
        geometry.addr(address_space - 1).map(Addr::get),
        Some(address_space - 1)
    );
    assert_eq!(geometry.addr(address_space), None);
    assert_eq!(
        geometry.set_index(geometry.sets() - 1).map(SetIndex::get),
        Some(geometry.sets() - 1)
    );
    assert_eq!(geometry.set_index(geometry.sets()), None);
    assert_eq!(
        geometry.way_index(geometry.ways() - 1).map(WayIndex::get),
        Some(geometry.ways() - 1)
    );
    assert_eq!(geometry.way_index(geometry.ways()), None);
    let words = geometry.words_per_line();
    assert_eq!(
        geometry.word_index(words - 1).map(WordIndex::get),
        Some(words - 1)
    );
    assert_eq!(geometry.word_index(words), None);
    let tags = 1_u32 << geometry.tag_bits();
    assert_eq!(geometry.tag(tags - 1).map(Tag::get), Some(tags - 1));
    assert_eq!(geometry.tag(tags), None);

    let widest = Config {
        address_bits: MAX_ADDRESS_BITS,
        ..Config::default()
    }
    .validate()
    .unwrap();
    assert_eq!(widest.addr(u32::MAX).map(Addr::get), Some(u32::MAX));
    assert!(widest
        .tag(u32::MAX >> (widest.index_bits() + widest.offset_bits()))
        .is_some());
}

// Decoding

/// `with_cache` at another address width and word size.
fn config_of(
    address_bits: u8,
    word_bytes: WordBytes,
    size_bytes: u32,
    line_bytes: u32,
    associativity: u32,
) -> Config {
    Config {
        address_bits,
        word_bytes,
        ..with_cache(size_bytes, line_bytes, associativity)
    }
}

/// A decoded address as the address table writes it: the bits of each field
/// separated by spaces, then the tag's range and text, the index's range and
/// value, and the offset's range and value, `None` for an empty field.
type TableRow = (
    String,
    (u8, u8, String),
    Option<(u8, u8, u32)>,
    Option<(u8, u8, u32)>,
);

/// `decoded` as a row of the address table.
fn table_row(decoded: &Decoded) -> TableRow {
    let bits: Vec<&str> = decoded
        .fields
        .iter()
        .map(|field| field.bits.as_str())
        .collect();
    let ranges = decoded.bit_ranges;
    (
        bits.join(" "),
        (ranges.tag.hi, ranges.tag.lo, decoded.tag_text.clone()),
        ranges
            .index
            .map(|range| (range.hi, range.lo, decoded.index.get())),
        ranges
            .offset
            .map(|range| (range.hi, range.lo, decoded.offset)),
    )
}

cases! {
    check_table_row;
    // 12-bit, 128 B direct-mapped, 16 B lines
    the_address_table_row_of_a_12_bit_direct_mapped_cache => (
        config_of(12, WordBytes::Four, 128, 16, 1),
        0x0A4,
        "00001 010 0100",
        (11, 7, "0x01"),
        Some((6, 4, 2)),
        Some((3, 0, 4)),
    ),
    // 16-bit, the spec's default width, same cache
    the_address_table_row_of_a_16_bit_direct_mapped_cache => (
        config_of(16, WordBytes::Four, 128, 16, 1),
        0x00A4,
        "000000001 010 0100",
        (15, 7, "0x001"),
        Some((6, 4, 2)),
        Some((3, 0, 4)),
    ),
    // 12-bit, 128 B 2-way (4 sets)
    the_address_table_row_of_a_12_bit_two_way_cache => (
        config_of(12, WordBytes::Four, 128, 16, 2),
        0x1A0,
        "000110 10 0000",
        (11, 6, "0x06"),
        Some((5, 4, 2)),
        Some((3, 0, 0)),
    ),
    // 12-bit, 128 B fully associative
    the_address_table_row_of_a_12_bit_fully_associative_cache => (
        config_of(12, WordBytes::Four, 128, 16, 8),
        0x0A4,
        "00001010 0100",
        (11, 4, "0x0A"),
        None,
        Some((3, 0, 4)),
    ),
}

/// Checks `decoded`, the decoding of `addr` under `geometry`, against the
/// geometry: the fields cover the address, most significant first, their
/// ranges follow the widths, and their values put back together give the
/// address.
#[track_caller]
fn check_decoded(geometry: &Geometry, addr: u32, decoded: &Decoded) {
    let context = format!("{geometry:?} {addr:#x}");
    let (address_bits, tag_bits, index_bits, offset_bits) = (
        geometry.address_bits(),
        geometry.tag_bits(),
        geometry.index_bits(),
        geometry.offset_bits(),
    );
    assert_eq!(decoded.addr.get(), addr, "{context}");
    assert_eq!(u32::from_str_radix(&decoded.bits, 2), Ok(addr), "{context}");
    assert_eq!(decoded.bits.len(), usize::from(address_bits), "{context}");

    let joined: String = decoded
        .fields
        .iter()
        .map(|field| field.bits.as_str())
        .collect();
    assert_eq!(joined, decoded.bits, "{context}");
    for field in &decoded.fields {
        assert_eq!(
            field.bits.len(),
            usize::from(field.range.width),
            "{context}"
        );
        assert_eq!(
            field.range.hi - field.range.lo + 1,
            field.range.width,
            "{context}"
        );
    }
    let kinds: Vec<_> = decoded.fields.iter().map(|field| field.field).collect();
    let expected_kinds: Vec<_> = [
        Some(FieldKind::Tag),
        (index_bits > 0).then_some(FieldKind::Index),
        (offset_bits > 0).then_some(FieldKind::Offset),
    ]
    .into_iter()
    .flatten()
    .collect();
    assert_eq!(kinds, expected_kinds, "{context}");

    let ranges = decoded.bit_ranges;
    let widths = |range: BitRange| (range.hi, range.lo, range.width);
    assert_eq!(
        widths(ranges.tag),
        (address_bits - 1, index_bits + offset_bits, tag_bits),
        "{context}"
    );
    assert_eq!(
        ranges.index.map(widths),
        (index_bits > 0).then(|| (index_bits + offset_bits - 1, offset_bits, index_bits)),
        "{context}"
    );
    assert_eq!(
        ranges.offset.map(widths),
        (offset_bits > 0).then(|| (offset_bits - 1, 0, offset_bits)),
        "{context}"
    );

    let (tag, index, offset) = (decoded.tag.get(), decoded.index.get(), decoded.offset);
    assert!(geometry.tag(tag).is_some(), "{context}");
    assert!(geometry.set_index(index).is_some(), "{context}");
    assert!(offset < geometry.line_bytes(), "{context}");
    assert_eq!(
        (u64::from(tag) << (index_bits + offset_bits))
            | (u64::from(index) << offset_bits)
            | u64::from(offset),
        u64::from(addr),
        "{context}"
    );
    assert_eq!(decoded.line.get(), addr - offset, "{context}");
    assert_eq!(
        geometry.line_addr(decoded.tag, decoded.index),
        decoded.line,
        "{context}"
    );
    assert!(
        geometry.word_index(decoded.word.get()).is_some(),
        "{context}"
    );
    let word = geometry.word_bytes();
    assert_eq!(decoded.word.get() * word + decoded.byte_in_word, offset);
    assert_eq!(decoded.aligned, addr % word == 0, "{context}");
}

#[test]
fn decoding_splits_every_placement_line_size_and_width() {
    let widths = [8, 12, 16, 32];
    // Every power of two up to `max`: the line sizes and line counts the
    // limits allow, up to 16 eight-byte words and `MAX_LINES` lines.
    let powers_up_to = |max: u32| {
        std::iter::successors(Some(1_u32), |&n| n.checked_mul(2))
            .take_while(move |&n| n <= max)
            .collect::<Vec<_>>()
    };
    let line_sizes = powers_up_to(MAX_WORDS_PER_LINE * 8);
    let line_counts = powers_up_to(MAX_LINES);
    let mut seen = BTreeSet::new();
    let mut shapes = BTreeSet::new();
    for address_bits in widths {
        let last = u32::try_from((1_u64 << address_bits) - 1).unwrap();
        let addrs = [0, 1, 0x0A4, 0x0A6, last / 5 * 2, last / 3, last - 1, last];
        for &line in &line_sizes {
            // The narrowest word that keeps the line within a row's words.
            let word_bytes = WordBytes::ALL
                .into_iter()
                .find(|word| word.bytes() <= line && line / word.bytes() <= MAX_WORDS_PER_LINE)
                .unwrap();
            for &lines in &line_counts {
                for associativity in [1, 2, 4, lines] {
                    let config =
                        config_of(address_bits, word_bytes, lines * line, line, associativity);
                    let Ok(geometry) = config.validate() else {
                        continue;
                    };
                    let placement = match (geometry.placement(), geometry.ways()) {
                        (Placement::FullyAssociative, _) => "fully associative",
                        (Placement::DirectMapped, _) => "direct-mapped",
                        (Placement::SetAssociative, 2) => "2-way",
                        (Placement::SetAssociative, _) => "4-way",
                    };
                    seen.insert((address_bits, line, placement));
                    shapes.insert((geometry.sets(), geometry.ways()));
                    for addr in addrs {
                        check_decoded(&geometry, addr, &geometry.decode(addr).unwrap());
                    }
                }
            }
        }
    }
    // The largest shapes decode too: as many sets, and as many ways, as the
    // cache has lines.
    let most_lines = line_counts.last().copied().unwrap();
    assert!(shapes.contains(&(most_lines, 1)), "{shapes:?}");
    assert!(shapes.contains(&(1, most_lines)), "{shapes:?}");
    for address_bits in widths {
        for &line in &line_sizes {
            assert!(
                seen.iter()
                    .any(|&(bits, size, _)| (bits, size) == (address_bits, line)),
                "no valid cache with {line} B lines at {address_bits} bits"
            );
        }
        for placement in ["direct-mapped", "2-way", "4-way", "fully associative"] {
            assert!(
                seen.iter()
                    .any(|&(bits, _, kind)| (bits, kind) == (address_bits, placement)),
                "no {placement} cache at {address_bits} bits"
            );
        }
    }
}

#[test]
fn decoded_fields_carry_the_strips_labels() {
    // (config, address) → (kind, value text, note) for each field
    let cases = [
        (
            config_of(12, WordBytes::Four, 128, 16, 1),
            0x0A4,
            vec![
                (FieldKind::Tag, "0x01", "5 bits"),
                (FieldKind::Index, "2", "3 bits → set 2"),
                (FieldKind::Offset, "4", "4 bits → word 1, byte 0"),
            ],
        ),
        // Two sets: one index bit.
        (
            config_of(12, WordBytes::Four, 32, 16, 1),
            0x0B4,
            vec![
                (FieldKind::Tag, "0x05", "7 bits"),
                (FieldKind::Index, "1", "1 bit → set 1"),
                (FieldKind::Offset, "4", "4 bits → word 1, byte 0"),
            ],
        ),
        // Eight-byte words: the byte within the word.
        (
            config_of(12, WordBytes::Eight, 128, 16, 1),
            0x0AC,
            vec![
                (FieldKind::Tag, "0x01", "5 bits"),
                (FieldKind::Index, "2", "3 bits → set 2"),
                (FieldKind::Offset, "12", "4 bits → word 1, byte 4"),
            ],
        ),
        // Fully associative: no index.
        (
            config_of(12, WordBytes::Four, 128, 16, 8),
            0x0A4,
            vec![
                (FieldKind::Tag, "0x0A", "8 bits"),
                (FieldKind::Offset, "4", "4 bits → word 1, byte 0"),
            ],
        ),
        // One-byte lines: no offset.
        (
            config_of(12, WordBytes::One, 16, 1, 1),
            0x0A4,
            vec![
                (FieldKind::Tag, "0x0A", "8 bits"),
                (FieldKind::Index, "4", "4 bits → set 4"),
            ],
        ),
    ];
    for (config, addr, expected) in cases {
        let decoded = config.validate().unwrap().decode(addr).unwrap();
        let fields: Vec<_> = decoded
            .fields
            .iter()
            .map(|field| (field.field, field.value_text.as_str(), field.note.as_str()))
            .collect();
        assert_eq!(fields, expected, "{config:?}");
    }
}

#[test]
fn one_line_cache_decodes_as_fully_associative() {
    let geometry = config_of(12, WordBytes::Four, 16, 16, 1)
        .validate()
        .unwrap();
    let decoded = geometry.decode(0x0A4).unwrap();
    assert_eq!(decoded.index.get(), 0);
    assert_eq!(decoded.bit_ranges.index, None);
    let kinds: Vec<_> = decoded.fields.iter().map(|field| field.field).collect();
    assert_eq!(kinds, [FieldKind::Tag, FieldKind::Offset]);
    assert_eq!(decoded.tag.get(), 0x0A);
}

#[test]
fn misaligned_addresses_decode_with_aligned_false() {
    let geometry = config_of(12, WordBytes::Four, 128, 16, 1)
        .validate()
        .unwrap();
    let decoded = geometry.decode(0x0A6).unwrap();
    assert!(!decoded.aligned);
    assert_eq!(
        (decoded.offset, decoded.word.get(), decoded.byte_in_word),
        (6, 1, 2)
    );
    assert_eq!(
        (decoded.line_text.as_str(), decoded.index.get()),
        ("0x0A0", 2)
    );
    assert_eq!(
        decoded.fields.last().map(|field| field.note.as_str()),
        Some("4 bits → word 1, byte 2")
    );
    // Every address is aligned to one-byte words.
    let bytes = config_of(12, WordBytes::One, 128, 16, 1)
        .validate()
        .unwrap();
    for addr in 0xA0..0xB0 {
        assert!(bytes.decode(addr).unwrap().aligned, "{addr:#x}");
    }
}

cases! {
    check_out_of_range;
    address_0x100_does_not_decode_at_8_bits =>
        (8, 0x100, "0x100 does not fit an 8-bit address (0x00–0xFF)."),
    address_0x1000_does_not_decode_at_12_bits =>
        (12, 0x1000, "0x1000 does not fit a 12-bit address (0x000–0xFFF)."),
    the_largest_address_does_not_decode_at_12_bits =>
        (12, u32::MAX, "0xFFFFFFFF does not fit a 12-bit address (0x000–0xFFF)."),
    address_0x10000_does_not_decode_at_16_bits =>
        (16, 0x1_0000, "0x10000 does not fit a 16-bit address (0x0000–0xFFFF)."),
    address_0x40000_does_not_decode_at_18_bits =>
        (18, 0x4_0000, "0x40000 does not fit an 18-bit address (0x00000–0x3FFFF)."),
}

#[test]
fn the_widest_address_decodes_at_32_bits() {
    let widest = config_of(32, WordBytes::Four, 128, 16, 1)
        .validate()
        .unwrap();
    assert_eq!(widest.decode(u32::MAX).unwrap().addr.get(), u32::MAX);
}

#[test]
fn decoded_texts_are_padded_to_the_widths() {
    // (address bits, associativity) → (address, line, tag) texts of 0x0A4
    let cases = [
        ((8, 1), ("0xA4", "0xA0", "0x01")),
        ((12, 1), ("0x0A4", "0x0A0", "0x01")),
        ((12, 8), ("0x0A4", "0x0A0", "0x0A")),
        ((16, 1), ("0x00A4", "0x00A0", "0x001")),
        ((32, 1), ("0x000000A4", "0x000000A0", "0x0000001")),
    ];
    for ((address_bits, associativity), (addr_text, line_text, tag_text)) in cases {
        let decoded = config_of(address_bits, WordBytes::Four, 128, 16, associativity)
            .validate()
            .unwrap()
            .decode(0x0A4)
            .unwrap();
        assert_eq!(
            (
                decoded.addr_text.as_str(),
                decoded.line_text.as_str(),
                decoded.tag_text.as_str()
            ),
            (addr_text, line_text, tag_text),
            "{address_bits} bits"
        );
    }
}

#[test]
fn line_addresses_are_rebuilt_from_tag_and_set() {
    let geometry = config_of(12, WordBytes::Four, 128, 16, 2)
        .validate()
        .unwrap();
    let line = geometry.line_addr(geometry.tag(0x06).unwrap(), geometry.set_index(2).unwrap());
    assert_eq!(line.get(), 0x1A0);
    let fully_associative = config_of(12, WordBytes::Four, 128, 16, 8)
        .validate()
        .unwrap();
    let line = fully_associative.line_addr(
        fully_associative.tag(0xFF).unwrap(),
        fully_associative.set_index(0).unwrap(),
    );
    assert_eq!(line.get(), 0xFF0);
}

#[test]
fn line_addresses_cut_unchecked_fields_to_their_widths() {
    // A tag or set deserialized from outside is unchecked until it is
    // validated; each keeps only its field's bits, so the line stays within
    // the width and decodes back to the fields it was built from.
    let tag = |value: u32| serde_json::from_value::<Tag>(json!(value)).unwrap();
    let set = |value: u32| serde_json::from_value::<SetIndex>(json!(value)).unwrap();
    // 12-bit addresses, 8 sets of 16 B lines: 5 tag bits, 3 index bits.
    let direct_mapped = config_of(12, WordBytes::Four, 128, 16, 1)
        .validate()
        .unwrap();
    // 12-bit addresses, one set of 16 B lines: 8 tag bits, no index.
    let fully_associative = config_of(12, WordBytes::Four, 128, 16, 8)
        .validate()
        .unwrap();
    // 32-bit addresses, one set of 1 B lines: a 32-bit tag.
    let whole_address = config_of(32, WordBytes::One, 8, 1, 8).validate().unwrap();
    let max = u32::MAX;
    let cases = [
        (direct_mapped, tag(32), set(0), (0x000, 0, 0)),
        (direct_mapped, tag(0), set(9), (0x010, 0, 1)),
        (direct_mapped, tag(0x3F), set(15), (0xFF0, 0x1F, 7)),
        (direct_mapped, tag(max), set(max), (0xFF0, 0x1F, 7)),
        (fully_associative, tag(0x100), set(1), (0x000, 0, 0)),
        (fully_associative, tag(0x1FF), set(3), (0xFF0, 0xFF, 0)),
        (whole_address, tag(max), set(1), (max, max, 0)),
    ];
    for (geometry, tag, set, (line, tag_kept, set_kept)) in cases {
        let context = format!("{geometry:?} {tag:?} {set:?}");
        let rebuilt = geometry.line_addr(tag, set);
        assert_eq!(rebuilt.get(), line, "{context}");
        let decoded = geometry.decode(rebuilt.get()).unwrap();
        assert_eq!(
            (decoded.tag.get(), decoded.index.get(), decoded.offset),
            (tag_kept, set_kept, 0),
            "{context}"
        );
    }
}

// Typed input

#[test]
fn typed_addresses_are_hex_with_or_without_0x() {
    let geometry = config_of(12, WordBytes::Four, 128, 16, 1)
        .validate()
        .unwrap();
    let a4 = AddressInput::Valid {
        decoded: geometry.decode(0x0A4).unwrap(),
    };
    for text in [
        "0x0A4",
        "0X0a4",
        "0A4",
        "a4",
        "A4",
        "0x00000A4",
        "  0x0A4\t",
    ] {
        assert_eq!(geometry.read_address(text), a4, "{text:?}");
    }
    // Decimal is not read: addresses are hex everywhere.
    assert_eq!(
        geometry.read_address("164"),
        AddressInput::Valid {
            decoded: geometry.decode(0x164).unwrap()
        }
    );
    assert_eq!(
        geometry.read_address("ffc"),
        AddressInput::Valid {
            decoded: geometry.decode(0xFFC).unwrap()
        }
    );
    for text in ["", " ", "\t\n"] {
        assert_eq!(geometry.read_address(text), AddressInput::Empty, "{text:?}");
    }
}

cases! {
    check_misaligned_typed_address;
    a_typed_address_two_bytes_past_a_word_names_the_words_around_it => (
        12,
        WordBytes::Four,
        0x0A6,
        "0x0A6 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8.",
    ),
    a_typed_address_one_byte_past_a_word_names_the_words_around_it => (
        12,
        WordBytes::Four,
        0x0A5,
        "0x0A5 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8.",
    ),
    // The next word would not fit the width.
    a_typed_address_in_the_last_word_names_only_the_word_before => (
        12,
        WordBytes::Four,
        0xFFE,
        "0xFFE is not aligned to a 4-byte word; use 0xFFC.",
    ),
    a_typed_address_misaligned_to_two_byte_words_names_the_words_around_it => (
        16,
        WordBytes::Two,
        0x00A5,
        "0x00A5 is not aligned to a 2-byte word; use 0x00A4 or 0x00A6.",
    ),
    a_typed_address_misaligned_to_eight_byte_words_names_the_words_around_it => (
        12,
        WordBytes::Eight,
        0x0A4,
        "0x0A4 is not aligned to an 8-byte word; use 0x0A0 or 0x0A8.",
    ),
    a_typed_address_in_the_last_eight_byte_word_names_only_the_word_before => (
        8,
        WordBytes::Eight,
        0xFC,
        "0xFC is not aligned to an 8-byte word; use 0xF8.",
    ),
}

#[test]
fn text_that_is_not_an_address_gets_the_widths_message() {
    let twelve = "Type a 12-bit hex address, 0x000 to 0xFFF.";
    let cases = [
        (8, "0x100", "Type an 8-bit hex address, 0x00 to 0xFF."),
        (12, "0x1000", twelve),
        (12, "1000", twelve),
        (12, "fffff", twelve),
        (12, "0x10000000000000000", twelve),
        (12, "xyz", twelve),
        (12, "0x", twelve),
        (12, "x0A4", twelve),
        (12, "-4", twelve),
        (12, "+A4", twelve),
        (12, "0x-4", twelve),
        (12, "0x0A4 1", twelve),
        (12, "0 x0A4", twelve),
        (12, "0x0_A4", twelve),
        (12, "1.5", twelve),
        (12, "٣", twelve),
        (
            16,
            "0x10000",
            "Type a 16-bit hex address, 0x0000 to 0xFFFF.",
        ),
        (
            18,
            "40000",
            "Type an 18-bit hex address, 0x00000 to 0x3FFFF.",
        ),
        (
            32,
            "0x100000000",
            "Type a 32-bit hex address, 0x00000000 to 0xFFFFFFFF.",
        ),
    ];
    for (address_bits, text, message) in cases {
        let geometry = config_of(address_bits, WordBytes::Four, 128, 16, 1)
            .validate()
            .unwrap();
        assert_eq!(
            geometry.read_address(text),
            AddressInput::Invalid {
                message: message.to_owned()
            },
            "{text:?} at {address_bits} bits"
        );
    }
    let widest = config_of(32, WordBytes::Four, 128, 16, 1)
        .validate()
        .unwrap();
    assert!(matches!(
        widest.read_address("0xFFFFFFFC"),
        AddressInput::Valid { decoded } if decoded.addr.get() == 0xFFFF_FFFC
    ));
}

/// A geometry with `word_bytes`-byte words.
fn words_of(word_bytes: WordBytes) -> Config {
    config_of(12, word_bytes, 128, 16, 1)
}

#[test]
fn typed_values_are_decimal_or_hex_and_may_be_negative() {
    // (word size, text) → (stored value, its text)
    let cases = [
        (WordBytes::Four, "7", 7, "7"),
        (WordBytes::Four, " 53 ", 53, "53"),
        (WordBytes::Four, "007", 7, "7"),
        (WordBytes::Four, "0x2A", 42, "42"),
        (WordBytes::Four, "0X2a", 42, "42"),
        (WordBytes::Four, "4096", 4_096, "4096"),
        (WordBytes::Four, "10000", 10_000, "10,000"),
        (WordBytes::Four, "-0", 0, "0"),
        (WordBytes::Four, "-3", 4_294_967_293, "4,294,967,293"),
        (WordBytes::Four, "\u{2212}3", 4_294_967_293, "4,294,967,293"),
        (WordBytes::Four, "-0x3", 4_294_967_293, "4,294,967,293"),
        (
            WordBytes::Four,
            "4294967295",
            4_294_967_295,
            "4,294,967,295",
        ),
        (
            WordBytes::Four,
            "-2147483648",
            2_147_483_648,
            "2,147,483,648",
        ),
        (WordBytes::One, "-3", 253, "253"),
        (WordBytes::One, "255", 255, "255"),
        (WordBytes::One, "-128", 128, "128"),
        (WordBytes::One, "0xFF", 255, "255"),
        (WordBytes::Two, "-1", 65_535, "65,535"),
        (
            WordBytes::Eight,
            "18446744073709551615",
            u64::MAX,
            "18,446,744,073,709,551,615",
        ),
        (
            WordBytes::Eight,
            "0xFFFFFFFFFFFFFFFF",
            u64::MAX,
            "18,446,744,073,709,551,615",
        ),
        (
            WordBytes::Eight,
            "-1",
            u64::MAX,
            "18,446,744,073,709,551,615",
        ),
        (
            WordBytes::Eight,
            "-9223372036854775808",
            1 << 63,
            "9,223,372,036,854,775,808",
        ),
    ];
    for (word_bytes, text, value, value_text) in cases {
        let geometry = words_of(word_bytes).validate().unwrap();
        let WordInput::Valid {
            value: word,
            text: shown,
        } = geometry.read_word(text)
        else {
            panic!(
                "{text:?} in a {word_bytes:?} word: {:?}",
                geometry.read_word(text)
            );
        };
        assert_eq!(
            (word.get(), shown.as_str()),
            (value, value_text),
            "{text:?}"
        );
    }
    let geometry = words_of(WordBytes::Four).validate().unwrap();
    for text in ["", "  "] {
        assert_eq!(geometry.read_word(text), WordInput::Empty, "{text:?}");
    }
}

#[test]
fn typed_values_the_word_cannot_hold_are_refused() {
    let cases = [
        (
            WordBytes::One,
            "300",
            "300 does not fit a 1-byte word (−128 to 255).",
        ),
        (
            WordBytes::One,
            "256",
            "256 does not fit a 1-byte word (−128 to 255).",
        ),
        (
            WordBytes::One,
            "0x100",
            "256 does not fit a 1-byte word (−128 to 255).",
        ),
        (
            WordBytes::One,
            "-129",
            "−129 does not fit a 1-byte word (−128 to 255).",
        ),
        (
            WordBytes::One,
            "1000",
            "1000 does not fit a 1-byte word (−128 to 255).",
        ),
        (
            WordBytes::Two,
            "70000",
            "70,000 does not fit a 2-byte word (−32,768 to 65,535).",
        ),
        (
            WordBytes::Four,
            "4294967296",
            "4,294,967,296 does not fit a 4-byte word (−2,147,483,648 to 4,294,967,295).",
        ),
        (
            WordBytes::Four,
            "-0x80000001",
            "−2,147,483,649 does not fit a 4-byte word (−2,147,483,648 to 4,294,967,295).",
        ),
        (
            WordBytes::Eight,
            "-9223372036854775809",
            "−9,223,372,036,854,775,809 does not fit an 8-byte word \
             (−9,223,372,036,854,775,808 to 18,446,744,073,709,551,615).",
        ),
    ];
    for (word_bytes, text, message) in cases {
        let geometry = words_of(word_bytes).validate().unwrap();
        assert_eq!(
            geometry.read_word(text),
            WordInput::Invalid {
                message: message.to_owned()
            },
            "{text:?}"
        );
    }
}

#[test]
fn text_that_is_not_a_number_gets_the_word_sizes_message() {
    let four = "Type a whole number for a 4-byte word, −2,147,483,648 to 4,294,967,295.";
    let geometry = words_of(WordBytes::Four).validate().unwrap();
    for text in [
        "abc",
        "2A",
        "0x",
        "-",
        "--3",
        "+5",
        "- 3",
        "0x-3",
        "0xG",
        "1.5",
        "1,000",
        "1e3",
        "7 8",
        "٣",
        "18446744073709551616",
    ] {
        assert_eq!(
            geometry.read_word(text),
            WordInput::Invalid {
                message: four.to_owned()
            },
            "{text:?}"
        );
    }
}

cases! {
    check_not_a_number;
    text_that_is_not_a_number_gets_the_one_byte_word_message => (
        WordBytes::One,
        "Type a whole number for a 1-byte word, −128 to 255.",
    ),
    text_that_is_not_a_number_gets_the_two_byte_word_message => (
        WordBytes::Two,
        "Type a whole number for a 2-byte word, −32,768 to 65,535.",
    ),
    text_that_is_not_a_number_gets_the_eight_byte_word_message => (
        WordBytes::Eight,
        "Type a whole number for an 8-byte word, \
         −9,223,372,036,854,775,808 to 18,446,744,073,709,551,615.",
    ),
}

// Serialized shapes

#[test]
fn geometry_and_repair_serialize_to_camel_case() {
    let config = Config {
        address_bits: 16,
        ..with_cache(128, 16, 2)
    };
    let geometry = config.validate().unwrap();
    assert_eq!(
        serde_json::to_string(&geometry).unwrap(),
        r#"{"addressBits":16,"wordBytes":4,"lineBytes":16,"ways":2,"sets":4,"lines":8,"wordsPerLine":4,"offsetBits":4,"indexBits":2,"tagBits":10,"placement":"setAssociative"}"#
    );
    let placements = [
        Placement::DirectMapped,
        Placement::SetAssociative,
        Placement::FullyAssociative,
    ];
    let names: Vec<_> = placements
        .iter()
        .map(|placement| serde_json::to_string(placement).unwrap())
        .collect();
    assert_eq!(
        names,
        [
            r#""directMapped""#,
            r#""setAssociative""#,
            r#""fullyAssociative""#
        ]
    );

    let repair = Config {
        cache: CacheConfig {
            associativity: 3,
            ..config.cache
        },
        ..config
    }
    .repair()
    .unwrap();
    let value = serde_json::to_value(&repair).unwrap();
    assert_eq!(value["label"], "Open with 2-way instead");
    assert_eq!(value["config"], serde_json::to_value(config).unwrap());
}

#[test]
fn decoded_serializes_to_the_camel_case_shape() {
    let geometry = config_of(12, WordBytes::Four, 128, 16, 1)
        .validate()
        .unwrap();
    assert_eq!(
        serde_json::to_string(&geometry.decode(0x0A4).unwrap()).unwrap(),
        concat!(
            r#"{"addr":164,"addrText":"0x0A4","line":160,"lineText":"0x0A0","tag":1,"#,
            r#""tagText":"0x01","index":2,"offset":4,"word":1,"byteInWord":0,"aligned":true,"#,
            r#""bitRanges":{"tag":{"hi":11,"lo":7,"width":5},"index":{"hi":6,"lo":4,"width":3},"#,
            r#""offset":{"hi":3,"lo":0,"width":4}},"bits":"000010100100","fields":["#,
            r#"{"field":"tag","bits":"00001","valueText":"0x01","note":"5 bits","#,
            r#""range":{"hi":11,"lo":7,"width":5}},"#,
            r#"{"field":"index","bits":"010","valueText":"2","note":"3 bits → set 2","#,
            r#""range":{"hi":6,"lo":4,"width":3}},"#,
            r#"{"field":"offset","bits":"0100","valueText":"4","note":"4 bits → word 1, byte 0","#,
            r#""range":{"hi":3,"lo":0,"width":4}}]}"#
        )
    );
    // An empty field is absent, not a range with hi below lo.
    let fully_associative = config_of(12, WordBytes::Four, 128, 16, 8)
        .validate()
        .unwrap()
        .decode(0x0A4)
        .unwrap();
    assert_eq!(
        serde_json::to_value(fully_associative.bit_ranges).unwrap(),
        json!({"tag": {"hi": 11, "lo": 4, "width": 8}, "offset": {"hi": 3, "lo": 0, "width": 4}})
    );
    let one_byte_lines = config_of(12, WordBytes::One, 16, 1, 1)
        .validate()
        .unwrap()
        .decode(0x0A4)
        .unwrap();
    assert_eq!(
        serde_json::to_value(one_byte_lines.bit_ranges).unwrap(),
        json!({"tag": {"hi": 11, "lo": 4, "width": 8}, "index": {"hi": 3, "lo": 0, "width": 4}})
    );
    let names: Vec<_> = [FieldKind::Tag, FieldKind::Index, FieldKind::Offset]
        .iter()
        .map(|kind| serde_json::to_string(kind).unwrap())
        .collect();
    assert_eq!(names, [r#""tag""#, r#""index""#, r#""offset""#]);
}

#[test]
fn typed_inputs_serialize_with_a_status_tag() {
    let geometry = config_of(12, WordBytes::Four, 128, 16, 1)
        .validate()
        .unwrap();
    let decoded = |addr| serde_json::to_value(geometry.decode(addr).unwrap()).unwrap();
    let cases = [
        ("", json!({"status": "empty"})),
        (
            "0x0A4",
            json!({"status": "valid", "decoded": decoded(0x0A4)}),
        ),
        (
            "0x0A6",
            json!({
                "status": "misaligned",
                "decoded": decoded(0x0A6),
                "message": "0x0A6 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8."
            }),
        ),
        (
            "zz",
            json!({"status": "invalid", "message": "Type a 12-bit hex address, 0x000 to 0xFFF."}),
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(
            serde_json::to_value(geometry.read_address(text)).unwrap(),
            expected,
            "{text:?}"
        );
    }

    // A word is exact to 64 bits in JSON.
    let eight = words_of(WordBytes::Eight).validate().unwrap();
    assert_eq!(
        serde_json::to_string(&eight.read_word("-1")).unwrap(),
        r#"{"status":"valid","value":18446744073709551615,"text":"18,446,744,073,709,551,615"}"#
    );
    assert_eq!(
        serde_json::to_string(&eight.read_word("")).unwrap(),
        r#"{"status":"empty"}"#
    );
    assert_eq!(
        serde_json::to_value(eight.read_word("300 B")).unwrap(),
        json!({
            "status": "invalid",
            "message": "Type a whole number for an 8-byte word, \
                        −9,223,372,036,854,775,808 to 18,446,744,073,709,551,615."
        })
    );
}
