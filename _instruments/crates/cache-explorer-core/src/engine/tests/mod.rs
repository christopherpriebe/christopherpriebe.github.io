//! The engine's unit tests, a file for each of its parts: `access`, `exec`,
//! `memory_port` and `edit`. Here are the helpers they share: the sinks that
//! record the events of an op, a run of accesses on a machine, and builders of
//! requests, configurations and changed cells.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use super::access::access;
use super::exec::{load_program, ready, step, Until};
use super::port::MemoryPort;
use super::*;
use crate::cache::{Cache, Dirtiness, Slot};
use crate::config::{
    AllocatePolicy, CacheConfig, Config, MemoryInit, Mode, Preset, Replacement, Timing, WordBytes,
    WritePolicy,
};
use crate::cpu::{CpuState, Halted};
use crate::event::Counter;
use crate::isa::Program;
use crate::limits::{
    INSTRUCTION_BUDGET, MAX_ADDRESS_BITS, MAX_EVENTS_PER_ENTRY, MAX_LINES, MAX_SHARE_OPS,
};
use crate::machine::{RunPlace, Stalled};
use crate::memory::StaleInfo;
use crate::op::{EditPatch, EditTarget, OpError, ProgramFormat, WayPatch};
use crate::prng::Prng;
use crate::replacement::VictimReason;
use crate::units::{Cycle, LineAddr, Register, Tag, Word, WordIndex};

use EventKind::{
    Complete, Decode, Evict, Fill, Hit, MemWrite, Miss, Rank, Stall, Victim, Write, WriteBack,
    WriteThrough,
};

/// Whether every register of `cpu` holds 0.
fn registers_zero(cpu: &CpuState) -> bool {
    Register::ALL
        .into_iter()
        .all(|register| cpu.register(register) == 0)
}

/// Keeps every event and the machine after it, and stops after `stop_after`
/// events when that is set.
#[derive(Default)]
struct Recorder {
    events: Vec<MicroEvent>,
    after: Vec<Machine>,
    stop_after: Option<usize>,
}

impl EventSink for Recorder {
    fn records(&self) -> bool {
        true
    }

    fn event(&mut self, event: Option<MicroEvent>, after: &Machine) -> Flow {
        self.events
            .push(event.expect("a recording sink gets every event"));
        self.after.push(after.clone());
        if self.stop_after == Some(self.after.len()) {
            Flow::Stop
        } else {
            Flow::Continue
        }
    }
}

/// Counts events without recording them, and stops after `stop_after`.
#[derive(Default)]
struct Quiet {
    seen: usize,
    stop_after: Option<usize>,
}

impl EventSink for Quiet {
    fn records(&self) -> bool {
        false
    }

    fn event(&mut self, event: Option<MicroEvent>, _after: &Machine) -> Flow {
        assert!(event.is_none(), "a quiet sink gets no events");
        self.seen += 1;
        if self.stop_after == Some(self.seen) {
            Flow::Stop
        } else {
            Flow::Continue
        }
    }
}

/// A run: a machine and the numbering of the ops applied to it, one access
/// per op.
struct Run {
    machine: Machine,
    ops: u32,
    next_id: u64,
}

impl Run {
    fn new(config: Config) -> Self {
        Self {
            machine: Machine::start(config).unwrap(),
            ops: 0,
            next_id: 0,
        }
    }

