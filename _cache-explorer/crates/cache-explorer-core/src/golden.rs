//! The golden fixtures, and the canonical text of their events that the
//! snapshot tests hold.
//!
//! Only built with the `golden` feature, which the core's own tests and the
//! facade's tests turn on: the core compares the text with its committed
//! snapshots natively, and the facade compares the same text produced in
//! WebAssembly, so a change of wording or behaviour is always a reviewed
//! snapshot diff and the two builds are known to agree.
//!
//! Each fixture is a configuration and what a student could do on it, in
//! order: loads and stores issued by hand, program and trace loads, and
//! single steps of the loaded program. Together they cover the ten access
//! cases, the design's flows A and B, the dirty eviction of frame 3b, untimed
//! and timed and as a program, every kind of stall, the widths that change
//! how numbers are written, and programs: loops, branches, `r0`, a store that
//! truncates a register, a fault, the end of the instruction budget, a trace,
//! and a load issued by hand beside a program.

use serde::Serialize;
use thiserror::Error;

use crate::config::{
    AllocatePolicy, CacheConfig, Config, ConfigError, Preset, Replacement, Timing, WordBytes,
    WritePolicy,
};
use crate::event::{ChangeRef, MicroEvent};
use crate::geometry::AccessError;
use crate::limits::INSTRUCTION_BUDGET;
use crate::op::{self, AccessKind, Op, OpError, ProgramFormat};
use crate::session::Session;
use crate::units::{Addr, Word};

/// A run the snapshots pin: a configuration and what is done on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    /// The fixture's name, which is also its snapshot's name.
    pub name: &'static str,
    /// What the fixture shows, for whoever reviews its snapshot.
    pub description: &'static str,
    /// The run's configuration.
    pub config: Config,
    /// What is done, one op after another from the run's start.
    pub issues: Vec<Issue>,
}

/// One op a student could apply by hand: a load or store issued by hand, a
/// program or trace loaded, or one instruction of the loaded program
/// executed, as Space executes it at the end of the log. A hand-issued load
/// writes `r1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Issue {
    /// A load of the word at `addr`.
    Load {
        /// The word's address.
        addr: Addr,
    },
    /// A store of `value` to the word at `addr`.
    Store {
        /// The word's address.
        addr: Addr,
        /// The value. It must fit the configuration's word, as an issued
        /// store's value must, or the fixture is refused.
        value: Word,
    },
    /// A program or trace loaded in place of the current one. It must parse
    /// under the configuration, or the fixture is refused.
    Program {
        /// Its source.
        source: String,
        /// Whether the source is assembly or a trace.
        format: ProgramFormat,
    },
    /// The loaded program's next instruction executed. There must be one,
    /// or the fixture is refused.
    Step,
}

/// Why a fixture's events cannot be produced.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum FixtureError {
    /// The fixture's configuration is invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// One of its accesses has an address out of range or not aligned to a
    /// word.
    #[error(transparent)]
    Access(#[from] AccessError),
    /// One of its stores has a value wider than the word. The engine would
    /// keep only the value's low bits and say nothing, so the fixture is
    /// refused, as an issued store would be:
    /// `300 does not fit a 1-byte word (0–255).`
    #[error("{}", op::value_too_wide(*.value, *.word_bytes))]
    Value {
        /// The store's value.
        value: Word,
        /// The word size, in bytes.
        word_bytes: u32,
    },
    /// One of its programs does not parse, or one of its steps has no
    /// instruction to execute: no program is loaded, or it has finished or
    /// stopped.
    #[error(transparent)]
    Op(OpError),
}

impl From<OpError> for FixtureError {
    /// The fixture's error for an op the session refused: an access's bad
    /// address or a store's value too wide as their own variants, and every
    /// other refusal as it is.
    fn from(error: OpError) -> Self {
        match error {
            OpError::Access(error) => FixtureError::Access(error),
            OpError::ValueTooWide { value, word_bytes } => {
                FixtureError::Value { value, word_bytes }
            }
            error => FixtureError::Op(error),
        }
    }
}

