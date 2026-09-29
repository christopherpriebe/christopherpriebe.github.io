//! The configuration bar: every option with the configuration it produces,
//! Full kept across size and line changes, the timing fields, typed timing
//! numbers, and a configuration checked on its own.

#![expect(clippy::disallowed_macros, reason = "assertions are how tests fail")]

use cache_explorer_core::limits::{
    MAX_ADDRESS_BITS, MAX_CYCLES, MAX_LINES, MAX_SEED, MAX_WORDS_PER_LINE, MIN_ADDRESS_BITS,
    MIN_CYCLES,
};
use cache_explorer_core::{
    validate_config, AllocatePolicy, CacheConfig, Config, ConfigCheck, ConfigError, ConfigField,
    ConfigGroupKind, ConfigNumberField, ConfigNumberInput, ConfigOption, Op, Preset, Replacement,
    Session, Timing, WordBytes, WritePolicy,
};
use serde_json::json;

use bar::{bar, labels, option, options, shape};
use common::{grouped, with_cache};
use support::check_rule_field;

#[cfg(test)]
mod common;

/// The tests' helpers other than the bar's. They sit in a module compiled
/// for tests only, where a failed `unwrap` is a failed test.
#[cfg(test)]
mod support {
    use cache_explorer_core::{ConfigError, ConfigField};
    use serde_json::json;

    /// Checks that `error` names `field`, which serializes as `name`.
    pub(crate) fn check_rule_field(error: &ConfigError, field: ConfigField, name: &str) {
        assert_eq!(error.field(), field, "{error:?}");
        assert_eq!(
            serde_json::to_value(field).unwrap(),
            json!(name),
            "{error:?}"
        );
    }
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
fn the_bar_offers_its_groups_and_their_options() {
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
                ..fifo.config.cache
            },
            ..fifo.config
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
                    assert_eq!(
                        option.title,
                        error.map(|error| error.to_string()),
                        "{} {:?}",
                        option.label,
                        shown
                    );
                }
                if option.pressed {
                    assert_eq!(option.config, shown, "{} {:?}", option.label, shown);
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
    // Nothing in the log is timed: the fallback timing, 1 and 20.
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
    session.apply(Op::Config { config: three }).unwrap();
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
                ..shown
            }
        }
    );
    let timed = Config {
        timing: Some(Timing {
            hit_cycles: 2,
            miss_cycles: 9,
        }),
        ..shown
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
                ..shown
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
        ..two_way
    };
    let through = Config {
        cache: CacheConfig {
            write_policy: WritePolicy::WriteThrough,
            ..two_way.cache
        },
        ..two_way
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

cases! {
    check_rule_field;
    an_address_width_error_names_the_address_bits_field => (
        &ConfigError::AddressBits { got: 7 },
        ConfigField::AddressBits,
        "addressBits",
    ),
    a_cache_size_error_names_the_size_field => (
        &ConfigError::SizeNotPowerOfTwo { got: 100 },
        ConfigField::SizeBytes,
        "sizeBytes",
    ),
    a_line_size_error_names_the_line_size_field => (
        &ConfigError::LineNotPowerOfTwo { got: 12 },
        ConfigField::LineBytes,
        "lineBytes",
    ),
    a_line_smaller_than_a_word_names_the_line_size_field => (
        &ConfigError::LineSmallerThanWord { line: 2, word: 4 },
        ConfigField::LineBytes,
        "lineBytes",
    ),
    a_line_larger_than_the_cache_names_the_size_field => (
        &ConfigError::LineLargerThanCache { line: 32, size: 16 },
        ConfigField::SizeBytes,
        "sizeBytes",
    ),
    too_many_lines_name_the_size_field => (
        &ConfigError::TooManyLines {
            lines: MAX_LINES * 2,
            size: MAX_LINES * 32,
            line: 16,
        },
        ConfigField::SizeBytes,
        "sizeBytes",
    ),
    too_many_words_name_the_line_size_field => (
        &ConfigError::TooManyWords {
            words: MAX_WORDS_PER_LINE * 2,
            line: MAX_WORDS_PER_LINE * 2,
            word: 1,
        },
        ConfigField::LineBytes,
        "lineBytes",
    ),
    zero_ways_name_the_associativity_field => (
        &ConfigError::AssociativityZero,
        ConfigField::Associativity,
        "associativity",
    ),
    ways_that_are_not_a_power_of_two_name_the_associativity_field => (
        &ConfigError::AssociativityNotPowerOfTwo { got: 3, lines: 8 },
        ConfigField::Associativity,
        "associativity",
    ),
    more_ways_than_lines_name_the_associativity_field => (
        &ConfigError::AssociativityTooLarge { got: 16, lines: 8 },
        ConfigField::Associativity,
        "associativity",
    ),
    a_cache_with_no_tag_bits_names_the_size_field => (
        &ConfigError::NoTagBits { bits: 8 },
        ConfigField::SizeBytes,
        "sizeBytes",
    ),
    a_timing_error_names_the_timing_field => (
        &ConfigError::Timing {
            field: ConfigNumberField::HitCycles,
            got: 0,
        },
        ConfigField::Timing,
        "timing",
    ),
    a_seed_error_names_the_seed_field => (
        &ConfigError::SeedTooLarge { got: MAX_SEED + 1 },
        ConfigField::Seed,
        "seed",
    ),
}

#[test]
fn the_word_size_field_is_named_word_bytes() {
    assert_eq!(
        serde_json::to_value(ConfigField::WordBytes).unwrap(),
        json!("wordBytes")
    );
}
