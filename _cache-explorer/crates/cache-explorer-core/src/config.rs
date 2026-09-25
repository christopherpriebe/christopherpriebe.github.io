//! The configuration of a run and the rules it must satisfy.
//!
//! A [`Config`] is fixed for a run; a new configuration starts a new run.
//! [`Config::validate`] checks the rules in a fixed order and computes the
//! [`Geometry`] of a valid configuration; the first rule that fails is the
//! [`ConfigError`], whose `Display` is the message the student reads.
//! [`Config::repair`] finds the nearest valid configuration for the "Open
//! with … instead" action, and [`Config::summary`] is the configuration's
//! one-line description. The named configurations, the design's three
//! geometries, live here too.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::format;
use crate::geometry::{Geometry, Placement};
use crate::limits::{
    MAX_ADDRESS_BITS, MAX_CYCLES, MAX_LINES, MAX_SEED, MAX_WORDS_PER_LINE, MIN_ADDRESS_BITS,
    MIN_CYCLES,
};

/// A cache and machine configuration. Fixed for a run; a new configuration
/// starts a new run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    /// Width of a byte address, from [`MIN_ADDRESS_BITS`] to
    /// [`MAX_ADDRESS_BITS`]. `Config::default()` uses 12, the width of every
    /// design frame, where the spec's default is 16.
    pub address_bits: u8,
    /// Bytes per word.
    pub word_bytes: WordBytes,
    /// Cache organisation and policies.
    pub cache: CacheConfig,
    /// `Some` turns on timed mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing: Option<Timing>,
    /// Seed for random replacement; at most [`MAX_SEED`], 2^53 − 1, so it
    /// survives a JavaScript number.
    pub seed: u64,
    /// Presentation flag carried for share links: whether the student chose
    /// Advanced. The run never reads it, so two configurations that differ
    /// only here produce identical runs.
    #[serde(default)]
    pub advanced: bool,
    /// Contents of memory no run has written yet.
    #[serde(default, skip_serializing_if = "MemoryInit::is_zero")]
    pub memory_init: MemoryInit,
}

/// The cache's organisation and policies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CacheConfig {
    /// Total data bytes; a power of two.
    pub size_bytes: u32,
    /// Bytes per line; a power of two, at least one word.
    pub line_bytes: u32,
    /// Ways per set. 1 is direct-mapped; equal to the line count is fully
    /// associative.
    pub associativity: u32,
    /// Which way a miss evicts from a full set.
    pub replacement: Replacement,
    /// What a store that hits does to memory.
    pub write_policy: WritePolicy,
    /// Whether a store that misses brings its line in.
    pub allocate_policy: AllocatePolicy,
}

/// The costs of timed mode, in cycles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Timing {
    /// Cycles every access costs: the hit time.
    pub hit_cycles: u32,
    /// Cycles a miss adds, and a dirty write-back adds again: the miss penalty.
    pub miss_cycles: u32,
}

/// Bytes per word. Serialized as the number 1, 2, 4 or 8.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify), tsify(type = "1 | 2 | 4 | 8"))]
#[serde(try_from = "u8", into = "u8")]
pub enum WordBytes {
    /// 1-byte words.
    One,
    /// 2-byte words.
    Two,
    /// 4-byte words.
    Four,
    /// 8-byte words.
    Eight,
}

/// Which way a miss evicts once every way of its set is valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum Replacement {
    /// The least recently used way.
    Lru,
    /// The way filled longest ago.
    Fifo,
    /// A way drawn from the run's seeded generator.
    Random,
}

/// What a store that hits does to memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum WritePolicy {
    /// Only the cached line changes, and it becomes dirty; memory is written
    /// when the line is evicted.
    WriteBack,
    /// Memory is written too; lines are never dirty.
    WriteThrough,
}

/// Whether a store that misses brings its line into the cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum AllocatePolicy {
    /// The line is filled, then written as on a hit.
    WriteAllocate,
    /// Only memory is written; the cache is unchanged.
    WriteNoAllocate,
}

