//! Every limit and tuning constant of the core, in one place.
//!
//! Validation, the parsers, the share loader, the session, the engine and the
//! stats read these constants; no value here is written as a literal anywhere
//! else, and the tests name the constants rather than their values. Messages
//! that state a limit print the constant, so changing a value changes the
//! wording with it.
//!
//! The values marked tunable are starting points that may be lowered after
//! measuring, down to the floors the shipped data and the tests need, or
//! raised within the relations the code relies on. The unit tests at the
//! bottom of this file check both, so a value moved too far fails here,
//! naming what needs it, before any other test does.

/// The narrowest byte address, in bits. Not tunable: the spec's range starts
/// here, and repair's smallest cache relies on it to keep a tag bit.
pub const MIN_ADDRESS_BITS: u8 = 8;

/// The widest byte address, in bits. Not tunable: the spec's range ends here,
/// and an address is a `u32`.
pub const MAX_ADDRESS_BITS: u8 = 32;

/// The most lines a cache may have, and so the most ways of a fully
/// associative cache. Every set must fit on screen, and the shadow cache that
/// classifies misses has this many entries. Tunable.
pub const MAX_LINES: u32 = 64;

/// The most words a line may hold. Bounds the width of a cache row and the
/// bytes one event can move. Tunable, within the relations tested below.
pub const MAX_WORDS_PER_LINE: u32 = 16;

/// The shortest hit time or miss penalty, in cycles. Not tunable: a zero cost
/// would make timed mode teach nothing.
pub const MIN_CYCLES: u32 = 1;

/// The longest hit time or miss penalty, in cycles. Keeps cycle counts exact
/// as JavaScript numbers over a whole session. Tunable.
pub const MAX_CYCLES: u32 = 1_000;

/// The largest seed for random replacement: 2^53 − 1. Not tunable: a larger
/// seed would not survive the trip through a JavaScript number.
pub const MAX_SEED: u64 = (1 << 53) - 1;

/// The most instructions one `Run` op may ask for, so a single op, whether
/// typed, read from a shared link or written in a module, is bounded work.
/// Tunable.
pub const MAX_RUN_STEPS: u32 = 1_000;

/// The largest program or trace source, in bytes (16 KiB), so parsing a
/// pasted or shared program is bounded. Tunable.
pub const MAX_PROGRAM_BYTES: u32 = 16 * 1024;

/// The most source lines in a program or trace, so parsing a pasted or shared
/// program is bounded however short its lines are. Tunable.
pub const MAX_PROGRAM_LINES: u32 = 1_000;

/// The most instructions in one assembled program, so a pasted or shared
/// program is bounded in the memory it takes and in the listing the page
/// draws. Tunable.
pub const MAX_PROGRAM_INSTRUCTIONS: u32 = 500;

/// The longest label, in Unicode scalar values, so every label fits the
/// program listing and the messages that quote it. Tunable.
pub const MAX_LABEL_CHARS: u32 = 32;

/// The most errors one parse reports; the rest are dropped, so pasting text
/// that is not a program gives a short list the student can read rather than
/// an error for every line. Tunable.
pub const MAX_PARSE_ERRORS: u32 = 20;

/// The instructions one program load may execute before it is stopped, so a
/// loop without an exit ends with a message instead of running forever.
/// Counted per `Program` load and reset by a new run. Tunable.
pub const INSTRUCTION_BUDGET: u32 = 10_000;

/// The most micro-events one session stores, over the whole log. The main
/// lever on memory use and on the time a shared link takes to load. Tunable.
pub const SESSION_EVENT_LIMIT: u32 = 100_000;

/// The most events one entry can produce. Not tunable: it is a bound the
/// engine guarantees (an access has at most ten events), and the session's
/// room check relies on it.
pub const MAX_EVENTS_PER_ENTRY: u32 = 16;

/// Events between checkpoints, and so the longest quiet replay a seek needs.
/// Trades memory against seek time. Tunable.
pub const CHECKPOINT_INTERVAL: u32 = 2_000;

/// Events one step of a sliced share load replays, one step per animation
/// frame. Trades responsiveness against load time. Tunable.
pub const LOAD_SLICE_EVENTS: u32 = 2_000;

/// Bytes of program source that take about as long to parse as one event
/// takes to replay. A sliced share load counts the parsing an op does as
/// this many bytes an event: a `Program` op parses its source, and a
/// `Config` op parses the loaded program again under its configuration,
/// work its one `Setup` event does not measure. So a slice of such ops is no
/// longer than a slice of events. Tunable, after measuring both.
pub const PARSE_BYTES_PER_EVENT: u32 = 64;

