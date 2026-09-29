//! Properties, over generated configurations and generated sessions.
//!
//! A configuration is valid by construction, across every placement,
//! replacement and write policy, word size and address width, timed or not,
//! with either memory contents. A session is a configuration and what a
//! student does to it: accesses issued by hand, some of them malformed;
//! programs and traces from templates, and their steps and runs; edits, each
//! planned first and applied when the plan or its suggestion allows;
//! configuration changes, Reset and mode switches; and undo, redo,
//! scrubbing, seeking, Space and Shift+Space. Each action is read against
//! the session as it stands, and every address it names falls in a pool of
//! three times the cache's lines, so hits, conflicts and dirty evictions all
//! happen. A long session starts with long runs of a loop, and after what
//! the student does its log is cut back and grown again, so that it crosses
//! the checkpoints the session keeps, before the cut and after.
//!
//! The fold is a pure function, and undo, redo, scrubbing, seeking and
//! Space reach the machines it reaches, from checkpoints too; share strings
//! and saved files give the same session back, a saved file being the
//! compact JSON a link deflates, of the size the session keeps after every
//! call and within what a file can hold; the cache's invariants and
//! the counters' identities hold at every event; every event lists exactly
//! the cells it changed; a refused call changes nothing, and an accepted one
//! never rewrites the log before the point it appends at; a planned edit
//! applies and drops no dirty data without saying so, and one the plan
//! refuses is refused as an op too, for want of room in the log as well,
//! near each of its limits; every configuration repairs to a valid one,
//! whose geometry splits and rebuilds every address; the parsers take any
//! text; and every input type round-trips through JSON.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use cache_explorer_core::limits::MAX_PARSE_ERRORS;
use cache_explorer_core::{
    parse_module_index, AccessKind, Addr, AllocatePolicy, Boundary, ChangeRef, Config, EventKind,
    EventPos, LineAddr, MemoryWordView, MicroEvent, Module, ModuleRun, Op, Outcome, ParseResult,
    Predicate, ProgramFormat, ProgramSource, Session, SetIndex, ShareDoc, StepOutcome, Tag,
    WayIndex, WayLoc, WayPredicate, Word, WritePolicy,
};
use proptest::prelude::*;

use support::{
    arb_access_match, arb_config, arb_config_match, arb_edit_patch, arb_edit_target,
    arb_event_match, arb_long_session, arb_near_limit, arb_op, arb_predicate, arb_raw_config,
    arb_session, arb_share_doc, arb_step, arb_text, arb_valid_config, arb_way_form, arb_way_patch,
    cells, changes, check, checked_positions, end, event_kinds, every_position, load_in_slices,
    number, number_fields, op_ends, op_start, play, play_long, registers, round_trip, seeks_reach,
    undo_stops, without_mode, Checks, Fold, Limits, COUNTERS,
};

/// How the properties over whole sessions run: with a quarter of the cases
/// proptest runs by default, or of `PROPTEST_CASES` when it is set, since
/// each case builds a session and walks every event of it. They run through
/// [`check`], which keeps that quarter. A failure is printed with its
/// smallest case rather than written to a file beside this one; once the bug
/// is fixed, that case becomes a test of its own.
fn sessions() -> ProptestConfig {
    let base = values();
    ProptestConfig {
        cases: (base.cases / 4).max(1),
        ..base
    }
}

/// How the property over long sessions runs: with a sixteenth of the cases
/// of those over whole sessions, since each builds a log several checkpoint
/// intervals long and replays it from position 0 to every position it
/// checks.
fn long_sessions() -> ProptestConfig {
    let base = sessions();
    ProptestConfig {
        cases: (base.cases / 16).max(1),
        ..base
    }
}

/// How the property over the limits of the log runs: with half the cases of
/// that over long sessions, since each walks logs tens of thousands of
/// events long up to each limit and past it, cloning them at every step.
fn near_limits() -> ProptestConfig {
    let base = long_sessions();
    ProptestConfig {
        cases: (base.cases / 2).max(1),
        ..base
    }
}