impl Fixture {
    /// The fixture's events, as a session records them when each issue is
    /// applied as an op of its own at the end of the log: ids from 0, one op
    /// index per issue, and within run 1 one entry per access and per
    /// instruction, a program load being an unnumbered setup.
    ///
    /// An `Error` event, a fault or the end of the budget, ends its op; the
    /// fixture goes on with its next issue, as a session would.
    ///
    /// # Errors
    ///
    /// [`FixtureError::Config`] if the configuration is invalid. Otherwise,
    /// for the first issue the session refuses: [`FixtureError::Access`] if
    /// its address does not fit the address width or is not a multiple of
    /// the word size, [`FixtureError::Value`] if it is a store whose value
    /// does not fit the word, or [`FixtureError::Op`] if it is a program that
    /// does not parse, or a step with no instruction to execute.
    pub fn events(&self) -> Result<Vec<MicroEvent>, FixtureError> {
        let mut session = Session::with_config(self.config.clone())?;
        for issue in &self.issues {
            session.apply(issue.op())?;
        }
        Ok(session.events().to_vec())
    }
}

impl Issue {
    /// The op that applies the issue.
    fn op(&self) -> Op {
        match self {
            Issue::Load { addr } => Op::Issue {
                kind: AccessKind::Read,
                addr: *addr,
                value: None,
            },
            Issue::Store { addr, value } => Op::Issue {
                kind: AccessKind::Write,
                addr: *addr,
                value: Some(*value),
            },
            Issue::Program { source, format } => Op::Program {
                source: source.clone(),
                format: *format,
            },
            Issue::Step => Op::Step,
        }
    }
}

/// The canonical text of `events`, which the snapshots hold: one line per
/// event with its label, kind, cause, changed cells and counters, cycle and
/// explain line, in that order and separated by ` | `, and `-` for a field
/// that is absent or empty.
///
/// Kinds, causes and counters are written as they serialize (`writeBack`,
/// `writePolicy`, `writebacks`). A cell is its kind and place: `valid 2/0`
/// for set 2, way 0; `word 2/0/1` for its word 1; `memory 0x1A4`; `r1`; `pc`.
pub fn render(events: &[MicroEvent]) -> String {
    let lines: Vec<String> = events
        .iter()
        .map(|event| {
            let cause = event.cause.map_or_else(absent, |cause| name(&cause));
            let changed = if event.changed.is_empty() {
                absent()
            } else {
                let cells: Vec<String> = event.changed.iter().map(cell).collect();
                cells.join(", ")
            };
            let cycle = event.cycle.map_or_else(absent, |cycle| cycle.to_string());
            format!(
                "{} | {} | {cause} | {changed} | {cycle} | {}\n",
                text(&event.label),
                name(&event.kind),
                text(&event.explain)
            )
        })
        .collect();
    lines.concat()
}

/// The text of an absent field.
fn absent() -> String {
    "-".to_owned()
}

/// A text field as written, or `-` when it is empty.
fn text(field: &str) -> &str {
    if field.is_empty() {
        "-"
    } else {
        field
    }
}

/// The name `value` serializes to, so the text uses the names that cross
/// the boundary.
fn name(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        _ => String::new(),
    }
}

/// A changed cell or counter as the canonical text writes it.
fn cell(change: &ChangeRef) -> String {
    match *change {
        ChangeRef::Valid { set, way } => format!("valid {set}/{way}"),
        ChangeRef::Tag { set, way } => format!("tag {set}/{way}"),
        ChangeRef::Dirty { set, way } => format!("dirty {set}/{way}"),
        ChangeRef::Rank { set, way } => format!("rank {set}/{way}"),
        ChangeRef::Word { set, way, word } => format!("word {set}/{way}/{word}"),
        ChangeRef::Memory { addr } => format!("memory {addr}"),
        ChangeRef::Register { register } => register.to_string(),
        ChangeRef::Pc => "pc".to_owned(),
        ChangeRef::Counter { counter } => name(&counter),
    }
}

/// Every golden fixture, in a fixed order.
pub fn fixtures() -> Vec<Fixture> {
    let mut fixtures = flows();
    fixtures.extend(stores());
    fixtures.extend(widths());
    fixtures.extend(timed());
    fixtures.extend(programs());
    fixtures
}