/// Contents of memory no run has written yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum MemoryInit {
    /// Every byte reads 0, as the spec says.
    #[default]
    Zero,
    /// The design's filler: the word at address `a` reads
    /// `(a / word_bytes × 13 + 5) mod 97`, so a fill brings in visible values.
    Pattern,
}

/// One of the two timing numbers: the hit time or the miss penalty.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum ConfigNumberField {
    /// `Timing::hit_cycles`.
    HitCycles,
    /// `Timing::miss_cycles`.
    MissCycles,
}

/// The nearest valid configuration to an invalid one, and the label of the
/// action that opens it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Repair {
    /// The repaired configuration; it validates.
    pub config: Config,
    /// The action's label, such as "Open with 2-way instead", naming the
    /// repair of the first rule that failed.
    pub label: String,
}

/// One of the named configurations: the design's three geometries, kept for
/// the golden fixtures, the tests and the page's first visit.
///
/// All use `Config::default()`'s address width, word size and seed, pattern
/// memory, and Basics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// `direct-mapped-8`: 128 B, 16 B lines, direct-mapped (8 sets), LRU,
    /// write-back, write-allocate.
    DirectMapped8,
    /// `two-way-lru`: 128 B, 16 B lines, 2-way (4 sets), LRU, write-back,
    /// write-allocate.
    TwoWayLru,
    /// `four-way-write-back`: 256 B, 16 B lines, 4-way (4 sets), LRU,
    /// write-back, write-allocate.
    FourWayWriteBack,
}

/// Why a configuration is invalid: the first rule it breaks, in the order
/// `Config::validate` checks them, which is the order of the variants.
/// `Display` is the message the student reads.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ConfigError {
    /// The address width is outside the supported range.
    #[error(
        "Address width must be between {} and {} bits; got {}.",
        format::setting(MIN_ADDRESS_BITS),
        format::setting(MAX_ADDRESS_BITS),
        format::setting(*.got)
    )]
    AddressBits {
        /// The width given.
        got: u8,
    },
    /// The cache size is not a power of two.
    #[error(
        "Cache size must be a power of two, such as 64, 128 or 256 bytes; got {}.",
        format::setting(*.got)
    )]
    SizeNotPowerOfTwo {
        /// The size given, in bytes.
        got: u32,
    },
    /// The line size is not a power of two.
    #[error(
        "Line size must be a power of two, such as 8, 16 or 32 bytes; got {}.",
        format::setting(*.got)
    )]
    LineNotPowerOfTwo {
        /// The line size given, in bytes.
        got: u32,
    },
    /// A line is smaller than a word.
    #[error(
        "A line must hold at least one word: {} B lines are smaller than {} B words.",
        format::setting(*.line),
        format::setting(*.word)
    )]
    LineSmallerThanWord {
        /// The line size, in bytes.
        line: u32,
        /// The word size, in bytes.
        word: u32,
    },
    /// A line is larger than the whole cache.
    #[error(
        "The cache must hold at least one line: {} {} B cache cannot hold {} B lines.",
        format::a_or_an(*.size),
        format::setting(*.size),
        format::setting(*.line)
    )]
    LineLargerThanCache {
        /// The line size, in bytes.
        line: u32,
        /// The cache size, in bytes.
        size: u32,
    },
    /// The cache has more lines than fit on screen.
    #[error(
        "Keep the cache to at most {} lines so every set fits on screen; {} B ÷ {} B = {} lines.",
        format::setting(MAX_LINES),
        format::setting(*.size),
        format::setting(*.line),
        format::setting(*.lines)
    )]
    TooManyLines {
        /// Lines in the cache.
        lines: u32,
        /// The cache size, in bytes.
        size: u32,
        /// The line size, in bytes.
        line: u32,
    },
    /// A line holds more words than a row can show.
    #[error(
        "A line can hold at most {} words; {} B ÷ {} B = {}.",
        format::setting(MAX_WORDS_PER_LINE),
        format::setting(*.line),
        format::setting(*.word),
        format::setting(*.words)
    )]
    TooManyWords {
        /// Words per line.
        words: u32,
        /// The line size, in bytes.
        line: u32,
        /// The word size, in bytes.
        word: u32,
    },
    /// The associativity is 0.
    #[error("Associativity must be at least 1.")]
    AssociativityZero,
    /// The associativity is not a power of two.
    #[error(
        "Associativity {} is not a power of two. It must be {} for {} {}-line cache.",
        format::setting(*.got),
        associativity_choices(*.lines),
        format::a_or_an(*.lines),
        format::setting(*.lines)
    )]
    AssociativityNotPowerOfTwo {
        /// The associativity given.
        got: u32,
        /// Lines in the cache.
        lines: u32,
    },
    /// The associativity exceeds the line count.
    #[error(
        "{} {}-line cache can have at most {} {}; got {}.",
        format::capital_a_or_an(*.lines),
        format::setting(*.lines),
        format::setting(*.lines),
        format::plural(*.lines, "way", "ways"),
        format::setting(*.got)
    )]
    AssociativityTooLarge {
        /// The associativity given.
        got: u32,
        /// Lines in the cache.
        lines: u32,
    },
    /// One way would cover the whole address space, leaving no tag bits.
    #[error(
        "With {}-bit addresses one way of this cache would cover all of memory, leaving no \
         tag bits. Use wider addresses or a smaller cache.",
        format::setting(*.bits)
    )]
    NoTagBits {
        /// The address width.
        bits: u8,
    },
    /// A timing number is outside the supported range.
    #[error(
        "{} must be between {} and {} cycles; got {}.",
        timing_name(*.field),
        format::setting(MIN_CYCLES),
        format::setting(MAX_CYCLES),
        format::setting(*.got)
    )]
    Timing {
        /// Which number is out of range.
        field: ConfigNumberField,
        /// The number given, in cycles.
        got: u32,
    },
    /// The seed would not survive a JavaScript number.
    #[error(
        "The seed must be at most {}; got {}.",
        format::setting(MAX_SEED),
        format::setting(*.got)
    )]
    SeedTooLarge {
        /// The seed given.
        got: u64,
    },
}