/// How the properties over single values run, which cost little: with as
/// many cases as proptest runs by default, or `PROPTEST_CASES`.
fn values() -> ProptestConfig {
    ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// The tests' helpers and generators. They sit in a module compiled for
/// tests only, where a failed `unwrap` is a failed test, as in the tests
/// themselves.
#[cfg(test)]
mod support {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt::Debug;

    use cache_explorer_core::limits::{
        CHECKPOINT_INTERVAL, INSTRUCTION_BUDGET, LOAD_SLICE_EVENTS, MAX_CYCLES,
        MAX_EVENTS_PER_ENTRY, MAX_LINES, MAX_PROGRAM_BYTES, MAX_RUN_STEPS, MAX_SEED,
        MAX_SHARE_JSON_BYTES, MAX_SHARE_OPS, MAX_WORDS_PER_LINE, MIN_ADDRESS_BITS, MIN_CYCLES,
        SESSION_EVENT_LIMIT,
    };
    use cache_explorer_core::{
        AccessKind, AccessMatch, Addr, AddrLoc, AllPredicate, AllocatePolicy, AnyPredicate,
        Boundary, CacheConfig, Callout, CalloutArea, CellRef, ChangeRef, CmpOp, Config,
        ConfigChange, ConfigMatch, ConfigNumberField, ConfigPredicate, Consequence, Counter,
        CounterPredicate, Counters, EditPatch, EditPlan, EditTarget, EventKind, EventMatch,
        EventPos, EventPredicate, Geometry, LastAccessPredicate, LinePredicate, LoadProgress,
        Machine, MemoryInit, MemoryPredicate, MicroEvent, NotPredicate, Op, OpError, Outcome,
        Position, Predicate, Preset, ProgramFormat, ProgramSource, RefusalReason, Register,
        RegisterPredicate, Replacement, Session, ShareAt, ShareDoc, ShareLoad, State, Step,
        StepOutcome, Timeline, Timing, Tone, WayForm, WayLoc, WayPatch, WayPredicate, WayView,
        WordBytes, WritePolicy,
    };
    use proptest::collection::vec;
    use proptest::option;
    use proptest::prelude::*;
    use proptest::sample::select;
    use proptest::test_runner::{TestCaseError, TestRunner};
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use serde_json::json;

    /// The widest address width the generated configurations start from;
    /// a width is raised past it only when the tag needs a bit.
    const WIDEST_ADDRESS: u8 = 16;

    /// The longest hit time generated, so that misses dominate the cycles.
    const LONGEST_HIT: u32 = 3;

    /// The longest miss penalty generated.
    const LONGEST_MISS: u32 = 50;

    /// The most instructions a generated `Run` asks for.
    const LONGEST_RUN: u32 = 20;

    /// The most actions a generated session has.
    const ACTIONS: usize = 64;

    /// The most actions a student takes in a long session, between its long
    /// runs.
    const LONG_ACTIONS: usize = 16;

    /// How many positions of a long session are picked at random to check.
    const PICKS: usize = 8;

    /// Every counter, in the order the Stats region names them.
    pub(crate) const COUNTERS: [Counter; 14] = [
        Counter::Accesses,
        Counter::Loads,
        Counter::Stores,
        Counter::Hits,
        Counter::Misses,
        Counter::Evictions,
        Counter::Writebacks,
        Counter::Writethroughs,
        Counter::Writearounds,
        Counter::Fills,
        Counter::Cycles,
        Counter::Compulsory,
        Counter::Capacity,
        Counter::Conflict,
    ];

    /// `value` as one of the core's number types, read as JSON reads it:
    /// unchecked, as an op or a predicate from outside carries it.
    pub(crate) fn number<T: DeserializeOwned>(value: u64) -> T {
        serde_json::from_value(value.into()).unwrap()
    }

    /// A value of the core read from `value`, as JSON from outside is read.
    fn read<T: DeserializeOwned>(value: serde_json::Value) -> T {
        serde_json::from_value(value).unwrap()
    }

    /// A test failure saying `error`.
    fn fail(error: impl std::fmt::Display) -> TestCaseError {
        TestCaseError::fail(error.to_string())
    }

    /// Checks `property` of the values `strategy` generates, as many as
    /// `config` says. The `proptest!` macro would read `PROPTEST_CASES`
    /// again over `config`, which runs the properties over whole sessions
    /// as many times as those over single values; a runner of their own
    /// keeps their share of the cases.
    #[track_caller]
    pub(crate) fn check<S: Strategy>(
        config: ProptestConfig,
        strategy: &S,
        property: impl Fn(S::Value) -> Result<(), TestCaseError>,
    ) {
        let mut runner = TestRunner::new(config);
        if let Err(error) = runner.run(strategy, property) {
            panic!("{error}\n{runner}");
        }
    }

    /// `counter`'s value in `counters`.
    fn count(counters: &Counters, counter: Counter) -> u64 {
        match counter {
            Counter::Accesses => counters.accesses.into(),
            Counter::Loads => counters.loads.into(),
            Counter::Stores => counters.stores.into(),
            Counter::Hits => counters.hits.into(),
            Counter::Misses => counters.misses.into(),
            Counter::Evictions => counters.evictions.into(),
            Counter::Writebacks => counters.writebacks.into(),
            Counter::Writethroughs => counters.writethroughs.into(),
            Counter::Writearounds => counters.writearounds.into(),
            Counter::Fills => counters.fills.into(),
            Counter::Cycles => counters.cycles.get(),
            Counter::Compulsory => counters.compulsory.into(),
            Counter::Capacity => counters.capacity.into(),
            Counter::Conflict => counters.conflict.into(),
        }
    }

    // --- Configurations -------------------------------------------------

    /// A configuration valid by construction: any word size; lines of one
    /// word up to as many as a line may hold; up to as many lines as a cache
    /// may hold, in any placement; an address width from the narrowest up
    /// to 16 bits, raised until the tag has a bit; any policies and memory
    /// contents; timed or not, with short hit times; any seed.
    pub(crate) fn arb_config() -> impl Strategy<Value = Config> {
        let shape = (
            select(WordBytes::ALL.to_vec()),
            0..=MAX_WORDS_PER_LINE.ilog2(),
            0..=MAX_LINES.ilog2(),
            any::<u32>(),
            MIN_ADDRESS_BITS..=WIDEST_ADDRESS,
        );
        let timing = (
            MIN_CYCLES..=LONGEST_HIT.min(MAX_CYCLES),
            MIN_CYCLES..=LONGEST_MISS.min(MAX_CYCLES),
        )
            .prop_map(|(hit_cycles, miss_cycles)| Timing {
                hit_cycles,
                miss_cycles,
            });
        let rest = (
            arb_policies(),
            option::of(timing),
            0..=MAX_SEED,
            any::<bool>(),
        );
        (shape, rest).prop_map(|(shape, rest)| {
            let (word_bytes, words, lines, ways, width) = shape;
            let (policies, timing, seed, advanced) = rest;
            let (replacement, write_policy, allocate_policy, memory_init) = policies;
            let line_bytes = word_bytes.bytes() << words;
            let ways = ways % (lines + 1);
            // One tag bit at least above the index and the offset.
            let needed = lines - ways + line_bytes.ilog2() + 1;
            Config {
                address_bits: width.max(u8::try_from(needed).unwrap()),
                word_bytes,
                cache: CacheConfig {
                    size_bytes: line_bytes << lines,
                    line_bytes,
                    associativity: 1 << ways,
                    replacement,
                    write_policy,
                    allocate_policy,
                },
                timing,
                seed,
                advanced,
                memory_init,
            }
        })
    }

    /// Any configuration: every field drawn from anywhere, zeros and values
    /// far out of range included.
    pub(crate) fn arb_raw_config() -> impl Strategy<Value = Config> {
        let bytes = || {
            prop_oneof![
                Just(0_u32),
                1_u32..=4096,
                (0_u32..32).prop_map(|power| 1 << power),
                any::<u32>(),
            ]
        };
        let cycles = || {
            prop_oneof![
                Just(0_u32),
                MIN_CYCLES..=MAX_CYCLES.saturating_add(1),
                any::<u32>(),
            ]
        };
        let timing = (cycles(), cycles()).prop_map(|(hit_cycles, miss_cycles)| Timing {
            hit_cycles,
            miss_cycles,
        });
        let shape = (
            any::<u8>(),
            select(WordBytes::ALL.to_vec()),
            bytes(),
            bytes(),
            bytes(),
        );
        let rest = (
            arb_policies(),
            option::of(timing),
            prop_oneof![0..=MAX_SEED, any::<u64>()],
            any::<bool>(),
        );
        (shape, rest).prop_map(|(shape, rest)| {
            let (address_bits, word_bytes, size_bytes, line_bytes, associativity) = shape;
            let (policies, timing, seed, advanced) = rest;
            let (replacement, write_policy, allocate_policy, memory_init) = policies;
            Config {
                address_bits,
                word_bytes,
                cache: CacheConfig {
                    size_bytes,
                    line_bytes,
                    associativity,
                    replacement,
                    write_policy,
                    allocate_policy,
                },
                timing,
                seed,
                advanced,
                memory_init,
            }
        })
    }

    /// Any replacement, write and allocate policies and memory contents.
    fn arb_policies(
    ) -> impl Strategy<Value = (Replacement, WritePolicy, AllocatePolicy, MemoryInit)> {
        (
            select(vec![
                Replacement::Lru,
                Replacement::Fifo,
                Replacement::Random,
            ]),
            select(vec![WritePolicy::WriteBack, WritePolicy::WriteThrough]),
            select(vec![
                AllocatePolicy::WriteAllocate,
                AllocatePolicy::WriteNoAllocate,
            ]),
            select(vec![MemoryInit::Zero, MemoryInit::Pattern]),
        )
    }

    /// A valid configuration: one generated valid, or the repair of any
    /// configuration, whose address widths reach the widest.
    pub(crate) fn arb_valid_config() -> impl Strategy<Value = Config> {
        prop_oneof![
            arb_config(),
            arb_raw_config()
                .prop_map(|config| config.repair().map_or(config, |repair| repair.config)),
        ]
    }

    // --- The pool of addresses ------------------------------------------

    /// The geometry of the run the cursor is in.
    fn geometry(session: &Session) -> Geometry {
        session.config().validate().unwrap()
    }

    /// How many lines the addresses a student names are drawn from: three
    /// times the cache's, so that sets overflow, or as many as the addresses
    /// hold if that is fewer.
    fn pool_lines(geometry: &Geometry) -> u32 {
        let space = (1_u64 << geometry.address_bits()) / u64::from(geometry.line_bytes());
        let pool = u64::from(geometry.lines()) * 3;
        u32::try_from(space.min(pool)).unwrap()
    }

    /// The line of the pool `pick` names: a quarter of the picks name one
    /// of four lines, which then hit; a quarter one of one more line than a
    /// set has ways, in each of the first two sets, which then conflict and
    /// are evicted; and the rest any line.
    fn pool_line(geometry: &Geometry, pick: u32) -> u32 {
        let choice = pick >> 2;
        let tags = geometry.ways() + 1;
        let line = match pick % 4 {
            0 => choice % 4,
            1 => choice % tags * geometry.sets() + choice / tags % 2,
            _ => choice,
        };
        line % pool_lines(geometry)
    }

    /// The address of word `word` of the line of the pool `line` picks, the
    /// word taken modulo how many a line has.
    fn pool_addr(geometry: &Geometry, line: u32, word: u32) -> u32 {
        pool_line(geometry, line) * geometry.line_bytes()
            + (word % geometry.words_per_line()) * geometry.word_bytes()
    }

    /// The largest value a word holds.
    fn word_max(geometry: &Geometry) -> u64 {
        u64::MAX >> (64 - 8 * geometry.word_bytes())
    }

    /// The number an address or a line's text shows, `0x0A4`.
    fn hex(text: &str) -> u32 {
        u32::from_str_radix(text.trim_start_matches("0x"), 16).unwrap()
    }

    /// The number a data value's text shows, `10,000`.
    fn data(text: &str) -> u64 {
        text.replace(',', "").parse().unwrap()
    }

    // --- Actions --------------------------------------------------------

    /// What a student does next. Its numbers are read against the session
    /// as it stands when it is done: an address is a word of a line of the
    /// pool, a way is one [`pick_way`] picks, a position or a count of ops
    /// is taken modulo how many there are, and a value is cut to the word.
    #[derive(Clone, Debug)]
    pub(crate) enum Action {
        /// An access issued by hand.
        Issue(IssueChoice),
        /// A program or trace loaded from a template.
        Program(ProgramChoice),
        /// One instruction.
        Step,
        /// Instructions until an access completes.
        StepAccess,
        /// Up to `n` instructions; none is refused.
        Run { n: u32 },
        /// An edit, planned first.
        Edit(EditIntent),
        /// A configuration change, Reset or a mode switch.
        Change(Change),
        /// Undo, as many times.
        Undo { times: u32 },
        /// Undo until nothing is left to undo.
        UndoAll,
        /// Redo, as many times.
        Redo { times: u32 },
        /// A seek to a position, modulo how many there are.
        Seek { to: u32 },
        /// A scrub to a count of ops, modulo how many there are.
        Scrub { to: u32 },
        /// Space, with the drawer holding a program or empty.
        Space { drawer: Option<ProgramChoice> },
        /// Shift+Space, likewise.
        ShiftSpace { drawer: Option<ProgramChoice> },
    }

    /// An access issued by hand, perhaps one the session refuses.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct IssueChoice {
        /// A store rather than a load.
        store: bool,
        /// The line of the pool.
        line: u32,
        /// The word of the line.
        word: u32,
        /// A store's value.
        value: u64,
        /// What is wrong with it, if anything.
        flaw: Option<Flaw>,
    }

    /// What makes an issued access one the session refuses.
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum Flaw {
        /// An address one byte past a word's start; with 1-byte words it is
        /// aligned, and the access is issued.
        Misaligned,
        /// The first address past the address width.
        OutOfRange,
        /// A store without a value, or a load with one.
        Value,
        /// A value the word cannot hold.
        TooWide,
    }

    impl IssueChoice {
        /// The op, and the address it names when that fits the addresses.
        fn op(self, geometry: &Geometry) -> (Op, Option<u32>) {
            let max = word_max(geometry);
            let mut addr = pool_addr(geometry, self.line, self.word);
            let mut value = self.store.then_some(self.value & max);
            match self.flaw {
                None => {}
                Some(Flaw::Misaligned) => addr += 1,
                Some(Flaw::OutOfRange) => {
                    addr = 1_u32
                        .checked_shl(geometry.address_bits().into())
                        .unwrap_or(addr);
                }
                Some(Flaw::Value) => value = (!self.store).then_some(self.value & max),
                Some(Flaw::TooWide) => {
                    value = Some(
                        max.checked_add(1)
                            .map_or(self.value, |over| over + self.value % over),
                    );
                }
            }
            let fits = u64::from(addr) < 1_u64 << geometry.address_bits();
            let kind = if self.store {
                AccessKind::Write
            } else {
                AccessKind::Read
            };
            let op = Op::Issue {
                kind,
                addr: number(addr.into()),
                value: value.map(number),
            };
            (op, fits.then_some(addr))
        }
    }

    /// The programs a student loads: assembly that walks an array, strides
    /// through the lines of one set, or alternates between two lines of one
    /// set; a trace; assembly that faults; assembly that does not parse; and
    /// a loop that never ends. Every instruction has a line of its own, from
    /// the first line on.
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum Template {
        /// Loads and stores each word of a stretch of the pool.
        Walk,
        /// Loads the next line of the same set, again and again.
        Stride,
        /// A load of one line and a store to another in the same set, in
        /// turn.
        Pair,
        /// Loads and stores of the pool.
        Trace,
        /// A load from a register holding an address that is misaligned or
        /// past the address width, which stops the program.
        Fault,
        /// An unknown instruction.
        Broken,
        /// Loads and stores of the pool, round a loop that never ends; for
        /// the long runs of a long session only.
        Endless,
    }

    /// A program from a template, with the numbers it is read with.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct ProgramChoice {
        /// The template.
        template: Template,
        /// Where it starts in the pool.
        start: u32,
        /// A second number: a word, a stride, a value.
        other: u32,
        /// How many times its loop goes round, or its trace's length.
        count: u32,
    }

    impl ProgramChoice {
        /// The words a trace or a loop of accesses names, in order: one to
        /// six of the pool, their lines one to five apart.
        fn accesses(self, geometry: &Geometry) -> Vec<u32> {
            let n = 1 + self.count % 6;
            let step = 1 + self.other % 5;
            (0..n)
                .map(|i| {
                    let line = self.start.wrapping_add(i * step);
                    pool_addr(geometry, line, self.other.wrapping_add(i))
                })
                .collect()
        }

        /// Whether access `i` of a trace or a loop of accesses is a store.
        fn stores(self, i: u32) -> bool {
            (self.other >> i) & 1 == 1
        }

        /// The program under `geometry`, and every address it can reach.
        fn program(self, geometry: &Geometry) -> (ProgramSource, Vec<u32>) {
            let line = geometry.line_bytes();
            let word = geometry.word_bytes();
            let pool = pool_lines(geometry);
            let pool_bytes = pool * line;
            let first = pool_line(geometry, self.start);
            let base = first * line;
            let (source, addrs) = match self.template {
                Template::Walk => {
                    let n = 1 + self.count % ((pool_bytes - base) / word).min(8);
                    let source = format!(
                        "add r2, r0, #{base}\nadd r3, r0, #{n}\nloop: ld r1, [r2]\n\
                         st [r2], r3\nadd r2, r2, #{word}\nadd r3, r3, #-1\nbnez r3, loop"
                    );
                    (source, (0..n).map(|i| base + i * word).collect())
                }
                Template::Stride => {
                    let stride = geometry.sets() * line;
                    let n = 1 + self.count % ((pool_bytes - base - 1) / stride + 1).min(6);
                    let source = format!(
                        "add r2, r0, #{base}\nadd r3, r0, #{n}\nloop: ld r1, [r2]\n\
                         add r2, r2, #{stride}\nadd r3, r3, #-1\nbnez r3, loop"
                    );
                    (source, (0..n).map(|i| base + i * stride).collect())
                }
                Template::Pair => {
                    let offset = (self.other % geometry.words_per_line()) * word;
                    let (one, two) = (first, (first + geometry.sets()) % pool);
                    let (load, store) = (one * line + offset, two * line + offset);
                    let n = 1 + self.count % 4;
                    let value = self.other % 100;
                    let source = format!(
                        "add r3, r0, #{n}\nloop: ld r1, [{load:#X}]\n\
                         st [{store:#X}], #{value}\nadd r3, r3, #-1\nbnez r3, loop"
                    );
                    (source, vec![load, store])
                }
                Template::Trace => {
                    let addrs = self.accesses(geometry);
                    let lines: Vec<String> = (0..)
                        .zip(&addrs)
                        .map(|(i, addr)| {
                            if self.stores(i) {
                                format!("W {addr:#X} {}", self.other.wrapping_add(i) % 100)
                            } else {
                                format!("R {addr:#X}")
                            }
                        })
                        .collect();
                    let source = ProgramSource {
                        source: lines.join("\n"),
                        format: ProgramFormat::Trace,
                    };
                    return (source, addrs);
                }
                Template::Fault => {
                    let past = 1_u64 << geometry.address_bits();
                    let addr = if word > 1 && self.other % 2 == 0 {
                        u64::from(base + 1)
                    } else {
                        past
                    };
                    (format!("add r2, r0, #{addr}\nld r1, [r2]"), Vec::new())
                }
                Template::Broken => ("lw r1, [0x0]".to_owned(), Vec::new()),
                Template::Endless => {
                    let addrs = self.accesses(geometry);
                    // `r4` is never 0, and `r5` counts the rounds, so each
                    // store writes a new value.
                    let body: Vec<String> = (0..)
                        .zip(&addrs)
                        .map(|(i, addr)| {
                            let label = if i == 0 { "loop: " } else { "" };
                            if self.stores(i) {
                                format!("{label}st [{addr:#X}], r5")
                            } else {
                                format!("{label}ld r1, [{addr:#X}]")
                            }
                        })
                        .collect();
                    let body = body.join("\n");
                    let source = format!("add r4, r0, #1\n{body}\nadd r5, r5, #1\nbnez r4, loop");
                    (source, addrs)
                }
            };
            let source = ProgramSource {
                source,
                format: ProgramFormat::Asm,
            };
            (source, addrs)
        }
    }

    /// An edit a student means to make, planned before it is applied. A
    /// way is picked as [`pick_way`] reads its number.
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum EditIntent {
        /// Empty a way; with `fields`, sending a dirty bit too, which an
        /// empty way cannot have.
        Invalidate { way: u32, fields: bool },
        /// Give a way a line, named by any byte of it, perhaps with a dirty
        /// bit and a rank. An even `line` picks a line of the way's own set,
        /// an odd one any line of the pool.
        Line {
            way: u32,
            line: u32,
            byte: u32,
            dirty: Option<bool>,
            rank: Option<u32>,
        },
        /// Give one word of a way's line a value, perhaps with a dirty bit.
        Word {
            way: u32,
            word: u32,
            value: u64,
            dirty: Option<bool>,
        },
        /// Set a way's dirty bit if it is clear, or clear it if it is set.
        Dirty { way: u32 },
        /// Give a way a rank, perhaps one no way has.
        Rank { way: u32, rank: u32 },
        /// Give a word of memory a value. An even `line` picks a word of a
        /// line the cache holds, an odd one any word of the pool.
        Memory { line: u32, word: u32, value: u64 },
    }

    /// The set and way `pick` names in `state`: a quarter of the picks name
    /// any way; half a way holding a dirty line, if there is one; and the
    /// rest a way holding a line, if there is one.
    fn pick_way(state: &State, pick: u32) -> (u32, u32) {
        let geometry = &state.geometry;
        let choice = pick >> 2;
        let held: Vec<(u32, u32, bool)> = (0_u32..)
            .zip(&state.cache.sets)
            .flat_map(|(set, view)| {
                (0_u32..)
                    .zip(&view.ways)
                    .filter(|(_, way)| way.valid)
                    .map(move |(way, view)| (set, way, view.dirty))
            })
            .collect();
        let dirty: Vec<(u32, u32, bool)> = held.iter().copied().filter(|way| way.2).collect();
        let among = match pick % 4 {
            0 => &[][..],
            1 | 2 if !dirty.is_empty() => &dirty[..],
            _ => &held[..],
        };
        match among.get(choice as usize % among.len().max(1)) {
            Some(&(set, way, _)) => (set, way),
            None => (
                choice % geometry.sets(),
                choice / geometry.sets() % geometry.ways(),
            ),
        }
    }

    impl EditIntent {
        /// The target and patch at the cursor of `session`, and the address
        /// the edit names, if it names one.
        fn resolve(self, session: &Session) -> (EditTarget, EditPatch, Option<u32>) {
            let state = session.state();
            let geometry = state.geometry;
            let way_target = |(set, way): (u32, u32)| -> EditTarget {
                read(json!({"kind": "way", "set": set, "way": way}))
            };
            let with =
                |mut patch: serde_json::Value, key: &str, value: Option<serde_json::Value>| {
                    if let Some(value) = value {
                        patch[key] = value;
                    }
                    patch
                };
            match self {
                EditIntent::Invalidate { way, fields } => {
                    let patch = json!({"kind": "way", "valid": false});
                    let patch = with(patch, "dirty", fields.then_some(true.into()));
                    (way_target(pick_way(&state, way)), read(patch), None)
                }
                EditIntent::Line {
                    way,
                    line,
                    byte,
                    dirty,
                    rank,
                } => {
                    let (set, index) = pick_way(&state, way);
                    let line = if line % 2 == 0 {
                        let tags = pool_lines(&geometry) / geometry.sets();
                        (line >> 1) % tags * geometry.sets() + set
                    } else {
                        pool_line(&geometry, line >> 1)
                    };
                    let addr = line * geometry.line_bytes() + byte % geometry.line_bytes();
                    let patch = json!({"kind": "way", "valid": true, "line": addr});
                    let patch = with(patch, "dirty", dirty.map(Into::into));
                    let rank = rank.map(|rank| (rank % (geometry.ways() + 1)).into());
                    let patch = with(patch, "rank", rank);
                    (way_target((set, index)), read(patch), Some(addr))
                }
                EditIntent::Word {
                    way,
                    word,
                    value,
                    dirty,
                } => {
                    let (set, index) = pick_way(&state, way);
                    let view = way_view(&state, set, index);
                    let mut words: Vec<u64> = view.words.iter().map(|text| data(text)).collect();
                    words.resize(geometry.words_per_line() as usize, 0);
                    let at = (word % geometry.words_per_line()) as usize;
                    words[at] = value & word_max(&geometry);
                    let patch = json!({"kind": "way", "data": words});
                    let patch = with(patch, "dirty", dirty.map(Into::into));
                    (way_target((set, index)), read(patch), None)
                }
                EditIntent::Dirty { way } => {
                    let (set, index) = pick_way(&state, way);
                    let dirty = !way_view(&state, set, index).dirty;
                    let patch = json!({"kind": "way", "dirty": dirty});
                    (way_target((set, index)), read(patch), None)
                }
                EditIntent::Rank { way, rank } => (
                    way_target(pick_way(&state, way)),
                    read(json!({"kind": "way", "rank": rank % (geometry.ways() + 1)})),
                    None,
                ),
                EditIntent::Memory { line, word, value } => {
                    let (set, index) = pick_way(&state, line >> 1);
                    let held = way_view(&state, set, index)
                        .line_text
                        .filter(|_| line % 2 == 0);
                    let addr = match held {
                        Some(text) => {
                            let word = word % geometry.words_per_line();
                            hex(&text) + word * geometry.word_bytes()
                        }
                        None => pool_addr(&geometry, line >> 1, word),
                    };
                    let value = value & word_max(&geometry);
                    (
                        read(json!({"kind": "memory", "addr": addr})),
                        read(json!({"kind": "memory", "value": value})),
                        Some(addr),
                    )
                }
            }
        }
    }

    /// The view of way `way` of set `set` in `state`.
    fn way_view(state: &State, set: u32, way: u32) -> WayView {
        state.cache.sets[set as usize].ways[way as usize].clone()
    }

    /// A configuration change, Reset or a switch between Basics and
    /// Advanced.
    #[derive(Clone, Debug)]
    pub(crate) enum Change {
        /// `Op::Config`, appended as it is.
        Apply(Config),
        /// A change from the configuration bar, then "Reset and apply" if
        /// it asks first and `reset` says so.
        Propose { config: Config, reset: bool },
        /// Reset: a new run of the displayed configuration.
        Reset,
        /// A switch of mode, then "Reset and apply" if it asks first and
        /// `reset` says so, and the switch again.
        Mode { advanced: bool, reset: bool },
        /// A configuration that does not validate, appended and proposed;
        /// one that validates after all is left alone.
        Invalid(Config),
    }

    /// The actions a session is made of: accesses issued by hand, half of
    /// them; programs; execution; edits; configuration changes; and cursor
    /// moves.
    fn arb_action() -> impl Strategy<Value = Action> {
        prop_oneof![
            50 => arb_issue().prop_map(Action::Issue),
            5 => arb_program().prop_map(Action::Program),
            15 => prop_oneof![
                Just(Action::Step),
                Just(Action::StepAccess),
                (0..=LONGEST_RUN.min(MAX_RUN_STEPS)).prop_map(|n| Action::Run { n }),
            ],
            10 => arb_edit_intent().prop_map(Action::Edit),
            3 => arb_change().prop_map(Action::Change),
            17 => arb_move(),
        ]
    }

    /// An access issued by hand; one in eight has a flaw.
    fn arb_issue() -> impl Strategy<Value = IssueChoice> {
        let flaw = prop_oneof![
            7 => Just(None),
            1 => select(vec![Flaw::Misaligned, Flaw::OutOfRange, Flaw::Value, Flaw::TooWide])
                .prop_map(Some),
        ];
        (
            any::<bool>(),
            any::<u32>(),
            any::<u32>(),
            any::<u64>(),
            flaw,
        )
            .prop_map(|(store, line, word, value, flaw)| IssueChoice {
                store,
                line,
                word,
                value,
                flaw,
            })
    }

    /// A program from a template; one in twelve faults, and one in twelve
    /// does not parse.
    fn arb_program() -> impl Strategy<Value = ProgramChoice> {
        let template = prop_oneof![
            3 => Just(Template::Walk),
            2 => Just(Template::Stride),
            2 => Just(Template::Pair),
            3 => Just(Template::Trace),
            1 => Just(Template::Fault),
            1 => Just(Template::Broken),
        ];
        (template, any::<u32>(), any::<u32>(), any::<u32>()).prop_map(
            |(template, start, other, count)| ProgramChoice {
                template,
                start,
                other,
                count,
            },
        )
    }

    /// An edit intent of any kind.
    fn arb_edit_intent() -> impl Strategy<Value = EditIntent> {
        let dirty = || option::of(any::<bool>());
        prop_oneof![
            (any::<u32>(), any::<bool>())
                .prop_map(|(way, fields)| EditIntent::Invalidate { way, fields }),
            (
                any::<u32>(),
                any::<u32>(),
                any::<u32>(),
                dirty(),
                option::of(any::<u32>())
            )
                .prop_map(|(way, line, byte, dirty, rank)| EditIntent::Line {
                    way,
                    line,
                    byte,
                    dirty,
                    rank,
                }),
            (any::<u32>(), any::<u32>(), any::<u64>(), dirty()).prop_map(
                |(way, word, value, dirty)| EditIntent::Word {
                    way,
                    word,
                    value,
                    dirty,
                }
            ),
            any::<u32>().prop_map(|way| EditIntent::Dirty { way }),
            (any::<u32>(), any::<u32>()).prop_map(|(way, rank)| EditIntent::Rank { way, rank }),
            (any::<u32>(), any::<u32>(), any::<u64>())
                .prop_map(|(line, word, value)| EditIntent::Memory { line, word, value }),
        ]
    }

    /// A configuration change of any kind.
    fn arb_change() -> impl Strategy<Value = Change> {
        prop_oneof![
            arb_config().prop_map(Change::Apply),
            (arb_config(), any::<bool>())
                .prop_map(|(config, reset)| Change::Propose { config, reset }),
            Just(Change::Reset),
            (any::<bool>(), any::<bool>())
                .prop_map(|(advanced, reset)| Change::Mode { advanced, reset }),
            arb_raw_config().prop_map(Change::Invalid),
        ]
    }

    /// A cursor move: undo, long enough runs of it to reach position 0
    /// included; redo; a seek, often to position 0; a scrub; Space and
    /// Shift+Space, with or without a program in the drawer.
    fn arb_move() -> impl Strategy<Value = Action> {
        prop_oneof![
            4 => (1_u32..=3).prop_map(|times| Action::Undo { times }),
            1 => Just(Action::UndoAll),
            3 => (1_u32..=3).prop_map(|times| Action::Redo { times }),
            3 => prop_oneof![Just(0), any::<u32>()].prop_map(|to| Action::Seek { to }),
            2 => any::<u32>().prop_map(|to| Action::Scrub { to }),
            4 => option::of(arb_program()).prop_map(|drawer| Action::Space { drawer }),
            3 => option::of(arb_program()).prop_map(|drawer| Action::ShiftSpace { drawer }),
        ]
    }

    /// A session: its initial configuration and what the student does.
    pub(crate) fn arb_session() -> impl Strategy<Value = (Config, Vec<Action>)> {
        (arb_config(), vec(arb_action(), 0..=ACTIONS))
    }

    /// A session whose log crosses checkpoints: long runs of a loop, then
    /// what a student does, then the log cut back and grown again by long
    /// runs of another loop, past a checkpoint it had.
    #[derive(Clone, Debug)]
    pub(crate) struct LongSession {
        /// The first run's configuration.
        config: Config,
        /// The loop the first long runs go round, and the one the runs after
        /// the cut go round, so that the log differs from the cut on.
        loops: (ProgramChoice, ProgramChoice),
        /// How many instructions each long run asks for, taken in turn.
        runs: Vec<u32>,
        /// How many events the first long runs reach.
        first: u32,
        /// What the student does after them.
        actions: Vec<Action>,
        /// How many ops the cut keeps, modulo one more than the op holding
        /// the last multiple of the checkpoint interval, so that its
        /// checkpoint goes.
        cut: u32,
        /// How many events past the cut the log grows again.
        again: u32,
        /// Positions to check besides those around the checkpoints, each
        /// modulo how many there are.
        pub(crate) picks: Vec<u32>,
        /// The most events a slice of a saved file's load replays.
        pub(crate) slice: u32,
    }

    /// A session whose log crosses checkpoints, in runs of a quarter of
    /// the most a run may ask for and up, so that a few of them do it.
    pub(crate) fn arb_long_session() -> impl Strategy<Value = LongSession> {
        let endless = || {
            (any::<u32>(), any::<u32>(), any::<u32>()).prop_map(|(start, other, count)| {
                ProgramChoice {
                    template: Template::Endless,
                    start,
                    other,
                    count,
                }
            })
        };
        let log = (
            arb_config(),
            (endless(), endless()),
            vec(MAX_RUN_STEPS / 4..=MAX_RUN_STEPS, 1..=3),
            CHECKPOINT_INTERVAL + 1..=3 * CHECKPOINT_INTERVAL,
        );
        let rest = (
            vec(arb_action(), 0..=LONG_ACTIONS),
            any::<u32>(),
            CHECKPOINT_INTERVAL + 1..=2 * CHECKPOINT_INTERVAL,
            vec(any::<u32>(), PICKS),
            1..=LOAD_SLICE_EVENTS,
        );
        (log, rest).prop_map(|(log, rest)| {
            let (config, loops, runs, first) = log;
            let (actions, cut, again, picks, slice) = rest;
            LongSession {
                config,
                loops,
                runs,
                first,
                actions,
                cut,
                again,
                picks,
                slice,
            }
        })
    }

    // --- The student ----------------------------------------------------

    /// Which properties of each call the student checks while acting.
    #[derive(Clone, Copy, Debug, Default)]
    pub(crate) struct Checks {
        /// A refused call changes nothing.
        pub(crate) refusals: bool,
        /// A call keeps every op before the point it appends at, and the
        /// initial configuration.
        pub(crate) append_only: bool,
        /// A plan applies, planned again gives itself, keeps the invariants,
        /// and names the dirty data it drops; a suggestion plans; and at the
        /// end of an op, an edit the plan refuses is refused as an op, with
        /// the plan's refusal.
        pub(crate) edits: bool,
    }

    /// What a call did to the log.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        /// It appended ops, or might have.
        Appended,
        /// It moved the cursor or switched the mode, and appended nothing.
        Moved,
        /// It was refused, or did nothing.
        Refused,
    }

    impl Call {
        /// The call an op's result describes.
        fn of<T, E>(result: &Result<T, E>) -> Self {
            if result.is_ok() {
                Call::Appended
            } else {
                Call::Refused
            }
        }
    }

    /// Everything a refused call must leave as it was.
    #[derive(Debug, PartialEq)]
    struct Snapshot {
        /// The log, the undone ops included.
        ops: Vec<Op>,
        /// Every event, the undone ones included.
        events: Vec<MicroEvent>,
        /// The cursor.
        position: Position,
        /// What the page draws.
        state: State,
        /// The log's view.
        timeline: Timeline,
        /// The machine at the cursor.
        machine: Machine,
        /// What a link would hold: the mode chosen and the cursor too.
        doc: ShareDoc,
    }

    impl Snapshot {
        /// The snapshot of `session`.
        fn of(session: &Session) -> Self {
            Self {
                ops: session.ops().to_vec(),
                events: session.events_range(0..u32::MAX).to_vec(),
                position: session.position(),
                state: session.state(),
                timeline: session.timeline(),
                machine: session.machine().clone(),
                doc: session.share_doc(),
            }
        }
    }

    /// The log before a call, and where the call appends.
    struct Before {
        /// The ops.
        ops: Vec<Op>,
        /// How many of them the call keeps if it appends: those up to the
        /// end of the op holding the cursor.
        kept: usize,
        /// The whole session, when refusals are checked.
        snapshot: Option<Snapshot>,
    }

    /// A session and a student acting on it.
    pub(crate) struct Student {
        /// The session.
        pub(crate) session: Session,
        /// Every address an action has named, and every address a program
        /// loaded can reach: all the memory the session can touch.
        pub(crate) addrs: BTreeSet<u32>,
        /// What the student checks at each call.
        checks: Checks,
        /// The initial configuration.
        initial: Config,
    }

    /// The student's session after `actions` on a first run of `config`,
    /// checking `checks` along the way.
    pub(crate) fn play(
        config: &Config,
        actions: &[Action],
        checks: Checks,
    ) -> Result<Student, TestCaseError> {
        let mut student = Student::new(config, checks)?;
        for action in actions {
            student.act(action)?;
        }
        Ok(student)
    }

    /// The student's session after the long runs and the actions of `long`,
    /// checking `checks` along the way; the log is not cut back yet.
    pub(crate) fn play_long(long: &LongSession, checks: Checks) -> Result<Student, TestCaseError> {
        let mut student = Student::new(&long.config, checks)?;
        student.grow(long.loops.0, &long.runs, long.first)?;
        for action in &long.actions {
            student.act(action)?;
        }
        Ok(student)
    }

    impl Student {
        /// A student on a first run of `config`, checking `checks`.
        fn new(config: &Config, checks: Checks) -> Result<Self, TestCaseError> {
            Ok(Student {
                session: Session::with_config(*config).map_err(fail)?,
                addrs: BTreeSet::new(),
                checks,
                initial: *config,
            })
        }

        /// Loads the loop `endless` where the cursor is, then runs it, as
        /// many instructions at a time as `runs` say in turn, until the log
        /// is `to` events long; a loop that has used its budget is loaded
        /// again.
        fn grow(
            &mut self,
            endless: ProgramChoice,
            runs: &[u32],
            to: u32,
        ) -> Result<(), TestCaseError> {
            self.load_endless(endless)?;
            for &n in runs.iter().cycle() {
                if self.session.position().pos.get() >= to {
                    break;
                }
                if !self.apply(Op::Run { n })? {
                    self.load_endless(endless)?;
                    prop_assert!(self.apply(Op::Run { n })?, "a fresh loop does not run");
                }
            }
            prop_assert!(self.session.position().pos.get() >= to);
            Ok(())
        }

        /// Loads the loop `choice` makes under the displayed run's geometry.
        fn load_endless(&mut self, choice: ProgramChoice) -> Result<(), TestCaseError> {
            let (program, addrs) = choice.program(&geometry(&self.session));
            self.addrs.extend(addrs);
            let op = Op::Program {
                source: program.source,
                format: program.format,
            };
            prop_assert!(self.apply(op)?, "the loop does not load");
            Ok(())
        }

        /// Cuts the log back and grows it again, as `long` says: scrubs to
        /// the end of an op, or to position 0, before the op holding the
        /// last multiple of the checkpoint interval, whose checkpoint the
        /// next op appended drops, and runs the second loop from there as
        /// far again as `long` says. A log shorter than an interval is cut
        /// before its last op.
        pub(crate) fn cut_and_grow(&mut self, long: &LongSession) -> Result<(), TestCaseError> {
            let ends = op_ends(&self.session);
            let last = end(&self.session).get() / CHECKPOINT_INTERVAL * CHECKPOINT_INTERVAL;
            let holding = ends.iter().position(|&end| last > 0 && end >= last);
            let keep = holding.map_or(ends.len(), |op| op + 1);
            let ops = long.cut % u32::try_from(keep).map_err(fail)?.max(1);
            self.cursor(|session| {
                session.scrub_to(ops);
                true
            })?;
            let cut = self.session.position().pos.get();
            self.grow(long.loops.1, &long.runs, cut + long.again)
        }

        /// Does `action`.
        fn act(&mut self, action: &Action) -> Result<(), TestCaseError> {
            let geometry = geometry(&self.session);
            match action {
                Action::Issue(issue) => {
                    let (op, addr) = issue.op(&geometry);
                    self.addrs.extend(addr);
                    self.apply(op).map(drop)
                }
                Action::Program(choice) => {
                    let (program, addrs) = choice.program(&geometry);
                    self.addrs.extend(addrs);
                    let op = Op::Program {
                        source: program.source,
                        format: program.format,
                    };
                    self.apply(op).map(drop)
                }
                Action::Step => self.apply(Op::Step).map(drop),
                Action::StepAccess => self.apply(Op::StepAccess).map(drop),
                Action::Run { n } => self.apply(Op::Run { n: *n }).map(drop),
                Action::Edit(intent) => self.edit(*intent),
                Action::Change(change) => self.change(change),
                Action::Undo { times } => {
                    for _ in 0..*times {
                        self.cursor(Session::undo)?;
                    }
                    Ok(())
                }
                Action::UndoAll => {
                    while self.session.position().pos.get() > 0 {
                        self.cursor(Session::undo)?;
                    }
                    self.cursor(Session::undo)
                }
                Action::Redo { times } => {
                    for _ in 0..*times {
                        self.cursor(Session::redo)?;
                    }
                    Ok(())
                }
                Action::Seek { to } => {
                    let pos = to % (end(&self.session).get() + 1);
                    self.cursor(|session| {
                        session.seek(number(pos.into()));
                        true
                    })
                }
                Action::Scrub { to } => {
                    let ops = to % (self.session.op_count() + 1);
                    self.cursor(|session| {
                        session.scrub_to(ops);
                        true
                    })
                }
                Action::Space { drawer } => self.space(*drawer, &geometry, Session::step_event),
                Action::ShiftSpace { drawer } => {
                    self.space(*drawer, &geometry, Session::step_access)
                }
            }
        }

        /// The log as it stands before a call.
        fn before(&self) -> Before {
            let kept = self
                .session
                .position()
                .op
                .map_or(0, |op| op.get() as usize + 1);
            Before {
                ops: self.session.ops().to_vec(),
                kept,
                snapshot: self.checks.refusals.then(|| Snapshot::of(&self.session)),
            }
        }

        /// Checks what the call since `before` did, as `call` describes it.
        fn after(&self, before: Before, call: Call) -> Result<(), TestCaseError> {
            if self.checks.append_only {
                prop_assert_eq!(self.session.initial(), &self.initial);
                let ops = self.session.ops();
                if call == Call::Appended {
                    prop_assert!(ops.len() >= before.kept);
                    prop_assert_eq!(&ops[..before.kept], &before.ops[..before.kept]);
                } else {
                    prop_assert_eq!(ops, before.ops.as_slice());
                }
            }
            if let (Some(snapshot), Call::Refused) = (before.snapshot, call) {
                prop_assert_eq!(Snapshot::of(&self.session), snapshot);
            }
            if self.checks.append_only {
                // The saved file's size, kept op by op, is the file's own,
                // and within what a file can hold.
                let json = self.session.to_json();
                prop_assert_eq!(self.session.to_json_len() as usize, json.len());
                prop_assert!(json.len() <= MAX_SHARE_JSON_BYTES as usize);
            }
            Ok(())
        }

        /// Appends `op`, and says whether it was accepted. An accepted op
        /// returns its events, the last of the log, where the cursor rests.
        fn apply(&mut self, op: Op) -> Result<bool, TestCaseError> {
            let before = self.before();
            let result = self.session.apply(op).map(<[MicroEvent]>::to_vec);
            if let (Ok(events), true) = (&result, self.checks.append_only) {
                let all = self.session.events_range(0..u32::MAX);
                prop_assert!(!events.is_empty());
                prop_assert_eq!(events.as_slice(), &all[all.len() - events.len()..]);
                prop_assert_eq!(self.session.position().boundary, Boundary::LogEnd);
            }
            self.after(before, Call::of(&result))?;
            Ok(result.is_ok())
        }

        /// A cursor move that says whether it moved.
        fn cursor(&mut self, call: impl FnOnce(&mut Session) -> bool) -> Result<(), TestCaseError> {
            let before = self.before();
            let moved = call(&mut self.session);
            self.after(before, if moved { Call::Moved } else { Call::Refused })
        }

        /// Space or Shift+Space, with `drawer`'s program in the drawer.
        fn space(
            &mut self,
            drawer: Option<ProgramChoice>,
            geometry: &Geometry,
            call: impl FnOnce(&mut Session, Option<&ProgramSource>) -> StepOutcome,
        ) -> Result<(), TestCaseError> {
            let program = drawer.map(|choice| {
                let (program, addrs) = choice.program(geometry);
                self.addrs.extend(addrs);
                program
            });
            let before = self.before();
            let outcome = call(&mut self.session, program.as_ref());
            let call = match outcome {
                StepOutcome::Moved => Call::Moved,
                StepOutcome::Applied => Call::Appended,
                StepOutcome::Idle { .. } => Call::Refused,
            };
            self.after(before, call)
        }

        /// A configuration change.
        fn change(&mut self, change: &Change) -> Result<(), TestCaseError> {
            match change {
                Change::Apply(config) => self.apply(Op::Config { config: *config }).map(drop),
                Change::Propose { config, reset } => {
                    let before = self.before();
                    match self.session.propose_config(*config) {
                        Ok(ConfigChange::Applied) => self.after(before, Call::Appended),
                        Ok(ConfigChange::NeedsReset { config, .. }) => {
                            self.after(before, Call::Refused)?;
                            if *reset {
                                // The bar asks only about a change that
                                // agreeing applies.
                                let accepted = self.apply(Op::Config { config })?;
                                prop_assert!(accepted, "a change the bar asked about was refused");
                            }
                            Ok(())
                        }
                        Err(_) => self.after(before, Call::Refused),
                    }
                }
                Change::Reset => {
                    let config = *self.session.config();
                    self.apply(Op::Config { config }).map(drop)
                }
                Change::Mode { advanced, reset } => {
                    let before = self.before();
                    match self.session.set_advanced(*advanced) {
                        ConfigChange::Applied => self.after(before, Call::Moved),
                        ConfigChange::NeedsReset { config, .. } => {
                            self.after(before, Call::Refused)?;
                            if *reset && self.apply(Op::Config { config })? {
                                let before = self.before();
                                let change = self.session.set_advanced(*advanced);
                                prop_assert_eq!(change, ConfigChange::Applied);
                                self.after(before, Call::Moved)?;
                            }
                            Ok(())
                        }
                    }
                }
                Change::Invalid(config) => {
                    if config.validate().is_ok() {
                        return Ok(());
                    }
                    let accepted = self.apply(Op::Config { config: *config })?;
                    prop_assert!(!accepted, "an invalid configuration was appended");
                    let before = self.before();
                    let result = self.session.propose_config(*config);
                    prop_assert!(result.is_err(), "an invalid configuration was proposed");
                    self.after(before, Call::Refused)
                }
            }
        }

        /// An edit: planned, then applied if planned; otherwise, at the end
        /// of an op, applied as the op the plan refused, and then its
        /// suggestion, planned and applied, if the refusal offers one.
        fn edit(&mut self, intent: EditIntent) -> Result<(), TestCaseError> {
            let (target, patch, addr) = intent.resolve(&self.session);
            self.addrs.extend(addr);
            let refusal = match self.session.plan_edit(&target, &patch) {
                Ok(plan) => return self.apply_plan(target, patch, &plan),
                Err(refusal) => refusal,
            };
            // At the end of an op, the session checks the op on the machine
            // the plan read, so the op is refused as the plan was, whether
            // or not the refusal offers another edit.
            if self.session.position().boundary >= Boundary::OpEnd {
                let before = self.before();
                let result = self.session.apply(Op::Edit { target, patch }).map(drop);
                if self.checks.edits {
                    // This harness never sets a gate, so `Gated` is unreached
                    // here; it is listed alongside `SessionFull`, `LinkFull`
                    // and `FileFull` because all four are op-level refusals
                    // `plan_edit` reports before the rules on the machine
                    // ever see the edit, so `apply` reports the same `OpError`
                    // rather than wrapping it as `OpError::Edit`.
                    let expected = match refusal.reason {
                        RefusalReason::Gated => OpError::Gated,
                        RefusalReason::SessionFull => OpError::SessionFull,
                        RefusalReason::LinkFull => OpError::LinkFull,
                        RefusalReason::FileFull => OpError::FileFull,
                        _ => OpError::Edit(Box::new(refusal.clone())),
                    };
                    prop_assert_eq!(result.as_ref(), Err(&expected));
                }
                self.after(before, Call::of(&result))?;
            }
            if let Some(suggestion) = refusal.suggestion.map(|boxed| *boxed) {
                self.addrs
                    .extend(edit_addr(&suggestion.target, &suggestion.patch));
                let planned = self
                    .session
                    .plan_edit(&suggestion.target, &suggestion.patch);
                if self.checks.edits {
                    prop_assert!(
                        planned.is_ok(),
                        "the suggestion {:?} for {:?} does not plan: {:?}",
                        suggestion,
                        refusal.reason,
                        planned
                    );
                }
                return match planned {
                    Ok(plan) => self.apply_plan(suggestion.target, suggestion.patch, &plan),
                    Err(_) => Ok(()),
                };
            }
            Ok(())
        }

        /// Applies the edit `plan` planned for `target` and `patch`.
        fn apply_plan(
            &mut self,
            target: EditTarget,
            patch: EditPatch,
            plan: &EditPlan,
        ) -> Result<(), TestCaseError> {
            let dirty = if self.checks.edits {
                let again = self.session.plan_edit(&plan.target, &plan.patch);
                prop_assert_eq!(again.as_ref(), Ok(plan));
                Some(self.dirty_words(target))
            } else {
                None
            };
            let before = self.before();
            let result = self.session.apply(Op::Edit { target, patch }).map(drop);
            if let Some((way, differing)) = dirty {
                prop_assert!(result.is_ok(), "a planned edit was refused: {:?}", result);
                prop_assert_eq!(self.session.machine().check_invariants(), Ok(()));
                self.check_discards(target, plan, way, differing)?;
            }
            self.after(before, Call::of(&result))
        }

        /// The view of the way `target` names, and the addresses of the
        /// words of its line that are newer than memory's: the words the way
        /// would lose with its line.
        fn dirty_words(&self, target: EditTarget) -> (Option<WayView>, Vec<u32>) {
            let EditTarget::Way { set, way } = target else {
                return (None, Vec::new());
            };
            let view = way_view(&self.session.state(), set.get(), way.get());
            let geometry = geometry(&self.session);
            let differing = match (&view.line_text, view.dirty) {
                (Some(line), true) => (0..geometry.words_per_line())
                    .map(|word| hex(line) + word * geometry.word_bytes())
                    .filter(|&addr| {
                        let addr = geometry.addr(addr).unwrap();
                        self.session.machine().stale(addr).is_some()
                    })
                    .collect(),
                _ => Vec::new(),
            };
            (Some(view), differing)
        }

        /// Checks that `plan` names the dirty words its way lost, and only
        /// then: `way` is the way before the edit, `differing` the addresses
        /// of its words newer than memory.
        fn check_discards(
            &self,
            target: EditTarget,
            plan: &EditPlan,
            way: Option<WayView>,
            differing: Vec<u32>,
        ) -> Result<(), TestCaseError> {
            let discards: Vec<&Consequence> = plan
                .consequences
                .iter()
                .filter(|consequence| matches!(consequence, Consequence::DiscardsDirtyData { .. }))
                .collect();
            let (EditTarget::Way { set, way: index }, Some(way)) = (target, way) else {
                prop_assert!(
                    discards.is_empty(),
                    "a memory edit discards: {:?}",
                    discards
                );
                return Ok(());
            };
            let now = way_view(&self.session.state(), set.get(), index.get());
            let lost = !now.valid || now.line_text != way.line_text;
            if lost && !differing.is_empty() {
                let expected = Consequence::DiscardsDirtyData {
                    set,
                    way: index,
                    addrs: differing
                        .into_iter()
                        .map(|addr| number(addr.into()))
                        .collect(),
                };
                prop_assert_eq!(discards, vec![&expected]);
                prop_assert_eq!(plan.tone, Tone::Caution);
            } else {
                prop_assert!(
                    discards.is_empty(),
                    "nothing newer is lost, yet: {:?}",
                    discards
                );
            }
            Ok(())
        }

        /// Watches every line the session can touch, so the memory table
        /// has the same rows at every event of a run.
        pub(crate) fn watch(&mut self) {
            let lines: Vec<_> = self.addrs.iter().map(|&addr| number(addr.into())).collect();
            self.session.set_memory_watch(&lines);
        }
    }

    /// The address an edit names, if it names one.
    fn edit_addr(target: &EditTarget, patch: &EditPatch) -> Option<u32> {
        match (target, patch) {
            (EditTarget::Memory { addr }, _) => Some(addr.get()),
            (_, EditPatch::Way(WayPatch { line, .. })) => line.map(Addr::get),
            (_, EditPatch::Memory { .. }) => None,
        }
    }

    // --- Walking the log ------------------------------------------------

    /// The position after the last event of the log.
    pub(crate) fn end(session: &Session) -> EventPos {
        number(session.events_range(0..u32::MAX).len() as u64)
    }

    /// Where each op of the log ends: the position after its last event.
    pub(crate) fn op_ends(session: &Session) -> Vec<u32> {
        let mut ends = vec![0; session.ops().len()];
        for event in session.events_range(0..u32::MAX) {
            ends[event.op_index.get() as usize] = u32::try_from(event.id + 1).unwrap();
        }
        ends
    }

    /// The undo stops of the log, in order: position 0, and the end of each
    /// op except a program load that another op follows.
    pub(crate) fn undo_stops(session: &Session) -> Vec<u32> {
        let ops = session.ops();
        let ends = op_ends(session);
        let mut stops = vec![0];
        for (index, (op, end)) in ops.iter().zip(&ends).enumerate() {
            if index + 1 == ops.len() || !matches!(op, Op::Program { .. }) {
                stops.push(*end);
            }
        }
        stops
    }

    /// `config` without its mode: the flag the fold never reads.
    pub(crate) fn without_mode(config: &Config) -> Config {
        Config {
            advanced: false,
            ..*config
        }
    }

    /// The start of op `op`: the end of the op before it, or position 0.
    pub(crate) fn op_start(ends: &[u32], op: usize) -> u32 {
        op.checked_sub(1).map_or(0, |before| ends[before])
    }

    /// Moves the cursor to every position of the log in turn, from 0 to the
    /// end, and visits it with the event there.
    pub(crate) fn every_position(
        session: &mut Session,
        mut visit: impl FnMut(&Session, Option<&MicroEvent>) -> Result<(), TestCaseError>,
    ) -> Result<(), TestCaseError> {
        let events = session.events_range(0..u32::MAX).to_vec();
        let at = std::iter::once(None).chain(events.iter().map(Some));
        for (pos, event) in (0_u32..).zip(at) {
            session.seek(number(pos.into()));
            prop_assert_eq!(session.position().pos.get(), pos);
            visit(session, event)?;
        }
        Ok(())
    }

    /// The machines the fold of a log reaches, each replayed from position
    /// 0 once, when it is first asked for.
    pub(crate) struct Fold {
        /// The log's initial configuration.
        initial: Config,
        /// The log.
        ops: Vec<Op>,
        /// The machines replayed so far, by position.
        machines: BTreeMap<u32, Machine>,
    }

    impl Fold {
        /// The fold of `session`'s log, the undone ops included.
        pub(crate) fn of(session: &Session) -> Self {
            Fold {
                initial: *session.initial(),
                ops: session.ops().to_vec(),
                machines: BTreeMap::new(),
            }
        }

        /// The machine at position `pos`.
        pub(crate) fn at(&mut self, pos: u32) -> &Machine {
            self.machines.entry(pos).or_insert_with(|| {
                Session::replay_to(&self.initial, &self.ops, number(pos.into())).unwrap()
            })
        }
    }

    /// The positions of a long log to check: position 0, the end of every
    /// op, the positions `picks` name, and those around each multiple of the
    /// checkpoint interval, where a checkpoint is taken at the first entry
    /// boundary at or after it that an op ends at or a long op executes an
    /// instruction at. Around a multiple, those are the position before it
    /// and every one from it to the first entry boundary; the second
    /// boundary, since a new run's setup can end one before an error ends
    /// its op; the position after each boundary; and a position past any
    /// entry a checkpoint of the log before a cut could be at.
    pub(crate) fn checked_positions(session: &Session, picks: &[u32]) -> BTreeSet<u32> {
        let events = session.events_range(0..u32::MAX);
        let end = end(session).get();
        let boundary = |pos: u32| events.get(pos as usize).is_none_or(|event| event.seq == 1);
        let mut positions: BTreeSet<u32> = [0, end].into();
        positions.extend(op_ends(session));
        positions.extend(picks.iter().map(|pick| pick % (end + 1)));
        for multiple in (CHECKPOINT_INTERVAL..=end).step_by(CHECKPOINT_INTERVAL as usize) {
            let mut boundaries = (multiple..=end).filter(|&pos| boundary(pos));
            let first = boundaries.next().unwrap_or(end);
            let second = boundaries.next().unwrap_or(first);
            positions.extend(multiple - 1..=first);
            let past = multiple + 2 * MAX_EVENTS_PER_ENTRY;
            positions.extend([first + 1, second, second + 1, past].map(|pos| pos.min(end)));
        }
        positions
    }

    /// Checks that a seek to each of `positions`, in turn, reaches the
    /// machine `fold` reaches there.
    pub(crate) fn seeks_reach(
        session: &mut Session,
        positions: impl IntoIterator<Item = u32>,
        fold: &mut Fold,
    ) -> Result<(), TestCaseError> {
        for pos in positions {
            session.seek(number(pos.into()));
            prop_assert_eq!(session.position().pos.get(), pos);
            prop_assert_eq!(session.machine(), fold.at(pos), "at {}", pos);
        }
        Ok(())
    }

    /// The session a saved file holds, loaded at most `slice` events at a
    /// time.
    pub(crate) fn load_in_slices(json: &str, slice: u32) -> Result<Session, TestCaseError> {
        let mut load = ShareLoad::from_json(json);
        loop {
            match load.advance(slice) {
                LoadProgress::Replaying { .. } => {}
                LoadProgress::Done { .. } => return load.take_session().map_err(fail),
                LoadProgress::Failed { failure } => return Err(fail(format!("{failure:?}"))),
            }
        }
    }

    /// Every cell and counter an event's `changed` can name, at the cursor,
    /// with its value as the page shows it; a way's tag and words are
    /// absent while the way is invalid, and its rank while the rank column
    /// is hidden.
    pub(crate) fn cells(session: &Session) -> BTreeMap<ChangeRef, Option<String>> {
        let state = session.state();
        let mut cells = BTreeMap::new();
        for set in &state.cache.sets {
            for (way, view) in (0_u64..).zip(&set.ways) {
                let (set, way) = (set.set, number(way));
                cells.insert(ChangeRef::Valid { set, way }, Some(view.valid.to_string()));
                cells.insert(ChangeRef::Tag { set, way }, view.tag_text.clone());
                cells.insert(ChangeRef::Dirty { set, way }, Some(view.dirty.to_string()));
                if let Some(rank) = view.rank {
                    cells.insert(ChangeRef::Rank { set, way }, Some(rank.value.to_string()));
                }
                for word in 0..state.geometry.words_per_line() {
                    let text = view.words.get(word as usize).cloned();
                    let word = state.geometry.word_index(word).unwrap();
                    cells.insert(ChangeRef::Word { set, way, word }, text);
                }
            }
        }
        for line in session.memory_lines(0, u32::MAX) {
            for word in line.words {
                cells.insert(ChangeRef::Memory { addr: word.addr }, Some(word.text));
            }
        }
        for register in &state.cpu.registers {
            let cell = ChangeRef::Register {
                register: register.register,
            };
            cells.insert(cell, Some(register.text.clone()));
        }
        cells.insert(ChangeRef::Pc, Some(pc(&state).to_string()));
        for counter in COUNTERS {
            let value = count(&state.stats.counters, counter);
            cells.insert(ChangeRef::Counter { counter }, Some(value.to_string()));
        }
        cells
    }

    /// The pc of the program loaded in `state`, for the templates' programs,
    /// which have one instruction a line from the first: its instruction's
    /// line less one, or the instruction count once it has finished; 0 with
    /// no program.
    fn pc(state: &State) -> u32 {
        state
            .program
            .as_ref()
            .map_or(0, |program| match program.current_line {
                Some(line) if !program.finished => line - 1,
                _ => program.instructions,
            })
    }

    /// The cells whose values differ between `before` and `after`, among
    /// those both have, with the tag and every word of a way whose valid
    /// bit or tag changed: a different line occupies it.
    pub(crate) fn changes(
        before: &BTreeMap<ChangeRef, Option<String>>,
        after: &BTreeMap<ChangeRef, Option<String>>,
        geometry: &Geometry,
    ) -> BTreeSet<ChangeRef> {
        let mut changed: BTreeSet<ChangeRef> = after
            .iter()
            .filter(|&(cell, value)| before.get(cell).is_some_and(|old| old != value))
            .map(|(cell, _)| *cell)
            .collect();
        let occupied: Vec<_> = changed
            .iter()
            .filter_map(|cell| match *cell {
                ChangeRef::Valid { set, way } | ChangeRef::Tag { set, way } => Some((set, way)),
                _ => None,
            })
            .collect();
        for (set, way) in occupied {
            changed.insert(ChangeRef::Tag { set, way });
            for word in 0..geometry.words_per_line() {
                let word = geometry.word_index(word).unwrap();
                changed.insert(ChangeRef::Word { set, way, word });
            }
        }
        changed
    }

    // --- Sessions at the log's limits -----------------------------------

    /// How many events short of [`LAST_OPEN`] the sessions near the events
    /// limit are built.
    const EVENTS_SHORT: u32 = 30;

    /// How many ops short of the most a link can hold the session near the
    /// ops limit is built.
    const OPS_SHORT: u32 = 4;

    /// The most events a log can hold and still take another op at its
    /// end: the limit, less the most an entry has. An op is refused for want
    /// of room once the log holds more.
    const LAST_OPEN: u32 = SESSION_EVENT_LIMIT - MAX_EVENTS_PER_ENTRY;

    /// The most bytes of room an edit can need in a saved file.
    const FILE_WINDOW: usize = 2048;

    /// The bytes of room in a saved file, beyond what the edit needs, that
    /// the walks along the file limit start with.
    const FILE_SLACK: usize = 40;

    /// How many events short of [`LAST_OPEN`] each walk starts, a negative
    /// number being past it.
    const SHORTS: std::ops::RangeInclusive<i32> = -1..=4;

    /// The accesses every session at a limit starts with, so that its cache
    /// holds lines, some of them dirty, and memory words the cache has not
    /// written.
    const WARM_UP: [(AccessKind, u32, Option<u64>); 4] = [
        (AccessKind::Write, 0x0A4, Some(9)),
        (AccessKind::Read, 0x1A4, None),
        (AccessKind::Write, 0x2A4, Some(4)),
        (AccessKind::Read, 0x0B0, None),
    ];

    /// A load of the assembly program `source`.
    fn assembly(source: &str) -> Op {
        Op::Program {
            source: source.to_owned(),
            format: ProgramFormat::Asm,
        }
    }

    /// The loop the sessions at a limit run: no exit and no memory access,
    /// so one event per instruction.
    const ENDLESS: &str = "        add r1, r0, #1\nloop:   bnez r1, loop";

    /// A load of the loop, followed by a comment of `letters` letters, which
    /// grows the op's JSON by exactly one byte a letter.
    fn padded(letters: usize) -> Op {
        assembly(&format!("{ENDLESS}\n; {}", "x".repeat(letters)))
    }

    /// A session with an empty log on `four-way-write-back`, after
    /// [`WARM_UP`].
    fn warmed() -> Session {
        let mut session = Session::with_config(Preset::FourWayWriteBack.config()).unwrap();
        for (kind, addr, value) in WARM_UP {
            let op = Op::Issue {
                kind,
                addr: number(addr.into()),
                value: value.map(number),
            };
            session.apply(op).unwrap();
        }
        session
    }

    /// `session` with more ops appended, `ops` of them in all, each running
    /// the loop as many instructions as spread the log's events evenly over
    /// them, so that its events number `events`; a loop that has used its
    /// budget is loaded again, and the last op loads it, so that whatever
    /// follows has all its budget.
    #[track_caller]
    fn spread(mut session: Session, ops: u32, events: u32) -> Session {
        let mut left = 0;
        while session.op_count() < ops - 1 {
            if left == 0 {
                session.apply(padded(0)).unwrap();
                // The budget, less the instruction the load starts with.
                left = INSTRUCTION_BUDGET - 1;
                continue;
            }
            let ops_left = ops - 1 - session.op_count();
            let events_left = events.saturating_sub(end(&session).get());
            let n = (events_left / ops_left).clamp(1, MAX_RUN_STEPS).min(left);
            session.apply(Op::Run { n }).unwrap();
            left -= n;
        }
        session.apply(padded(0)).unwrap();
        assert!(end(&session).get().abs_diff(events) < 8);
        session
    }

    /// Runs the loop on `session`, in one op, to `short` events short of
    /// [`LAST_OPEN`]; or, if `short` is negative, past it: the op that
    /// starts with room for another entry and ends without it.
    fn run_to(session: &mut Session, short: i32) {
        let gap = LAST_OPEN - end(session).get();
        let n = if short < 0 {
            gap + 3
        } else {
            gap - short.unsigned_abs()
        };
        session.apply(Op::Run { n }).unwrap();
    }

    /// An edit to plan and apply near each limit of the log.
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct NearLimit {
        /// The edit, resolved on the session at each limit.
        edit: EditIntent,
        /// How many undo stops the cursor goes back, once a limit has been
        /// passed, before the edit is checked once more.
        undos: u32,
    }

    /// An edit, and how far the cursor goes back.
    pub(crate) fn arb_near_limit() -> impl Strategy<Value = NearLimit> {
        (arb_edit_intent(), 0..=2_u32).prop_map(|(edit, undos)| NearLimit { edit, undos })
    }

    /// The sessions near each limit of the log, built once, since building
    /// one takes a log as long as the limit, and cloned for each case.
    pub(crate) struct Limits {
        /// A log of a few dozen ops whose events are a few dozen short of
        /// [`LAST_OPEN`].
        events: Session,
        /// A log a few ops short of the most a link can hold, whose events
        /// are a few dozen short of [`LAST_OPEN`].
        ops: Session,
        /// A log of padded programs, whose saved file has room for one more
        /// padded program of `letters` letters, to the byte, and no more,
        /// and whose events are a few dozen short of [`LAST_OPEN`].
        file: Session,
        /// The letters of the longest padded program `file` has room for.
        letters: usize,
    }

    impl Limits {
        /// The sessions near each limit.
        #[track_caller]
        pub(crate) fn new() -> Self {
            let short = LAST_OPEN - EVENTS_SHORT;
            let events = spread(warmed(), 80, short);
            let ops = spread(warmed(), MAX_SHARE_OPS - OPS_SHORT, short);
            // Half the longest program at a time, while the longest fits:
            // what is left then has room for more than half of it.
            let most = MAX_PROGRAM_BYTES as usize - ENDLESS.len() - "\n; ".len();
            let fits = |session: &Session, letters| session.clone().apply(padded(letters)).is_ok();
            let mut file = warmed();
            while fits(&file, most) {
                file.apply(padded(most / 2)).unwrap();
            }
            let ops_so_far = file.op_count();
            let file = spread(file, ops_so_far + 60, short);
            let (mut letters, mut high) = (0, most);
            assert!(fits(&file, letters));
            while high - letters > 1 {
                let mid = letters.midpoint(high);
                if fits(&file, mid) {
                    letters = mid;
                } else {
                    high = mid;
                }
            }
            assert!(letters > FILE_WINDOW + FILE_SLACK);
            Limits {
                events,
                ops,
                file,
                letters,
            }
        }

        /// `file` with `room` bytes left in its saved file, to the byte.
        fn file_with(&self, room: usize) -> Session {
            let mut session = self.file.clone();
            session.apply(padded(self.letters - room)).unwrap();
            session
        }
    }

    /// The error refusing an op for want of room that `reason` names, if it
    /// names one.
    fn room_error(reason: RefusalReason) -> Option<OpError> {
        match reason {
            RefusalReason::SessionFull => Some(OpError::SessionFull),
            RefusalReason::LinkFull => Some(OpError::LinkFull),
            RefusalReason::FileFull => Some(OpError::FileFull),
            _ => None,
        }
    }

    /// What the checks along a limit have found: whether the edit had room,
    /// and the refusals for want of room it met.
    #[derive(Default)]
    struct Seen {
        /// The edit had room somewhere.
        open: bool,
        /// The refusals met.
        refusals: Vec<OpError>,
    }

    impl Seen {
        /// Notes the refusal `refused`, if it is one.
        fn note(&mut self, refused: Option<OpError>) {
            match refused {
                Some(error) => self.refusals.push(error),
                None => self.open = true,
            }
        }

        /// Checks that the edit had room somewhere, and that each of
        /// `errors` refused it somewhere.
        fn met(&self, errors: &[OpError]) -> Result<(), TestCaseError> {
            prop_assert!(self.open, "the edit never had room");
            for error in errors {
                prop_assert!(self.refusals.contains(error), "{:?} was never met", error);
            }
            Ok(())
        }
    }

    /// Plans the edit of `patch` on `target` and applies it as an op on
    /// `session`, and checks that the plan names no room the log lacks that
    /// the op does not, and the op none that the plan does not, whatever
    /// else refuses them. Gives the refusal for want of room, if there is
    /// one. An edit accepted is left undone after the cursor, which is
    /// where it was.
    fn room_refusals_agree(
        session: &mut Session,
        target: EditTarget,
        patch: &EditPatch,
    ) -> Result<Option<OpError>, TestCaseError> {
        let planned = session
            .plan_edit(&target, patch)
            .err()
            .and_then(|refusal| room_error(refusal.reason));
        let at = session.position().pos;
        let op = Op::Edit {
            target,
            patch: patch.clone(),
        };
        let applied = session.apply(op).map(drop);
        if applied.is_ok() {
            session.seek(at);
        }
        let applied = applied.err().filter(|error| {
            matches!(
                error,
                OpError::SessionFull | OpError::LinkFull | OpError::FileFull
            )
        });
        prop_assert_eq!(&planned, &applied);
        Ok(planned)
    }

    /// [`room_refusals_agree`] with the cursor where it is and, when the
    /// last op has more than one event, inside that op, where the edit goes
    /// at the op's end all the same: so the room is the same, and the plan
    /// names it before it asks for the op to be finished.
    fn room_refusals_agree_here(
        session: &mut Session,
        target: EditTarget,
        patch: &EditPatch,
    ) -> Result<Option<OpError>, TestCaseError> {
        let at = session.position().pos;
        let refused = room_refusals_agree(session, target, patch)?;
        if let Some(before) = at.get().checked_sub(1) {
            session.seek(number(before.into()));
            if session.position().boundary < Boundary::OpEnd {
                let inside = room_refusals_agree(session, target, patch)?;
                prop_assert_eq!(&inside, &refused);
            }
            session.seek(at);
        }
        Ok(refused)
    }

    impl NearLimit {
        /// Checks that the edit is refused for want of room as an op
        /// exactly when the plan says so, with the same error, near each
        /// limit, on both sides of it, and that the refusals each limit
        /// leads to have all been met, in every order they can come in.
        pub(crate) fn check(self, limits: &Limits) -> Result<(), TestCaseError> {
            let mut events = Seen::default();
            self.walks(&limits.events, &mut events)?;
            events.met(&[OpError::SessionFull])?;
            let mut ops = Seen::default();
            self.walks(&limits.ops, &mut ops)?;
            ops.met(&[OpError::LinkFull, OpError::SessionFull])?;
            let mut file = Seen::default();
            self.file(limits, &mut file)?;
            file.met(&[OpError::FileFull, OpError::SessionFull])
        }

        /// Walks along the log of `session` from each of [`SHORTS`].
        fn walks(self, session: &Session, seen: &mut Seen) -> Result<(), TestCaseError> {
            for short in SHORTS {
                let mut session = session.clone();
                run_to(&mut session, short);
                self.walk(session, seen)?;
            }
            Ok(())
        }

        /// Goes on with `session` by ops of one event, until one is
        /// refused, checking the edit before each; then goes back, and
        /// checks it once more.
        fn walk(self, mut session: Session, seen: &mut Seen) -> Result<(), TestCaseError> {
            let (target, patch, _) = self.edit.resolve(&session);
            loop {
                let refused = room_refusals_agree_here(&mut session, target, &patch)?;
                seen.note(refused);
                if session.apply(Op::Step).is_err() {
                    break;
                }
            }
            for _ in 0..self.undos {
                session.undo();
            }
            room_refusals_agree_here(&mut session, target, &patch).map(drop)
        }

        /// Checks the edit with the room of the saved file just short of
        /// what it needs, just enough, and one byte more, each on its own;
        /// then walks from a few more bytes of room. A saved file's room is
        /// left to the byte by a padded program of the letters that leave
        /// exactly that room.
        fn file(self, limits: &Limits, seen: &mut Seen) -> Result<(), TestCaseError> {
            let (target, patch, _) = self.edit.resolve(&limits.file);
            let refused = |room: usize| {
                let op = Op::Edit {
                    target,
                    patch: patch.clone(),
                };
                limits.file_with(room).apply(op).map(drop) == Err(OpError::FileFull)
            };
            prop_assert!(refused(0), "no op fits in no room");
            prop_assert!(
                !refused(FILE_WINDOW),
                "an edit needs over {FILE_WINDOW} bytes"
            );
            // The edit is refused for want of room with `low` bytes, and has
            // room with `high`.
            let (mut low, mut high) = (0, FILE_WINDOW);
            while high - low > 1 {
                let mid = low.midpoint(high);
                if refused(mid) {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            for (room, closed) in [(low, true), (high, false), (high + 1, false)] {
                let mut session = limits.file_with(room);
                let refusal = room_refusals_agree_here(&mut session, target, &patch)?;
                prop_assert_eq!(refusal.is_some(), closed, "{} bytes", room);
                seen.note(refusal);
            }
            for short in SHORTS {
                let mut session = limits.file_with(high + FILE_SLACK);
                run_to(&mut session, short);
                self.walk(session, seen)?;
            }
            Ok(())
        }
    }

    // --- Round trips ----------------------------------------------------

    /// Checks that `value` reads back from its JSON text, and from its JSON
    /// value, as itself, and writes the same text again.
    pub(crate) fn round_trip<T>(value: &T) -> Result<(), TestCaseError>
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let text = serde_json::to_string(value).map_err(fail)?;
        let back: T = serde_json::from_str(&text)
            .map_err(|error| TestCaseError::fail(format!("{text} does not read: {error}")))?;
        prop_assert_eq!(&back, value);
        prop_assert_eq!(serde_json::to_string(&back).map_err(fail)?, text);
        let tree = serde_json::to_value(value).map_err(fail)?;
        let back: T = serde_json::from_value(tree.clone())
            .map_err(|error| TestCaseError::fail(format!("{tree} does not read: {error}")))?;
        prop_assert_eq!(&back, value);
        Ok(())
    }

    /// Any text a student or a file could hold: printable text, or a soup
    /// of the tokens the parsers know and the characters that trip them.
    pub(crate) fn arb_text() -> impl Strategy<Value = String> {
        // The tokens, separated by `|`.
        const TOKENS: &str = "ld|st|add|bnez|LD|Add|lw|r0|r1|r7|r8|R3|rx|[|]|,|:|;|#|-|\u{2212}|\
            0x|0X|0x0A4|0xA6|0x1000|0x10000000000000000|164|7|300|18446744073709551615|\
            99999999999999999999|#-1|#0x10|loop|start|_x|2go|\
            a_label_well_past_the_longest_a_label_may_be|R|W|w|r|X| |  |\t|\n|\r\n|\r|\0|\
            \u{e9}|\u{1d11e}|\u{feff}|{|}|\"|=|%|0|1";
        prop_oneof![
            any::<String>(),
            vec(select(TOKENS.split('|').collect::<Vec<_>>()), 0..48)
                .prop_map(|tokens| tokens.concat()),
        ]
    }

    /// Any op, its numbers unchecked, as JSON from outside can hold one.
    pub(crate) fn arb_op() -> impl Strategy<Value = Op> {
        let kind = select(vec![AccessKind::Read, AccessKind::Write]);
        let format = select(vec![ProgramFormat::Asm, ProgramFormat::Trace]);
        prop_oneof![
            arb_raw_config().prop_map(|config| Op::Config { config }),
            (kind, any::<u32>(), option::of(any::<u64>())).prop_map(|(kind, addr, value)| {
                Op::Issue {
                    kind,
                    addr: number(addr.into()),
                    value: value.map(number),
                }
            }),
            (any::<String>(), format).prop_map(|(source, format)| Op::Program { source, format }),
            Just(Op::Step),
            Just(Op::StepAccess),
            any::<u32>().prop_map(|n| Op::Run { n }),
            (arb_edit_target(), arb_edit_patch())
                .prop_map(|(target, patch)| Op::Edit { target, patch }),
        ]
    }

    /// Any edit target.
    pub(crate) fn arb_edit_target() -> impl Strategy<Value = EditTarget> {
        prop_oneof![
            (any::<u32>(), any::<u32>()).prop_map(|(set, way)| EditTarget::Way {
                set: number(set.into()),
                way: number(way.into()),
            }),
            any::<u32>().prop_map(|addr| EditTarget::Memory {
                addr: number(addr.into())
            }),
        ]
    }

    /// Any edit patch.
    pub(crate) fn arb_edit_patch() -> impl Strategy<Value = EditPatch> {
        prop_oneof![
            arb_way_patch().prop_map(EditPatch::Way),
            any::<u64>().prop_map(|value| EditPatch::Memory {
                value: number(value)
            }),
        ]
    }

    /// Any way patch, each field given or not.
    pub(crate) fn arb_way_patch() -> impl Strategy<Value = WayPatch> {
        (
            option::of(any::<bool>()),
            option::of(any::<u32>()),
            option::of(vec(any::<u64>(), 0..20)),
            option::of(any::<bool>()),
            option::of(any::<u32>()),
        )
            .prop_map(|(valid, line, data, dirty, rank)| WayPatch {
                valid,
                line: line.map(|line| number(line.into())),
                data: data.map(|data| data.into_iter().map(number).collect()),
                dirty,
                rank,
            })
    }

    /// Any way editor form.
    pub(crate) fn arb_way_form() -> impl Strategy<Value = WayForm> {
        (
            any::<bool>(),
            any::<String>(),
            vec(any::<String>(), 0..6),
            any::<bool>(),
            option::of(any::<u32>()),
        )
            .prop_map(|(valid, line_text, word_texts, dirty, rank)| WayForm {
                valid,
                line_text,
                word_texts,
                dirty,
                rank,
            })
    }

    /// Any share document, its format version and its ops unchecked.
    pub(crate) fn arb_share_doc() -> impl Strategy<Value = ShareDoc> {
        let at = (any::<u32>(), any::<u32>()).prop_map(|(op, event)| ShareAt { op, event });
        (
            any::<u32>(),
            arb_raw_config(),
            vec(arb_op(), 0..6),
            option::of(at),
        )
            .prop_map(|(schema_version, config, ops, at)| ShareDoc {
                schema_version,
                config,
                ops,
                at,
            })
    }

    /// Any event match.
    pub(crate) fn arb_event_match() -> impl Strategy<Value = EventMatch> {
        (select(event_kinds().to_vec()), option::of(any::<u32>()))
            .prop_map(|(kind, entry)| EventMatch { kind, entry })
    }

    /// Any access match.
    pub(crate) fn arb_access_match() -> impl Strategy<Value = AccessMatch> {
        (
            option::of(select(vec![AccessKind::Read, AccessKind::Write])),
            option::of(any::<u32>()),
            option::of(select(vec![Outcome::Hit, Outcome::Miss])),
        )
            .prop_map(|(kind, addr, outcome)| AccessMatch {
                kind,
                addr: addr.map(|addr| number(addr.into())),
                outcome,
            })
    }

    /// Any configuration match.
    pub(crate) fn arb_config_match() -> impl Strategy<Value = ConfigMatch> {
        let sizes = (
            option::of(any::<u32>()),
            option::of(any::<u32>()),
            option::of(any::<u32>()),
        );
        let policies = (
            option::of(select(vec![
                Replacement::Lru,
                Replacement::Fifo,
                Replacement::Random,
            ])),
            option::of(select(vec![
                WritePolicy::WriteBack,
                WritePolicy::WriteThrough,
            ])),
            option::of(select(vec![
                AllocatePolicy::WriteAllocate,
                AllocatePolicy::WriteNoAllocate,
            ])),
            option::of(any::<bool>()),
        );
        (sizes, policies).prop_map(
            |(
                (size_bytes, line_bytes, associativity),
                (replacement, write_policy, allocate_policy, timed),
            )| {
                ConfigMatch {
                    size_bytes,
                    line_bytes,
                    associativity,
                    replacement,
                    write_policy,
                    allocate_policy,
                    timed,
                }
            },
        )
    }

    /// Every event kind.
    pub(crate) fn event_kinds() -> [EventKind; 16] {
        [
            EventKind::Decode,
            EventKind::Hit,
            EventKind::Miss,
            EventKind::Victim,
            EventKind::WriteBack,
            EventKind::WriteThrough,
            EventKind::Fill,
            EventKind::Write,
            EventKind::Rank,
            EventKind::MemWrite,
            EventKind::Stall,
            EventKind::Complete,
            EventKind::Edit,
            EventKind::Error,
            EventKind::Evict,
            EventKind::Setup,
        ]
    }

    /// Every register.
    pub(crate) fn registers() -> [Register; 8] {
        [
            Register::R0,
            Register::R1,
            Register::R2,
            Register::R3,
            Register::R4,
            Register::R5,
            Register::R6,
            Register::R7,
        ]
    }

    /// Any predicate, nested up to three deep, its numbers unchecked.
    pub(crate) fn arb_predicate() -> impl Strategy<Value = Predicate> {
        let way = (any::<u32>(), any::<u32>()).prop_map(|(set, way)| WayLoc {
            set: number(set.into()),
            way: number(way.into()),
        });
        let addr = any::<u32>().prop_map(|addr| AddrLoc {
            addr: number(addr.into()),
        });
        let flags = (option::of(any::<bool>()), option::of(any::<bool>()));
        let leaf = prop_oneof![
            (
                way,
                flags.clone(),
                option::of(any::<u32>()),
                option::of(any::<u32>())
            )
                .prop_map(|(way, (valid, dirty), tag, rank)| Predicate::Way(
                    WayPredicate {
                        way,
                        valid,
                        tag: tag.map(|tag| number(tag.into())),
                        dirty,
                        rank,
                    }
                )),
            (addr.clone(), flags).prop_map(|(line, (present, dirty))| Predicate::Line(
                LinePredicate {
                    line,
                    present,
                    dirty
                }
            )),
            (addr, any::<u64>()).prop_map(|(memory, equals)| Predicate::Memory(MemoryPredicate {
                memory,
                equals: number(equals),
            })),
            (
                select(COUNTERS.to_vec()),
                select(vec![CmpOp::Equal, CmpOp::AtLeast, CmpOp::AtMost]),
                any::<u32>()
            )
                .prop_map(|(counter, op, value)| Predicate::Counter(
                    CounterPredicate {
                        counter,
                        op,
                        value: number(value.into())
                    }
                )),
            (select(registers().to_vec()), any::<u64>()).prop_map(|(register, equals)| {
                Predicate::Register(RegisterPredicate {
                    register,
                    equals: number(equals),
                })
            }),
            arb_event_match().prop_map(|event| Predicate::Event(EventPredicate { event })),
            arb_access_match()
                .prop_map(|last_access| Predicate::LastAccess(LastAccessPredicate { last_access })),
            arb_config_match().prop_map(|config| Predicate::Config(ConfigPredicate { config })),
        ];
        leaf.prop_recursive(3, 24, 4, |inner| {
            prop_oneof![
                vec(inner.clone(), 0..4).prop_map(|all| Predicate::All(AllPredicate { all })),
                vec(inner.clone(), 0..4).prop_map(|any| Predicate::Any(AnyPredicate { any })),
                inner.prop_map(|not| Predicate::Not(NotPredicate { not: Box::new(not) })),
            ]
        })
    }

    /// Any module step, its predicates and ops unchecked.
    pub(crate) fn arb_step() -> impl Strategy<Value = Step> {
        let areas = vec![
            CalloutArea::AddressStrip,
            CalloutArea::Cache,
            CalloutArea::Memory,
            CalloutArea::Log,
            CalloutArea::Stats,
            CalloutArea::Config,
            CalloutArea::Cpu,
            CalloutArea::Transport,
        ];
        let callout = option::of(prop_oneof![
            select(areas).prop_map(Callout::Area),
            (any::<u32>(), option::of(any::<u32>())).prop_map(|(set, way)| Callout::Cell(
                CellRef {
                    set: number(set.into()),
                    way: way.map(|way| number(way.into())),
                }
            )),
        ]);
        let ops = || option::of(vec(arb_op(), 0..3));
        let text = || option::of(any::<String>());
        prop_oneof![
            (
                any::<String>(),
                arb_predicate(),
                text(),
                ops(),
                callout.clone()
            )
                .prop_map(|(text, expect, hint, show_me, callout)| Step::Instruct {
                    text,
                    expect,
                    hint,
                    show_me,
                    callout,
                }),
            (
                (any::<String>(), vec(any::<String>(), 0..4), any::<u32>()),
                (any::<String>(), ops(), text(), callout.clone())
            )
                .prop_map(
                    |((text, options, answer), (explain, then, hint, callout))| {
                        Step::Predict {
                            text,
                            options,
                            answer,
                            explain,
                            then,
                            hint,
                            callout,
                        }
                    }
                ),
            (any::<String>(), arb_predicate(), callout.clone()).prop_map(
                |(text, until, callout)| Step::Observe {
                    text,
                    until,
                    callout
                }
            ),
            (any::<String>(), callout).prop_map(|(text, callout)| Step::Explain { text, callout }),
        ]
    }

    /// The typed-number fields of the configuration bar.
    pub(crate) fn number_fields() -> [ConfigNumberField; 2] {
        [ConfigNumberField::HitCycles, ConfigNumberField::MissCycles]
    }
}

