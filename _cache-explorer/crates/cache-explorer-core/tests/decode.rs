//! Configuration and geometry: the defaults and the named configurations,
//! every validation rule with its message and its repair, the geometry a
//! valid configuration yields with its checked constructors, and the
//! configuration's serialized shape.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::limits::{
    MAX_ADDRESS_BITS, MAX_CYCLES, MAX_LINES, MAX_SEED, MAX_WORDS_PER_LINE, MIN_ADDRESS_BITS,
    MIN_CYCLES,
};
use cache_explorer_core::{
    presets, Addr, AllocatePolicy, CacheConfig, Config, ConfigError, ConfigNumberField, MemoryInit,
    Placement, Preset, Replacement, SetIndex, Tag, Timing, WayIndex, WordBytes, WordIndex,
    WritePolicy,
};

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
