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

/// The narrowest byte address, in bits. Not tunable: repair's smallest cache
/// relies on it to keep a tag bit.
pub const MIN_ADDRESS_BITS: u8 = 8;

/// The widest byte address, in bits. Not tunable: an address is a `u32`.
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

/// The instructions one program load may execute before it is stopped.
///
/// A loop without an exit ends with a message instead of running forever.
/// Counted per `Program` load and reset by a new run. Tunable.
pub const INSTRUCTION_BUDGET: u32 = 10_000;

/// The most micro-events one session stores, over the whole log.
///
/// The main lever on memory use and on the time a shared link takes to load.
/// Tunable. Set so that the heaviest logs measured stay under 64 MB at their
/// peak: runs of accesses that each move every rank of a set of `MAX_LINES`
/// ways, where one event in four is a `Rank` of about 2 KB that lists every
/// way in its changed cells and in its log line; and runs that touch a new
/// 128-byte line every few events. Kept below 65,536, past which the events a
/// share load gathers double their room to 131,072 before they move into the
/// session.
///
/// A saved file keeps room for a cursor this many events into an op, so a
/// raise that gives this value another digit takes a byte from every saved
/// file's room: see [`MAX_SHARE_JSON_BYTES`].
pub const SESSION_EVENT_LIMIT: u32 = 40_000;

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

/// Bytes of program source that parse in about the time one event replays.
///
/// A sliced share load counts the parsing an op does as this many bytes an
/// event: a `Program` op parses its source, and a `Config` op parses the
/// loaded program again under its configuration, work its one `Setup` event
/// does not measure. So a slice of such ops is no longer than a slice of
/// events. Tunable, after measuring both.
pub const PARSE_BYTES_PER_EVENT: u32 = 64;

/// The largest share string, in bytes (64 KiB), so decoding a link is bounded
/// whatever was pasted. Tunable.
pub const MAX_SHARE_TEXT_BYTES: u32 = 64 * 1024;

/// The largest inflated share document or loaded file, in bytes (1 MiB).
///
/// It is also the limit of the inflate itself, so a short link cannot
/// decompress into unbounded memory; and the most a session's saved file may
/// hold, so every session can be saved and opened again: the session refuses
/// an op that could take its file past it
/// ([`OpError::FileFull`](crate::OpError::FileFull)). Tunable.
///
/// A file's ops are checked against this limit less the rest of the file at
/// its longest, which includes the longest place a cursor can have: event
/// [`SESSION_EVENT_LIMIT`] of op [`MAX_SHARE_OPS`] − 1. A load checks a
/// file's ops against the same room. So raising either count until it takes
/// another digit leaves every file a byte less room, and a file saved at the
/// limit before no longer opens, unless
/// [`SCHEMA_VERSION`](crate::SCHEMA_VERSION) is bumped and the loader bounds
/// the older version's files by the room they kept. A test below fails on
/// such a raise.
pub const MAX_SHARE_JSON_BYTES: u32 = 1024 * 1024;

/// The most ops one share document, and one session's log, may hold.
///
/// Bounding a document's ops keeps reading and checking a link or a file
/// bounded before any of it replays, and bounding a log's ops keeps every
/// session's log within a link. Tunable, within the relations tested below.
///
/// A saved file keeps room for a cursor in its last op, whose index is one
/// less than this, so a raise that gives that index another digit takes a
/// byte from every saved file's room: see [`MAX_SHARE_JSON_BYTES`].
pub const MAX_SHARE_OPS: u32 = 10_000;

/// The access outcomes the hit and miss sparkline shows. Tunable.
pub const RECENT_OUTCOMES: u32 = 24;