/// The error of reading a word size that is not 1, 2, 4 or 8 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
#[error("word size must be 1, 2, 4 or 8 bytes; got {got}")]
pub struct WordBytesError {
    /// The number read.
    pub got: u8,
}

impl Config {
    /// Checks the rules in order and returns the geometry of a valid
    /// configuration; the first rule that fails is the error.
    ///
    /// # Errors
    ///
    /// The [`ConfigError`] of the first rule the configuration breaks.
    pub fn validate(&self) -> Result<Geometry, ConfigError> {
        let address_bits = self.address_bits;
        if !(MIN_ADDRESS_BITS..=MAX_ADDRESS_BITS).contains(&address_bits) {
            return Err(ConfigError::AddressBits { got: address_bits });
        }
        let (sets, ways, lines) = self.cache.organisation(self.word_bytes)?;
        let word = self.word_bytes.bytes();
        let line = self.cache.line_bytes;
        let offset_bits = log2(line);
        let index_bits = log2(sets);
        // Computed signed: 8-bit addresses with a 512 B direct-mapped cache of
        // 32 B lines leave 8 − 4 − 5 = −1 tag bits.
        let tag_bits = i32::from(address_bits) - i32::from(index_bits) - i32::from(offset_bits);
        let tag_bits = u8::try_from(tag_bits)
            .ok()
            .filter(|&bits| bits >= 1)
            .ok_or(ConfigError::NoTagBits { bits: address_bits })?;
        if let Some(timing) = self.timing {
            check_cycles(ConfigNumberField::HitCycles, timing.hit_cycles)?;
            check_cycles(ConfigNumberField::MissCycles, timing.miss_cycles)?;
        }
        if self.seed > MAX_SEED {
            return Err(ConfigError::SeedTooLarge { got: self.seed });
        }
        Ok(Geometry {
            address_bits,
            word_bytes: word,
            line_bytes: line,
            ways,
            sets,
            lines,
            words_per_line: line / word,
            offset_bits,
            index_bits,
            tag_bits,
            placement: Placement::of(sets, ways),
        })
    }

