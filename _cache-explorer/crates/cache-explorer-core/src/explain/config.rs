//! The wording of the configuration: its summaries, the label of a repair
//! and the labels and captions of the configuration bar's options.

use crate::config::{
    AllocatePolicy, CacheConfig, Config, ConfigError, Replacement, WordBytes, WritePolicy,
};
use crate::format;
use crate::geometry::{Geometry, Placement};
use crate::limits::MIN_CYCLES;

/// The configuration's summary line, as the design's `cfgSummary` writes
/// it: the size, the line size and the placement, then the replacement
/// policy unless the cache is direct-mapped: "128 B · 16 B lines · 2-way ·
/// LRU". With `advanced`, then the write and allocate policies: " ·
/// write-back · allocate". The placement is read from the numbers as given,
/// so an invalid configuration has a summary too.
pub(crate) fn config_summary(config: &Config, advanced: bool) -> String {
    let cache = &config.cache;
    let mut text = format!(
        "{} B \u{b7} {} B lines \u{b7} {}",
        format::setting(cache.size_bytes),
        format::setting(cache.line_bytes),
        placement_name(cache)
    );
    if cache.placement() != Placement::DirectMapped {
        text.push_str(" \u{b7} ");
        text.push_str(replacement_name(cache.replacement));
    }
    if advanced {
        text.push_str(" \u{b7} ");
        text.push_str(match cache.write_policy {
            WritePolicy::WriteBack => "write-back",
            WritePolicy::WriteThrough => "write-through",
        });
        text.push_str(" \u{b7} ");
        text.push_str(match cache.allocate_policy {
            AllocatePolicy::WriteAllocate => "allocate",
            AllocatePolicy::WriteNoAllocate => "no-allocate",
        });
    }
    text
}

/// The short summary a shared run's loading screen opens with: the
/// placement and the replacement policy, then the size, as in "2-way LRU ·
/// 128 B". It starts a sentence, so its first word has a capital:
/// "Direct-mapped · 128 B", "Fully associative LRU · 128 B".
pub(crate) fn short_summary(config: &Config) -> String {
    let cache = &config.cache;
    let replacement = replacement_name(cache.replacement);
    let placement = match cache.placement() {
        Placement::DirectMapped => "Direct-mapped".to_owned(),
        Placement::FullyAssociative => format!("Fully associative {replacement}"),
        Placement::SetAssociative => {
            format!("{}-way {replacement}", format::setting(cache.associativity))
        }
    };
    format!("{placement} \u{b7} {} B", format::setting(cache.size_bytes))
}

/// The label of the action that opens `repaired`, the repair of a
/// configuration whose first broken rule is `first`: it names that rule's
/// repair, read from the repaired configuration and its `geometry`, as in
/// "Open with 2-way instead" or "Open as a 1,024 B cache instead".
pub(crate) fn repair_label(first: &ConfigError, repaired: &Config, geometry: &Geometry) -> String {
    let size = repaired.cache.size_bytes;
    match first {
        ConfigError::AddressBits { .. } => format!(
            "Open with {}-bit addresses instead",
            format::setting(repaired.address_bits)
        ),
        ConfigError::SizeNotPowerOfTwo { .. }
        | ConfigError::LineLargerThanCache { .. }
        | ConfigError::NoTagBits { .. }
        | ConfigError::TooManyLines { .. } => format!(
            "Open as {} {} B cache instead",
            format::a_or_an(size),
            format::setting(size)
        ),
        ConfigError::LineNotPowerOfTwo { .. }
        | ConfigError::LineSmallerThanWord { .. }
        | ConfigError::TooManyWords { .. } => format!(
            "Open with {} B lines instead",
            format::setting(repaired.cache.line_bytes)
        ),
        ConfigError::AssociativityZero
        | ConfigError::AssociativityNotPowerOfTwo { .. }
        | ConfigError::AssociativityTooLarge { .. } => match geometry.placement() {
            Placement::FullyAssociative => "Open as fully associative instead".to_owned(),
            Placement::DirectMapped => "Open as direct-mapped instead".to_owned(),
            Placement::SetAssociative => {
                format!("Open with {}-way instead", format::setting(geometry.ways()))
            }
        },
        ConfigError::Timing { .. } => {
            let (hit, miss) = repaired.timing.map_or((MIN_CYCLES, MIN_CYCLES), |timing| {
                (timing.hit_cycles, timing.miss_cycles)
            });
            format!(
                "Open with {} {}/{}-cycle timing instead",
                format::a_or_an(hit),
                format::setting(hit),
                format::setting(miss)
            )
        }
        ConfigError::SeedTooLarge { .. } => {
            format!("Open with seed {} instead", format::setting(repaired.seed))
        }
    }
}

/// The label of a cache-size or line-size option: "64 B".
pub(crate) fn bytes_option(bytes: u32) -> String {
    format!("{} B", format::setting(bytes))
}

/// The label of an associativity option other than Full: "Direct" for one
/// way, and otherwise "2-way".
pub(crate) fn ways_option(ways: u32) -> String {
    if ways == 1 {
        "Direct".to_owned()
    } else {
        format!("{}-way", format::setting(ways))
    }
}

/// The label of the fully associative option.
pub(crate) fn full_option() -> String {
    "Full".to_owned()
}

/// Why the associativity option with as many ways as a cache of `lines`
/// lines is disabled: "Same as Full for an 8-line cache."
pub(crate) fn same_as_full(lines: u32) -> String {
    format!(
        "Same as Full for {} {}-line cache.",
        format::a_or_an(lines),
        format::setting(lines)
    )
}