    /// Executes `request` as the next op and returns its events with the
    /// machine after each. Every invariant that must hold mid-access holds
    /// after every event, the counters add up, and no event is an error:
    /// hits and misses make up the accesses, the three classes of miss make
    /// up the misses, and no write-through happens under write-back nor any
    /// write-around under write-allocate.
    /// In timed mode every event is stamped with the machine's cycle count,
    /// each miss stalls once, and the access costs the hit time, plus the
    /// miss penalty for a miss and again for a write-back; untimed, nothing
    /// is stamped, nothing stalls and no cycle is counted.
    #[track_caller]
    fn trace(&mut self, request: Request) -> Vec<(MicroEvent, Machine)> {
        let before = self.machine.counters;
        let mut recorder = Recorder::default();
        let mut out = Emitter::new(
            &mut recorder,
            OpIndex::new_unchecked(self.ops),
            self.next_id,
        );
        assert_eq!(access(&mut self.machine, request, &mut out), Flow::Continue);
        self.ops += 1;
        self.next_id += recorder.events.len() as u64;
        assert!(recorder.events.len() <= MAX_EVENTS_PER_ENTRY as usize);
        for (event, after) in recorder.events.iter().zip(&recorder.after) {
            assert_ne!(event.kind, EventKind::Error, "{}", event.explain);
            assert_eq!(after.check_invariants(), Ok(()), "after {}", event.label);
            let counters = after.counters;
            assert_eq!(
                counters.hits + counters.misses,
                counters.accesses,
                "after {}",
                event.label
            );
            assert_eq!(
                counters.loads + counters.stores,
                counters.accesses,
                "after {}",
                event.label
            );
            assert_eq!(
                counters.compulsory + counters.capacity + counters.conflict,
                counters.misses,
                "after {}",
                event.label
            );
            let cache = &after.config.cache;
            if cache.write_policy == WritePolicy::WriteBack {
                assert_eq!(counters.writethroughs, 0, "after {}", event.label);
            }
            if cache.allocate_policy == AllocatePolicy::WriteAllocate {
                assert_eq!(counters.writearounds, 0, "after {}", event.label);
            }
            let stamp = after.config.timing.map(|_| counters.cycles);
            assert_eq!(event.cycle, stamp, "{}", event.label);
        }
        let after = self.machine.counters;
        let stalls = recorder
            .events
            .iter()
            .filter(|event| event.kind == Stall)
            .count();
        let (misses, writebacks) = (
            after.misses - before.misses,
            after.writebacks - before.writebacks,
        );
        if let Some(timing) = self.machine.config.timing {
            assert_eq!(stalls, misses as usize);
            let cost = u64::from(timing.hit_cycles)
                + u64::from(misses + writebacks) * u64::from(timing.miss_cycles);
            assert_eq!(after.cycles.get() - before.cycles.get(), cost);
        } else {
            assert_eq!(stalls, 0);
            assert_eq!(after.cycles.get(), 0);
        }
        assert!(!self.machine.in_flight());
        recorder.events.into_iter().zip(recorder.after).collect()
    }

    #[track_caller]
    fn events(&mut self, request: Request) -> Vec<MicroEvent> {
        self.trace(request)
            .into_iter()
            .map(|(event, _)| event)
            .collect()
    }

    /// A load of `addr` into r1, as a hand-issued load makes it.
    #[track_caller]
    fn load(&mut self, addr: u32) -> Vec<MicroEvent> {
        self.events(load(addr, Register::R1))
    }

    #[track_caller]
    fn store(&mut self, addr: u32, value: u64) -> Vec<MicroEvent> {
        self.events(store(addr, value))
    }

    fn way(&self, set_index: u32, way_index: u32) -> &crate::cache::Way {
        self.machine
            .cache
            .way(set(set_index), way(way_index))
            .unwrap()
    }

    fn memory(&self, at: u32) -> u64 {
        self.machine.memory.word(addr(at)).get()
    }
}

/// A load of `at` into `into`, issued by hand.
fn load(at: u32, into: Register) -> Request {
    Request::Load {
        core: (),
        addr: addr(at),
        into,
        origin: Origin::Hand,
    }
}

/// A store of `value` to `at`, issued by hand.
fn store(at: u32, value: u64) -> Request {
    Request::Store {
        core: (),
        addr: addr(at),
        value,
        from: None,
        origin: Origin::Hand,
    }
}

/// The tests' address width: 12 bits. The addresses the tests use, up to
/// 0x2A4, and the request streams need at least 10 bits, so the width is set
/// here rather than taken from `Config::default()`, and changing the default
/// does not break these tests.
const ADDRESS_BITS: u8 = 12;

/// A 16 B-line cache of `size` bytes and `ways` ways under the given
/// policies, with the tests' address width, `Config::default()`'s word size
/// and pattern memory, so a fill brings in visible values.
fn config(
    size: u32,
    ways: u32,
    replacement: Replacement,
    write_policy: WritePolicy,
    allocate_policy: AllocatePolicy,
) -> Config {
    Config {
        address_bits: ADDRESS_BITS,
        cache: CacheConfig {
            size_bytes: size,
            line_bytes: 16,
            associativity: ways,
            replacement,
            write_policy,
            allocate_policy,
        },
        memory_init: MemoryInit::Pattern,
        ..Config::default()
    }
}