    /// The nearest valid configuration, for "Open with 2-way instead"; `None`
    /// if this one is already valid.
    ///
    /// One pass of closed-form steps, so it cannot oscillate the way repairing
    /// rule by rule could (with size 2, word 4 and line 1, "line = word" and
    /// "line = size" would undo each other):
    ///
    /// 1. the address width is clamped to its range;
    /// 2. the line size is rounded down to a power of two (0 becomes 1), then
    ///    clamped between one word and the most words a line may hold;
    /// 3. the cache size is rounded down to a power of two (0 becomes 1), then
    ///    raised to at least one line: the cache grows, the line never shrinks;
    /// 4. if the cache was fully associative, its associativity at least its
    ///    line count, the associativity becomes the new line count; otherwise
    ///    it becomes at least 1, is rounded down to a power of two, and is
    ///    lowered to at most the line count;
    /// 5. the cache size becomes the largest power of two, not above the size
    ///    from step 3, with at most `MAX_LINES` lines and a tag bit, a fully
    ///    associative cache keeping as many ways as lines and any other
    ///    lowering its associativity to the line count if it no longer fits;
    /// 6. timing is clamped to its range, and the seed to `MAX_SEED`.
    ///
    /// The address width, word size, policies, memory contents and
    /// `advanced` are otherwise kept.
    pub fn repair(&self) -> Option<Repair> {
        let first = self.validate().err()?;
        let config = self.repaired();
        let geometry = config.validate().ok()?;
        let label = first.repair_label(&config, &geometry);
        Some(Repair { config, label })
    }

    /// True when the configuration is usable in Basics: write-back,
    /// write-allocate and untimed.
    pub fn fits_basics(&self) -> bool {
        self.cache.write_policy == WritePolicy::WriteBack
            && self.cache.allocate_policy == AllocatePolicy::WriteAllocate
            && self.timing.is_none()
    }

    /// The design's summary line: the size, the line size and the placement,
    /// then the replacement policy unless the cache is direct-mapped, as in
    /// "128 B · 16 B lines · 2-way · LRU"; in Advanced, then the write and
    /// allocate policies, " · write-back · allocate". Timing is not part of it.
    pub fn summary(&self, advanced: bool) -> String {
        let cache = &self.cache;
        let mut text = format!(
            "{} B · {} B lines · {}",
            format::setting(cache.size_bytes),
            format::setting(cache.line_bytes),
            cache.placement_name()
        );
        if cache.placement() != Placement::DirectMapped {
            text.push_str(" · ");
            text.push_str(replacement_name(cache.replacement));
        }
        if advanced {
            text.push_str(" · ");
            text.push_str(match cache.write_policy {
                WritePolicy::WriteBack => "write-back",
                WritePolicy::WriteThrough => "write-through",
            });
            text.push_str(" · ");
            text.push_str(match cache.allocate_policy {
                AllocatePolicy::WriteAllocate => "allocate",
                AllocatePolicy::WriteNoAllocate => "no-allocate",
            });
        }
        text
    }

    /// The short summary a shared run's loading screen opens with: the
    /// placement and the replacement policy, then the size, as in "2-way LRU ·
    /// 128 B". It starts a sentence, so it starts with a capital: "Direct-mapped
    /// · 128 B", "Fully associative LRU · 128 B".
    pub fn short_summary(&self) -> String {
        let cache = &self.cache;
        let replacement = replacement_name(cache.replacement);
        let placement = match cache.placement() {
            Placement::DirectMapped => "Direct-mapped".to_owned(),
            Placement::FullyAssociative => format!("Fully associative {replacement}"),
            Placement::SetAssociative => {
                format!("{}-way {replacement}", format::setting(cache.associativity))
            }
        };
        format!("{placement} · {} B", format::setting(cache.size_bytes))
    }

    /// This configuration with write-back, write-allocate and no timing.
    #[must_use]
    pub fn to_basics(&self) -> Config {
        Config {
            cache: CacheConfig {
                write_policy: WritePolicy::WriteBack,
                allocate_policy: AllocatePolicy::WriteAllocate,
                ..self.cache.clone()
            },
            timing: None,
            ..self.clone()
        }
    }

    /// The configuration of the page's first visit and of "Start fresh":
    /// `direct-mapped-8`, which is `Config::default()` with pattern memory, so
    /// the first fill brings in visible values.
    pub fn first_visit() -> Config {
        Preset::DirectMapped8.config()
    }