/// The fold is a pure function of the initial configuration and the log: two
/// replays agree, with each other and with the session built one call at a
/// time, at the end of its log.
#[test]
fn replay_is_a_pure_function_of_the_log() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        let session = &mut student.session;
        let initial = *session.initial();
        let ops = session.ops().to_vec();
        let replay = Session::replay(&initial, &ops);
        prop_assert_eq!(&replay, &Session::replay(&initial, &ops));
        let replay = replay.unwrap();
        session.seek(end(session));
        prop_assert_eq!(session.machine(), &replay.machine);
        prop_assert_eq!(session.events_range(0..u32::MAX), replay.events.as_slice());
        Ok(())
    });
}

/// Scrubbing to a count of ops reaches the fold of that many ops; undo from
/// the end of the log visits every undo stop back to position 0, and redo
/// every one forward again; from any position, undo and redo go to the stops
/// before and after it; and a seek reaches the fold stopped at its position.
#[test]
fn undo_redo_scrubbing_and_seeking_reach_the_fold() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        let session = &mut student.session;
        let initial = *session.initial();
        let ops = session.ops().to_vec();
        let ends = op_ends(session);
        let fold = |pos: u32| Session::replay_to(&initial, &ops, number(pos.into())).unwrap();
        for done in 0..=ops.len() {
            session.scrub_to(u32::try_from(done).unwrap());
            let replay = Session::replay(&initial, &ops[..done]).unwrap();
            prop_assert_eq!(session.machine(), &replay.machine);
            let at = done.checked_sub(1).map_or(0, |last| ends[last]);
            prop_assert_eq!(session.position().pos.get(), at);
            prop_assert_eq!(session.cursor() as usize, done);
        }
        let stops = undo_stops(session);
        session.seek(end(session));
        for &stop in stops.iter().rev().skip(1) {
            prop_assert!(session.undo());
            prop_assert_eq!(session.position().pos.get(), stop);
            prop_assert_eq!(session.machine(), &fold(stop));
        }
        prop_assert!(!session.undo());
        for &stop in stops.iter().skip(1) {
            prop_assert!(session.redo());
            prop_assert_eq!(session.position().pos.get(), stop);
            prop_assert_eq!(session.machine(), &fold(stop));
        }
        prop_assert!(!session.redo());
        for pos in 0..=end(session).get() {
            session.seek(number(pos.into()));
            prop_assert_eq!(session.machine(), &fold(pos));
            // Undo goes back to the last stop at or before the start of the
            // op holding the cursor; redo on to the first stop after it.
            let holding = session.position().op.map(|op| op.get() as usize);
            let back = holding.map(|op| {
                let start = op_start(&ends, op);
                stops
                    .iter()
                    .copied()
                    .filter(|&stop| stop <= start)
                    .max()
                    .unwrap_or(0)
            });
            prop_assert_eq!(session.undo(), back.is_some());
            prop_assert_eq!(session.position().pos.get(), back.unwrap_or(pos));
            session.seek(number(pos.into()));
            let on = stops.iter().copied().find(|&stop| stop > pos);
            prop_assert_eq!(session.redo(), on.is_some());
            prop_assert_eq!(session.position().pos.get(), on.unwrap_or(pos));
        }
        Ok(())
    });
}