/// The design's flows, frame 3b's accesses and a direct-mapped conflict:
/// loads under each replacement policy, and a dirty eviction.
fn flows() -> Vec<Fixture> {
    let flow_b = [
        load(0x000),
        load(0x040),
        load(0x000),
        load(0x080),
        load(0x0C0),
    ];
    vec![
        Fixture {
            name: "flow-a",
            description: "The design's flow A on direct-mapped-8: a load misses in an empty \
                          one-way set and brings in the whole line, so a load of its neighbour \
                          hits (cases 2 and 1).",
            config: Preset::DirectMapped8.config(),
            issues: vec![load(0x0A4), load(0x0A8)],
        },
        Fixture {
            name: "flow-b-lru",
            description: "The design's flow B on two-way-lru: 0x000 and 0x040 fill set 0, a hit \
                          makes way 0 the most recent, then 0x080 evicts way 1 (victim row \
                          4.3) and 0x0C0 evicts way 0 (cases 2, 1 and 3).",
            config: Preset::TwoWayLru.config(),
            issues: flow_b.to_vec(),
        },
        Fixture {
            name: "flow-b-fifo",
            description: "Flow B under FIFO: the hit moves no rank, so 0x080 evicts way 0, \
                          filled earliest, and 0x0C0 evicts way 1.",
            config: two_way(Replacement::Fifo, 12_345),
            issues: flow_b.to_vec(),
        },
        Fixture {
            name: "flow-b-random-seed-1",
            description: "Flow B under random replacement with seed 1: the victims come from \
                          the seeded generator, and no rank ever moves.",
            config: two_way(Replacement::Random, 1),
            issues: flow_b.to_vec(),
        },
        Fixture {
            name: "frame-3b-accesses",
            description: "Frame 3b's accesses issued by hand on two-way-lru, untimed, in the \
                          order of its program: a store dirties 0x0B0 in set 3, a load fills \
                          way 0 of set 2 and a store dirties 0x1A0 in way 1, a hit makes way 0 \
                          the most recent, and the load of 0x2A4 writes back and evicts the \
                          dirty 0x1A0 from way 1 (cases 7, 2, 7, 1 and 4). Every load goes to \
                          r1, where the program loads r1, r2 and r3.",
            config: Preset::TwoWayLru.config(),
            issues: vec![
                store(0x0B0, 5),
                load(0x0A4),
                store(0x1A4, 7),
                load(0x0A8),
                load(0x2A4),
            ],
        },
        Fixture {
            name: "direct-mapped-conflict",
            description: "Direct-mapped-8: 0x000 and 0x080 share set 0, its only way, so each \
                          load evicts the other (cases 2 and 3). The first two misses are \
                          compulsory; the return of 0x000 is a conflict, since a fully \
                          associative cache of eight lines would still hold it.",
            config: Preset::DirectMapped8.config(),
            issues: vec![load(0x000), load(0x080), load(0x000)],
        },
    ]
}

/// Stores under each write policy and each allocate policy.
fn stores() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "write-back-stores",
            description: "Store hits under write-back on direct-mapped-8: a clean line becomes \
                          dirty and memory stale; a dirty line is written again, once with a \
                          value memory already holds; then a store miss writes the dirty line \
                          back before it evicts it (cases 2, 5 and 7).",
            config: Preset::DirectMapped8.config(),
            issues: vec![
                load(0x0A4),
                store(0x0A4, 7),
                store(0x0A8, 9),
                store(0x0AC, 79),
                load(0x0B0),
                store(0x0B0, 92),
                store(0x1A4, 3),
            ],
        },
        Fixture {
            name: "write-through-stores",
            description: "Write-through on direct-mapped-8: a store miss fills then writes the \
                          cache and memory; store hits write both, the second with the value \
                          memory already holds; a store miss evicts a clean line (cases 8 and \
                          6).",
            config: direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate),
            issues: vec![
                store(0x0A4, 7),
                store(0x0A8, 9),
                store(0x0A8, 9),
                store(0x1A4, 3),
            ],
        },
        Fixture {
            name: "write-through-two-way",
            description: "Write-through on two ways with LRU: after a store miss and a store \
                          hit, the rank move comes last, after the write-through (cases 2, 8 \
                          and 6).",
            config: Config {
                cache: CacheConfig {
                    write_policy: WritePolicy::WriteThrough,
                    ..Preset::TwoWayLru.config().cache
                },
                ..Preset::TwoWayLru.config()
            },
            issues: vec![load(0x0A4), store(0x1A4, 3), store(0x0A4, 5)],
        },
        Fixture {
            name: "write-no-allocate-write-back",
            description: "Write-no-allocate under write-back: a store miss goes around the \
                          cache and counts as a write-around; the second writes the value \
                          memory already holds (case 9).",
            config: direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteNoAllocate),
            issues: vec![store(0x0A4, 7), store(0x0A8, 66)],
        },
        Fixture {
            name: "write-no-allocate-write-through",
            description: "Write-no-allocate under write-through: store misses go around the \
                          cache and count as write-arounds, not write-throughs (case 10).",
            config: direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteNoAllocate),
            issues: vec![store(0x0A4, 7), store(0x1A4, 3)],
        },
        Fixture {
            name: "no-allocate-store-then-load",
            description: "A store miss under write-no-allocate leaves the cache empty, so a load \
                          of the same line misses too and reads the stored value from memory \
                          (cases 9 and 2). Both misses are compulsory: the store filled nothing.",
            config: direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteNoAllocate),
            issues: vec![store(0x0A4, 7), load(0x0A4)],
        },
    ]
}

