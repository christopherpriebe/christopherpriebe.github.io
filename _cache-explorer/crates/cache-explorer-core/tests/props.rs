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
//! happen.
//!
//! The fold is a pure function, and undo, redo, scrubbing, seeking and
//! Space reach the machines it reaches; share strings and saved files give
//! the same session back; the cache's invariants and the counters'
//! identities hold at every event; every event lists exactly the cells it
//! changed; a refused call changes nothing, and an accepted one never
//! rewrites the log before the point it appends at; a planned edit applies
//! and drops no dirty data without saying so; every configuration repairs to
//! a valid one, whose geometry splits and rebuilds every address; the
//! parsers take any text; and every input type round-trips through JSON.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use std::collections::{BTreeMap, BTreeSet};

use cache_explorer_core::limits::MAX_PARSE_ERRORS;
use cache_explorer_core::{
    parse_module_index, AccessKind, Addr, AllocatePolicy, Boundary, ChangeRef, Config, EventKind,
    EventPos, LineAddr, MemoryWordView, MicroEvent, Module, ModuleRun, Op, OpIndex, Outcome,
    ParseResult, Predicate, ProgramFormat, ProgramSource, Session, SetIndex, ShareDoc, StepOutcome,
    Tag, WayIndex, WayLoc, WayPredicate, Word, WritePolicy,
};
use proptest::prelude::*;

use support::{
    arb_access_match, arb_config, arb_config_match, arb_edit_patch, arb_edit_target,
    arb_event_match, arb_op, arb_predicate, arb_raw_config, arb_session, arb_share_doc, arb_step,
    arb_text, arb_valid_config, arb_way_form, arb_way_patch, cells, changes, end, event_kinds,
    every_position, number, number_fields, op_ends, op_start, play, registers, round_trip,
    undo_stops, without_mode, Checks, COUNTERS,
};