/// Space from position 0 visits every position of the log in turn, each the
/// fold's machine; a quiet replay, which words nothing, reaches the machine
/// a recording one does at the end of every op; every event's id is its
/// position less one; runs are numbered from 1 and start at a new
/// configuration's setup; and each entry's events are numbered from 1, its
/// label saying which entry of the run and which event of it.
#[test]
fn space_visits_every_event_in_order() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        let session = &mut student.session;
        let initial = *session.initial();
        let ops = session.ops().to_vec();
        let end = end(session).get();
        session.seek(number(0));
        for pos in 1..=end {
            prop_assert_eq!(session.step_event(None), StepOutcome::Moved);
            prop_assert_eq!(session.position().pos.get(), pos);
            let fold = Session::replay_to(&initial, &ops, number(pos.into())).unwrap();
            prop_assert_eq!(session.machine(), &fold);
        }
        for (done, end) in (1..).zip(op_ends(session)) {
            let recorded = Session::replay(&initial, &ops[..done]).unwrap().machine;
            let quiet = Session::replay_to(&initial, &ops, number(end.into()));
            prop_assert_eq!(quiet, Ok(recorded));
        }
        let events = session.events_range(0..u32::MAX).to_vec();
        let mut run = 1;
        let mut entry = 0;
        let mut previous: Option<&MicroEvent> = None;
        for (id, event) in (0_u64..).zip(&events) {
            prop_assert_eq!(event.id, id);
            let config_op = matches!(ops[event.op_index.get() as usize], Op::Config { .. });
            if event.kind == EventKind::Setup && config_op {
                run += 1;
                entry = 0;
            }
            prop_assert_eq!(event.run, run);
            if let Some(previous) = previous.filter(|_| event.seq > 1) {
                prop_assert_eq!(event.seq, previous.seq + 1);
                prop_assert_eq!(
                    (event.op_index, event.entry),
                    (previous.op_index, previous.entry)
                );
            } else {
                prop_assert_eq!(event.seq, 1);
                if event.kind != EventKind::Setup {
                    entry += 1;
                }
            }
            if let Some(number) = event.entry {
                prop_assert_eq!(number, entry);
                prop_assert_eq!(&event.label, &format!("{number}.{}", event.seq));
            } else {
                prop_assert_eq!(event.kind, EventKind::Setup);
                prop_assert_eq!(event.label.as_str(), "");
            }
            previous = Some(event);
        }
        Ok(())
    });
}