/// The shapes that change how numbers are written: a one-line cache, every
/// address width and every word size.
fn widths() -> Vec<Fixture> {
    let mut fixtures = vec![Fixture {
        name: "one-line-cache",
        description: "A 16 B cache of one 16 B line: fully associative with one way, so the \
                          decode names no set and the victim line does not say direct-mapped \
                          (cases 2, 1 and 3).",
        config: Config {
            cache: CacheConfig {
                size_bytes: 16,
                ..Preset::DirectMapped8.config().cache
            },
            ..Preset::DirectMapped8.config()
        },
        issues: vec![load(0x0A4), load(0x0A8), load(0x1A4)],
    }];
    for (name, description, address_bits) in [
        (
            "address-bits-8",
            "The same accesses with 8-bit addresses: two hex digits and a one-bit tag.",
            8,
        ),
        (
            "address-bits-12",
            "The same accesses with 12-bit addresses: three hex digits and a five-bit tag.",
            12,
        ),
        (
            "address-bits-16",
            "The same accesses with 16-bit addresses: four hex digits and a nine-bit tag, \
             written with three digits.",
            16,
        ),
        (
            "address-bits-32",
            "The same accesses with 32-bit addresses: eight hex digits and a 25-bit tag, \
             written with seven digits.",
            32,
        ),
    ] {
        fixtures.push(Fixture {
            name,
            description,
            config: Config {
                address_bits,
                ..Preset::DirectMapped8.config()
            },
            issues: vec![load(0x0A8), load(0x028), store(0x0A8, 7)],
        });
    }
    for (name, description, word_bytes, largest) in [
        (
            "word-bytes-1",
            "1-byte words: sixteen to a line; the largest word, 255, stored and loaded.",
            WordBytes::One,
            u64::from(u8::MAX),
        ),
        (
            "word-bytes-2",
            "2-byte words: eight to a line; the largest word, 65,535, stored and loaded.",
            WordBytes::Two,
            u64::from(u16::MAX),
        ),
        (
            "word-bytes-4",
            "4-byte words: four to a line; the largest word, 4,294,967,295, stored and loaded.",
            WordBytes::Four,
            u64::from(u32::MAX),
        ),
        (
            "word-bytes-8",
            "8-byte words: two to a line; the largest word, 2^64 \u{2212} 1, stored and loaded.",
            WordBytes::Eight,
            u64::MAX,
        ),
    ] {
        fixtures.push(Fixture {
            name,
            description,
            config: Config {
                word_bytes,
                ..Preset::DirectMapped8.config()
            },
            issues: vec![store(0x0A8, largest), load(0x0A8)],
        });
    }
    fixtures
}

/// Timed mode: the cycle stamps of each case, and every kind of stall.
fn timed() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "frame-3b-accesses-timed",
            description: "Frame 3b's accesses as in frame-3b-accesses, timed 1/20: each miss \
                          stalls once its victim is chosen, and the dirty eviction stalls for 40 \
                          cycles, 20 for the write-back and 20 for the fill. Cycles are charged \
                          where the time is spent, so the write-back row reads 85 and the fill \
                          105, where the design charges the whole miss at the Miss row (cases 7, \
                          2, 7, 1 and 4).",
            config: timed_config(Preset::TwoWayLru.config(), 1, 20),
            issues: vec![
                store(0x0B0, 5),
                load(0x0A4),
                store(0x1A4, 7),
                load(0x0A8),
                load(0x2A4),
            ],
        },
        Fixture {
            name: "timed-write-through",
            description: "Write-through on direct-mapped-8, timed 1/20: a store miss stalls for \
                          the fill, a store hit costs the hit time alone although it writes \
                          memory too, and a load that evicts a clean line stalls for the fill \
                          only (cases 8, 6 and 3).",
            config: timed_config(
                direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate),
                1,
                20,
            ),
            issues: vec![store(0x0A4, 7), store(0x0A8, 9), load(0x1A4)],
        },
        Fixture {
            name: "timed-write-no-allocate",
            description: "Write-no-allocate under write-back, timed 1/20: a store miss stalls \
                          right after the compare while the store goes to memory, the load of \
                          its line then misses and stalls for the fill, and a store hit costs \
                          the hit time (cases 9, 2 and 5).",
            config: timed_config(
                direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteNoAllocate),
                1,
                20,
            ),
            issues: vec![store(0x0A4, 7), load(0x0A4), store(0x0A8, 9)],
        },
        Fixture {
            name: "timed-one-cycle-misses",
            description: "Direct-mapped-8 timed with 3-cycle hits and 1-cycle misses: the stall \
                          of a single cycle is singular, a dirty eviction stalls for 2, and a \
                          hit costs more than a transfer (cases 7, 4 and 1).",
            config: timed_config(Preset::DirectMapped8.config(), 3, 1),
            issues: vec![store(0x0A4, 7), load(0x1A4), load(0x1A8)],
        },
    ]
}