/// The named configuration `preset` at the tests' address width.
fn preset(preset: Preset) -> Config {
    Config {
        address_bits: ADDRESS_BITS,
        ..preset.config()
    }
}

/// `direct-mapped-8` under the given write and allocate policies.
fn direct_mapped(write_policy: WritePolicy, allocate_policy: AllocatePolicy) -> Config {
    config(128, 1, Replacement::Lru, write_policy, allocate_policy)
}

/// The floor under `MAX_CYCLES`: the shipped timing charges 20-cycle misses,
/// so the constant may be lowered this far without a test edit, and a test
/// that charged more would have its configuration refused.
const CYCLE_FLOOR: u32 = 20;

/// Timed mode with `hit`-cycle hits and `miss`-cycle misses, neither above
/// the floor under `MAX_CYCLES`.
#[track_caller]
fn timing(hit: u32, miss: u32) -> Timing {
    assert!(
        hit.max(miss) <= CYCLE_FLOOR,
        "MAX_CYCLES may be lowered to {CYCLE_FLOOR}; a test charges no more"
    );
    Timing {
        hit_cycles: hit,
        miss_cycles: miss,
    }
}

/// `config` in timed mode, with `hit`-cycle hits and `miss`-cycle misses.
fn timed(config: Config, hit: u32, miss: u32) -> Config {
    Config {
        timing: Some(timing(hit, miss)),
        ..config
    }
}

/// `two-way-lru` under the given replacement policy: 4 sets of 2 ways,
/// write-back, write-allocate.
fn two_way(replacement: Replacement) -> Config {
    config(
        128,
        2,
        replacement,
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    )
}

fn addr(value: u32) -> Addr {
    Addr::new_unchecked(value)
}

fn line_addr(value: u32) -> LineAddr {
    LineAddr::new_unchecked(value)
}

fn set(index: u32) -> SetIndex {
    SetIndex::new_unchecked(index)
}

fn way(index: u32) -> WayIndex {
    WayIndex::new_unchecked(index)
}

fn kinds(events: &[MicroEvent]) -> Vec<EventKind> {
    events.iter().map(|event| event.kind).collect()
}

fn valid(s: u32, w: u32) -> ChangeRef {
    ChangeRef::Valid {
        set: set(s),
        way: way(w),
    }
}

fn tag(s: u32, w: u32) -> ChangeRef {
    ChangeRef::Tag {
        set: set(s),
        way: way(w),
    }
}

fn dirty(s: u32, w: u32) -> ChangeRef {
    ChangeRef::Dirty {
        set: set(s),
        way: way(w),
    }
}

fn rank_of(s: u32, w: u32) -> ChangeRef {
    ChangeRef::Rank {
        set: set(s),
        way: way(w),
    }
}

fn word(s: u32, w: u32, i: u32) -> ChangeRef {
    ChangeRef::Word {
        set: set(s),
        way: way(w),
        word: WordIndex::new_unchecked(i),
    }
}

fn memory(at: u32) -> ChangeRef {
    ChangeRef::Memory { addr: addr(at) }
}

fn register(register: Register) -> ChangeRef {
    ChangeRef::Register { register }
}

fn counter(counter: Counter) -> ChangeRef {
    ChangeRef::Counter { counter }
}

/// A way taking or losing a 16 B line of 4-byte words: its valid bit, its
/// tag and its four words, with `extra`, in sorted order.
fn line_change(s: u32, w: u32, extra: &[ChangeRef]) -> Vec<ChangeRef> {
    let mut refs = vec![valid(s, w), tag(s, w)];
    refs.extend((0..4).map(|i| word(s, w, i)));
    refs.extend_from_slice(extra);
    refs.sort_unstable();
    refs
}

/// The one event of `kind` in `events`.
#[track_caller]
fn event(events: &[MicroEvent], kind: EventKind) -> &MicroEvent {
    let mut found = events.iter().filter(|event| event.kind == kind);
    let first = found.next().unwrap_or_else(|| panic!("no {kind:?} event"));
    assert!(found.next().is_none(), "more than one {kind:?} event");
    first
}