/// A share string and a saved file each give back the initial configuration,
/// whose flag is the mode chosen, the whole log with its undone ops, the
/// cursor, position 0 included, every event and the state, which may differ
/// only in the displayed configuration's flag; the session's share string is
/// its document's.
#[test]
fn share_strings_and_files_give_the_session_back() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        let session = &mut student.session;
        let end = end(session).get();
        // Where the student left the cursor, then position 0, then halfway.
        for pos in [None, Some(0), Some(end / 2)] {
            if let Some(pos) = pos {
                session.seek(number(pos.into()));
            }
            let doc = session.share_doc();
            prop_assert_eq!(session.to_share_string(), Ok(doc.to_share_string()));
            // A saved file is the compact JSON the link deflates.
            prop_assert_eq!(session.to_json(), doc.to_json());
            prop_assert_eq!(session.to_json(), serde_json::to_string(&doc).unwrap());
            prop_assert_eq!(session.to_json_len() as usize, session.to_json().len());
            let link = doc.to_share_string();
            prop_assert_eq!(ShareDoc::from_share_string(&link), Ok(doc.clone()));
            prop_assert_eq!(ShareDoc::from_json(&doc.to_json()), Ok(doc.clone()));
            for loaded in [
                Session::from_share_string(&session.to_share_string().unwrap()),
                Session::from_json(&session.to_json()),
            ] {
                let loaded = loaded.unwrap();
                prop_assert_eq!(loaded.initial(), &doc.config);
                prop_assert_eq!(
                    without_mode(loaded.initial()),
                    without_mode(session.initial())
                );
                prop_assert_eq!(loaded.ops(), session.ops());
                prop_assert_eq!(loaded.position(), session.position());
                prop_assert_eq!(
                    loaded.events_range(0..u32::MAX),
                    session.events_range(0..u32::MAX)
                );
                let (mut ours, mut theirs) = (session.state(), loaded.state());
                prop_assert_eq!(without_mode(&ours.config), without_mode(&theirs.config));
                ours.config.advanced = false;
                theirs.config.advanced = false;
                prop_assert_eq!(theirs, ours);
            }
        }
        Ok(())
    });
}

