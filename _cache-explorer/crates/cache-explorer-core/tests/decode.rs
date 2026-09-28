//! Configuration, geometry and decoding: the defaults and the named
//! configurations, every validation rule with its message and its repair, the
//! geometry a valid configuration yields with its checked constructors, the
//! decoding of addresses under every placement, line size and address width,
//! typed addresses and values with their messages, and the serialized shapes.
//! And the configuration bar: every option with the configuration it
//! produces, Full kept across size and line changes, the timing fields, typed
//! timing numbers, and a configuration checked on its own.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::limits::{
    MAX_ADDRESS_BITS, MAX_CYCLES, MAX_LINES, MAX_SEED, MAX_WORDS_PER_LINE, MIN_ADDRESS_BITS,
    MIN_CYCLES,
};
use std::collections::BTreeSet;

use cache_explorer_core::{
    presets, validate_config, AccessError, Addr, AddressInput, AllocatePolicy, BitRange,
    CacheConfig, Config, ConfigCheck, ConfigError, ConfigField, ConfigGroupKind, ConfigNumberField,
    ConfigNumberInput, ConfigOption, Decoded, FieldKind, Geometry, MemoryInit, Op, Placement,
    Preset, Replacement, Session, SetIndex, Tag, Timing, WayIndex, WordBytes, WordIndex, WordInput,
    WritePolicy,
};
use serde_json::json;

use bar::{bar, labels, option, options, shape};