/// The label of a replacement option: "LRU", "FIFO", "Random".
pub(crate) fn replacement_option(replacement: Replacement) -> String {
    replacement_name(replacement).to_owned()
}

/// The label of an address-width option: "12-bit".
pub(crate) fn address_option(bits: u8) -> String {
    format!("{}-bit", format::setting(bits))
}

/// The label of a word-size option: "4 B".
pub(crate) fn word_option(word: WordBytes) -> String {
    bytes_option(word.bytes())
}

/// The label of a write-policy option: "Write-back", "Write-through".
pub(crate) fn write_option(policy: WritePolicy) -> String {
    match policy {
        WritePolicy::WriteBack => "Write-back",
        WritePolicy::WriteThrough => "Write-through",
    }
    .to_owned()
}

/// The label of an allocate-policy option, in the group headed "On a store
/// miss": "Allocate", "No allocate".
pub(crate) fn allocate_option(policy: AllocatePolicy) -> String {
    match policy {
        AllocatePolicy::WriteAllocate => "Allocate",
        AllocatePolicy::WriteNoAllocate => "No allocate",
    }
    .to_owned()
}

/// The placement as the summaries write it: "direct-mapped", "fully
/// associative" or "2-way".
fn placement_name(cache: &CacheConfig) -> String {
    match cache.placement() {
        Placement::DirectMapped => "direct-mapped".to_owned(),
        Placement::FullyAssociative => "fully associative".to_owned(),
        Placement::SetAssociative => format!("{}-way", format::setting(cache.associativity)),
    }
}

/// The replacement policy as the summaries and options name it.
fn replacement_name(replacement: Replacement) -> &'static str {
    match replacement {
        Replacement::Lru => "LRU",
        Replacement::Fifo => "FIFO",
        Replacement::Random => "Random",
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::disallowed_macros, reason = "assertions are how tests fail")]

    use super::*;
    use crate::config::Preset;
    use crate::explain::testing::{geometry, ADDRESS_BITS};
    use crate::limits::MAX_LINES;

    fn config(size: u32, ways: u32, replacement: Replacement) -> Config {
        Config {
            address_bits: ADDRESS_BITS,
            cache: CacheConfig {
                size_bytes: size,
                associativity: ways,
                replacement,
                ..Preset::DirectMapped8.config().cache
            },
            ..Preset::DirectMapped8.config()
        }
    }

    #[test]
    fn summaries_name_the_placement_policy_and_sizes() {
        let two_way = config(128, 2, Replacement::Lru);
        assert_eq!(
            config_summary(&two_way, false),
            "128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU"
        );
        assert_eq!(
            config_summary(&two_way, true),
            "128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU \u{b7} write-back \u{b7} allocate"
        );
        assert_eq!(short_summary(&two_way), "2-way LRU \u{b7} 128 B");
        let direct = config(128, 1, Replacement::Fifo);
        assert_eq!(
            config_summary(&direct, false),
            "128 B \u{b7} 16 B lines \u{b7} direct-mapped"
        );
        assert_eq!(short_summary(&direct), "Direct-mapped \u{b7} 128 B");
        let full = config(2_048, 128, Replacement::Random);
        assert_eq!(
            config_summary(&full, false),
            "2,048 B \u{b7} 16 B lines \u{b7} fully associative \u{b7} Random"
        );
        assert_eq!(
            short_summary(&full),
            "Fully associative Random \u{b7} 2,048 B"
        );
    }

    #[test]
    fn a_repair_label_names_the_repair_of_the_first_rule_broken() {
        let broken = config(128, 3, Replacement::Lru);
        let first = broken.validate().unwrap_err();
        let repaired = config(128, 2, Replacement::Lru);
        assert_eq!(
            repair_label(&first, &repaired, &geometry(&repaired)),
            "Open with 2-way instead"
        );
        // Twice as many lines as the limit, repaired to the limit, which
        // holds whatever the limit is tuned to.
        let size = MAX_LINES * 16;
        let too_many = ConfigError::TooManyLines {
            lines: 2 * MAX_LINES,
            size: 2 * size,
            line: 16,
        };
        let smaller = config(size, 1, Replacement::Lru);
        assert_eq!(
            repair_label(&too_many, &smaller, &geometry(&smaller)),
            format!(
                "Open as {} {} B cache instead",
                format::a_or_an(size),
                format::setting(size)
            )
        );
    }

    #[test]
    fn option_labels_are_the_designs() {
        assert_eq!(bytes_option(64), "64 B");
        assert_eq!(bytes_option(1_024), "1,024 B");
        assert_eq!(ways_option(1), "Direct");
        assert_eq!(ways_option(2), "2-way");
        assert_eq!(ways_option(16), "16-way");
        assert_eq!(full_option(), "Full");
        assert_eq!(replacement_option(Replacement::Fifo), "FIFO");
        assert_eq!(address_option(12), "12-bit");
        assert_eq!(word_option(WordBytes::Eight), "8 B");
        assert_eq!(write_option(WritePolicy::WriteThrough), "Write-through");
        assert_eq!(
            allocate_option(AllocatePolicy::WriteNoAllocate),
            "No allocate"
        );
        assert_eq!(same_as_full(8), "Same as Full for an 8-line cache.");
        assert_eq!(same_as_full(16), "Same as Full for a 16-line cache.");
        assert_eq!(same_as_full(1), "Same as Full for a 1-line cache.");
    }
}