/// At every event, the ranks of every set are a permutation of its ways;
/// under random replacement, and in a set of one way, each way's rank is its
/// number, edits included.
#[test]
fn ranks_stay_a_permutation() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, _| {
            let state = session.state();
            let ways = state.geometry.ways();
            for set in &state.cache.sets {
                if state.cache.show_rank {
                    let mut ranks: Vec<u32> =
                        set.ways.iter().map(|way| way.rank.unwrap().value).collect();
                    ranks.sort_unstable();
                    prop_assert_eq!(ranks, (0..ways).collect::<Vec<_>>());
                } else {
                    // The rank column is hidden, so the ranks are read the
                    // way a module's predicate reads them.
                    for way in 0..ways {
                        let identity = Predicate::Way(WayPredicate {
                            way: WayLoc {
                                set: set.set,
                                way: number(way.into()),
                            },
                            valid: None,
                            tag: None,
                            dirty: None,
                            rank: Some(way),
                        });
                        let held = session.eval_predicate(&identity);
                        prop_assert!(held, "set {:?} way {} is not rank {}", set.set, way, way);
                    }
                }
            }
            Ok(())
        })
    });
}

/// Under LRU and FIFO, every valid way ranks before every empty one at every
/// entry boundary.
#[test]
fn valid_ways_rank_before_empty_ones_between_entries() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, _| {
            let state = session.state();
            if state.position.boundary == Boundary::MidEntry || !state.cache.show_rank {
                return Ok(());
            }
            for set in &state.cache.sets {
                let ranks = |valid: bool| {
                    set.ways
                        .iter()
                        .filter(move |way| way.valid == valid)
                        .map(|way| way.rank.unwrap().value)
                };
                if let (Some(last), Some(first)) = (ranks(true).max(), ranks(false).min()) {
                    prop_assert!(last < first, "set {:?}: {:?}", set.set, set.ways);
                }
            }
            Ok(())
        })
    });
}