/// The largest share string, in bytes (64 KiB), so decoding a link is bounded
/// whatever was pasted. Tunable.
pub const MAX_SHARE_TEXT_BYTES: u32 = 64 * 1024;

/// The largest inflated share document, and the largest loaded file, in bytes
/// (1 MiB). It is also the limit of the inflate itself, so a short link cannot
/// decompress into unbounded memory. Tunable.
pub const MAX_SHARE_JSON_BYTES: u32 = 1024 * 1024;

/// The most ops one share document may hold, so reading and checking a link
/// or a file is bounded before any of it replays. Tunable.
pub const MAX_SHARE_OPS: u32 = 10_000;

/// The access outcomes the hit and miss sparkline shows. Tunable; the design
/// shows 24.
pub const RECENT_OUTCOMES: u32 = 24;

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail
    #![allow(clippy::assertions_on_constants)] // the constants are what these tests check

    use super::*;
    use crate::config::{presets, WordBytes};

    /// Integers below this survive a JavaScript number exactly.
    const JS_EXACT: u64 = 1 << 53;

    /// The widest word, in bytes.
    fn widest_word() -> u64 {
        WordBytes::ALL
            .iter()
            .map(|word| u64::from(word.bytes()))
            .max()
            .unwrap_or_default()
    }

    #[test]
    fn ranges_are_ordered() {
        assert!(MIN_ADDRESS_BITS <= MAX_ADDRESS_BITS);
        assert!(
            u32::from(MAX_ADDRESS_BITS) <= u32::BITS,
            "an address is a u32"
        );
        assert!(MIN_CYCLES >= 1, "a zero cost is not a cost");
        assert!(MIN_CYCLES <= MAX_CYCLES);
        assert!(
            MAX_SEED < JS_EXACT,
            "a seed crosses to JavaScript as a number"
        );
    }

    #[test]
    fn the_largest_line_leaves_a_tag_bit_at_the_narrowest_address() {
        // Repair's smallest cache, one line, keeps a tag bit, and a fully
        // associative cache is never refused for having none.
        assert!(u64::from(MAX_WORDS_PER_LINE) * widest_word() < 1 << MIN_ADDRESS_BITS);
    }

    #[test]
    fn cycle_counts_stay_exact_in_javascript() {
        // No event charges more than MAX_CYCLES.
        assert!(u64::from(SESSION_EVENT_LIMIT) * u64::from(MAX_CYCLES) < JS_EXACT);
    }

    #[test]
    fn traffic_byte_counts_stay_exact_in_javascript() {
        // No event moves more than one line of the widest words.
        let largest_line = u64::from(MAX_WORDS_PER_LINE) * widest_word();
        assert!(u64::from(SESSION_EVENT_LIMIT) * largest_line < JS_EXACT);
    }

    #[test]
    fn an_empty_session_has_room_for_an_entry() {
        assert!(MAX_EVENTS_PER_ENTRY < SESSION_EVENT_LIMIT);
    }

    #[test]
    fn intervals_and_windows_are_not_zero() {
        assert!(
            CHECKPOINT_INTERVAL > 0,
            "the session checkpoints every CHECKPOINT_INTERVAL events"
        );
        assert!(LOAD_SLICE_EVENTS > 0, "a sliced load must make progress");
        assert!(
            PARSE_BYTES_PER_EVENT > 0,
            "a sliced load divides the bytes it parses by PARSE_BYTES_PER_EVENT"
        );
        assert!(
            RECENT_OUTCOMES > 0,
            "the sparkline shows at least the latest access"
        );
    }

    #[test]
    fn the_named_configurations_fit_the_limits() {
        for preset in presets() {
            let config = preset.config();
            assert!(
                config.validate().is_ok(),
                "a limit is below what `{}` needs",
                preset.id()
            );
            let lines = config.cache.size_bytes / config.cache.line_bytes;
            assert!(
                lines <= MAX_LINES,
                "MAX_LINES is below the {} lines of `{}`",
                lines,
                preset.id()
            );
        }
    }

    #[test]
    fn the_shipped_data_fits_the_limits() {
        assert!(
            MAX_LINES >= 16,
            "module 06 starts with 8 B lines in a 128 B cache: 16 lines"
        );
        assert!(
            MAX_WORDS_PER_LINE >= 8,
            "module 06 uses 32 B lines of 4-byte words: 8 words"
        );
        assert!(
            MAX_CYCLES >= 20,
            "the fallback timing, module 09 and the timed golden fixture charge 20-cycle misses"
        );
        assert!(
            MAX_PROGRAM_INSTRUCTIONS >= 6,
            "frame 3b's program and the count-down-loop golden fixture have six instructions"
        );
        assert!(
            MAX_PROGRAM_LINES >= 7,
            "the count-down-loop golden fixture has seven lines"
        );
        assert!(
            MAX_PROGRAM_BYTES >= 196,
            "the count-down-loop golden fixture is 196 bytes"
        );
        assert!(
            INSTRUCTION_BUDGET >= 12,
            "the count-down-loop golden fixture executes 12 instructions"
        );
        assert!(
            MAX_LABEL_CHARS >= 5,
            "the design's first-visit program labels its first line `start`"
        );
    }

    #[test]
    fn the_unit_tests_fit_the_limits() {
        // A test that stays below a limit relies on it without naming it, and
        // that reliance is a floor like the shipped data's.
        assert!(
            RECENT_OUTCOMES >= 5,
            "the engine's stats test shows all five accesses of its run, with no suffix"
        );
        assert!(
            MAX_PROGRAM_INSTRUCTIONS >= 10 && MAX_PROGRAM_LINES >= 10 && MAX_PROGRAM_BYTES >= 227,
            "the engine's strided-loop program has ten instructions on ten lines, 227 bytes"
        );
        assert!(
            INSTRUCTION_BUDGET >= 45,
            "the engine's strided-loop program executes 45 instructions"
        );
        assert!(MAX_LABEL_CHARS >= 7, "tests/isa.rs branches to `nowhere`");
        assert!(
            MAX_PARSE_ERRORS >= 4,
            "tests/isa.rs lists the four errors of one program"
        );
        assert!(
            MAX_RUN_STEPS >= 12,
            "the sample sessions of tests/session.rs and tests/share.rs run their program with \
             a `Run` of 12"
        );
        // The parser tests build sources one over a limit that must stay
        // within the others, or another limit's error would come first. Each
        // relation below names the lines a test writes, newline included.
        assert!(
            MAX_PARSE_ERRORS + 3 <= MAX_PROGRAM_LINES
                && u64::from(MAX_PARSE_ERRORS + 3) * 6 <= u64::from(MAX_PROGRAM_BYTES),
            "the parse tests write one error a line, `lw r1` in 6 bytes, three past the \
             errors a result lists"
        );
        assert!(
            MAX_PROGRAM_INSTRUCTIONS + 2 <= MAX_PROGRAM_LINES
                && u64::from(MAX_PROGRAM_INSTRUCTIONS + 2) * 15 <= u64::from(MAX_PROGRAM_BYTES),
            "tests/isa.rs writes one instruction a line, `add r1, r1, #1` in 15 bytes, two past \
             the instruction limit"
        );
        assert!(
            u64::from(MAX_PROGRAM_LINES) * 7 + 14 <= u64::from(MAX_PROGRAM_BYTES),
            "tests/isa.rs writes `add r1, r1, #1`, then `; more` lines of 7 bytes until one \
             past the line limit"
        );
        // tests/share.rs fills the session from a link of MAX_SHARE_OPS / 2
        // pairs: a loop without an exit loaded, then a StepAccess that runs
        // it to the end of its budget, one event per instruction and the
        // budget's error, in fewer than 120 bytes of JSON a pair.
        assert!(
            u64::from(MAX_SHARE_OPS / 2) * u64::from(INSTRUCTION_BUDGET + 2)
                > u64::from(SESSION_EVENT_LIMIT),
            "tests/share.rs fills the session with MAX_SHARE_OPS / 2 program loads, each run \
             to the end of its budget"
        );
        assert!(
            u64::from(MAX_SHARE_OPS / 2) * 120 <= u64::from(MAX_SHARE_JSON_BYTES),
            "tests/share.rs writes MAX_SHARE_OPS / 2 pairs of ops in under 120 bytes each"
        );
        assert!(
            u64::from(MAX_LABEL_CHARS) * 2 + 11 <= u64::from(MAX_PROGRAM_BYTES),
            "tests/isa.rs defines a label at the limit and branches to it on one line, \
             `{{label}}: bnez r0, {{label}}`"
        );
    }
}