/// Where an event happened, what it changed and why: its address, set and
/// way, its changed cells and its cause.
type Shape<'a> = (
    Option<Addr>,
    Option<SetIndex>,
    Option<WayIndex>,
    &'a [ChangeRef],
    Option<Cause>,
);

/// The place, changed cells and cause of an event.
fn shape(event: &MicroEvent) -> Shape<'_> {
    (
        event.addr,
        event.set,
        event.way,
        &event.changed,
        event.cause,
    )
}

/// Every cell and counter that differs between two machines, by value, with
/// the identity rule: a way whose valid bit or tag changed lists its tag and
/// every word. Memory is compared over the first three times as many lines
/// as the cache has, where the request streams and the test programs stay.
fn diff(before: &Machine, after: &Machine) -> Vec<ChangeRef> {
    let geometry = after.geometry;
    let mut changed = Vec::new();
    for s in 0..geometry.sets() {
        for w in 0..geometry.ways() {
            let old = before.cache.way(set(s), way(w)).unwrap();
            let new = after.cache.way(set(s), way(w)).unwrap();
            if old.is_valid() != new.is_valid() {
                changed.push(valid(s, w));
            }
            if old.is_dirty() != new.is_dirty() {
                changed.push(dirty(s, w));
            }
            if old.rank != new.rank {
                changed.push(rank_of(s, w));
            }
            let words = |slot: &Slot| -> Vec<u64> {
                match slot {
                    Slot::Valid { data, .. } => (0..geometry.words_per_line())
                        .map(|i| {
                            crate::memory::word_at(
                                data,
                                i * geometry.word_bytes(),
                                geometry.word_bytes(),
                            )
                        })
                        .collect(),
                    Slot::Invalid => Vec::new(),
                }
            };
            if (old.is_valid(), old.tag()) == (new.is_valid(), new.tag()) {
                let (old_words, new_words) = (words(&old.slot), words(&new.slot));
                for (i, (a, b)) in (0..).zip(old_words.iter().zip(&new_words)) {
                    if a != b {
                        changed.push(word(s, w, i));
                    }
                }
            } else {
                changed.push(tag(s, w));
                changed.extend((0..geometry.words_per_line()).map(|i| word(s, w, i)));
            }
        }
    }
    // The stream's lines, which are the only ones memory can change.
    for line in (0..geometry.lines() * 3).map(|line| line * geometry.line_bytes()) {
        for i in 0..geometry.words_per_line() {
            let at = addr(line + i * geometry.word_bytes());
            if before.memory.word(at) != after.memory.word(at) {
                changed.push(memory(at.get()));
            }
        }
    }
    let registers = [
        Register::R0,
        Register::R1,
        Register::R2,
        Register::R3,
        Register::R4,
        Register::R5,
        Register::R6,
        Register::R7,
    ];
    for register in registers {
        if before.cpu.register(register) != after.cpu.register(register) {
            changed.push(self::register(register));
        }
    }
    if before.cpu.pc() != after.cpu.pc() {
        changed.push(ChangeRef::Pc);
    }
    let (old, new) = (before.counters, after.counters);
    let counts = [
        (Counter::Accesses, old.accesses, new.accesses),
        (Counter::Loads, old.loads, new.loads),
        (Counter::Stores, old.stores, new.stores),
        (Counter::Hits, old.hits, new.hits),
        (Counter::Misses, old.misses, new.misses),
        (Counter::Evictions, old.evictions, new.evictions),
        (Counter::Writebacks, old.writebacks, new.writebacks),
        (Counter::Writethroughs, old.writethroughs, new.writethroughs),
        (Counter::Writearounds, old.writearounds, new.writearounds),
        (Counter::Fills, old.fills, new.fills),
        (Counter::Compulsory, old.compulsory, new.compulsory),
        (Counter::Capacity, old.capacity, new.capacity),
        (Counter::Conflict, old.conflict, new.conflict),
    ];
    for (name, a, b) in counts {
        if a != b {
            changed.push(counter(name));
        }
    }
    if old.cycles != new.cycles {
        changed.push(counter(Counter::Cycles));
    }
    changed.sort_unstable();
    changed
}