/// At every event, a valid way's line decodes to the way's own set and tag,
/// and no set holds a tag twice.
#[test]
fn every_line_sits_once_in_its_own_set() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, _| {
            let state = session.state();
            for set in &state.cache.sets {
                let mut tags = BTreeSet::new();
                for way in set.ways.iter().filter(|way| way.valid) {
                    let line = way.line_text.as_deref().unwrap().trim_start_matches("0x");
                    let line = u32::from_str_radix(line, 16).unwrap();
                    let decoded = state.geometry.decode(line).unwrap();
                    prop_assert_eq!(decoded.index, set.set);
                    prop_assert_eq!(Some(&decoded.tag_text), way.tag_text.as_ref());
                    let once = tags.insert(decoded.tag);
                    prop_assert!(once, "set {:?} holds {} twice", set.set, decoded.tag_text);
                }
            }
            Ok(())
        })
    });
}

/// Under write-through no way is ever dirty.
#[test]
fn write_through_leaves_no_way_dirty() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, _| {
            let state = session.state();
            if state.config.cache.write_policy == WritePolicy::WriteThrough {
                for set in &state.cache.sets {
                    prop_assert!(set.ways.iter().all(|way| !way.dirty), "set {:?}", set.set);
                }
            }
            Ok(())
        })
    });
}

/// Hits and misses add up to the accesses at every event.
#[test]
fn hits_and_misses_add_up_to_accesses() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, _| {
            let counters = session.stats().counters;
            prop_assert_eq!(counters.hits + counters.misses, counters.accesses);
            prop_assert_eq!(counters.loads + counters.stores, counters.accesses);
            Ok(())
        })
    });
}

/// Between entries, a clean valid way holds memory's words; at every event,
/// a word is stale exactly when a dirty way holds it with another value, and
/// the stale mark names that way.
#[test]
fn clean_lines_match_memory_and_stale_words_have_dirty_lines() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        student.watch();
        let addrs = student.addrs.clone();
        every_position(&mut student.session, |session, _| {
            let state = session.state();
            let geometry = state.geometry;
            let rows = session.memory_lines(0, u32::MAX);
            let memory: BTreeMap<u32, &MemoryWordView> = rows
                .iter()
                .flat_map(|row| &row.words)
                .map(|word| (word.addr.get(), word))
                .collect();
            let between = state.position.boundary != Boundary::MidEntry;
            for set in &state.cache.sets {
                for (index, way) in (0_u32..).zip(&set.ways) {
                    let Some(line) = &way.line_text else { continue };
                    let line = u32::from_str_radix(line.trim_start_matches("0x"), 16).unwrap();
                    for (word, text) in (0_u32..).zip(&way.words) {
                        let addr = line + word * geometry.word_bytes();
                        let cell = memory.get(&addr).unwrap();
                        let differs = &cell.text != text;
                        let clean = !way.dirty && between;
                        prop_assert!(
                            !(clean && differs),
                            "{:?}/{} at {:#X}",
                            set.set,
                            index,
                            addr
                        );
                        let marked = cell
                            .stale
                            .as_ref()
                            .map(|stale| (stale.set, stale.way.get()));
                        let holder = (way.dirty && differs).then_some((set.set, index));
                        prop_assert_eq!(marked, holder);
                    }
                }
            }
            for &addr in &addrs {
                let Some(addr) = geometry.addr(addr) else {
                    continue;
                };
                if let Some(stale) = session.machine().stale(addr) {
                    let set = &state.cache.sets[stale.set.get() as usize];
                    let way = &set.ways[stale.way.get() as usize];
                    prop_assert!(way.valid && way.dirty, "{:?} is stale in {:?}", addr, way);
                    prop_assert_ne!(stale.cached, stale.memory);
                }
            }
            Ok(())
        })
    });
}

/// In timed mode the cycles are the hit time for every access and the miss
/// penalty for every miss and every write-back, at every entry boundary;
/// untimed, there are none, and no event has a cycle stamp.
#[test]
fn cycles_follow_the_counters() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, event| {
            let state = session.state();
            let counters = &state.stats.counters;
            let cycles = counters.cycles.get();
            if let Some(event) = event {
                prop_assert_eq!(event.cycle.is_some(), state.config.timing.is_some());
            }
            match state.config.timing {
                None => prop_assert_eq!(cycles, 0),
                Some(timing) if state.position.boundary != Boundary::MidEntry => {
                    let hit = u64::from(timing.hit_cycles);
                    let miss = u64::from(timing.miss_cycles);
                    let expected = u64::from(counters.accesses) * hit
                        + u64::from(counters.misses) * miss
                        + u64::from(counters.writebacks) * miss;
                    prop_assert_eq!(cycles, expected);
                }
                Some(_) => {}
            }
            Ok(())
        })
    });
}

/// Each event lists exactly the cells and counters it changed, sorted and
/// each once: over everything a change can name, the difference between the
/// states on either side of it, with the tag and every word of a way that
/// took another line. A new configuration's setup starts a run rather than
/// changing one, and is left out.
#[test]
fn each_event_lists_exactly_what_it_changed() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        student.watch();
        let ops = student.session.ops().to_vec();
        let mut before = None;
        every_position(&mut student.session, |session, event| {
            let now = cells(session);
            if let (Some(event), Some(before)) = (event, &before) {
                let config_op = matches!(ops[event.op_index.get() as usize], Op::Config { .. });
                if !(event.kind == EventKind::Setup && config_op) {
                    let geometry = session.state().geometry;
                    let changed: Vec<ChangeRef> =
                        changes(before, &now, &geometry).into_iter().collect();
                    let sorted = event.changed.windows(2).all(|pair| pair[0] < pair[1]);
                    prop_assert!(sorted, "{:?}", event.changed);
                    prop_assert_eq!(
                        &event.changed,
                        &changed,
                        "event {} ({:?}): {}",
                        event.label,
                        event.kind,
                        event.explain
                    );
                }
            }
            before = Some(now);
            Ok(())
        })
    });
}