    /// Steps 1 to 6 of [`Config::repair`].
    fn repaired(&self) -> Config {
        let word = self.word_bytes.bytes();
        let address_bits = self.address_bits.clamp(MIN_ADDRESS_BITS, MAX_ADDRESS_BITS);
        // Both bounds are powers of two, so the clamped line is one too.
        let largest_line = floor_power_of_two(MAX_WORDS_PER_LINE).saturating_mul(word);
        let line = floor_power_of_two(self.cache.line_bytes)
            .min(largest_line)
            .max(word);
        let size = floor_power_of_two(self.cache.size_bytes).max(line);
        // Whether the cache had one set: at least as many ways as lines, since
        // an invalid associativity may be more. Lines of 0 bytes give no line
        // count, so such a cache had no set to speak of.
        let fully_associative = self
            .cache
            .size_bytes
            .checked_div(self.cache.line_bytes)
            .is_some_and(|lines| self.cache.associativity >= lines);
        let associativity = if fully_associative {
            size / line
        } else {
            floor_power_of_two(self.cache.associativity).min(size / line)
        };
        let (size_bytes, associativity) =
            fit_size(address_bits, line, size, associativity, fully_associative);
        Config {
            address_bits,
            cache: CacheConfig {
                size_bytes,
                line_bytes: line,
                associativity,
                ..self.cache.clone()
            },
            timing: self.timing.map(|timing| Timing {
                hit_cycles: clamp_cycles(timing.hit_cycles),
                miss_cycles: clamp_cycles(timing.miss_cycles),
            }),
            seed: self.seed.min(MAX_SEED),
            ..self.clone()
        }
    }
}

impl Default for Config {
    /// The spec's defaults, except the address width: 12 bits, as in every
    /// design frame. The cache is `direct-mapped-8`'s geometry: 128 B of
    /// 16 B lines, direct-mapped, so 8 sets and 5 tag bits. Memory is zero, as
    /// the spec says; the page's first visit opens this configuration with
    /// pattern memory instead, as [`Config::first_visit`].
    fn default() -> Self {
        Config {
            address_bits: 12,
            word_bytes: WordBytes::Four,
            cache: CacheConfig {
                size_bytes: 128,
                line_bytes: 16,
                associativity: 1,
                replacement: Replacement::Lru,
                write_policy: WritePolicy::WriteBack,
                allocate_policy: AllocatePolicy::WriteAllocate,
            },
            timing: None,
            seed: 12345,
            advanced: false,
            memory_init: MemoryInit::Zero,
        }
    }
}

impl CacheConfig {
    /// Checks the cache rules, from the cache size to the associativity's
    /// limit, and returns the sets, ways and lines.
    ///
    /// The caps on lines and words come as soon as the line fits the cache,
    /// before the associativity: a cache with too many lines is refused for
    /// that rather than told which associativities it could have, and a line
    /// within the words cap is short enough that a fully associative cache
    /// always keeps a tag bit.
    fn organisation(&self, word_bytes: WordBytes) -> Result<(u32, u32, u32), ConfigError> {
        let size = self.size_bytes;
        let line = self.line_bytes;
        let word = word_bytes.bytes();
        let ways = self.associativity;
        if !size.is_power_of_two() {
            return Err(ConfigError::SizeNotPowerOfTwo { got: size });
        }
        if !line.is_power_of_two() {
            return Err(ConfigError::LineNotPowerOfTwo { got: line });
        }
        if line < word {
            return Err(ConfigError::LineSmallerThanWord { line, word });
        }
        if size < line {
            return Err(ConfigError::LineLargerThanCache { line, size });
        }
        let lines = size / line;
        if lines > MAX_LINES {
            return Err(ConfigError::TooManyLines { lines, size, line });
        }
        let words = line / word;
        if words > MAX_WORDS_PER_LINE {
            return Err(ConfigError::TooManyWords { words, line, word });
        }
        if ways == 0 {
            return Err(ConfigError::AssociativityZero);
        }
        if !ways.is_power_of_two() {
            return Err(ConfigError::AssociativityNotPowerOfTwo { got: ways, lines });
        }
        if ways > lines {
            return Err(ConfigError::AssociativityTooLarge { got: ways, lines });
        }
        Ok((lines / ways, ways, lines))
    }