/// Frame 3b's program, as the design writes it.
const FRAME_3B: &str = concat!(
    "  st  [0x0B0], #5\n",
    "  ld  r1, [0x0A4]\n",
    "  st  [0x1A4], #7\n",
    "  ld  r2, [0x0A8]\n",
    "  ld  r3, [0x2A4]\n",
    "  ld  r4, [0x1A4]",
);

/// The trace the design's drawer shows when switched to Trace.
const SAMPLE_TRACE: &str = "R 0x0A4\nR 0x0A8\nW 0x0A4 7\nR 0x1A4\nR 0x0A4";

/// A loop that counts `r1` down from 3, storing each value, then writes to
/// `r0`, which drops what it is given.
const COUNT_DOWN: &str = concat!(
    "; Count r1 down from 3, storing each value; then write r0.\n",
    "        add r1, r0, #3\n",
    "loop:   st [0x0A4], r1\n",
    "        add r1, r1, #-1\n",
    "        bnez r1, loop\n",
    "        add r0, r1, #5\n",
    "        ld r0, [0x0A4]",
);

/// A loop that walks memory a line at a time until the address it holds in
/// `r2` leaves the 12-bit address space.
const WALK_OFF_THE_END: &str = concat!(
    "; Walk memory a line at a time until r2 leaves it.\n",
    "        add r2, r0, #0xFE0\n",
    "loop:   ld r1, [r2]\n",
    "        add r2, r2, #16\n",
    "        bnez r2, loop",
);

/// A loop without an exit: `r1` holds 1 and nothing changes it, so the
/// branch is always taken.
const ENDLESS_LOOP: &str = concat!(
    "; r1 is never 0, so the loop has no exit.\n",
    "        add r1, r0, #1\n",
    "loop:   bnez r1, loop",
);

/// Stores of a register that holds more than a 1-byte word: once around
/// the cache, once into it.
const TRUNCATION: &str = concat!(
    "        add r2, r0, #300\n",
    "        st [0x0A4], r2   ; misses: around the cache\n",
    "        ld r3, [0x0A4]\n",
    "        st [0x0A4], r2   ; hits",
);