/// How the properties over whole sessions run: with a quarter of the cases
/// proptest runs by default, or of `PROPTEST_CASES` when it is set, since
/// each case builds a session and walks every event of it. A failure is
/// printed with its smallest case rather than written to a file beside this
/// one; once the bug is fixed, that case becomes a test of its own.
fn sessions() -> ProptestConfig {
    let base = values();
    ProptestConfig {
        cases: (base.cases / 4).max(1),
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
        MAX_CYCLES, MAX_LINES, MAX_RUN_STEPS, MAX_SEED, MAX_WORDS_PER_LINE, MIN_ADDRESS_BITS,
        MIN_CYCLES,
    };
    use cache_explorer_core::{
        AccessKind, AccessMatch, Addr, AddrLoc, AllPredicate, AllocatePolicy, AnyPredicate,
        Boundary, CacheConfig, Callout, CalloutArea, CellRef, ChangeRef, CmpOp, Config,
        ConfigChange, ConfigMatch, ConfigNumberField, ConfigPredicate, Consequence, Counter,
        CounterPredicate, Counters, EditPatch, EditPlan, EditTarget, EventKind, EventMatch,
        EventPos, EventPredicate, Geometry, LastAccessPredicate, LinePredicate, Machine,
        MemoryInit, MemoryPredicate, MicroEvent, NotPredicate, Op, OpError, Outcome, Position,
        Predicate, ProgramFormat, ProgramSource, Register, RegisterPredicate, Replacement, Session,
        ShareAt, ShareDoc, State, Step, StepOutcome, Timeline, Timing, Tone, WayForm, WayLoc,
        WayPatch, WayPredicate, WayView, WordBytes, WritePolicy,
    };
    use proptest::collection::vec;
    use proptest::option;
    use proptest::prelude::*;
    use proptest::sample::select;
    use proptest::test_runner::TestCaseError;
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
    /// set; a trace; assembly that faults; and assembly that does not parse.
    /// Every instruction has a line of its own, from the first line on.
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
                    let n = 1 + self.count % 6;
                    let step = 1 + self.other % 5;
                    let addrs: Vec<u32> = (0..n)
                        .map(|i| {
                            let line = self.start.wrapping_add(i * step);
                            pool_addr(geometry, line, self.other.wrapping_add(i))
                        })
                        .collect();
                    let lines: Vec<String> = (0..n)
                        .zip(&addrs)
                        .map(|(i, addr)| {
                            if (self.other >> i) & 1 == 1 {
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
        /// and names the dirty data it drops; a suggestion plans.
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
                events: session.events_range(0, u32::MAX).to_vec(),
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
        let mut student = Student {
            session: Session::with_config(config.clone()).map_err(fail)?,
            addrs: BTreeSet::new(),
            checks,
            initial: config.clone(),
        };
        for action in actions {
            student.act(action)?;
        }
        Ok(student)
    }

    impl Student {
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
            Ok(())
        }

        /// Appends `op`, and says whether it was accepted. An accepted op
        /// returns its events, the last of the log, where the cursor rests.
        fn apply(&mut self, op: Op) -> Result<bool, TestCaseError> {
            let before = self.before();
            let result = self.session.apply(op);
            if let (Ok(events), true) = (&result, self.checks.append_only) {
                let all = self.session.events_range(0, u32::MAX);
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
                Change::Apply(config) => self
                    .apply(Op::Config {
                        config: config.clone(),
                    })
                    .map(drop),
                Change::Propose { config, reset } => {
                    let before = self.before();
                    match self.session.propose_config(config.clone()) {
                        Ok(ConfigChange::Applied) => self.after(before, Call::Appended),
                        Ok(ConfigChange::NeedsReset { config, .. }) => {
                            self.after(before, Call::Refused)?;
                            if *reset {
                                self.apply(Op::Config { config })?;
                            }
                            Ok(())
                        }
                        Err(_) => self.after(before, Call::Refused),
                    }
                }
                Change::Reset => {
                    let config = self.session.config().clone();
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
                    let accepted = self.apply(Op::Config {
                        config: config.clone(),
                    })?;
                    prop_assert!(!accepted, "an invalid configuration was appended");
                    let before = self.before();
                    let result = self.session.propose_config(config.clone());
                    prop_assert!(result.is_err(), "an invalid configuration was proposed");
                    self.after(before, Call::Refused)
                }
            }
        }

        /// An edit: planned, then applied if planned; otherwise its
        /// suggestion, planned and applied, if the refusal offers one.
        fn edit(&mut self, intent: EditIntent) -> Result<(), TestCaseError> {
            let (target, patch, addr) = intent.resolve(&self.session);
            self.addrs.extend(addr);
            let refusal = match self.session.plan_edit(&target, &patch) {
                Ok(plan) => return self.apply_plan(target, patch, &plan),
                Err(refusal) => refusal,
            };
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
            // At the end of an op, the rules that refused the plan are the
            // ones the session checks the op against, so the op is refused.
            if self.session.position().boundary >= Boundary::OpEnd {
                let before = self.before();
                let result = self.session.apply(Op::Edit { target, patch });
                if self.checks.edits {
                    prop_assert!(
                        matches!(result, Err(OpError::Edit(_))),
                        "an edit the plan refused as {:?} was applied as {:?}",
                        refusal.reason,
                        result
                    );
                }
                self.after(before, Call::of(&result))?;
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
            let result = self.session.apply(Op::Edit { target, patch });
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
        number(session.events_range(0, u32::MAX).len() as u64)
    }

    /// Where each op of the log ends: the position after its last event.
    pub(crate) fn op_ends(session: &Session) -> Vec<u32> {
        let mut ends = vec![0; session.ops().len()];
        for event in session.events_range(0, u32::MAX) {
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
            ..config.clone()
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
        let events = session.events_range(0, u32::MAX).to_vec();
        let at = std::iter::once(None).chain(events.iter().map(Some));
        for (pos, event) in (0_u32..).zip(at) {
            session.seek(number(pos.into()));
            prop_assert_eq!(session.position().pos.get(), pos);
            visit(session, event)?;
        }
        Ok(())
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

proptest! {
    #![proptest_config(sessions())]

    /// The fold is a pure function of the initial configuration and the
    /// log: two replays agree, with each other and with the session built
    /// one call at a time, at the end of its log.
    #[test]
    fn replay_is_a_pure_function_of_the_log((config, actions) in arb_session()) {
        let mut student = play(&config, &actions, Checks::default())?;
        let session = &mut student.session;
        let initial = session.initial().clone();
        let ops = session.ops().to_vec();
        let replay = Session::replay(&initial, &ops);
        prop_assert_eq!(&replay, &Session::replay(&initial, &ops));
        let replay = replay.unwrap();
        session.seek(end(session));
        prop_assert_eq!(session.machine(), &replay.machine);
        prop_assert_eq!(session.events_range(0, u32::MAX), replay.events.as_slice());
    }

    /// Scrubbing to a count of ops reaches the fold of that many ops; undo
    /// from the end of the log visits every undo stop back to position 0,
    /// and redo every one forward again; from any position, undo and redo
    /// go to the stops before and after it; and a seek reaches the fold
    /// stopped at its position.
    #[test]
    fn undo_redo_scrubbing_and_seeking_reach_the_fold(
        (config, actions) in arb_session(),
    ) {
        let mut student = play(&config, &actions, Checks::default())?;
        let session = &mut student.session;
        let initial = session.initial().clone();
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
                stops.iter().copied().filter(|&stop| stop <= start).max().unwrap_or(0)
            });
            prop_assert_eq!(session.undo(), back.is_some());
            prop_assert_eq!(session.position().pos.get(), back.unwrap_or(pos));
            session.seek(number(pos.into()));
            let on = stops.iter().copied().find(|&stop| stop > pos);
            prop_assert_eq!(session.redo(), on.is_some());
            prop_assert_eq!(session.position().pos.get(), on.unwrap_or(pos));
        }
    }

    /// Space from position 0 visits every position of the log in turn,
    /// each the fold's machine; a quiet replay, which words nothing, reaches
    /// the machine a recording one does at the end of every op; every
    /// event's id is its position less one; runs are numbered from 1 and
    /// start at a new configuration's setup; and each entry's events are
    /// numbered from 1, its label saying which entry of the run and which
    /// event of it.
    #[test]
    fn space_visits_every_event_in_order((config, actions) in arb_session()) {
        let mut student = play(&config, &actions, Checks::default())?;
        let session = &mut student.session;
        let initial = session.initial().clone();
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
        let events = session.events_range(0, u32::MAX).to_vec();
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
    }

    /// A share string and a saved file each give back the initial
    /// configuration, whose flag is the mode chosen, the whole log with its
    /// undone ops, the cursor, position 0 included, every event and the
    /// state, which may differ only in the displayed configuration's flag;
    /// the session's share string is its document's.
    #[test]
    fn share_strings_and_files_give_the_session_back(
        (config, actions) in arb_session(),
    ) {
        let mut student = play(&config, &actions, Checks::default())?;
        let session = &mut student.session;
        let end = end(session).get();
        // Where the student left the cursor, then position 0, then halfway.
        for pos in [None, Some(0), Some(end / 2)] {
            if let Some(pos) = pos {
                session.seek(number(pos.into()));
            }
            let doc = session.share_doc();
            prop_assert_eq!(session.to_share_string(), doc.to_share_string());
            let link = doc.to_share_string();
            prop_assert_eq!(ShareDoc::from_share_string(&link), Ok(doc.clone()));
            prop_assert_eq!(ShareDoc::from_json(&doc.to_json()), Ok(doc.clone()));
            for loaded in [
                Session::from_share_string(&session.to_share_string()),
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
                    loaded.events_range(0, u32::MAX),
                    session.events_range(0, u32::MAX)
                );
                let (mut ours, mut theirs) = (session.state(), loaded.state());
                prop_assert_eq!(without_mode(&ours.config), without_mode(&theirs.config));
                ours.config.advanced = false;
                theirs.config.advanced = false;
                prop_assert_eq!(theirs, ours);
            }
        }
    }

    /// At every event, the ranks of every set are a permutation of its
    /// ways; under random replacement, and in a set of one way, each way's
    /// rank is its number, edits included.
    #[test]
    fn ranks_stay_a_permutation((config, actions) in arb_session()) {
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
                            way: WayLoc { set: set.set, way: number(way.into()) },
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
        })?;
    }

    /// Under LRU and FIFO, every valid way ranks before every empty one at
    /// every entry boundary.
    #[test]
    fn valid_ways_rank_before_empty_ones_between_entries(
        (config, actions) in arb_session(),
    ) {
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
        })?;
    }

    /// At every event, a valid way's line decodes to the way's own set and
    /// tag, and no set holds a tag twice.
    #[test]
    fn every_line_sits_once_in_its_own_set((config, actions) in arb_session()) {
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
        })?;
    }

    /// Under write-through no way is ever dirty.
    #[test]
    fn write_through_leaves_no_way_dirty((config, actions) in arb_session()) {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, _| {
            let state = session.state();
            if state.config.cache.write_policy == WritePolicy::WriteThrough {
                for set in &state.cache.sets {
                    prop_assert!(set.ways.iter().all(|way| !way.dirty), "set {:?}", set.set);
                }
            }
            Ok(())
        })?;
    }

    /// Hits and misses add up to the accesses at every event.
    #[test]
    fn hits_and_misses_add_up_to_accesses((config, actions) in arb_session()) {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, _| {
            let counters = session.stats().counters;
            prop_assert_eq!(counters.hits + counters.misses, counters.accesses);
            prop_assert_eq!(counters.loads + counters.stores, counters.accesses);
            Ok(())
        })?;
    }

    /// Between entries, a clean valid way holds memory's words; at every
    /// event, a word is stale exactly when a dirty way holds it with another
    /// value, and the stale mark names that way.
    #[test]
    fn clean_lines_match_memory_and_stale_words_have_dirty_lines(
        (config, actions) in arb_session(),
    ) {
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
                        prop_assert!(!(clean && differs), "{:?}/{} at {:#X}", set.set, index, addr);
                        let stale = cell.stale.as_ref().map(|stale| (stale.set, stale.way.get()));
                        let holder = (way.dirty && differs).then_some((set.set, index));
                        prop_assert_eq!(stale, holder);
                    }
                }
            }
            for &addr in &addrs {
                let Some(addr) = geometry.addr(addr) else { continue };
                if let Some(stale) = session.machine().stale(addr) {
                    let set = &state.cache.sets[stale.set.get() as usize];
                    let way = &set.ways[stale.way.get() as usize];
                    prop_assert!(way.valid && way.dirty, "{:?} is stale in {:?}", addr, way);
                    prop_assert_ne!(stale.cached, stale.memory);
                }
            }
            Ok(())
        })?;
    }

    /// In timed mode the cycles are the hit time for every access and the
    /// miss penalty for every miss and every write-back, at every entry
    /// boundary; untimed, there are none, and no event has a cycle stamp.
    #[test]
    fn cycles_follow_the_counters((config, actions) in arb_session()) {
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
        })?;
    }

    /// Each event lists exactly the cells and counters it changed, sorted
    /// and each once: over everything a change can name, the difference
    /// between the states on either side of it, with the tag and every word
    /// of a way that took another line. A new configuration's setup starts
    /// a run rather than changing one, and is left out.
    #[test]
    fn each_event_lists_exactly_what_it_changed((config, actions) in arb_session()) {
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
        })?;
    }

    /// A refused call, whether an op, a configuration change, a mode switch
    /// that asks first, Space or Shift+Space with nothing to do, or an undo
    /// or redo with nowhere to go, and with or without undone ops after the
    /// cursor, leaves the log, the undone ops, the cursor and the state as
    /// they were.
    #[test]
    fn a_refused_call_changes_nothing((config, actions) in arb_session()) {
        let checks = Checks { refusals: true, ..Checks::default() };
        play(&config, &actions, checks)?;
    }

    /// A plan applies, keeps the invariants, and planned again is itself; a
    /// refusal's suggestion always plans; and a plan that takes a dirty
    /// line from its way while some of its words are newer than memory's
    /// says so, naming them, and only then.
    #[test]
    fn planned_edits_apply_and_drop_no_dirty_data_silently(
        (config, actions) in arb_session(),
    ) {
        let checks = Checks { edits: true, ..Checks::default() };
        play(&config, &actions, checks)?;
    }

    /// The compulsory, capacity and conflict misses add up to the misses at
    /// every event.
    #[test]
    fn miss_classes_add_up_to_misses((config, actions) in arb_session()) {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, _| {
            let counters = session.stats().counters;
            let classes = counters.compulsory + counters.capacity + counters.conflict;
            prop_assert_eq!(classes, counters.misses);
            Ok(())
        })?;
    }

    /// The machine's invariants hold after every event, and no event is an
    /// internal error.
    #[test]
    fn the_invariants_hold_at_every_event((config, actions) in arb_session()) {
        let mut student = play(&config, &actions, Checks::default())?;
        every_position(&mut student.session, |session, event| {
            prop_assert_eq!(session.machine().check_invariants(), Ok(()));
            if let Some(event) = event {
                let internal = event.explain.starts_with("Internal error");
                prop_assert!(event.kind != EventKind::Error || !internal, "{}", event.explain);
            }
            Ok(())
        })?;
    }

    /// A write-back run counts no write-throughs, and a write-allocate run
    /// no write-arounds; the words written to memory are the write-throughs
    /// and write-arounds, the lines read the fills and the lines written
    /// the write-backs, and the bytes follow.
    #[test]
    fn memory_writes_follow_the_policies((config, actions) in arb_session()) {
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
        })?;
    }

    /// The log is append-only: every call keeps every op before the point
    /// it appends at, a call that appends nothing keeps them all, and the
    /// initial configuration never changes.
    #[test]
    fn the_log_is_append_only((config, actions) in arb_session()) {
        let checks = Checks { append_only: true, ..Checks::default() };
        play(&config, &actions, checks)?;
    }
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
        let session = Session::with_config(config.clone()).unwrap();
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
        round_trip(&number::<OpIndex>(small.into()))?;
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