/// Comma-grouped from 1,000 on: how messages and labels print limits and
/// configuration values.
fn grouped(n: impl Into<u64>) -> String {
    let digits = n.into().to_string();
    let mut text = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// `Config::default()` with another cache geometry.
fn with_cache(size_bytes: u32, line_bytes: u32, associativity: u32) -> Config {
    let default = Config::default();
    Config {
        cache: CacheConfig {
            size_bytes,
            line_bytes,
            associativity,
            ..default.cache.clone()
        },
        ..default
    }
}

/// `config` fails with `error`, whose message is `message`, and repairs to a
/// valid configuration under `label`.
fn check_rule(config: &Config, error: &ConfigError, message: &str, label: &str) {
    assert_eq!(config.validate().err().as_ref(), Some(error), "{config:?}");
    assert_eq!(error.to_string(), message);
    let repair = config.repair();
    assert_eq!(
        repair.as_ref().map(|repair| repair.label.as_str()),
        Some(label),
        "{config:?}"
    );
    let repaired = repair.map(|repair| repair.config);
    assert!(
        repaired
            .as_ref()
            .is_some_and(|config| config.validate().is_ok()),
        "{repaired:?}"
    );
}

// Defaults and named configurations

#[test]
fn default_config_validates_as_eight_direct_mapped_sets() {
    let config = Config::default();
    let geometry = config.validate().unwrap();
    assert_eq!(geometry.address_bits(), config.address_bits);
    assert_eq!(geometry.word_bytes(), 4);
    assert_eq!(geometry.line_bytes(), 16);
    assert_eq!(
        (
            geometry.sets(),
            geometry.ways(),
            geometry.lines(),
            geometry.words_per_line()
        ),
        (8, 1, 8, 4)
    );
    assert_eq!((geometry.offset_bits(), geometry.index_bits()), (4, 3));
    assert_eq!(geometry.tag_bits(), config.address_bits - 7);
    assert_eq!(geometry.placement(), Placement::DirectMapped);
}

/// The one test that pins the default width: 12 bits, the design's, where the
/// spec's default is 16.
#[test]
fn default_address_width_is_twelve_bits() {
    assert_eq!(Config::default().address_bits, 12);
}

#[test]
fn default_config_keeps_the_spec_defaults_elsewhere() {
    let config = Config::default();
    assert_eq!(config.word_bytes, WordBytes::Four);
    assert_eq!(config.cache.replacement, Replacement::Lru);
    assert_eq!(config.cache.write_policy, WritePolicy::WriteBack);
    assert_eq!(config.cache.allocate_policy, AllocatePolicy::WriteAllocate);
    assert_eq!(config.timing, None);
    assert_eq!(config.seed, 12345);
    assert!(!config.advanced);
    assert_eq!(config.memory_init, MemoryInit::Zero);
    assert!(config.fits_basics());
}

#[test]
fn first_visit_is_direct_mapped_8_with_pattern_memory() {
    let first_visit = Config::first_visit();
    assert_eq!(first_visit, Preset::DirectMapped8.config());
    assert_eq!(
        first_visit,
        Config {
            memory_init: MemoryInit::Pattern,
            ..Config::default()
        }
    );
}

#[test]
fn named_configurations_follow_their_table() {
    let default = Config::default();
    let rows: Vec<_> = presets()
        .into_iter()
        .map(|preset| {
            let config = preset.config();
            let geometry = config.validate().unwrap();
            assert_eq!(config.address_bits, default.address_bits, "{}", preset.id());
            assert_eq!(config.word_bytes, default.word_bytes, "{}", preset.id());
            assert_eq!(config.seed, default.seed, "{}", preset.id());
            assert_eq!(config.memory_init, MemoryInit::Pattern, "{}", preset.id());
            assert_eq!(
                config.cache.replacement,
                Replacement::Lru,
                "{}",
                preset.id()
            );
            assert!(config.fits_basics(), "{}", preset.id());
            assert!(!config.advanced, "{}", preset.id());
            (
                preset.id(),
                config.cache.size_bytes,
                config.cache.line_bytes,
                geometry.ways(),
                geometry.sets(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("direct-mapped-8", 128, 16, 1, 8),
            ("two-way-lru", 128, 16, 2, 4),
            ("four-way-write-back", 256, 16, 4, 4),
        ]
    );
}

#[test]
fn valid_configurations_need_no_repair() {
    assert_eq!(Config::default().repair(), None);
    for preset in presets() {
        assert_eq!(preset.config().repair(), None, "{}", preset.id());
    }
}

#[test]
fn basics_is_write_back_write_allocate_and_untimed() {
    let advanced = Config {
        cache: CacheConfig {
            write_policy: WritePolicy::WriteThrough,
            allocate_policy: AllocatePolicy::WriteNoAllocate,
            ..Config::default().cache
        },
        timing: Some(Timing {
            hit_cycles: 1,
            miss_cycles: 20,
        }),
        advanced: true,
        ..Config::first_visit()
    };
    assert!(!advanced.fits_basics());
    let basics = advanced.to_basics();
    assert!(basics.fits_basics());
    assert_eq!(basics.cache.write_policy, WritePolicy::WriteBack);
    assert_eq!(basics.cache.allocate_policy, AllocatePolicy::WriteAllocate);
    assert_eq!(basics.timing, None);
    // Everything else is kept, the presentation flag included.
    assert_eq!(
        Config {
            cache: advanced.cache.clone(),
            timing: advanced.timing,
            ..basics
        },
        advanced
    );

    for (write_policy, allocate_policy, timing) in [
        (
            WritePolicy::WriteThrough,
            AllocatePolicy::WriteAllocate,
            None,
        ),
        (
            WritePolicy::WriteBack,
            AllocatePolicy::WriteNoAllocate,
            None,
        ),
        (
            WritePolicy::WriteBack,
            AllocatePolicy::WriteAllocate,
            Some(Timing {
                hit_cycles: 1,
                miss_cycles: 20,
            }),
        ),
    ] {
        let config = Config {
            cache: CacheConfig {
                write_policy,
                allocate_policy,
                ..Config::default().cache
            },
            timing,
            ..Config::default()
        };
        assert!(!config.fits_basics(), "{config:?}");
    }
}

// Summaries

#[test]
fn summaries_name_the_size_the_placement_and_the_policy() {
    let fully_associative = Config {
        cache: CacheConfig {
            replacement: Replacement::Fifo,
            ..with_cache(128, 16, 8).cache
        },
        ..Config::default()
    };
    let large = Config {
        cache: CacheConfig {
            replacement: Replacement::Random,
            ..with_cache(2_048, 128, 2).cache
        },
        ..Config::default()
    };
    let cases = [
        (
            Preset::DirectMapped8.config(),
            "128 B · 16 B lines · direct-mapped",
            "Direct-mapped · 128 B",
        ),
        (
            Preset::TwoWayLru.config(),
            "128 B · 16 B lines · 2-way · LRU",
            "2-way LRU · 128 B",
        ),
        (
            Preset::FourWayWriteBack.config(),
            "256 B · 16 B lines · 4-way · LRU",
            "4-way LRU · 256 B",
        ),
        (
            fully_associative,
            "128 B · 16 B lines · fully associative · FIFO",
            "Fully associative FIFO · 128 B",
        ),
        // One line in one set: fully associative, as its geometry says.
        (
            with_cache(16, 16, 1),
            "16 B · 16 B lines · fully associative · LRU",
            "Fully associative LRU · 16 B",
        ),
        (
            large,
            "2,048 B · 128 B lines · 2-way · Random",
            "2-way Random · 2,048 B",
        ),
    ];
    for (config, summary, short_summary) in cases {
        assert_eq!(config.summary(false), summary, "{config:?}");
        assert_eq!(config.short_summary(), short_summary, "{config:?}");
    }
}

#[test]
fn advanced_summaries_add_the_write_and_allocate_policies() {
    assert_eq!(
        Config::default().summary(true),
        "128 B · 16 B lines · direct-mapped · write-back · allocate"
    );
    let two_way = Preset::TwoWayLru.config();
    let config = Config {
        cache: CacheConfig {
            write_policy: WritePolicy::WriteThrough,
            allocate_policy: AllocatePolicy::WriteNoAllocate,
            ..two_way.cache.clone()
        },
        timing: Some(Timing {
            hit_cycles: 1,
            miss_cycles: 20,
        }),
        ..two_way
    };
    assert_eq!(
        config.summary(true),
        "128 B · 16 B lines · 2-way · LRU · write-through · no-allocate"
    );
    // Timing is never in a summary, and the policies only in Advanced.
    assert_eq!(config.summary(false), "128 B · 16 B lines · 2-way · LRU");
    assert_eq!(config.short_summary(), "2-way LRU · 128 B");
}

#[test]
fn an_invalid_configuration_is_summarised_as_given() {
    assert_eq!(
        with_cache(128, 16, 16).summary(false),
        "128 B · 16 B lines · 16-way · LRU"
    );
    assert_eq!(
        with_cache(100, 0, 1).summary(false),
        "100 B · 0 B lines · direct-mapped"
    );
}

// Validation rules, their messages and their repairs

/// The caps on lines and words are checked as soon as the cache's shape is
/// well formed, before its associativity and its tag: a cache with too many
/// lines is not told which associativities it could have, and a fully
/// associative cache is never refused for having no tag bits.
#[test]
fn rules_are_checked_in_order_and_the_first_failure_wins() {
    // At the narrowest width one line of this size covers all of memory, and
    // it holds more words than a line may.
    let long_line = 1_u32 << MIN_ADDRESS_BITS;
    let too_many_lines = (MAX_LINES + 1).next_power_of_two();
    let mut config = Config {
        address_bits: MAX_ADDRESS_BITS + 1,
        cache: CacheConfig {
            size_bytes: 100,
            line_bytes: 24,
            associativity: 0,
            ..Config::default().cache
        },
        timing: Some(Timing {
            hit_cycles: 0,
            miss_cycles: 0,
        }),
        seed: u64::MAX,
        ..Config::default()
    };
    let first_error = |config: &Config| config.validate().err();
    assert_eq!(
        first_error(&config),
        Some(ConfigError::AddressBits {
            got: MAX_ADDRESS_BITS + 1
        })
    );
    config.address_bits = MIN_ADDRESS_BITS;
    assert_eq!(
        first_error(&config),
        Some(ConfigError::SizeNotPowerOfTwo { got: 100 })
    );
    config.cache.size_bytes = too_many_lines * long_line;
    assert_eq!(
        first_error(&config),
        Some(ConfigError::LineNotPowerOfTwo { got: 24 })
    );
    config.cache.line_bytes = long_line;
    assert_eq!(
        first_error(&config),
        Some(ConfigError::TooManyLines {
            lines: too_many_lines,
            size: too_many_lines * long_line,
            line: long_line
        })
    );
    // One line: the associativity and the tag are wrong too.
    config.cache.size_bytes = long_line;
    assert_eq!(
        first_error(&config),
        Some(ConfigError::TooManyWords {
            words: long_line / 4,
            line: long_line,
            word: 4
        })
    );
    // 16 lines of 16 B, direct-mapped once the associativity is 1.
    config.cache.size_bytes = 256;
    config.cache.line_bytes = 16;
    assert_eq!(first_error(&config), Some(ConfigError::AssociativityZero));
    config.cache.associativity = 3;
    assert_eq!(
        first_error(&config),
        Some(ConfigError::AssociativityNotPowerOfTwo { got: 3, lines: 16 })
    );
    config.cache.associativity = 1;
    assert_eq!(
        first_error(&config),
        Some(ConfigError::NoTagBits {
            bits: MIN_ADDRESS_BITS
        })
    );
    config.address_bits = MAX_ADDRESS_BITS;
    assert_eq!(
        first_error(&config),
        Some(ConfigError::Timing {
            field: ConfigNumberField::HitCycles,
            got: 0
        })
    );
    config.timing = None;
    assert_eq!(
        first_error(&config),
        Some(ConfigError::SeedTooLarge { got: u64::MAX })
    );
    config.seed = MAX_SEED;
    assert_eq!(first_error(&config), None);
}

#[test]
fn address_width_outside_its_range() {
    let message = |got: u8| {
        format!(
            "Address width must be between {} and {} bits; got {got}.",
            grouped(MIN_ADDRESS_BITS),
            grouped(MAX_ADDRESS_BITS)
        )
    };
    for got in [0, MIN_ADDRESS_BITS - 1] {
        check_rule(
            &Config {
                address_bits: got,
                ..Config::default()
            },
            &ConfigError::AddressBits { got },
            &message(got),
            &format!("Open with {MIN_ADDRESS_BITS}-bit addresses instead"),
        );
    }
    for got in [MAX_ADDRESS_BITS + 1, u8::MAX] {
        check_rule(
            &Config {
                address_bits: got,
                ..Config::default()
            },
            &ConfigError::AddressBits { got },
            &message(got),
            &format!("Open with {MAX_ADDRESS_BITS}-bit addresses instead"),
        );
    }
}

#[test]
fn cache_size_that_is_not_a_power_of_two() {
    for (got, got_text, line, repaired) in [
        (100, "100", 16, "a 64 B"),
        (1_000, "1,000", 32, "a 512 B"),
        (0, "0", 16, "a 16 B"),
    ] {
        check_rule(
            &with_cache(got, line, 1),
            &ConfigError::SizeNotPowerOfTwo { got },
            &format!(
                "Cache size must be a power of two, such as 64, 128 or 256 bytes; got {got_text}."
            ),
            &format!("Open as {repaired} cache instead"),
        );
    }
}

#[test]
fn line_size_that_is_not_a_power_of_two() {
    for (got, repaired) in [(24, 16), (0, 4)] {
        check_rule(
            &with_cache(128, got, 1),
            &ConfigError::LineNotPowerOfTwo { got },
            &format!("Line size must be a power of two, such as 8, 16 or 32 bytes; got {got}."),
            &format!("Open with {repaired} B lines instead"),
        );
    }
}

#[test]
fn line_smaller_than_a_word() {
    check_rule(
        &with_cache(128, 2, 1),
        &ConfigError::LineSmallerThanWord { line: 2, word: 4 },
        "A line must hold at least one word: 2 B lines are smaller than 4 B words.",
        "Open with 4 B lines instead",
    );
}

/// Repairing rule by rule would loop here: raising the line to a word makes it
/// larger than the cache, and shrinking it to the cache makes it smaller than a
/// word. The one-pass repair grows the cache instead.
#[test]
fn size_two_word_four_line_one_repairs_in_one_pass() {
    let config = with_cache(2, 1, 1);
    check_rule(
        &config,
        &ConfigError::LineSmallerThanWord { line: 1, word: 4 },
        "A line must hold at least one word: 1 B lines are smaller than 4 B words.",
        "Open with 4 B lines instead",
    );
    let repaired = config.repair().unwrap().config;
    assert_eq!(
        (
            repaired.cache.size_bytes,
            repaired.cache.line_bytes,
            repaired.cache.associativity
        ),
        (4, 4, 1)
    );
}

#[test]
fn line_larger_than_the_cache() {
    check_rule(
        &with_cache(8, 16, 1),
        &ConfigError::LineLargerThanCache { line: 16, size: 8 },
        "The cache must hold at least one line: an 8 B cache cannot hold 16 B lines.",
        "Open as a 16 B cache instead",
    );
    check_rule(
        &with_cache(4, 16, 1),
        &ConfigError::LineLargerThanCache { line: 16, size: 4 },
        "The cache must hold at least one line: a 4 B cache cannot hold 16 B lines.",
        "Open as a 16 B cache instead",
    );
}

#[test]
fn associativity_zero() {
    check_rule(
        &with_cache(128, 16, 0),
        &ConfigError::AssociativityZero,
        "Associativity must be at least 1.",
        "Open as direct-mapped instead",
    );
    // A one-line cache with one way is fully associative: it has one set.
    check_rule(
        &with_cache(16, 16, 0),
        &ConfigError::AssociativityZero,
        "Associativity must be at least 1.",
        "Open as fully associative instead",
    );
}

#[test]
fn associativity_that_is_not_a_power_of_two() {
    let cases = [
        (
            128,
            3,
            8,
            "1, 2, 4 or 8 (fully associative) for an 8-line cache",
            "Open with 2-way instead",
        ),
        (
            128,
            6,
            8,
            "1, 2, 4 or 8 (fully associative) for an 8-line cache",
            "Open with 4-way instead",
        ),
        (
            128,
            12,
            8,
            "1, 2, 4 or 8 (fully associative) for an 8-line cache",
            "Open as fully associative instead",
        ),
        (
            256,
            3,
            16,
            "1, 2, 4, 8 or 16 (fully associative) for a 16-line cache",
            "Open with 2-way instead",
        ),
        (
            32,
            3,
            2,
            "1 or 2 (fully associative) for a 2-line cache",
            "Open as fully associative instead",
        ),
        (
            16,
            3,
            1,
            "1 (fully associative) for a 1-line cache",
            "Open as fully associative instead",
        ),
    ];
    for (size, got, lines, choices, label) in cases {
        check_rule(
            &with_cache(size, 16, got),
            &ConfigError::AssociativityNotPowerOfTwo { got, lines },
            &format!("Associativity {got} is not a power of two. It must be {choices}."),
            label,
        );
    }
}

#[test]
fn associativity_above_the_line_count() {
    check_rule(
        &with_cache(128, 16, 16),
        &ConfigError::AssociativityTooLarge { got: 16, lines: 8 },
        "An 8-line cache can have at most 8 ways; got 16.",
        "Open as fully associative instead",
    );
    check_rule(
        &with_cache(64, 16, 8),
        &ConfigError::AssociativityTooLarge { got: 8, lines: 4 },
        "A 4-line cache can have at most 4 ways; got 8.",
        "Open as fully associative instead",
    );
    check_rule(
        &with_cache(16, 16, 2),
        &ConfigError::AssociativityTooLarge { got: 2, lines: 1 },
        "A 1-line cache can have at most 1 way; got 2.",
        "Open as fully associative instead",
    );
}

#[test]
fn no_tag_bits_when_one_way_covers_memory() {
    let message =
        "With 8-bit addresses one way of this cache would cover all of memory, leaving no tag \
                   bits. Use wider addresses or a smaller cache.";
    // 8 − 4 − 5 = −1 tag bits: the width must be computed signed.
    let negative = Config {
        address_bits: 8,
        ..with_cache(512, 32, 1)
    };
    check_rule(
        &negative,
        &ConfigError::NoTagBits { bits: 8 },
        message,
        "Open as a 128 B cache instead",
    );
    let repaired = negative.repair().unwrap().config.validate().unwrap();
    assert_eq!(
        (
            repaired.tag_bits(),
            repaired.index_bits(),
            repaired.offset_bits()
        ),
        (1, 2, 5)
    );

    // 8 − 4 − 4 = 0 tag bits.
    let zero = Config {
        address_bits: 8,
        ..with_cache(256, 16, 1)
    };
    check_rule(
        &zero,
        &ConfigError::NoTagBits { bits: 8 },
        message,
        "Open as a 128 B cache instead",
    );

    // A set-associative cache keeps its ways while it shrinks.
    let two_way = Config {
        address_bits: 8,
        ..with_cache(512, 32, 2)
    };
    check_rule(
        &two_way,
        &ConfigError::NoTagBits { bits: 8 },
        message,
        "Open as a 256 B cache instead",
    );
    assert_eq!(two_way.repair().unwrap().config.cache.associativity, 2);
}

#[test]
fn more_lines_than_fit_on_screen() {
    // The smallest power-of-two line count above the limit, and the largest
    // within it.
    let too_many = (MAX_LINES + 1).next_power_of_two();
    let fitting = too_many / 2;
    let message = format!(
        "Keep the cache to at most {} lines so every set fits on screen; {} B ÷ 16 B = {} lines.",
        grouped(MAX_LINES),
        grouped(too_many * 16),
        grouped(too_many)
    );
    let label = format!("Open as a {} B cache instead", grouped(fitting * 16));
    let error = ConfigError::TooManyLines {
        lines: too_many,
        size: too_many * 16,
        line: 16,
    };

    let direct_mapped = Config {
        address_bits: MAX_ADDRESS_BITS,
        ..with_cache(too_many * 16, 16, 1)
    };
    check_rule(&direct_mapped, &error, &message, &label);
    assert_eq!(
        direct_mapped.repair().unwrap().config.cache.associativity,
        1
    );

    // A fully associative cache stays fully associative.
    let fully_associative = Config {
        address_bits: MAX_ADDRESS_BITS,
        ..with_cache(too_many * 16, 16, too_many)
    };
    check_rule(&fully_associative, &error, &message, &label);
    let repaired = fully_associative
        .repair()
        .unwrap()
        .config
        .validate()
        .unwrap();
    assert_eq!(
        (repaired.ways(), repaired.placement()),
        (fitting, Placement::FullyAssociative)
    );

    // Too many lines is the first failure even when the associativity is
    // wrong too, since no associativity would make such a cache valid.
    check_rule(
        &Config {
            address_bits: MAX_ADDRESS_BITS,
            ..with_cache(too_many * 16, 16, 3)
        },
        &error,
        &message,
        &label,
    );
}

#[test]
fn more_words_per_line_than_a_row_holds() {
    let too_many = (MAX_WORDS_PER_LINE + 1).next_power_of_two();
    let line = too_many * 4;
    check_rule(
        &Config {
            address_bits: MAX_ADDRESS_BITS,
            ..with_cache(line, line, 1)
        },
        &ConfigError::TooManyWords {
            words: too_many,
            line,
            word: 4,
        },
        &format!(
            "A line can hold at most {} words; {} B ÷ 4 B = {}.",
            grouped(MAX_WORDS_PER_LINE),
            grouped(line),
            grouped(too_many)
        ),
        &format!("Open with {} B lines instead", grouped(too_many / 2 * 4)),
    );
}

/// A one-line cache is fully associative, so once its line is within the
/// words cap it keeps a tag bit at any width. A line too long for that cap
/// is refused for its length, even where it would also cover all of memory,
/// and the repair shortens the line and keeps the cache fully associative.
#[test]
fn a_line_too_long_for_a_row_is_refused_before_the_tag_rule() {
    let line = 1_u32 << MIN_ADDRESS_BITS;
    let words = line / 8;
    assert!(words > MAX_WORDS_PER_LINE);
    let config = Config {
        address_bits: MIN_ADDRESS_BITS,
        word_bytes: WordBytes::Eight,
        ..with_cache(line, line, 1)
    };
    let shorter = (MAX_WORDS_PER_LINE + 1).next_power_of_two() / 2 * 8;
    check_rule(
        &config,
        &ConfigError::TooManyWords {
            words,
            line,
            word: 8,
        },
        &format!(
            "A line can hold at most {} words; {} B ÷ 8 B = {}.",
            grouped(MAX_WORDS_PER_LINE),
            grouped(line),
            grouped(words)
        ),
        &format!("Open with {} B lines instead", grouped(shorter)),
    );
    let repaired = config.repair().unwrap().config;
    let geometry = repaired.validate().unwrap();
    assert_eq!(repaired.cache.size_bytes, line);
    assert_eq!(
        (geometry.line_bytes(), geometry.ways(), geometry.placement()),
        (shorter, line / shorter, Placement::FullyAssociative)
    );
}

/// Shortening the lines of a fully associative cache gives it more lines,
/// and its ways follow them, as the configuration bar's line options do.
#[test]
fn shorter_lines_keep_a_fully_associative_cache_fully_associative() {
    let too_many = (MAX_WORDS_PER_LINE + 1).next_power_of_two();
    let line = too_many * 4;
    let size = line * 8;
    let config = with_cache(size, line, 8);
    let shorter = too_many / 2 * 4;
    let repair = config.repair().unwrap();
    assert_eq!(
        repair.label,
        format!("Open with {} B lines instead", grouped(shorter))
    );
    let geometry = repair.config.validate().unwrap();
    assert_eq!(repair.config.cache.size_bytes, size);
    assert_eq!(
        (geometry.line_bytes(), geometry.ways(), geometry.placement()),
        (shorter, size / shorter, Placement::FullyAssociative)
    );
}

#[test]
fn timing_outside_its_range() {
    let timed = |hit_cycles, miss_cycles| Config {
        timing: Some(Timing {
            hit_cycles,
            miss_cycles,
        }),
        ..Config::default()
    };
    let range = format!(
        "between {} and {} cycles",
        grouped(MIN_CYCLES),
        grouped(MAX_CYCLES)
    );
    check_rule(
        &timed(MIN_CYCLES - 1, 20),
        &ConfigError::Timing {
            field: ConfigNumberField::HitCycles,
            got: MIN_CYCLES - 1,
        },
        &format!("Hit time must be {range}; got {}.", grouped(MIN_CYCLES - 1)),
        &format!(
            "Open with a {}/20-cycle timing instead",
            grouped(MIN_CYCLES)
        ),
    );
    check_rule(
        &timed(1, MAX_CYCLES + 1),
        &ConfigError::Timing {
            field: ConfigNumberField::MissCycles,
            got: MAX_CYCLES + 1,
        },
        &format!(
            "Miss penalty must be {range}; got {}.",
            grouped(MAX_CYCLES + 1)
        ),
        &format!("Open with a 1/{}-cycle timing instead", grouped(MAX_CYCLES)),
    );
    // The hit time is checked first, and both numbers are repaired.
    check_rule(
        &timed(0, 0),
        &ConfigError::Timing {
            field: ConfigNumberField::HitCycles,
            got: 0,
        },
        &format!("Hit time must be {range}; got 0."),
        &format!(
            "Open with a {0}/{0}-cycle timing instead",
            grouped(MIN_CYCLES)
        ),
    );
    check_rule(
        &timed(8, 0),
        &ConfigError::Timing {
            field: ConfigNumberField::MissCycles,
            got: 0,
        },
        &format!("Miss penalty must be {range}; got 0."),
        &format!(
            "Open with an 8/{}-cycle timing instead",
            grouped(MIN_CYCLES)
        ),
    );
}

#[test]
fn seed_that_would_not_survive_a_javascript_number() {
    for got in [MAX_SEED + 1, u64::MAX] {
        check_rule(
            &Config {
                seed: got,
                ..Config::default()
            },
            &ConfigError::SeedTooLarge { got },
            &format!(
                "The seed must be at most {}; got {}.",
                grouped(MAX_SEED),
                grouped(got)
            ),
            &format!("Open with seed {} instead", grouped(MAX_SEED)),
        );
    }
    assert!(Config {
        seed: MAX_SEED,
        ..Config::default()
    }
    .validate()
    .is_ok());
}

#[test]
fn repair_keeps_what_was_valid() {
    let config = Config {
        word_bytes: WordBytes::Two,
        cache: CacheConfig {
            size_bytes: 100,
            line_bytes: 16,
            associativity: 2,
            replacement: Replacement::Fifo,
            write_policy: WritePolicy::WriteThrough,
            allocate_policy: AllocatePolicy::WriteNoAllocate,
        },
        timing: Some(Timing {
            hit_cycles: 2,
            miss_cycles: 15,
        }),
        seed: 7,
        advanced: true,
        memory_init: MemoryInit::Pattern,
        ..Config::default()
    };
    let repaired = config.repair().unwrap().config;
    assert_eq!(repaired.cache.size_bytes, 64);
    assert_eq!(
        repaired,
        Config {
            cache: CacheConfig {
                size_bytes: 64,
                ..config.cache.clone()
            },
            ..config
        }
    );
}

// Geometry

#[test]
fn geometry_follows_the_address_split() {
    // (address bits, word, size, line, associativity) → (tag, index, offset, placement)
    let cases = [
        (
            (12, WordBytes::Four, 128, 16, 1),
            (5, 3, 4, Placement::DirectMapped),
        ),
        (
            (16, WordBytes::Four, 128, 16, 1),
            (9, 3, 4, Placement::DirectMapped),
        ),
        (
            (12, WordBytes::Four, 128, 16, 2),
            (6, 2, 4, Placement::SetAssociative),
        ),
        (
            (12, WordBytes::Four, 256, 16, 4),
            (6, 2, 4, Placement::SetAssociative),
        ),
        (
            (12, WordBytes::Four, 128, 16, 8),
            (8, 0, 4, Placement::FullyAssociative),
        ),
        (
            (8, WordBytes::One, 16, 1, 1),
            (4, 4, 0, Placement::DirectMapped),
        ),
        (
            (32, WordBytes::Eight, 1_024, 64, 4),
            (24, 2, 6, Placement::SetAssociative),
        ),
    ];
    for ((address_bits, word_bytes, size, line, associativity), expected) in cases {
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
        assert_eq!(geometry.lines(), size / line);
        assert_eq!(geometry.sets() * geometry.ways(), geometry.lines());
        assert_eq!(geometry.words_per_line(), line / word_bytes.bytes());
        assert_eq!(
            geometry.tag_bits() + geometry.index_bits() + geometry.offset_bits(),
            address_bits
        );
    }
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
            ..config.clone()
        }
        .validate()
        .unwrap();
        assert_eq!(
            (geometry.sets(), geometry.ways(), geometry.index_bits()),
            (1, lines, 0)
        );
        assert_eq!(geometry.tag_bits(), config.address_bits - 4);
        assert_eq!(geometry.placement(), Placement::FullyAssociative);
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

#[test]
fn decoding_follows_the_address_table() {
    let rows = [
        // 12-bit, 128 B direct-mapped, 16 B lines
        (
            config_of(12, WordBytes::Four, 128, 16, 1),
            0x0A4,
            (
                "00001 010 0100",
                (11, 7, "0x01"),
                Some((6, 4, 2)),
                Some((3, 0, 4)),
            ),
        ),
        // 16-bit, the spec's default width, same cache
        (
            config_of(16, WordBytes::Four, 128, 16, 1),
            0x00A4,
            (
                "000000001 010 0100",
                (15, 7, "0x001"),
                Some((6, 4, 2)),
                Some((3, 0, 4)),
            ),
        ),
        // 12-bit, 128 B 2-way (4 sets)
        (
            config_of(12, WordBytes::Four, 128, 16, 2),
            0x1A0,
            (
                "000110 10 0000",
                (11, 6, "0x06"),
                Some((5, 4, 2)),
                Some((3, 0, 0)),
            ),
        ),
        // 12-bit, 128 B fully associative
        (
            config_of(12, WordBytes::Four, 128, 16, 8),
            0x0A4,
            ("00001010 0100", (11, 4, "0x0A"), None, Some((3, 0, 4))),
        ),
    ];
    for (config, addr, (bits, (tag_hi, tag_lo, tag_text), index, offset)) in rows {
        let decoded = config.validate().unwrap().decode(addr).unwrap();
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
        assert_eq!(decoded.bits, bits.replace(' ', ""));
    }
}

/// Checks `decoded`, the decoding of `addr` under `geometry`, against the
/// geometry: the fields cover the address, most significant first, their
/// ranges follow the widths, and their values put back together give the
/// address.
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
        assert_eq!(field.range.hi - field.range.lo + 1, field.range.width);
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

#[test]
fn addresses_beyond_the_width_do_not_decode() {
    let cases = [
        (8, 0x100, "0x100 does not fit an 8-bit address (0x00–0xFF)."),
        (
            12,
            0x1000,
            "0x1000 does not fit a 12-bit address (0x000–0xFFF).",
        ),
        (
            12,
            u32::MAX,
            "0xFFFFFFFF does not fit a 12-bit address (0x000–0xFFF).",
        ),
        (
            16,
            0x1_0000,
            "0x10000 does not fit a 16-bit address (0x0000–0xFFFF).",
        ),
        (
            18,
            0x4_0000,
            "0x40000 does not fit an 18-bit address (0x00000–0x3FFFF).",
        ),
    ];
    for (address_bits, addr, message) in cases {
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

#[test]
fn misaligned_typed_addresses_name_the_aligned_ones() {
    let cases = [
        (
            12,
            WordBytes::Four,
            0x0A6,
            "0x0A6 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8.",
        ),
        (
            12,
            WordBytes::Four,
            0x0A5,
            "0x0A5 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8.",
        ),
        // The next word would not fit the width.
        (
            12,
            WordBytes::Four,
            0xFFE,
            "0xFFE is not aligned to a 4-byte word; use 0xFFC.",
        ),
        (
            16,
            WordBytes::Two,
            0x00A5,
            "0x00A5 is not aligned to a 2-byte word; use 0x00A4 or 0x00A6.",
        ),
        (
            12,
            WordBytes::Eight,
            0x0A4,
            "0x0A4 is not aligned to an 8-byte word; use 0x0A0 or 0x0A8.",
        ),
        (
            8,
            WordBytes::Eight,
            0xFC,
            "0xFC is not aligned to an 8-byte word; use 0xF8.",
        ),
    ];
    for (address_bits, word_bytes, addr, message) in cases {
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
        assert_eq!(error.to_string(), message);
    }
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
    let others = [
        (
            WordBytes::One,
            "Type a whole number for a 1-byte word, −128 to 255.",
        ),
        (
            WordBytes::Two,
            "Type a whole number for a 2-byte word, −32,768 to 65,535.",
        ),
        (
            WordBytes::Eight,
            "Type a whole number for an 8-byte word, \
             −9,223,372,036,854,775,808 to 18,446,744,073,709,551,615.",
        ),
    ];
    for (word_bytes, message) in others {
        let geometry = words_of(word_bytes).validate().unwrap();
        assert_eq!(
            geometry.read_word("seven"),
            WordInput::Invalid {
                message: message.to_owned()
            }
        );
    }
}

// Serialized shapes

#[test]
fn config_serializes_to_the_camel_case_shape() {
    let config = Config {
        address_bits: 16,
        word_bytes: WordBytes::Two,
        cache: CacheConfig {
            size_bytes: 256,
            line_bytes: 8,
            associativity: 2,
            replacement: Replacement::Fifo,
            write_policy: WritePolicy::WriteThrough,
            allocate_policy: AllocatePolicy::WriteNoAllocate,
        },
        timing: Some(Timing {
            hit_cycles: 1,
            miss_cycles: 20,
        }),
        seed: 7,
        advanced: true,
        memory_init: MemoryInit::Pattern,
    };
    let json = r#"{"addressBits":16,"wordBytes":2,"cache":{"sizeBytes":256,"lineBytes":8,"associativity":2,"replacement":"fifo","writePolicy":"writeThrough","allocatePolicy":"writeNoAllocate"},"timing":{"hitCycles":1,"missCycles":20},"seed":7,"advanced":true,"memoryInit":"pattern"}"#;
    assert_eq!(serde_json::to_string(&config).unwrap(), json);
    assert_eq!(serde_json::from_str::<Config>(json).unwrap(), config);
}

#[test]
fn untimed_zero_memory_configs_omit_timing_and_memory_init() {
    let config = Config {
        address_bits: 16,
        word_bytes: WordBytes::Four,
        cache: CacheConfig {
            size_bytes: 128,
            line_bytes: 16,
            associativity: 1,
            replacement: Replacement::Random,
            write_policy: WritePolicy::WriteBack,
            allocate_policy: AllocatePolicy::WriteAllocate,
        },
        timing: None,
        seed: 1,
        advanced: false,
        memory_init: MemoryInit::Zero,
    };
    let json = r#"{"addressBits":16,"wordBytes":4,"cache":{"sizeBytes":128,"lineBytes":16,"associativity":1,"replacement":"random","writePolicy":"writeBack","allocatePolicy":"writeAllocate"},"seed":1,"advanced":false}"#;
    assert_eq!(serde_json::to_string(&config).unwrap(), json);
    assert_eq!(serde_json::from_str::<Config>(json).unwrap(), config);
}

#[test]
fn optional_fields_take_their_defaults_when_absent() {
    let json = r#"{"addressBits":16,"wordBytes":4,"cache":{"sizeBytes":128,"lineBytes":16,"associativity":1,"replacement":"lru","writePolicy":"writeBack","allocatePolicy":"writeAllocate"},"seed":12345}"#;
    let config: Config = serde_json::from_str(json).unwrap();
    assert_eq!(config.timing, None);
    assert!(!config.advanced);
    assert_eq!(config.memory_init, MemoryInit::Zero);
}

#[test]
fn every_named_configuration_round_trips_through_json() {
    for preset in presets() {
        let config = preset.config();
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(
            serde_json::from_str::<Config>(&json).unwrap(),
            config,
            "{}",
            preset.id()
        );
    }
}

#[test]
fn enum_values_are_camel_case_strings() {
    let names = |values: Vec<serde_json::Value>| -> Vec<String> {
        values
            .into_iter()
            .map(|value| value.as_str().unwrap_or_default().to_owned())
            .collect()
    };
    let replacements = [Replacement::Lru, Replacement::Fifo, Replacement::Random];
    assert_eq!(
        names(
            replacements
                .iter()
                .map(|value| serde_json::to_value(value).unwrap())
                .collect()
        ),
        ["lru", "fifo", "random"]
    );
    let write_policies = [WritePolicy::WriteBack, WritePolicy::WriteThrough];
    assert_eq!(
        names(
            write_policies
                .iter()
                .map(|value| serde_json::to_value(value).unwrap())
                .collect()
        ),
        ["writeBack", "writeThrough"]
    );
    let allocate_policies = [
        AllocatePolicy::WriteAllocate,
        AllocatePolicy::WriteNoAllocate,
    ];
    assert_eq!(
        names(
            allocate_policies
                .iter()
                .map(|value| serde_json::to_value(value).unwrap())
                .collect()
        ),
        ["writeAllocate", "writeNoAllocate"]
    );
    let memory_inits = [MemoryInit::Zero, MemoryInit::Pattern];
    assert_eq!(
        names(
            memory_inits
                .iter()
                .map(|value| serde_json::to_value(value).unwrap())
                .collect()
        ),
        ["zero", "pattern"]
    );
    let fields = [ConfigNumberField::HitCycles, ConfigNumberField::MissCycles];
    assert_eq!(
        names(
            fields
                .iter()
                .map(|value| serde_json::to_value(value).unwrap())
                .collect()
        ),
        ["hitCycles", "missCycles"]
    );
    for replacement in replacements {
        let json = serde_json::to_string(&replacement).unwrap();
        assert_eq!(
            serde_json::from_str::<Replacement>(&json).unwrap(),
            replacement
        );
    }
}

#[test]
fn word_bytes_is_one_of_four_numbers() {
    for word in WordBytes::ALL {
        let json = serde_json::to_string(&word).unwrap();
        assert_eq!(json, word.bytes().to_string());
        assert_eq!(serde_json::from_str::<WordBytes>(&json).unwrap(), word);
    }
    for got in [0, 3, 16] {
        let message = format!("word size must be 1, 2, 4 or 8 bytes; got {got}");
        let error = serde_json::from_str::<WordBytes>(&got.to_string()).unwrap_err();
        assert_eq!(error.to_string(), message);
        let in_config = edited(r#""wordBytes":4"#, &format!(r#""wordBytes":{got}"#));
        let error = serde_json::from_str::<Config>(&in_config)
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with(&format!("{message} at line 1")),
            "{error}"
        );
    }
    let error = serde_json::from_str::<WordBytes>("256")
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("invalid value"), "{error}");
}

/// A configuration's JSON with one edit to the default text.
fn edited(from: &str, to: &str) -> String {
    let json = r#"{"addressBits":16,"wordBytes":4,"cache":{"sizeBytes":128,"lineBytes":16,"associativity":1,"replacement":"lru","writePolicy":"writeBack","allocatePolicy":"writeAllocate"},"timing":{"hitCycles":1,"missCycles":20},"seed":12345}"#;
    json.replacen(from, to, 1)
}

#[test]
fn unknown_fields_are_rejected() {
    assert!(serde_json::from_str::<Config>(&edited("", "")).is_ok());
    for (from, to, field) in [
        (r#""seed""#, r#""sead""#, "sead"),
        (r#""associativity""#, r#""ways""#, "ways"),
        (r#""hitCycles""#, r#""hit""#, "hit"),
        (
            r#""seed":12345"#,
            r#""seed":12345,"preset":"two-way-lru""#,
            "preset",
        ),
    ] {
        let error = serde_json::from_str::<Config>(&edited(from, to))
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with(&format!("unknown field `{field}`")),
            "{error}"
        );
    }
}

#[test]
fn unknown_enum_values_are_rejected() {
    for (from, to, variant) in [
        (r#""lru""#, r#""plru""#, "plru"),
        (r#""writeBack""#, r#""write-back""#, "write-back"),
        (r#""writeAllocate""#, r#""allocate""#, "allocate"),
        (
            r#""seed":12345"#,
            r#""seed":12345,"memoryInit":"random""#,
            "random",
        ),
    ] {
        let error = serde_json::from_str::<Config>(&edited(from, to))
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with(&format!("unknown variant `{variant}`")),
            "{error}"
        );
    }
}

#[test]
fn required_fields_are_required() {
    for (from, field) in [
        (r#","seed":12345"#, "seed"),
        (r#""associativity":1,"#, "associativity"),
        (r#","missCycles":20"#, "missCycles"),
    ] {
        let error = serde_json::from_str::<Config>(&edited(from, ""))
            .unwrap_err()
            .to_string();
        assert!(
            error.starts_with(&format!("missing field `{field}`")),
            "{error}"
        );
    }
}

#[test]
fn out_of_range_numbers_are_rejected_by_type() {
    for (from, to) in [
        (r#""addressBits":16"#, r#""addressBits":256"#),
        (r#""sizeBytes":128"#, r#""sizeBytes":-128"#),
        (r#""seed":12345"#, r#""seed":18446744073709551616"#),
    ] {
        let error = serde_json::from_str::<Config>(&edited(from, to))
            .unwrap_err()
            .to_string();
        // "invalid value" for an integer the field's type cannot hold, and
        // "invalid type" for one too large for any integer type.
        assert!(error.starts_with("invalid "), "{error}");
    }
}

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
            ..config.cache.clone()
        },
        ..config.clone()
    }
    .repair()
    .unwrap();
    let value = serde_json::to_value(&repair).unwrap();
    assert_eq!(value["label"], "Open with 2-way instead");
    assert_eq!(value["config"], serde_json::to_value(&config).unwrap());
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

/// The configuration bar's helpers. They sit in a module compiled for
/// tests only, where a failed `unwrap` is a failed test.
#[cfg(test)]
mod bar {
    use cache_explorer_core::{Config, ConfigControls, ConfigGroupKind, ConfigOption, Session};

    /// The configuration bar a session on `Config::default()` draws for
    /// `shown`.
    pub(crate) fn bar(shown: &Config) -> ConfigControls {
        Session::with_config(Config::default())
            .unwrap()
            .config_controls(shown)
    }

    /// The options of `group` in `controls`.
    pub(crate) fn options(controls: &ConfigControls, group: ConfigGroupKind) -> &[ConfigOption] {
        &controls
            .groups
            .iter()
            .find(|each| each.group == group)
            .unwrap()
            .options
    }

    /// The option labelled `label` of `group` in `controls`.
    pub(crate) fn option<'c>(
        controls: &'c ConfigControls,
        group: ConfigGroupKind,
        label: &str,
    ) -> &'c ConfigOption {
        options(controls, group)
            .iter()
            .find(|option| option.label == label)
            .unwrap()
    }

    /// The size, line size and associativity of `option`'s configuration.
    pub(crate) fn shape(option: &ConfigOption) -> (u32, u32, u32) {
        let cache = &option.config.cache;
        (cache.size_bytes, cache.line_bytes, cache.associativity)
    }

    /// The labels of `group`'s options in `controls`, with their marks.
    pub(crate) fn labels(
        controls: &ConfigControls,
        group: ConfigGroupKind,
    ) -> Vec<(String, bool, bool)> {
        options(controls, group)
            .iter()
            .map(|option| (option.label.clone(), option.pressed, option.disabled))
            .collect()
    }
}

#[test]
fn the_bar_offers_the_designs_groups_and_options() {
    let controls = bar(&Config::default());
    let groups: Vec<(ConfigGroupKind, bool)> = controls
        .groups
        .iter()
        .map(|group| (group.group, group.advanced_only))
        .collect();
    assert_eq!(
        groups,
        [
            (ConfigGroupKind::CacheSize, false),
            (ConfigGroupKind::LineSize, false),
            (ConfigGroupKind::Associativity, false),
            (ConfigGroupKind::Replacement, false),
            (ConfigGroupKind::AddressBits, false),
            (ConfigGroupKind::WordSize, false),
            (ConfigGroupKind::WritePolicy, true),
            (ConfigGroupKind::AllocatePolicy, true),
        ]
    );
    let names = |group| -> Vec<String> {
        labels(&controls, group)
            .into_iter()
            .map(|(label, _, _)| label)
            .collect()
    };
    assert_eq!(
        names(ConfigGroupKind::CacheSize),
        ["64 B", "128 B", "256 B", "512 B"]
    );
    assert_eq!(names(ConfigGroupKind::LineSize), ["8 B", "16 B", "32 B"]);
    assert_eq!(
        names(ConfigGroupKind::Associativity),
        ["Direct", "2-way", "4-way", "8-way", "Full"]
    );
    assert_eq!(
        names(ConfigGroupKind::Replacement),
        ["LRU", "FIFO", "Random"]
    );
    assert_eq!(
        names(ConfigGroupKind::WordSize),
        ["1 B", "2 B", "4 B", "8 B"]
    );
    assert_eq!(
        names(ConfigGroupKind::WritePolicy),
        ["Write-back", "Write-through"]
    );
    assert_eq!(
        names(ConfigGroupKind::AllocatePolicy),
        ["Allocate", "No allocate"]
    );
    let widths: Vec<String> = (MIN_ADDRESS_BITS..=MAX_ADDRESS_BITS)
        .map(|bits| format!("{bits}-bit"))
        .collect();
    assert_eq!(names(ConfigGroupKind::AddressBits), widths);
    // The current choices are pressed, and a pressed option produces the
    // configuration shown.
    for group in &controls.groups {
        let pressed: Vec<&ConfigOption> = group
            .options
            .iter()
            .filter(|option| option.pressed)
            .collect();
        assert_eq!(pressed.len(), 1, "{:?}", group.group);
        assert_eq!(pressed[0].config, Config::default(), "{:?}", group.group);
    }
    assert_eq!(
        controls.summary,
        "128 B \u{b7} 16 B lines \u{b7} direct-mapped"
    );
    assert_eq!(controls.error, None);
    // Each other option changes its own field alone.
    let fifo = option(&controls, ConfigGroupKind::Replacement, "FIFO");
    assert_eq!(fifo.config.cache.replacement, Replacement::Fifo);
    assert_eq!(
        Config {
            cache: CacheConfig {
                replacement: Replacement::Lru,
                ..fifo.config.cache.clone()
            },
            ..fifo.config.clone()
        },
        Config::default()
    );
    let words = option(&controls, ConfigGroupKind::WordSize, "8 B");
    assert_eq!(words.config.word_bytes, WordBytes::Eight);
    let through = option(&controls, ConfigGroupKind::WritePolicy, "Write-through");
    assert_eq!(through.config.cache.write_policy, WritePolicy::WriteThrough);
    let around = option(&controls, ConfigGroupKind::AllocatePolicy, "No allocate");
    assert_eq!(
        around.config.cache.allocate_policy,
        AllocatePolicy::WriteNoAllocate
    );
    let wide = option(&controls, ConfigGroupKind::AddressBits, "16-bit");
    assert_eq!(wide.config.address_bits, 16);
}

#[test]
fn size_and_line_options_keep_full_full() {
    // 128 B of 16 B lines, fully associative: eight ways.
    let full = with_cache(128, 16, 8);
    let controls = bar(&full);
    assert_eq!(
        labels(&controls, ConfigGroupKind::Associativity),
        [
            ("Direct".to_owned(), false, false),
            ("2-way".to_owned(), false, false),
            ("4-way".to_owned(), false, false),
            ("8-way".to_owned(), false, true),
            ("Full".to_owned(), true, false),
        ]
    );
    assert_eq!(
        option(&controls, ConfigGroupKind::Associativity, "8-way")
            .title
            .as_deref(),
        Some("Same as Full for an 8-line cache.")
    );
    // A smaller or larger cache, or other lines, keeps as many ways as it
    // has lines.
    let size = |label| shape(option(&controls, ConfigGroupKind::CacheSize, label));
    assert_eq!(size("64 B"), (64, 16, 4));
    assert_eq!(size("256 B"), (256, 16, 16));
    let line = |label| shape(option(&controls, ConfigGroupKind::LineSize, label));
    assert_eq!(line("8 B"), (128, 8, 16));
    assert_eq!(line("32 B"), (128, 32, 4));
    // One click from Full on 64 B gives a four-line Full cache.
    let smaller = bar(&option(&controls, ConfigGroupKind::CacheSize, "64 B").config);
    assert!(option(&smaller, ConfigGroupKind::Associativity, "Full").pressed);
    assert_eq!(
        option(&smaller, ConfigGroupKind::Associativity, "4-way")
            .title
            .as_deref(),
        Some("Same as Full for a 4-line cache.")
    );
}

#[test]
fn other_caches_lower_their_associativity_only_when_it_no_longer_fits() {
    // 8-way in 256 B of 16 B lines: sixteen lines, two sets.
    let eight_way = with_cache(256, 16, 8);
    let controls = bar(&eight_way);
    let size = |label| shape(option(&controls, ConfigGroupKind::CacheSize, label));
    assert_eq!(size("64 B"), (64, 16, 4));
    assert_eq!(size("128 B"), (128, 16, 8));
    assert_eq!(size("512 B"), (512, 16, 8));
    let line = |label| shape(option(&controls, ConfigGroupKind::LineSize, label));
    assert_eq!(line("32 B"), (256, 32, 8));
    assert_eq!(line("8 B"), (256, 8, 8));
    // 2-way keeps its two ways at every size offered.
    let two_way = bar(&with_cache(128, 16, 2));
    for option in options(&two_way, ConfigGroupKind::CacheSize) {
        assert_eq!(option.config.cache.associativity, 2, "{}", option.label);
    }
}

#[test]
fn associativities_beyond_the_line_count_are_disabled_with_their_message() {
    // 32 B of 16 B lines: two lines.
    let small = bar(&with_cache(32, 16, 1));
    assert_eq!(
        labels(&small, ConfigGroupKind::Associativity),
        [
            ("Direct".to_owned(), true, false),
            ("2-way".to_owned(), false, true),
            ("4-way".to_owned(), false, true),
            ("8-way".to_owned(), false, true),
            ("Full".to_owned(), false, false),
        ]
    );
    let titles: Vec<Option<&str>> = options(&small, ConfigGroupKind::Associativity)
        .iter()
        .map(|option| option.title.as_deref())
        .collect();
    assert_eq!(
        titles,
        [
            None,
            Some("Same as Full for a 2-line cache."),
            Some("A 2-line cache can have at most 2 ways; got 4."),
            Some("A 2-line cache can have at most 2 ways; got 8."),
            None
        ]
    );
    assert_eq!(
        shape(option(&small, ConfigGroupKind::Associativity, "Full")),
        (32, 16, 2)
    );
    // A one-line cache is Full, and one way is the same as Full.
    let one_line = bar(&with_cache(16, 16, 1));
    assert!(option(&one_line, ConfigGroupKind::Associativity, "Full").pressed);
    assert_eq!(
        option(&one_line, ConfigGroupKind::Associativity, "Direct")
            .title
            .as_deref(),
        Some("Same as Full for a 1-line cache.")
    );
}

#[test]
fn a_cache_without_lines_has_no_way_option_equal_to_full() {
    // 8 B of 16 B lines with no ways, as a malformed link or module can hold
    // it: there is no line, so no option is the same as Full, and the
    // pressed "0-way" carries the first rule the configuration breaks.
    let shown = with_cache(8, 16, 0);
    let error = shown.validate().unwrap_err();
    assert!(
        matches!(error, ConfigError::LineLargerThanCache { .. }),
        "{error:?}"
    );
    let controls = bar(&shown);
    let zero = option(&controls, ConfigGroupKind::Associativity, "0-way");
    assert!(zero.pressed && zero.disabled);
    assert_eq!(zero.title, Some(error.to_string()));
}

#[test]
fn a_value_off_the_lists_is_an_extra_pressed_option() {
    // 16-way in 512 B of 16 B lines: 32 lines.
    let sixteen = bar(&with_cache(512, 16, 16));
    let ways: Vec<String> = labels(&sixteen, ConfigGroupKind::Associativity)
        .into_iter()
        .map(|(label, _, _)| label)
        .collect();
    assert_eq!(
        ways,
        ["Direct", "2-way", "4-way", "8-way", "16-way", "Full"]
    );
    assert!(option(&sixteen, ConfigGroupKind::Associativity, "16-way").pressed);
    // A 32 B cache sorts first among the sizes.
    let tiny = bar(&with_cache(32, 16, 1));
    let sizes: Vec<(String, bool, bool)> = labels(&tiny, ConfigGroupKind::CacheSize);
    assert_eq!(sizes[0], ("32 B".to_owned(), true, false));
    assert_eq!(sizes.len(), 5);
    // An address width no configuration can have: pressed, and disabled with
    // its rule's message, which the error repeats with its repair.
    let wide = Config {
        address_bits: MAX_ADDRESS_BITS + 1,
        ..Config::default()
    };
    let controls = bar(&wide);
    let widths = options(&controls, ConfigGroupKind::AddressBits);
    let last = widths.last().unwrap();
    assert_eq!(last.label, format!("{}-bit", MAX_ADDRESS_BITS + 1));
    assert!(last.pressed && last.disabled);
    let message = ConfigError::AddressBits {
        got: MAX_ADDRESS_BITS + 1,
    }
    .to_string();
    assert_eq!(last.title.as_deref(), Some(message.as_str()));
    let error = controls.error.unwrap();
    assert_eq!(
        (error.field, error.message.as_str()),
        (ConfigField::AddressBits, message.as_str())
    );
    assert_eq!(
        error.repair.map(|repair| repair.config.address_bits),
        Some(MAX_ADDRESS_BITS)
    );
}

#[test]
fn every_option_is_disabled_exactly_when_its_configuration_is_invalid() {
    let one_byte_words = Config {
        word_bytes: WordBytes::One,
        ..with_cache(512, 8, 1)
    };
    let narrow = Config {
        address_bits: MIN_ADDRESS_BITS,
        ..with_cache(128, 16, 2)
    };
    let shown = [
        Config::default(),
        Preset::TwoWayLru.config(),
        Preset::FourWayWriteBack.config(),
        with_cache(16, 16, 1),
        with_cache(MAX_LINES * 16, 16, MAX_LINES),
        one_byte_words,
        narrow,
    ];
    for shown in shown {
        let controls = bar(&shown);
        for group in &controls.groups {
            for option in &group.options {
                let same_as_full = option
                    .title
                    .as_deref()
                    .is_some_and(|title| title.starts_with("Same as Full"));
                let error = option.config.validate().err();
                if !same_as_full {
                    assert_eq!(
                        option.disabled,
                        error.is_some(),
                        "{} {:?}",
                        option.label,
                        shown
                    );
                    assert_eq!(option.title, error.map(|error| error.to_string()));
                }
                if option.pressed {
                    assert_eq!(option.config, shown);
                }
            }
        }
    }
    // At 8-bit addresses a larger cache would cover all of memory: 256 B of
    // 16 B lines, direct-mapped, is 16 lines, within every `MAX_LINES` down
    // to its floor, and leaves no tag bit.
    let narrow = bar(&Config {
        address_bits: MIN_ADDRESS_BITS,
        ..with_cache(128, 16, 1)
    });
    let big = option(&narrow, ConfigGroupKind::CacheSize, "256 B");
    assert!(big.disabled);
    assert_eq!(
        big.title,
        Some(
            ConfigError::NoTagBits {
                bits: MIN_ADDRESS_BITS
            }
            .to_string()
        )
    );
    // A word wider than the line is refused.
    let short = bar(&with_cache(128, 4, 1));
    let wide_words = option(&short, ConfigGroupKind::WordSize, "8 B");
    assert_eq!(
        wide_words.title,
        Some(ConfigError::LineSmallerThanWord { line: 4, word: 8 }.to_string())
    );
}

#[test]
fn the_timing_fields_show_the_most_recent_timing_even_when_it_is_off() {
    let timed = |hit_cycles, miss_cycles| Timing {
        hit_cycles,
        miss_cycles,
    };
    let mut session = Session::with_config(Config::default()).unwrap();
    let timing = session.config_controls(session.config()).timing;
    // Nothing in the log is timed: the design's 1 and 20.
    assert!(!timing.on);
    assert_eq!(
        (timing.hit_text.as_str(), timing.miss_text.as_str()),
        ("1", "20")
    );
    assert_eq!(timing.toggle.timing, Some(timed(1, 20)));
    let three = Config {
        timing: Some(timed(3, 15)),
        ..Config::default()
    };
    session
        .apply(Op::Config {
            config: three.clone(),
        })
        .unwrap();
    let timing = session.config_controls(session.config()).timing;
    assert!(timing.on);
    assert_eq!(
        (timing.hit_text.as_str(), timing.miss_text.as_str()),
        ("3", "15")
    );
    assert_eq!(timing.toggle, Config::default());
    // Turned off, the fields keep the timing last used.
    session
        .apply(Op::Config {
            config: Config::default(),
        })
        .unwrap();
    let timing = session.config_controls(session.config()).timing;
    assert!(!timing.on);
    assert_eq!(
        (timing.hit_text.as_str(), timing.miss_text.as_str()),
        ("3", "15")
    );
    assert_eq!(timing.toggle, three);
    // Before that run, the log has no timing yet.
    session.undo();
    session.undo();
    let timing = session.config_controls(session.config()).timing;
    assert_eq!(
        (timing.hit_text.as_str(), timing.miss_text.as_str()),
        ("1", "20")
    );
}

#[test]
fn a_typed_timing_number_is_applied_or_explained() {
    let session = Session::with_config(Config::default()).unwrap();
    let shown = Config::default();
    let read = |field, text: &str| session.read_config_number(field, text, &shown);
    // A number in range turns timing on, with the other number as the
    // fields show it.
    assert_eq!(
        read(ConfigNumberField::HitCycles, " 5 "),
        ConfigNumberInput::Valid {
            config: Config {
                timing: Some(Timing {
                    hit_cycles: 5,
                    miss_cycles: 20
                }),
                ..shown.clone()
            }
        }
    );
    let timed = Config {
        timing: Some(Timing {
            hit_cycles: 2,
            miss_cycles: 9,
        }),
        ..shown.clone()
    };
    assert_eq!(
        session.read_config_number(
            ConfigNumberField::MissCycles,
            &MAX_CYCLES.to_string(),
            &timed
        ),
        ConfigNumberInput::Valid {
            config: Config {
                timing: Some(Timing {
                    hit_cycles: 2,
                    miss_cycles: MAX_CYCLES
                }),
                ..shown.clone()
            }
        }
    );
    // Out of range: the rule's message.
    let below = read(ConfigNumberField::HitCycles, &(MIN_CYCLES - 1).to_string());
    assert_eq!(
        below,
        ConfigNumberInput::Invalid {
            message: format!(
                "Hit time must be between {} and {} cycles; got {}.",
                grouped(MIN_CYCLES),
                grouped(MAX_CYCLES),
                grouped(MIN_CYCLES - 1)
            )
        }
    );
    let above = read(ConfigNumberField::MissCycles, &(MAX_CYCLES + 1).to_string());
    assert_eq!(
        above,
        ConfigNumberInput::Invalid {
            message: format!(
                "Miss penalty must be between {} and {} cycles; got {}.",
                grouped(MIN_CYCLES),
                grouped(MAX_CYCLES),
                grouped(MAX_CYCLES + 1)
            )
        }
    );
    // Anything else: what to type.
    let prompt = format!(
        "Type a whole number of cycles, {} to {}.",
        grouped(MIN_CYCLES),
        grouped(MAX_CYCLES)
    );
    for text in ["", "  ", "abc", "-3", "1.5", "0x10", "1,000", "99999999999"] {
        assert_eq!(
            read(ConfigNumberField::HitCycles, text),
            ConfigNumberInput::Invalid {
                message: prompt.clone()
            },
            "{text:?}"
        );
    }
    assert_eq!(
        serde_json::to_value(below).unwrap()["status"],
        json!("invalid")
    );
}

#[test]
fn a_configuration_checked_on_its_own_gives_its_summary_or_its_error() {
    let two_way = Preset::TwoWayLru.config();
    let ConfigCheck::Valid { summary, geometry } = validate_config(&two_way) else {
        panic!("two-way-lru is valid");
    };
    assert_eq!(summary, "128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU");
    assert_eq!(geometry, two_way.validate().unwrap());
    // Advanced when the configuration says so or needs it.
    let advanced = Config {
        advanced: true,
        ..two_way.clone()
    };
    let through = Config {
        cache: CacheConfig {
            write_policy: WritePolicy::WriteThrough,
            ..two_way.cache.clone()
        },
        ..two_way.clone()
    };
    for config in [advanced, through] {
        let ConfigCheck::Valid { summary, .. } = validate_config(&config) else {
            panic!("{config:?} is valid");
        };
        assert!(summary.ends_with("allocate"), "{summary}");
    }
    let three_way = with_cache(128, 16, 3);
    let check = validate_config(&three_way);
    let ConfigCheck::Invalid { error } = &check else {
        panic!("three ways is invalid");
    };
    assert_eq!(error.field, ConfigField::Associativity);
    assert_eq!(error.message, three_way.validate().unwrap_err().to_string());
    assert_eq!(error.repair, three_way.repair());
    let value = serde_json::to_value(&check).unwrap();
    assert_eq!(value["status"], json!("invalid"));
    assert_eq!(value["error"]["field"], json!("associativity"));
    assert_eq!(
        value["error"]["repair"]["label"],
        json!("Open with 2-way instead")
    );
}

#[test]
fn each_rule_names_the_field_its_repair_changes() {
    let cases = [
        (
            ConfigError::AddressBits { got: 7 },
            ConfigField::AddressBits,
            "addressBits",
        ),
        (
            ConfigError::SizeNotPowerOfTwo { got: 100 },
            ConfigField::SizeBytes,
            "sizeBytes",
        ),
        (
            ConfigError::LineNotPowerOfTwo { got: 12 },
            ConfigField::LineBytes,
            "lineBytes",
        ),
        (
            ConfigError::LineSmallerThanWord { line: 2, word: 4 },
            ConfigField::LineBytes,
            "lineBytes",
        ),
        (
            ConfigError::LineLargerThanCache { line: 32, size: 16 },
            ConfigField::SizeBytes,
            "sizeBytes",
        ),
        (
            ConfigError::TooManyLines {
                lines: MAX_LINES * 2,
                size: MAX_LINES * 32,
                line: 16,
            },
            ConfigField::SizeBytes,
            "sizeBytes",
        ),
        (
            ConfigError::TooManyWords {
                words: MAX_WORDS_PER_LINE * 2,
                line: MAX_WORDS_PER_LINE * 2,
                word: 1,
            },
            ConfigField::LineBytes,
            "lineBytes",
        ),
        (
            ConfigError::AssociativityZero,
            ConfigField::Associativity,
            "associativity",
        ),
        (
            ConfigError::AssociativityNotPowerOfTwo { got: 3, lines: 8 },
            ConfigField::Associativity,
            "associativity",
        ),
        (
            ConfigError::AssociativityTooLarge { got: 16, lines: 8 },
            ConfigField::Associativity,
            "associativity",
        ),
        (
            ConfigError::NoTagBits { bits: 8 },
            ConfigField::SizeBytes,
            "sizeBytes",
        ),
        (
            ConfigError::Timing {
                field: ConfigNumberField::HitCycles,
                got: 0,
            },
            ConfigField::Timing,
            "timing",
        ),
        (
            ConfigError::SeedTooLarge { got: MAX_SEED + 1 },
            ConfigField::Seed,
            "seed",
        ),
    ];
    for (error, field, name) in cases {
        assert_eq!(error.field(), field, "{error:?}");
        assert_eq!(serde_json::to_value(field).unwrap(), json!(name));
    }
    assert_eq!(
        serde_json::to_value(ConfigField::WordBytes).unwrap(),
        json!("wordBytes")
    );
}