/// Programs and traces: frame 3b's program, untimed and timed; the design's
/// sample trace; a loop that finishes, one that faults and one that uses its
/// budget; a store that truncates a register; and loads issued by hand beside
/// a program.
fn programs() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "frame-3b-program",
            description: "Frame 3b's program on two-way-lru, untimed, loaded and stepped to its \
                          end one instruction per op, as Space steps it. The load is a setup \
                          without a number or a label; each instruction's last event moves the \
                          pc, after the rank update when there is one; and access #5 reads as \
                          the design's table, into r3 (cases 7, 2, 7, 1, 4 and 3).",
            config: Preset::TwoWayLru.config(),
            issues: [vec![program(FRAME_3B)], steps(6)].concat(),
        },
        Fixture {
            name: "frame-3b-program-timed",
            description: "Frame 3b's program as in frame-3b-program, timed 1/20: the dirty \
                          eviction of access #5 stalls for 40 cycles, and a program load, like \
                          every event, is stamped with the run's cycle count.",
            config: timed_config(Preset::TwoWayLru.config(), 1, 20),
            issues: [vec![program(FRAME_3B)], steps(6)].concat(),
        },
        Fixture {
            name: "sample-trace",
            description: "The design's sample trace on direct-mapped-8: each R loads into r1 and \
                          each W stores its value, and the log counts accesses, not \
                          instructions (cases 2, 1, 5, 4 and 3).",
            config: Preset::DirectMapped8.config(),
            issues: [vec![trace(SAMPLE_TRACE)], steps(5)].concat(),
        },
        Fixture {
            name: "count-down-loop",
            description: "A bnez loop on direct-mapped-8 that counts r1 down from 3 with a \
                          negative immediate, storing each value from the register; the branch \
                          is taken twice, then falls through. An add and a load into r0 are \
                          dropped, and the last instruction finishes the program.",
            config: Preset::DirectMapped8.config(),
            issues: [vec![program(COUNT_DOWN)], steps(12)].concat(),
        },
        Fixture {
            name: "register-address-fault",
            description: "A bnez loop on direct-mapped-8 that loads through r2, a line at a \
                          time, until r2 holds 4096, which does not fit a 12-bit address: the \
                          load faults with an error entry of its own, and the pc stays at it.",
            config: Preset::DirectMapped8.config(),
            issues: [vec![program(WALK_OFF_THE_END)], steps(8)].concat(),
        },
        Fixture {
            name: "budget-exhaustion",
            description: "A bnez loop on direct-mapped-8 with no exit, stepped once past its \
                          budget: every instruction the budget allows executes, and the next \
                          is replaced by an error entry of its own, with the pc left at the \
                          branch. The snapshot holds one line per instruction, so it grows \
                          and shrinks with INSTRUCTION_BUDGET.",
            config: Preset::DirectMapped8.config(),
            issues: [
                vec![program(ENDLESS_LOOP)],
                steps(
                    usize::try_from(INSTRUCTION_BUDGET)
                        .unwrap_or(usize::MAX)
                        .saturating_add(1),
                ),
            ]
            .concat(),
        },
        Fixture {
            name: "store-truncation",
            description: "1-byte words under write-no-allocate: r2 holds 300, so a store of it \
                          keeps its low 8 bits, 44, and its result line says so, whether the \
                          store goes around the cache or hits it (cases 9, 2 and 5).",
            config: Config {
                word_bytes: WordBytes::One,
                ..direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteNoAllocate)
            },
            issues: [vec![program(TRUNCATION)], steps(4)].concat(),
        },
        Fixture {
            name: "issue-beside-program",
            description: "A load issued by hand writes r1 even while a program is loaded, and \
                          its line says the program's r1 changed; once the program has \
                          finished, it does not. A hand-issued access moves neither the pc nor \
                          the budget.",
            config: Preset::DirectMapped8.config(),
            issues: vec![
                program("ld r1, [0x0A4]\nld r2, [0x0A8]"),
                Issue::Step,
                load(0x1A4),
                store(0x0B0, 9),
                Issue::Step,
                load(0x0A4),
            ],
        },
    ]
}

/// A load of the word at `addr`.
fn load(addr: u32) -> Issue {
    Issue::Load {
        addr: Addr::new_unchecked(addr),
    }
}

/// A store of `value` to the word at `addr`.
fn store(addr: u32, value: u64) -> Issue {
    Issue::Store {
        addr: Addr::new_unchecked(addr),
        value: Word::new_unchecked(value),
    }
}

/// A load of the assembly program `source`.
fn program(source: &str) -> Issue {
    Issue::Program {
        source: source.to_owned(),
        format: ProgramFormat::Asm,
    }
}

/// A load of the trace `source`.
fn trace(source: &str) -> Issue {
    Issue::Program {
        source: source.to_owned(),
        format: ProgramFormat::Trace,
    }
}

/// `count` steps, one op each.
fn steps(count: usize) -> Vec<Issue> {
    vec![Issue::Step; count]
}

/// `direct-mapped-8` under the given write and allocate policies.
fn direct_mapped(write_policy: WritePolicy, allocate_policy: AllocatePolicy) -> Config {
    let config = Preset::DirectMapped8.config();
    Config {
        cache: CacheConfig {
            write_policy,
            allocate_policy,
            ..config.cache
        },
        ..config
    }
}

/// `config` in timed mode, with `hit`-cycle hits and `miss`-cycle misses.
fn timed_config(config: Config, hit: u32, miss: u32) -> Config {
    Config {
        timing: Some(Timing {
            hit_cycles: hit,
            miss_cycles: miss,
        }),
        ..config
    }
}

