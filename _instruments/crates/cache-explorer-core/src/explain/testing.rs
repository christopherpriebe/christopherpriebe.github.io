//! Fixtures shared by the area modules' tests under `explain`: cache
//! geometries built at the tests' fixed address width, and the address,
//! line and index values built from raw values without validation. Each
//! area module keeps its own value-specific helpers alongside its tests
//! (`tag_of`, `word`, `value` and `choice` in `access`; `config` in
//! `config`).

use crate::config::{CacheConfig, Config, Preset};
use crate::geometry::Geometry;
use crate::units::{Addr, LineAddr, SetIndex, WayIndex};

/// The tests' address width: 12 bits. The lines
/// these tests expect are written for it, so the width is set here rather
/// than taken from `Config::default()`, and changing the default does not
/// break these tests.
pub(super) const ADDRESS_BITS: u8 = 12;

pub(super) fn geometry(config: &Config) -> Geometry {
    config.validate().unwrap()
}

/// A named configuration's geometry at the tests' address width.
pub(super) fn preset(preset: Preset) -> Geometry {
    geometry(&Config {
        address_bits: ADDRESS_BITS,
        ..preset.config()
    })
}

/// A 128 B cache of 16 B lines with `ways` ways, at the tests' address
/// width.
pub(super) fn ways(ways: u32) -> Geometry {
    geometry(&Config {
        address_bits: ADDRESS_BITS,
        cache: CacheConfig {
            associativity: ways,
            ..Preset::DirectMapped8.config().cache
        },
        ..Preset::DirectMapped8.config()
    })
}

/// The one-line cache: 16 B of 16 B lines, fully associative with one way,
/// at the tests' address width.
pub(super) fn one_line() -> Geometry {
    geometry(&Config {
        address_bits: ADDRESS_BITS,
        cache: CacheConfig {
            size_bytes: 16,
            ..Preset::DirectMapped8.config().cache
        },
        ..Preset::DirectMapped8.config()
    })
}

pub(super) fn addr(value: u32) -> Addr {
    Addr::new_unchecked(value)
}

pub(super) fn line(value: u32) -> LineAddr {
    LineAddr::new_unchecked(value)
}

pub(super) fn set(index: u32) -> SetIndex {
    SetIndex::new_unchecked(index)
}

pub(super) fn way(index: u32) -> WayIndex {
    WayIndex::new_unchecked(index)
}