    /// The placement the summaries name, read from the numbers as given so
    /// that an invalid configuration has one too; for a valid configuration it
    /// is the geometry's.
    fn placement(&self) -> Placement {
        if self.size_bytes.checked_div(self.line_bytes) == Some(self.associativity) {
            Placement::FullyAssociative
        } else if self.associativity == 1 {
            Placement::DirectMapped
        } else {
            Placement::SetAssociative
        }
    }

    /// The placement as the summaries write it: "direct-mapped", "fully
    /// associative" or "2-way".
    fn placement_name(&self) -> String {
        match self.placement() {
            Placement::DirectMapped => "direct-mapped".to_owned(),
            Placement::FullyAssociative => "fully associative".to_owned(),
            Placement::SetAssociative => format!("{}-way", format::setting(self.associativity)),
        }
    }
}

impl WordBytes {
    /// Every word size, smallest first.
    pub const ALL: [WordBytes; 4] = [
        WordBytes::One,
        WordBytes::Two,
        WordBytes::Four,
        WordBytes::Eight,
    ];

    /// The word size in bytes.
    pub const fn bytes(self) -> u32 {
        match self {
            WordBytes::One => 1,
            WordBytes::Two => 2,
            WordBytes::Four => 4,
            WordBytes::Eight => 8,
        }
    }
}

impl TryFrom<u8> for WordBytes {
    type Error = WordBytesError;

    fn try_from(bytes: u8) -> Result<Self, Self::Error> {
        match bytes {
            1 => Ok(WordBytes::One),
            2 => Ok(WordBytes::Two),
            4 => Ok(WordBytes::Four),
            8 => Ok(WordBytes::Eight),
            got => Err(WordBytesError { got }),
        }
    }
}

impl From<WordBytes> for u8 {
    fn from(word: WordBytes) -> u8 {
        match word {
            WordBytes::One => 1,
            WordBytes::Two => 2,
            WordBytes::Four => 4,
            WordBytes::Eight => 8,
        }
    }
}

impl MemoryInit {
    /// True for [`MemoryInit::Zero`], the default, which is not serialized.
    pub const fn is_zero(&self) -> bool {
        matches!(self, MemoryInit::Zero)
    }
}

impl Preset {
    /// The configuration's id, such as `direct-mapped-8`.
    pub const fn id(self) -> &'static str {
        match self {
            Preset::DirectMapped8 => "direct-mapped-8",
            Preset::TwoWayLru => "two-way-lru",
            Preset::FourWayWriteBack => "four-way-write-back",
        }
    }

    /// The named configuration.
    pub fn config(self) -> Config {
        let (size_bytes, associativity) = match self {
            Preset::DirectMapped8 => (128, 1),
            Preset::TwoWayLru => (128, 2),
            Preset::FourWayWriteBack => (256, 4),
        };
        Config {
            cache: CacheConfig {
                size_bytes,
                line_bytes: 16,
                associativity,
                replacement: Replacement::Lru,
                write_policy: WritePolicy::WriteBack,
                allocate_policy: AllocatePolicy::WriteAllocate,
            },
            memory_init: MemoryInit::Pattern,
            ..Config::default()
        }
    }
}

/// The named configurations, in the design's order.
pub fn presets() -> [Preset; 3] {
    [
        Preset::DirectMapped8,
        Preset::TwoWayLru,
        Preset::FourWayWriteBack,
    ]
}