/// `two-way-lru` under another replacement policy and seed.
fn two_way(replacement: Replacement, seed: u64) -> Config {
    let config = Preset::TwoWayLru.config();
    Config {
        cache: CacheConfig {
            replacement,
            ..config.cache
        },
        seed,
        ..config
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::event::{Cause, Counter, EventKind};
    use crate::units::{Cycle, OpIndex, Register, SetIndex, WayIndex, WordIndex};

    fn event(label: &str, kind: EventKind) -> MicroEvent {
        MicroEvent {
            id: 0,
            op_index: OpIndex::new_unchecked(0),
            run: 1,
            entry: Some(1),
            seq: 1,
            label: label.to_owned(),
            cycle: None,
            kind,
            addr: None,
            set: None,
            way: None,
            changed: Vec::new(),
            explain: String::new(),
            cause: None,
        }
    }

    #[test]
    fn render_writes_one_line_per_event_with_every_field() {
        let set = SetIndex::new_unchecked(2);
        let way = WayIndex::new_unchecked(1);
        let events = [
            MicroEvent {
                cycle: Some(Cycle::new_unchecked(121)),
                changed: vec![
                    ChangeRef::Memory {
                        addr: Addr::new_unchecked(0x1A4),
                    },
                    ChangeRef::Counter {
                        counter: Counter::Writebacks,
                    },
                ],
                explain: "Way 1 is dirty: write line 0x1A0 back to memory first.".to_owned(),
                cause: Some(Cause::WritePolicy),
                ..event("5.4", EventKind::WriteBack)
            },
            MicroEvent {
                changed: vec![
                    ChangeRef::Valid { set, way },
                    ChangeRef::Tag { set, way },
                    ChangeRef::Dirty { set, way },
                    ChangeRef::Rank { set, way },
                    ChangeRef::Word {
                        set,
                        way,
                        word: WordIndex::new_unchecked(3),
                    },
                    ChangeRef::Register {
                        register: Register::R3,
                    },
                    ChangeRef::Pc,
                ],
                explain: "Read word 1 of way 1 (68) into r3.".to_owned(),
                ..event("5.7", EventKind::Complete)
            },
            event("", EventKind::Setup),
        ];
        assert_eq!(
            render(&events),
            "5.4 | writeBack | writePolicy | memory 0x1A4, writebacks | 121 | Way 1 is dirty: \
             write line 0x1A0 back to memory first.\n\
             5.7 | complete | - | valid 2/1, tag 2/1, dirty 2/1, rank 2/1, word 2/1/3, r3, pc | \
             - | Read word 1 of way 1 (68) into r3.\n\
             - | setup | - | - | - | -\n"
        );
        assert_eq!(render(&[]), "");
    }

    #[test]
    fn fixture_names_are_unique_and_every_fixture_runs() {
        let fixtures = fixtures();
        for (index, fixture) in fixtures.iter().enumerate() {
            assert!(
                fixtures
                    .iter()
                    .skip(index + 1)
                    .all(|other| other.name != fixture.name),
                "{} is named twice",
                fixture.name
            );
            let events = fixture.events().unwrap();
            assert!(!events.is_empty(), "{}", fixture.name);
            // Only a program's fault or budget ends an op early, and then as
            // the last event of the fixture's last op.
            for (at, event) in events.iter().enumerate() {
                if event.kind == EventKind::Error {
                    assert!(
                        event.explain.starts_with("Line ")
                            || event.explain == crate::explain::out_of_budget(),
                        "{}",
                        event.explain
                    );
                    assert_eq!(at, events.len() - 1, "{}", fixture.name);
                }
            }
        }
    }

    #[test]
    fn a_fixture_applies_program_loads_and_steps_as_ops() {
        let fixture = |issues| Fixture {
            name: "programs",
            description: "",
            config: Preset::DirectMapped8.config(),
            issues,
        };
        let events = fixture(vec![program("ld r1, [0x0A4]"), Issue::Step])
            .events()
            .unwrap();
        let numbers: Vec<(u64, u32, Option<u32>, u32, &str)> = events
            .iter()
            .map(|event| {
                (
                    event.id,
                    event.op_index.get(),
                    event.entry,
                    event.seq,
                    event.label.as_str(),
                )
            })
            .collect();
        assert_eq!(
            numbers,
            [
                (0, 0, None, 1, ""),
                (1, 1, Some(1), 1, "1.1"),
                (2, 1, Some(1), 2, "1.2"),
                (3, 1, Some(1), 3, "1.3"),
                (4, 1, Some(1), 4, "1.4"),
                (5, 1, Some(1), 5, "1.5"),
            ]
        );
        assert_eq!(
            fixture(vec![Issue::Step]).events(),
            Err(FixtureError::Op(OpError::NoProgram))
        );
        assert_eq!(
            fixture(vec![program("ld r1, [0x0A4]"), Issue::Step, Issue::Step]).events(),
            Err(FixtureError::Op(OpError::Finished))
        );
        let refused = fixture(vec![program("lw r1, [0x0A4]")]).events();
        assert!(
            matches!(
                &refused,
                Err(FixtureError::Op(OpError::Program { errors: 1, .. }))
            ),
            "{refused:?}"
        );
        assert_eq!(
            refused.map_err(|error| error.to_string()),
            Err(
                "The program has an error on line 1: unknown instruction `lw`; use ld, st, add \
                 or bnez."
                    .to_owned()
            )
        );
    }

    #[test]
    fn events_are_numbered_one_op_and_one_entry_per_access() {
        let fixture = fixtures()
            .into_iter()
            .find(|fixture| fixture.name == "flow-a")
            .unwrap();
        let events = fixture.events().unwrap();
        let numbers: Vec<(u64, u32, Option<u32>, u32, &str)> = events
            .iter()
            .map(|event| {
                (
                    event.id,
                    event.op_index.get(),
                    event.entry,
                    event.run,
                    event.label.as_str(),
                )
            })
            .collect();
        assert_eq!(
            numbers,
            [
                (0, 0, Some(1), 1, "1.1"),
                (1, 0, Some(1), 1, "1.2"),
                (2, 0, Some(1), 1, "1.3"),
                (3, 0, Some(1), 1, "1.4"),
                (4, 0, Some(1), 1, "1.5"),
                (5, 1, Some(2), 1, "2.1"),
                (6, 1, Some(2), 1, "2.2"),
                (7, 1, Some(2), 1, "2.3"),
            ]
        );
    }

    #[test]
    fn a_fixture_refuses_what_an_issue_would_refuse() {
        let config = Preset::DirectMapped8.config();
        let fixture = |issues| Fixture {
            name: "refused",
            description: "",
            config: config.clone(),
            issues,
        };
        let word_bytes = config.word_bytes.bytes();
        let misaligned = 0x0A4 + word_bytes / 2;
        assert_eq!(
            fixture(vec![load(misaligned)]).events(),
            Err(FixtureError::Access(AccessError::Misaligned {
                addr: Addr::new_unchecked(misaligned),
                address_bits: config.address_bits,
                word_bytes,
            }))
        );
        let beyond = 1 << config.address_bits;
        assert_eq!(
            fixture(vec![load(0x0A4), store(beyond, 1)]).events(),
            Err(FixtureError::Access(AccessError::OutOfRange {
                addr: u64::from(beyond),
                address_bits: config.address_bits,
            }))
        );
        let invalid = Fixture {
            config: Config {
                address_bits: 0,
                ..config.clone()
            },
            ..fixture(Vec::new())
        };
        assert!(matches!(invalid.events(), Err(FixtureError::Config(_))));
    }

    #[test]
    fn a_fixture_refuses_a_store_value_the_word_cannot_hold() {
        let fixture = |word_bytes, value| Fixture {
            name: "refused",
            description: "",
            config: Config {
                word_bytes,
                ..Preset::DirectMapped8.config()
            },
            issues: vec![load(0x0A8), store(0x0A8, value)],
        };
        let refused = fixture(WordBytes::One, 300).events();
        assert_eq!(
            refused,
            Err(FixtureError::Value {
                value: Word::new_unchecked(300),
                word_bytes: 1,
            })
        );
        assert_eq!(
            refused.map_err(|error| error.to_string()),
            Err("300 does not fit a 1-byte word (0\u{2013}255).".to_owned())
        );
        assert_eq!(
            FixtureError::Value {
                value: Word::new_unchecked(u64::from(u32::MAX) + 1),
                word_bytes: 4,
            }
            .to_string(),
            "4,294,967,296 does not fit a 4-byte word (0\u{2013}4,294,967,295)."
        );
        // The largest word of each size still fits.
        for (word_bytes, largest) in [(WordBytes::One, 255), (WordBytes::Eight, u64::MAX)] {
            assert!(fixture(word_bytes, largest).events().is_ok());
        }
    }
}