/// `op` applied to `machine` as op 1, whose first event gets id `first_id`:
/// the result, and the events.
fn apply_op(
    machine: &mut Machine,
    op: &Op,
    first_id: u64,
) -> (Result<(), OpError>, Vec<MicroEvent>) {
    let mut recorder = Recorder::default();
    let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(1), first_id);
    let result = apply(machine, op, &mut out);
    (result, recorder.events)
}

/// An edit of way `w` of set `s` with `patch`.
fn way_edit(s: u32, w: u32, patch: WayPatch) -> Op {
    Op::Edit {
        target: EditTarget::Way {
            set: set(s),
            way: way(w),
        },
        patch: EditPatch::Way(patch),
    }
}

/// One test per case of a table: each `name => (arguments)` becomes a test
/// named `name` that passes the arguments to `check`, so a failure names the
/// case it is in and the cases after it still run. The integration tests
/// have a macro of the same name and shape in `tests/common`.
macro_rules! cases {
    ($check:path; $($name:ident => ($($argument:expr),* $(,)?),)*) => {
        $(
            #[test]
            fn $name() {
                $check($($argument),*);
            }
        )*
    };
}

/// A run driven like a program: each load and each step is an op of its
/// own, and every event is recorded with the machine after it.
struct Driven {
    run: Run,
}

impl Driven {
    fn new(config: Config) -> Self {
        Self {
            run: Run::new(config),
        }
    }

    /// Applies `op` as the next op and returns its flow and its events with
    /// the machine after each. After every event the invariants that hold
    /// mid-access hold, the counters add up, and the pc is never past the
    /// program's end.
    #[track_caller]
    fn apply(
        &mut self,
        op: impl FnOnce(&mut Machine, &mut Emitter<'_>) -> Flow,
    ) -> (Flow, Vec<(MicroEvent, Machine)>) {
        let run = &mut self.run;
        let mut recorder = Recorder::default();
        let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(run.ops), run.next_id);
        let flow = op(&mut run.machine, &mut out);
        run.ops += 1;
        run.next_id += recorder.events.len() as u64;
        for (event, after) in recorder.events.iter().zip(&recorder.after) {
            assert_eq!(after.check_invariants(), Ok(()), "after {}", event.label);
            let counters = after.counters;
            assert_eq!(
                counters.hits + counters.misses,
                counters.accesses,
                "after {}",
                event.label
            );
            let length = after.cpu.program().map_or(0, |program| program.len());
            assert!(after.cpu.pc() <= length, "after {}", event.label);
        }
        (
            flow,
            recorder.events.into_iter().zip(recorder.after).collect(),
        )
    }

    /// Loads the assembly program `source`, which parses.
    fn load(&mut self, source: &str) -> Vec<(MicroEvent, Machine)> {
        self.load_as(source, ProgramFormat::Asm)
    }

    /// Loads `source` as `format`.
    #[track_caller]
    fn load_as(&mut self, source: &str, format: ProgramFormat) -> Vec<(MicroEvent, Machine)> {
        let program = Program::parse(&self.run.machine.geometry, source, format).unwrap();
        let (flow, trace) = self.apply(|machine, out| load_program(machine, (), program, out));
        assert_eq!(flow, Flow::Continue);
        trace
    }

    /// Executes the next instruction, which there is.
    #[track_caller]
    fn step(&mut self) -> Vec<(MicroEvent, Machine)> {
        assert_eq!(ready(&self.run.machine), Ok(()));
        self.apply(step).1
    }

    /// Steps until the program cannot go on, and returns every event.
    #[track_caller]
    fn finish(&mut self) -> Vec<(MicroEvent, Machine)> {
        let mut trace = Vec::new();
        while ready(&self.run.machine).is_ok() {
            trace.extend(self.step());
        }
        trace
    }

    /// A load or store issued by hand.
    #[track_caller]
    fn issue(&mut self, request: Request) -> Vec<(MicroEvent, Machine)> {
        let (flow, trace) = self.apply(|machine, out| access(machine, request, out));
        assert_eq!(flow, Flow::Continue);
        trace
    }

    fn machine(&self) -> &Machine {
        &self.run.machine
    }
}

mod access;
mod edit;
mod exec;
mod memory_port;