/// A refused call, whether an op, a configuration change, a mode switch that
/// asks first, Space or Shift+Space with nothing to do, or an undo or redo
/// with nowhere to go, and with or without undone ops after the cursor,
/// leaves the log, the undone ops, the cursor and the state as they were.
#[test]
fn a_refused_call_changes_nothing() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let checks = Checks {
            refusals: true,
            ..Checks::default()
        };
        play(&config, &actions, checks).map(drop)
    });
}

/// A plan applies, keeps the invariants, and planned again is itself; a
/// refusal's suggestion always plans; at the end of an op, an edit the plan
/// refuses, with or without a suggestion, is refused as an op with the same
/// refusal; and a plan that takes a dirty line from its way while some of
/// its words are newer than memory's says so, naming them, and only then.
#[test]
fn planned_edits_apply_and_drop_no_dirty_data_silently() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let checks = Checks {
            edits: true,
            ..Checks::default()
        };
        play(&config, &actions, checks).map(drop)
    });
}

/// The compulsory, capacity and conflict misses add up to the misses at
/// every event.
#[test]
fn miss_classes_add_up_to_misses() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, _| {
            let counters = session.stats().counters;
            let classes = counters.compulsory + counters.capacity + counters.conflict;
            prop_assert_eq!(classes, counters.misses);
            Ok(())
        })
    });
}

/// The machine's invariants hold after every event, and no event is an
/// internal error.
#[test]
fn the_invariants_hold_at_every_event() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, event| {
            prop_assert_eq!(session.machine().check_invariants(), Ok(()));
            if let Some(event) = event {
                let internal = event.explain.starts_with("Internal error");
                prop_assert!(
                    event.kind != EventKind::Error || !internal,
                    "{}",
                    event.explain
                );
            }
            Ok(())
        })
    });
}

/// A write-back run counts no write-throughs, and a write-allocate run no
/// write-arounds; the words written to memory are the write-throughs and
/// write-arounds, the lines read the fills and the lines written the
/// write-backs, and the bytes follow.
#[test]
fn memory_writes_follow_the_policies() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, _| {
            let stats = session.stats();
            let (counters, traffic) = (&stats.counters, &stats.traffic);
            let config = session.config();
            if config.cache.write_policy == WritePolicy::WriteBack {
                prop_assert_eq!(counters.writethroughs, 0);
            }
            if config.cache.allocate_policy == AllocatePolicy::WriteAllocate {
                prop_assert_eq!(counters.writearounds, 0);
            }
            let words = counters.writethroughs + counters.writearounds;
            prop_assert_eq!(traffic.word_writes, words);
            prop_assert_eq!(traffic.line_reads, counters.fills);
            prop_assert_eq!(traffic.line_writes, counters.writebacks);
            let geometry = session.state().geometry;
            let line = u64::from(geometry.line_bytes());
            let word = u64::from(geometry.word_bytes());
            prop_assert_eq!(traffic.bytes_read, u64::from(counters.fills) * line);
            prop_assert_eq!(
                traffic.bytes_written,
                u64::from(counters.writebacks) * line + u64::from(traffic.word_writes) * word
            );
            Ok(())
        })
    });
}

/// The log is append-only: every call keeps every op before the point it
/// appends at, a call that appends nothing keeps them all, and the initial
/// configuration never changes.
#[test]
fn the_log_is_append_only() {
    check(sessions(), &arb_session(), |(config, actions)| {
        let checks = Checks {
            append_only: true,
            ..Checks::default()
        };
        play(&config, &actions, checks).map(drop)
    });
}

/// A log several checkpoint intervals long, of long runs of a loop and of
/// what a student does after them, a refused call changing nothing, the log
/// staying append-only and edits going as planned, then cut back and grown
/// again past the checkpoints it had. Before the cut and after, a seek
/// around each checkpoint, to the end of each op or anywhere else reaches
/// the fold's machine; the ops appended at the cut build on the machine
/// there; undo and redo stop at every undo stop with the fold's machine; and
/// a share string, and a saved file loaded a slice at a time, give back a
/// session whose seeks reach the same machines.
#[test]
fn seeks_across_checkpoints_reach_the_fold() {
    check(long_sessions(), &arb_long_session(), |long| {
        let checks = Checks {
            refusals: true,
            append_only: true,
            edits: true,
        };
        let mut student = play_long(&long, checks)?;
        let positions = checked_positions(&student.session, &long.picks);
        let mut fold = Fold::of(&student.session);
        seeks_reach(&mut student.session, positions, &mut fold)?;
        student.cut_and_grow(&long)?;
        let session = &mut student.session;
        let end = end(session).get();
        let replay = Session::replay(session.initial(), session.ops()).unwrap();
        prop_assert_eq!(session.events_range(0..u32::MAX), replay.events.as_slice());
        prop_assert_eq!(session.machine(), &replay.machine);
        let positions = checked_positions(session, &long.picks);
        let mut fold = Fold::of(session);
        seeks_reach(session, positions.iter().copied(), &mut fold)?;
        let stops = undo_stops(session);
        session.seek(number(end.into()));
        for &stop in stops.iter().rev().skip(1) {
            prop_assert!(session.undo());
            prop_assert_eq!(session.position().pos.get(), stop);
            prop_assert_eq!(session.machine(), fold.at(stop));
        }
        prop_assert!(!session.undo());
        for &stop in stops.iter().skip(1) {
            prop_assert!(session.redo());
            prop_assert_eq!(session.position().pos.get(), stop);
            prop_assert_eq!(session.machine(), fold.at(stop));
        }
        prop_assert!(!session.redo());
        // Saved with the cursor where the first pick puts it, and opened
        // again: the same log, on the initial configuration in the mode
        // chosen, a flag the fold never reads but keeps in the machine.
        let at = long.picks[0] % (end + 1);
        session.seek(number(at.into()));
        let opened = [
            Session::from_share_string(&session.to_share_string().unwrap()).unwrap(),
            load_in_slices(&session.to_json(), long.slice)?,
        ];
        let mut fold = Fold::of(&opened[0]);
        for mut loaded in opened {
            prop_assert_eq!(
                without_mode(loaded.initial()),
                without_mode(session.initial())
            );
            prop_assert_eq!(loaded.ops(), session.ops());
            prop_assert_eq!(
                loaded.events_range(0..u32::MAX),
                session.events_range(0..u32::MAX)
            );
            prop_assert_eq!(loaded.position(), session.position());
            prop_assert_eq!(loaded.machine(), fold.at(at));
            seeks_reach(&mut loaded, positions.iter().copied(), &mut fold)?;
        }
        Ok(())
    });
}

/// Near each limit of the log, the events a session keeps, the ops a link
/// can hold and the bytes a saved file can hold, an edit is refused for want
/// of room as an op exactly when the plan says so, with the same error: at
/// the end of the log and inside its last op, on both sides of the limit,
/// with the events limit close by the other two so that either is reached
/// first or both together, and once the cursor has gone back.
#[test]
fn edits_are_refused_for_room_as_planned_near_each_limit() {
    let limits = Limits::new();
    check(near_limits(), &arb_near_limit(), |near| near.check(&limits));
}

/// The properties over whole sessions run their share of the cases, and the
/// property over long sessions its share, whatever `PROPTEST_CASES` says:
/// the `proptest!` macro would run as many as `PROPTEST_CASES` of each.
#[test]
fn session_properties_run_their_share_of_the_cases() {
    let cases = Cell::new(0);
    check(sessions(), &arb_session(), |_| {
        cases.set(cases.get() + 1);
        Ok(())
    });
    assert_eq!(cases.get(), sessions().cases);
    cases.set(0);
    check(long_sessions(), &arb_long_session(), |_| {
        cases.set(cases.get() + 1);
        Ok(())
    });
    assert_eq!(cases.get(), long_sessions().cases);
    assert_eq!(sessions().cases, (values().cases / 4).max(1));
}

proptest! {
    #![proptest_config(values())]

    /// Any configuration that does not validate repairs to one that does,
    /// keeping its word size, policies, memory contents, mode and whether
    /// it is timed; one that validates needs no repair.
    #[test]
    fn repair_always_gives_a_valid_configuration(config in arb_raw_config()) {
        let repair = config.repair();
        if config.validate().is_ok() {
            prop_assert_eq!(repair, None);
        } else {
            let repair = repair.unwrap();
            let valid = repair.config.validate().is_ok();
            prop_assert!(valid, "{:?} repairs to {:?}", config, repair.config);
            prop_assert!(!repair.label.is_empty());
            let kept = |config: &Config| {
                let cache = &config.cache;
                (
                    (config.word_bytes, config.memory_init, config.advanced),
                    (cache.replacement, cache.write_policy, cache.allocate_policy),
                    config.timing.is_some(),
                )
            };
            prop_assert_eq!(kept(&repair.config), kept(&config));
        }
    }

    /// A valid configuration's tag has a bit; its fields fill the address;
    /// and every address splits into a tag, a set and an offset from which
    /// its line and the address itself are rebuilt, while the first
    /// address past the width does not decode.
    #[test]
    fn every_address_splits_and_rebuilds(
        config in arb_valid_config(),
        picks in proptest::collection::vec(any::<u32>(), 32),
    ) {
        let geometry = config.validate().unwrap();
        prop_assert!(geometry.tag_bits() >= 1);
        let fields = [geometry.tag_bits(), geometry.index_bits(), geometry.offset_bits()];
        let width: u32 = fields.into_iter().map(u32::from).sum();
        prop_assert_eq!(width, u32::from(geometry.address_bits()));
        let max = u32::MAX >> (32 - u32::from(geometry.address_bits()));
        for addr in picks.into_iter().map(|pick| pick & max).chain([0, max]) {
            let decoded = geometry.decode(addr).unwrap();
            let line = geometry.line_addr(decoded.tag, decoded.index);
            prop_assert_eq!(line, decoded.line);
            prop_assert_eq!(line.get() + decoded.offset, addr);
            prop_assert!(decoded.index.get() < geometry.sets());
            prop_assert!(geometry.tag(decoded.tag.get()).is_some());
            prop_assert_eq!(decoded.word.get(), decoded.offset / geometry.word_bytes());
            prop_assert_eq!(decoded.byte_in_word, decoded.offset % geometry.word_bytes());
            prop_assert_eq!(decoded.aligned, addr % geometry.word_bytes() == 0);
        }
        if let Some(past) = max.checked_add(1) {
            prop_assert!(geometry.decode(past).is_err());
        }
    }

    /// The parsers take any text without panicking: a program or trace
    /// parses, or lists its first errors in source order, each at a line
    /// and a column from 1 and quoting them; and the typed-input readers,
    /// share documents, saved files and modules read any text too.
    #[test]
    fn the_parsers_are_total(config in arb_config(), text in arb_text()) {
        let session = Session::with_config(config).unwrap();
        let geometry = config.validate().unwrap();
        for format in [ProgramFormat::Asm, ProgramFormat::Trace] {
            let result = geometry.parse_program(&text, format);
            prop_assert_eq!(&session.parse_program(&text, format), &result);
            match result {
                ParseResult::Ok { instructions, .. } => prop_assert!(instructions >= 1),
                ParseResult::Invalid { errors } => {
                    prop_assert!(!errors.is_empty());
                    prop_assert!(errors.len() <= MAX_PARSE_ERRORS as usize);
                    for error in &errors {
                        prop_assert!(error.line >= 1 && error.column >= 1, "{:?}", error);
                        let (line, column) = (error.line, error.column);
                        let quoted = format!("Line {line}, column {column}: {}", error.message);
                        prop_assert_eq!(&error.text, &quoted);
                    }
                    let places: Vec<(u32, u32)> =
                        errors.iter().map(|error| (error.line, error.column)).collect();
                    let ordered = places.windows(2).all(|pair| pair[0] <= pair[1]);
                    prop_assert!(ordered, "{:?}", places);
                }
            }
        }
        let _ = session.read_address(&text);
        let _ = session.read_word(&text);
        let _ = session.draft_issue(AccessKind::Write, &text, &text);
        for field in number_fields() {
            let _ = session.read_config_number(field, &text, &config);
        }
        let _ = ShareDoc::from_share_string(&text);
        let _ = ShareDoc::from_json(&text);
        let _ = Session::from_share_string(&text);
        let _ = Session::from_json(&text);
        let _ = Module::parse(&text);
        let _ = ModuleRun::from_json(&text);
        let _ = parse_module_index(&text);
    }

    /// Every op reads back from its JSON as itself, whatever its kind and
    /// fields, through the flat form that checks them.
    #[test]
    fn ops_round_trip_through_json(
        op in arb_op(),
        target in arb_edit_target(),
        patch in arb_edit_patch(),
        way in arb_way_patch(),
    ) {
        round_trip(&op)?;
        round_trip(&target)?;
        round_trip(&patch)?;
        round_trip(&way)?;
    }

    /// Every configuration reads back from its JSON as itself, valid or
    /// not.
    #[test]
    fn configurations_round_trip_through_json(config in arb_raw_config()) {
        round_trip(&config)?;
        round_trip(&config.cache)?;
        round_trip(&config.timing)?;
        round_trip(&config.word_bytes)?;
        round_trip(&config.cache.replacement)?;
        round_trip(&config.cache.write_policy)?;
        round_trip(&config.cache.allocate_policy)?;
        round_trip(&config.memory_init)?;
    }

    /// Every predicate and module step reads back from its JSON as itself,
    /// through the flat forms that check them.
    #[test]
    fn predicates_and_steps_round_trip_through_json(
        predicate in arb_predicate(),
        step in arb_step(),
        event in arb_event_match(),
        access in arb_access_match(),
        config in arb_config_match(),
    ) {
        round_trip(&predicate)?;
        round_trip(&step)?;
        round_trip(&event)?;
        round_trip(&access)?;
        round_trip(&config)?;
    }

    /// Every share document, editor form and program source reads back
    /// from its JSON as itself.
    #[test]
    fn documents_and_forms_round_trip_through_json(
        doc in arb_share_doc(),
        form in arb_way_form(),
        source in any::<String>(),
    ) {
        round_trip(&doc)?;
        round_trip(&doc.at)?;
        round_trip(&form)?;
        for format in [ProgramFormat::Asm, ProgramFormat::Trace] {
            round_trip(&ProgramSource { source: source.clone(), format })?;
            round_trip(&format)?;
        }
    }

    /// Every number type and plain enum read from JSON reads back as
    /// itself.
    #[test]
    fn numbers_and_names_round_trip_through_json(
        small in any::<u32>(),
        large in any::<u64>(),
        pick in any::<usize>(),
    ) {
        round_trip(&number::<Addr>(small.into()))?;
        round_trip(&number::<LineAddr>(small.into()))?;
        round_trip(&number::<Tag>(small.into()))?;
        round_trip(&number::<SetIndex>(small.into()))?;
        round_trip(&number::<WayIndex>(small.into()))?;
        round_trip(&number::<EventPos>(small.into()))?;
        round_trip(&number::<Word>(large))?;
        round_trip(&WayLoc { set: number(small.into()), way: number(large >> 32) })?;
        round_trip(&registers()[pick % 8])?;
        round_trip(&event_kinds()[pick % 16])?;
        round_trip(&COUNTERS[pick % COUNTERS.len()])?;
        round_trip(&number_fields()[pick % 2])?;
        round_trip(&[Outcome::Hit, Outcome::Miss][pick % 2])?;
        round_trip(&[AccessKind::Read, AccessKind::Write][pick % 2])?;
    }
}