impl ConfigError {
    /// The label of [`Config::repair`]'s action when this is the first rule
    /// that failed: it names that rule's repair, read from the repaired
    /// configuration and its geometry.
    fn repair_label(&self, repaired: &Config, geometry: &Geometry) -> String {
        let size = repaired.cache.size_bytes;
        match self {
            ConfigError::AddressBits { .. } => {
                format!(
                    "Open with {}-bit addresses instead",
                    format::setting(repaired.address_bits)
                )
            }
            ConfigError::SizeNotPowerOfTwo { .. }
            | ConfigError::LineLargerThanCache { .. }
            | ConfigError::NoTagBits { .. }
            | ConfigError::TooManyLines { .. } => {
                format!(
                    "Open as {} {} B cache instead",
                    format::a_or_an(size),
                    format::setting(size)
                )
            }
            ConfigError::LineNotPowerOfTwo { .. }
            | ConfigError::LineSmallerThanWord { .. }
            | ConfigError::TooManyWords { .. } => {
                format!(
                    "Open with {} B lines instead",
                    format::setting(repaired.cache.line_bytes)
                )
            }
            ConfigError::AssociativityZero
            | ConfigError::AssociativityNotPowerOfTwo { .. }
            | ConfigError::AssociativityTooLarge { .. } => match geometry.placement {
                Placement::FullyAssociative => "Open as fully associative instead".to_owned(),
                Placement::DirectMapped => "Open as direct-mapped instead".to_owned(),
                Placement::SetAssociative => {
                    format!("Open with {}-way instead", format::setting(geometry.ways))
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
}

/// The associativities a cache of `lines` lines accepts, as the message for
/// one that is not a power of two lists them: every power of two up to the
/// line count, the last marked fully associative, as in "1, 2, 4 or 8 (fully
/// associative)".
fn associativity_choices(lines: u32) -> String {
    let choices: Vec<String> = std::iter::successors(Some(1_u32), |&ways| ways.checked_mul(2))
        .take_while(|&ways| ways <= lines)
        .map(format::setting)
        .collect();
    let text = match choices.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => format::setting(lines),
    };
    format!("{text} (fully associative)")
}

/// The replacement policy as the summaries name it.
fn replacement_name(replacement: Replacement) -> &'static str {
    match replacement {
        Replacement::Lru => "LRU",
        Replacement::Fifo => "FIFO",
        Replacement::Random => "Random",
    }
}

/// The name a timing message gives the number it is about.
fn timing_name(field: ConfigNumberField) -> &'static str {
    match field {
        ConfigNumberField::HitCycles => "Hit time",
        ConfigNumberField::MissCycles => "Miss penalty",
    }
}

/// Checks one timing number against its range.
fn check_cycles(field: ConfigNumberField, got: u32) -> Result<(), ConfigError> {
    if (MIN_CYCLES..=MAX_CYCLES).contains(&got) {
        Ok(())
    } else {
        Err(ConfigError::Timing { field, got })
    }
}

/// One timing number clamped to its range.
fn clamp_cycles(cycles: u32) -> u32 {
    cycles.clamp(MIN_CYCLES, MAX_CYCLES)
}

/// The largest power of two not above `n`; 1 for 0.
fn floor_power_of_two(n: u32) -> u32 {
    n.checked_ilog2().map_or(1, |bits| 1 << bits)
}

/// The base-2 logarithm of a power of two.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a u32 has at most 32 trailing zeros, which fits a u8"
)]
fn log2(power_of_two: u32) -> u8 {
    power_of_two.trailing_zeros() as u8
}

/// Step 5 of [`Config::repair`]: the largest power-of-two size, not above
/// `size`, whose line count is within `MAX_LINES` and whose tag keeps a bit,
/// with the associativity to go with it: the line count for a fully
/// associative cache, and otherwise `associativity` lowered to fit. A bounded
/// search over at most 32 sizes; one line always qualifies, because a line is
/// at most `MAX_WORDS_PER_LINE` words of at most 8 bytes and that is below the
/// narrowest address space, which a unit test of the limits checks.
fn fit_size(
    address_bits: u8,
    line: u32,
    size: u32,
    associativity: u32,
    fully_associative: bool,
) -> (u32, u32) {
    let mut candidate = size;
    while candidate > line {
        let lines = candidate / line;
        let ways = if fully_associative {
            lines
        } else {
            associativity.min(lines)
        };
        let used_bits = u32::from(log2(lines / ways)) + u32::from(log2(line));
        if lines <= MAX_LINES && u32::from(address_bits) > used_bits {
            return (candidate, ways);
        }
        candidate /= 2;
    }
    (line, 1)
}