/// The deepest predicates may nest inside `all`, `any` and `not`.
///
/// Reading a predicate is then bounded work, and a bounded depth of
/// recursion, whatever it comes from: `serde_json` stops module JSON nested
/// deeper than 128, but a predicate from JavaScript is read with no such
/// limit, and one nested a few thousand deep would exhaust the stack.
/// Tunable, down to 128, so that no module JSON meets this limit before
/// `serde_json`'s.
pub const MAX_PREDICATE_DEPTH: u32 = 128;

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
    fn a_log_of_short_ops_reaches_the_link_limit_before_the_event_limit() {
        // Every op yields at least one event, so the event limit alone would
        // let a log hold SESSION_EVENT_LIMIT ops, more than a link can hold.
        // The session refuses an op past MAX_SHARE_OPS too, so every log
        // fits a link. At one event an op, a log that long still has room
        // for an entry, so it is the link's limit that refuses the next op;
        // a log of longer ops reaches the event limit first.
        assert!(
            u64::from(MAX_SHARE_OPS) + u64::from(MAX_EVENTS_PER_ENTRY)
                <= u64::from(SESSION_EVENT_LIMIT)
        );
        // An empty session takes Space's first press: the drawer's program
        // loaded, then a step, two ops.
        assert!(MAX_SHARE_OPS >= 2);
    }

    /// The digits of `value` written in decimal.
    fn decimal_digits(value: u32) -> u32 {
        value.checked_ilog10().map_or(1, |log| log + 1)
    }

    #[test]
    fn a_saved_file_keeps_its_room_for_the_cursor() {
        // A saved file keeps room for the longest place a cursor can have,
        // `{"op":MAX_SHARE_OPS-1,"event":SESSION_EVENT_LIMIT}`, and a load
        // checks a file's ops against that same room. The two numbers take
        // nine digits together in the files of schema version 1. One more
        // digit would leave every file a byte less room, so a file saved at
        // the limit before would no longer open: raise a count that far only
        // with a new SCHEMA_VERSION whose loader bounds the older version's
        // files by the room they kept, then update this bound.
        assert!(
            decimal_digits(MAX_SHARE_OPS.saturating_sub(1)) + decimal_digits(SESSION_EVENT_LIMIT)
                <= 9,
            "a saved file keeps room for a cursor place of MAX_SHARE_OPS - 1 and \
             SESSION_EVENT_LIMIT in nine digits"
        );
    }

    #[test]
    fn module_json_meets_serde_jsons_nesting_limit_before_the_predicate_depth() {
        // serde_json refuses JSON nested deeper than 128, and each predicate
        // inside another is at least one level of JSON deeper, so module JSON
        // behaves as if predicates had no depth limit of their own.
        assert!(
            MAX_PREDICATE_DEPTH >= 128,
            "a module whose JSON serde_json reads must not meet MAX_PREDICATE_DEPTH"
        );
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
            MAX_PROGRAM_INSTRUCTIONS >= 16 && MAX_PROGRAM_LINES >= 16,
            "module 06's trace loads sixteen words; module 09's has ten"
        );
        assert!(
            MAX_RUN_STEPS >= 16,
            "modules 06 and 09 run their traces with a Run of 16 and 10"
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
            "the page's first-visit program labels its first line `start`"
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
        // tests/session.rs and tests/share.rs fill a log with MAX_SHARE_OPS
        // loads by hand of one word: a miss of five events, then hits of
        // three. The log must still have room for an entry, so that the
        // next op is refused for the link's limit, and a link must open it.
        assert!(
            5 + u64::from(MAX_SHARE_OPS.saturating_sub(1)) * 3 + u64::from(MAX_EVENTS_PER_ENTRY)
                <= u64::from(SESSION_EVENT_LIMIT),
            "tests/session.rs and tests/share.rs fill a log with MAX_SHARE_OPS loads by hand, \
             a miss of five events then hits of three, with room left for an entry"
        );
        assert!(
            u64::from(MAX_SHARE_OPS) * 64 + 1024 <= u64::from(MAX_SHARE_JSON_BYTES),
            "tests/session.rs and tests/share.rs fill a log with MAX_SHARE_OPS loads by hand, \
             which its saved file and its link must hold, in under 64 bytes of JSON each and \
             1 KiB for the rest of the document"
        );
        // tests/share.rs and tests/session.rs fill a saved file until the
        // session refuses an op for the file's limit, which must come before
        // the link's and the session's, each op a program load of one event
        // or a configuration change of one.
        assert!(
            MAX_SHARE_JSON_BYTES / (MAX_PROGRAM_BYTES / 2) < MAX_SHARE_OPS,
            "tests/share.rs and tests/session.rs load programs of at least half \
             MAX_PROGRAM_BYTES until the session refuses one for the saved file's limit"
        );
        assert!(
            MAX_SHARE_JSON_BYTES / 200 < MAX_SHARE_OPS
                && u64::from(MAX_SHARE_JSON_BYTES / 200) + u64::from(MAX_EVENTS_PER_ENTRY)
                    <= u64::from(SESSION_EVENT_LIMIT),
            "tests/share.rs changes the configuration, each change over 200 bytes of a saved \
             file, until the session refuses one for the file's limit"
        );
        assert!(
            u64::from(MAX_LABEL_CHARS) * 2 + 11 <= u64::from(MAX_PROGRAM_BYTES),
            "tests/isa.rs defines a label at the limit and branches to it on one line, \
             `{{label}}: bnez r0, {{label}}`"
        );
    }
}
