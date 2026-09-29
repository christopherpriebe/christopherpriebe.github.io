//! The access engine: the ten access cases with their events, order,
//! counters and changed cells, and frame 3b's dirty eviction with its log
//! lines; hit and miss sequences; victims under each replacement policy;
//! write-backs, write-throughs and write-arounds; miss counts and the class
//! of each miss, compulsory, capacity or conflict, in known examples and
//! against a fully associative LRU cache run beside the engine; the
//! sparkline's outcomes and the stats of a run; staleness; event numbering;
//! stopping mid-access; the invariant checks at entry ends; the internal
//! error of a lookup that finds no way; timed mode: where each case's
//! cycles are charged, the stall and its countdown, the cycle identity at
//! entry ends, and AMAT and the measured cost of fixed traces; and programs:
//! loading one, each instruction retiring at its last event, the changed
//! cells of every program event, stopping mid-instruction, faults, the
//! budget, and hand-issued accesses beside a program; and edits: the cells
//! each lists, the counters none moves, the lines they take in and out of
//! what the classification of misses remembers, and a refused edit.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use super::access::{access, Origin};
use super::exec::{load_program, ready, step};
use super::*;
use crate::cache::{Cache, Dirtiness, Slot};
use crate::config::{
    AllocatePolicy, CacheConfig, Config, MemoryInit, Preset, Replacement, Timing, WordBytes,
    WritePolicy,
};
use crate::cpu::{Cpu, Halted, Registers};
use crate::event::Counter;
use crate::isa::Program;
use crate::limits::{
    INSTRUCTION_BUDGET, MAX_ADDRESS_BITS, MAX_EVENTS_PER_ENTRY, MAX_LINES, MAX_SHARE_OPS,
};
use crate::machine::Stalled;
use crate::memory::StaleInfo;
use crate::op::{EditPatch, EditTarget, OpError, ProgramFormat, WayPatch};
use crate::prng::Prng;
use crate::replacement::VictimReason;
use crate::units::{Cycle, LineAddr, Register, Tag, Word, WordIndex};

use EventKind::{
    Complete, Decode, Evict, Fill, Hit, MemWrite, Miss, Rank, Stall, Victim, Write, WriteBack,
    WriteThrough,
};

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
            assert_eq!(counters.hits + counters.misses, counters.accesses);
            assert_eq!(counters.loads + counters.stores, counters.accesses);
            assert_eq!(
                counters.compulsory + counters.capacity + counters.conflict,
                counters.misses
            );
            let cache = &after.config.cache;
            if cache.write_policy == WritePolicy::WriteBack {
                assert_eq!(counters.writethroughs, 0);
            }
            if cache.allocate_policy == AllocatePolicy::WriteAllocate {
                assert_eq!(counters.writearounds, 0);
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

    fn events(&mut self, request: Request) -> Vec<MicroEvent> {
        self.trace(request)
            .into_iter()
            .map(|(event, _)| event)
            .collect()
    }

    /// A load of `addr` into r1, as a hand-issued load makes it.
    fn load(&mut self, addr: u32) -> Vec<MicroEvent> {
        self.events(load(addr, Register::R1))
    }

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
        addr: addr(at),
        into,
        origin: Origin::Hand,
    }
}

/// A store of `value` to `at`, issued by hand.
fn store(at: u32, value: u64) -> Request {
    Request::Store {
        addr: addr(at),
        value,
        from: None,
        origin: Origin::Hand,
    }
}

/// The tests' address width: 12 bits, as in every design frame. The design's
/// addresses, up to 0x2A4, and the request streams need at least 10 bits, so
/// the width is set here rather than taken from `Config::default()`, and
/// changing the default does not break these tests.
const ADDRESS_BITS: u8 = 12;

/// A 16 B-line cache of `size` bytes and `ways` ways under the given
/// policies, with the tests' address width, `Config::default()`'s word size
/// and the design's pattern memory, so a fill brings in visible values.
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

// ---------------------------------------------------------------------------
// The ten access cases.
// ---------------------------------------------------------------------------

#[test]
fn case_1_a_load_hit_reads_the_way_and_ranks_last_if_it_moved() {
    // 128 B two-way: 0x0A4 and 0x1A4 are in set 2, tags 0x02 and 0x06.
    let mut run = Run::new(two_way(Replacement::Lru));
    run.load(0x0A4);
    let events = run.events(load(0x0A4, Register::R2));
    assert_eq!(kinds(&events), [Decode, Hit, Complete]);
    assert_eq!(
        shape(&events[0]),
        (
            Some(addr(0x0A4)),
            Some(set(2)),
            None,
            &[][..],
            Some(Cause::LineSize)
        )
    );
    assert_eq!(
        shape(&events[1]),
        (
            Some(addr(0x0A4)),
            Some(set(2)),
            Some(way(0)),
            &[
                counter(Counter::Accesses),
                counter(Counter::Loads),
                counter(Counter::Hits)
            ][..],
            Some(Cause::Associativity)
        )
    );
    assert_eq!(
        shape(&events[2]),
        (
            Some(addr(0x0A4)),
            Some(set(2)),
            Some(way(0)),
            &[register(Register::R2)][..],
            None
        )
    );
    assert_eq!(run.machine.registers.get(Register::R2), 53);

    // A second line in the set pushes way 0 back, so the next hit on it
    // moves it to the front, reported last.
    run.load(0x1A4);
    let events = run.events(load(0x0A4, Register::R2));
    assert_eq!(kinds(&events), [Decode, Hit, Complete, Rank]);
    assert!(events[2].changed.is_empty(), "r2 already holds 53");
    assert_eq!(
        shape(&events[3]),
        (
            None,
            Some(set(2)),
            Some(way(0)),
            &[rank_of(2, 0), rank_of(2, 1)][..],
            Some(Cause::Replacement)
        )
    );
    assert_eq!((run.way(2, 0).rank, run.way(2, 1).rank), (0, 1));
    let counters = run.machine.counters;
    assert_eq!(
        (
            counters.accesses,
            counters.loads,
            counters.hits,
            counters.misses
        ),
        (4, 4, 2, 2)
    );
}

#[test]
fn case_2_a_load_miss_fills_an_invalid_way() {
    let mut run = Run::new(two_way(Replacement::Lru));
    let events = run.load(0x0A4);
    // Way 0 already ranks first, so the fill moves no rank.
    assert_eq!(kinds(&events), [Decode, Miss, Victim, Fill, Complete]);
    // The line was never filled in this run: a compulsory miss.
    assert_eq!(
        shape(&events[1]),
        (
            Some(addr(0x0A4)),
            Some(set(2)),
            None,
            &[
                counter(Counter::Accesses),
                counter(Counter::Loads),
                counter(Counter::Misses),
                counter(Counter::Compulsory)
            ][..],
            Some(Cause::Associativity)
        )
    );
    assert_eq!(
        shape(&events[2]),
        (
            None,
            Some(set(2)),
            Some(way(0)),
            &[][..],
            Some(Cause::Associativity)
        )
    );
    assert_eq!(
        shape(&events[3]),
        (
            Some(addr(0x0A0)),
            Some(set(2)),
            Some(way(0)),
            &line_change(2, 0, &[counter(Counter::Fills)])[..],
            Some(Cause::LineSize)
        )
    );
    assert_eq!(
        shape(&events[4]),
        (
            Some(addr(0x0A4)),
            Some(set(2)),
            Some(way(0)),
            &[register(Register::R1)][..],
            None
        )
    );
    assert_eq!(run.machine.registers.get(Register::R1), 53);
    match &run.way(2, 0).slot {
        Slot::Valid { tag, dirty, data } => {
            assert_eq!(*tag, Tag::new_unchecked(0x02));
            assert_eq!(*dirty, Dirtiness::Clean);
            assert_eq!(*data, *run.machine.memory.line(line_addr(0x0A0)));
        }
        Slot::Invalid => panic!("way 0 was filled"),
    }
    assert!(run.machine.memory.is_touched(line_addr(0x0A0)));

    // The set's other invalid way ranks behind way 0, so filling it moves
    // both ranks.
    let events = run.load(0x1A4);
    assert_eq!(kinds(&events), [Decode, Miss, Victim, Fill, Complete, Rank]);
    assert_eq!(event(&events, Victim).way, Some(way(1)));
    assert_eq!(event(&events, Rank).changed, [rank_of(2, 0), rank_of(2, 1)]);
    let counters = run.machine.counters;
    assert_eq!(
        (counters.misses, counters.fills, counters.evictions),
        (2, 2, 0)
    );
}

#[test]
fn case_2_in_a_one_way_set_the_empty_way_is_the_only_choice() {
    let mut run = Run::new(preset(Preset::DirectMapped8));
    let events = run.load(0x0A4);
    assert_eq!(kinds(&events), [Decode, Miss, Victim, Fill, Complete]);
    let victim = event(&events, Victim);
    assert_eq!(
        (victim.way, victim.cause),
        (Some(way(0)), Some(Cause::Associativity))
    );
}

#[test]
fn case_3_a_load_miss_evicts_a_clean_victim() {
    // Direct-mapped: 0x1A4 maps to 0x0A4's set 2 and replaces it.
    let mut run = Run::new(preset(Preset::DirectMapped8));
    run.load(0x0A4);
    let trace = run.trace(load(0x1A4, Register::R1));
    let events: Vec<MicroEvent> = trace.iter().map(|(event, _)| event.clone()).collect();
    assert_eq!(
        kinds(&events),
        [Decode, Miss, Victim, Evict, Fill, Complete]
    );
    assert_eq!(event(&events, Victim).cause, Some(Cause::Associativity));
    let evict = event(&events, Evict);
    assert_eq!(
        shape(evict),
        (
            Some(addr(0x0A0)),
            Some(set(2)),
            Some(way(0)),
            &line_change(2, 0, &[counter(Counter::Evictions)])[..],
            Some(Cause::Associativity)
        )
    );
    // Between the eviction and the fill the way is empty.
    assert_eq!(
        trace[3].1.cache.way(set(2), way(0)).unwrap().slot,
        Slot::Invalid
    );
    assert_eq!(event(&events, Complete).changed, [register(Register::R1)]);
    assert_eq!(run.machine.registers.get(Register::R1), 12);
    let counters = run.machine.counters;
    assert_eq!(
        (counters.misses, counters.fills, counters.evictions),
        (2, 2, 1)
    );
}

#[test]
fn case_3_with_a_choice_of_victims_the_policy_is_the_cause() {
    let mut run = Run::new(two_way(Replacement::Lru));
    run.load(0x0A4);
    run.load(0x1A4);
    let events = run.load(0x2A4);
    assert_eq!(
        kinds(&events),
        [Decode, Miss, Victim, Evict, Fill, Complete, Rank]
    );
    // Way 0 was used least recently.
    for kind in [Victim, Evict] {
        let event = event(&events, kind);
        assert_eq!(
            (event.way, event.cause),
            (Some(way(0)), Some(Cause::Replacement))
        );
    }
    assert_eq!(event(&events, Rank).changed, [rank_of(2, 0), rank_of(2, 1)]);
}

/// Frame 3b's access #5, the fourth access of this run: two-way LRU,
/// write-back; the load of 0x2A4 into r3 evicts a dirty 0x1A0 from way 1.
/// Returns the run after it and the access's events with the machine after
/// each.
fn frame_3b_access_5() -> (Run, Vec<(MicroEvent, Machine)>) {
    let mut run = Run::new(two_way(Replacement::Lru));
    run.load(0x0A4);
    run.store(0x1A4, 5);
    run.load(0x0A4);
    let trace = run.trace(load(0x2A4, Register::R3));
    (run, trace)
}

#[test]
fn case_4_a_dirty_victim_is_written_back_before_it_is_evicted() {
    let (run, trace) = frame_3b_access_5();
    let events: Vec<MicroEvent> = trace.into_iter().map(|(event, _)| event).collect();
    assert_eq!(
        kinds(&events),
        [Decode, Miss, Victim, WriteBack, Evict, Fill, Complete, Rank]
    );
    let causes: Vec<Option<Cause>> = events.iter().map(|event| event.cause).collect();
    assert_eq!(
        causes,
        [
            Some(Cause::LineSize),
            Some(Cause::Associativity),
            Some(Cause::Replacement),
            Some(Cause::WritePolicy),
            Some(Cause::Replacement),
            Some(Cause::LineSize),
            None,
            Some(Cause::Replacement)
        ]
    );
    let labels: Vec<&str> = events.iter().map(|event| event.label.as_str()).collect();
    assert_eq!(
        labels,
        ["4.1", "4.2", "4.3", "4.4", "4.5", "4.6", "4.7", "4.8"]
    );
    assert_eq!(
        shape(&events[3]),
        (
            Some(addr(0x1A0)),
            Some(set(2)),
            Some(way(1)),
            &[memory(0x1A4), counter(Counter::Writebacks)][..],
            Some(Cause::WritePolicy)
        )
    );
    assert_eq!(
        shape(&events[4]),
        (
            Some(addr(0x1A0)),
            Some(set(2)),
            Some(way(1)),
            &line_change(2, 1, &[dirty(2, 1), counter(Counter::Evictions)])[..],
            Some(Cause::Replacement)
        )
    );
    assert_eq!(
        shape(&events[5]),
        (
            Some(addr(0x2A0)),
            Some(set(2)),
            Some(way(1)),
            &line_change(2, 1, &[counter(Counter::Fills)])[..],
            Some(Cause::LineSize)
        )
    );
    assert_eq!(event(&events, Complete).changed, [register(Register::R3)]);
    assert_eq!(run.machine.registers.get(Register::R3), 68);
    assert_eq!(event(&events, Rank).changed, [rank_of(2, 0), rank_of(2, 1)]);
    assert_eq!((run.way(2, 1).rank, run.way(2, 0).rank), (0, 1));
    let counters = run.machine.counters;
    assert_eq!(
        (
            counters.accesses,
            counters.loads,
            counters.stores,
            counters.hits,
            counters.misses,
            counters.fills,
            counters.evictions,
            counters.writebacks
        ),
        (4, 3, 1, 1, 3, 3, 1, 1)
    );
}

#[test]
fn case_4_reads_as_the_design_words_frame_3b() {
    let (_, trace) = frame_3b_access_5();
    let lines: Vec<&str> = trace
        .iter()
        .map(|(event, _)| event.explain.as_str())
        .collect();
    assert_eq!(
        lines,
        [
            "Load 0x2A4: tag 0x0A, set 2, offset 4 (word 1).",
            "Set 2: no valid way holds tag 0x0A. Miss.",
            "Way 1 chosen: least recently used (rank 1).",
            "Way 1 is dirty: write line 0x1A0 back to memory first.",
            "Evict line 0x1A0 (tag 0x06) from way 1.",
            "Fill way 1 with line 0x2A0\u{2013}0x2AF from memory.",
            "Read word 1 of way 1 (68) into r3.",
            "LRU ranks in set 2: way 1 becomes 0 (most recent); way 0 is 1.",
        ]
    );
}

#[test]
fn case_4_the_victim_stays_dirty_until_the_eviction() {
    let (_, trace) = frame_3b_access_5();
    let victim = &trace[2].1;
    assert_eq!(
        victim
            .access
            .as_ref()
            .unwrap()
            .victim
            .map(|choice| choice.reason),
        Some(VictimReason::LeastRecentlyUsed { rank: 1 })
    );
    assert_eq!(
        victim.stale(addr(0x1A4)),
        Some(StaleInfo {
            set: set(2),
            way: way(1),
            cached: Word::new_unchecked(5),
            memory: Word::new_unchecked(12),
        })
    );

    // The write-back writes memory and leaves the way as it was, dirty bit
    // included; the word is stale no longer.
    let written_back = &trace[3].1;
    let held = written_back.cache.way(set(2), way(1)).unwrap();
    assert!(held.is_valid() && held.is_dirty());
    assert_eq!(held.tag(), Some(Tag::new_unchecked(0x06)));
    assert_eq!(written_back.memory.word(addr(0x1A4)).get(), 5);
    assert_eq!(written_back.stale(addr(0x1A4)), None);

    // The eviction clears valid, tag, dirty and data together, and the
    // fill brings the new line in clean.
    let evicted = &trace[4].1;
    assert_eq!(
        evicted.cache.way(set(2), way(1)).unwrap().slot,
        Slot::Invalid
    );
    let filled = trace[5].1.cache.way(set(2), way(1)).unwrap();
    assert_eq!(filled.tag(), Some(Tag::new_unchecked(0x0A)));
    assert!(!filled.is_dirty());
}

#[test]
fn case_5_a_store_hit_under_write_back_dirties_the_line() {
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    ));
    run.load(0x0A4);
    let events = run.store(0x0A4, 7);
    assert_eq!(kinds(&events), [Decode, Hit, Write]);
    assert_eq!(
        event(&events, Hit).changed,
        [
            counter(Counter::Accesses),
            counter(Counter::Stores),
            counter(Counter::Hits)
        ]
    );
    assert_eq!(
        shape(event(&events, Write)),
        (
            Some(addr(0x0A4)),
            Some(set(2)),
            Some(way(0)),
            &[dirty(2, 0), word(2, 0, 1)][..],
            Some(Cause::WritePolicy)
        )
    );
    assert!(run.way(2, 0).is_dirty());
    assert_eq!(run.memory(0x0A4), 53, "memory is untouched");
    assert_eq!(
        run.machine
            .stale(addr(0x0A4))
            .map(|stale| (stale.cached.get(), stale.memory.get())),
        Some((7, 53))
    );

    // Writing the same value again changes nothing; another value changes
    // only the word, since the line is already dirty.
    assert!(event(&run.store(0x0A4, 7), Write).changed.is_empty());
    assert_eq!(event(&run.store(0x0A4, 8), Write).changed, [word(2, 0, 1)]);
    let counters = run.machine.counters;
    assert_eq!(
        (
            counters.stores,
            counters.hits,
            counters.writebacks,
            counters.writethroughs
        ),
        (3, 3, 0, 0)
    );
}

#[test]
fn case_5_with_a_rank_move_the_rank_comes_after_the_write() {
    let mut run = Run::new(two_way(Replacement::Lru));
    run.load(0x0A4);
    run.load(0x1A4);
    let events = run.store(0x0A4, 7);
    assert_eq!(kinds(&events), [Decode, Hit, Write, Rank]);
}

#[test]
fn case_6_a_store_hit_under_write_through_writes_memory_too() {
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteThrough,
        AllocatePolicy::WriteAllocate,
    ));
    run.load(0x0A4);
    let trace = run.trace(store(0x0A4, 7));
    let events: Vec<MicroEvent> = trace.iter().map(|(event, _)| event.clone()).collect();
    assert_eq!(kinds(&events), [Decode, Hit, Write, WriteThrough]);
    assert_eq!(
        shape(event(&events, Write)),
        (
            Some(addr(0x0A4)),
            Some(set(2)),
            Some(way(0)),
            &[word(2, 0, 1)][..],
            Some(Cause::WritePolicy)
        )
    );
    assert_eq!(
        shape(event(&events, WriteThrough)),
        (
            Some(addr(0x0A4)),
            Some(set(2)),
            Some(way(0)),
            &[memory(0x0A4), counter(Counter::Writethroughs)][..],
            Some(Cause::WritePolicy)
        )
    );
    // Between the two, the clean line is ahead of memory, which only an
    // access in flight may be.
    let written = &trace[2].1;
    assert_eq!(written.memory.word(addr(0x0A4)).get(), 53);
    assert!(written.in_flight());
    assert_eq!(written.check_invariants(), Ok(()));
    let mut boundary = written.clone();
    boundary.access.as_mut().unwrap().done = true;
    assert_eq!(
        boundary.check_invariants(),
        Err(InvariantViolation::CleanLineDiffersFromMemory {
            set: set(2),
            way: way(0)
        })
    );
    assert!(!run.way(2, 0).is_dirty());
    assert_eq!(run.memory(0x0A4), 7);
    assert_eq!(run.machine.stale(addr(0x0A4)), None);
    assert_eq!(run.machine.counters.writethroughs, 1);

    // Writing what memory already holds still counts, but changes no cell.
    let events = run.store(0x0A4, 7);
    assert!(event(&events, Write).changed.is_empty());
    assert_eq!(
        event(&events, WriteThrough).changed,
        [counter(Counter::Writethroughs)]
    );
    assert_eq!(run.machine.counters.writethroughs, 2);
}

#[test]
fn case_7_a_store_miss_under_write_allocate_and_write_back_fills_then_writes() {
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    ));
    let events = run.store(0x0A4, 7);
    assert_eq!(kinds(&events), [Decode, Miss, Victim, Fill, Write]);
    assert_eq!(
        event(&events, Miss).changed,
        [
            counter(Counter::Accesses),
            counter(Counter::Stores),
            counter(Counter::Misses),
            counter(Counter::Compulsory)
        ]
    );
    assert_eq!(event(&events, Write).changed, [dirty(2, 0), word(2, 0, 1)]);
    assert_eq!(run.memory(0x0A4), 53);

    // The next store to the set writes the dirty line back first.
    let events = run.store(0x1A4, 9);
    assert_eq!(
        kinds(&events),
        [Decode, Miss, Victim, WriteBack, Evict, Fill, Write]
    );
    assert_eq!(
        event(&events, WriteBack).changed,
        [memory(0x0A4), counter(Counter::Writebacks)]
    );
    assert_eq!(run.memory(0x0A4), 7);
    assert_eq!(run.memory(0x1A4), 12);
    let counters = run.machine.counters;
    assert_eq!(
        (
            counters.misses,
            counters.fills,
            counters.evictions,
            counters.writebacks,
            counters.writearounds
        ),
        (2, 2, 1, 1, 0)
    );

    // In a two-way set the fill of the empty way moves the ranks.
    let mut run = Run::new(two_way(Replacement::Lru));
    run.load(0x0A4);
    assert_eq!(
        kinds(&run.store(0x1A4, 9)),
        [Decode, Miss, Victim, Fill, Write, Rank]
    );
}

#[test]
fn case_8_a_store_miss_under_write_allocate_and_write_through_fills_writes_and_writes_through() {
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteThrough,
        AllocatePolicy::WriteAllocate,
    ));
    let events = run.store(0x0A4, 7);
    assert_eq!(
        kinds(&events),
        [Decode, Miss, Victim, Fill, Write, WriteThrough]
    );
    assert_eq!(event(&events, Write).changed, [word(2, 0, 1)]);
    assert_eq!(
        event(&events, WriteThrough).changed,
        [memory(0x0A4), counter(Counter::Writethroughs)]
    );
    // Lines are never dirty under write-through, so a victim is only evicted.
    let events = run.store(0x1A4, 9);
    assert_eq!(
        kinds(&events),
        [Decode, Miss, Victim, Evict, Fill, Write, WriteThrough]
    );
    assert!(!event(&events, Evict).changed.contains(&dirty(2, 0)));
    let counters = run.machine.counters;
    assert_eq!(
        (
            counters.misses,
            counters.fills,
            counters.evictions,
            counters.writebacks,
            counters.writethroughs,
            counters.writearounds
        ),
        (2, 2, 1, 0, 2, 0)
    );

    let mut run = Run::new(Config {
        cache: CacheConfig {
            write_policy: WritePolicy::WriteThrough,
            ..two_way(Replacement::Lru).cache
        },
        ..two_way(Replacement::Lru)
    });
    run.load(0x0A4);
    assert_eq!(
        kinds(&run.store(0x1A4, 9)),
        [Decode, Miss, Victim, Fill, Write, WriteThrough, Rank]
    );
}

#[test]
fn cases_9_and_10_a_store_miss_under_write_no_allocate_writes_around_the_cache() {
    for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
        let mut run = Run::new(direct_mapped(write_policy, AllocatePolicy::WriteNoAllocate));
        let before = run.machine.cache.clone();
        let events = run.store(0x0A4, 7);
        assert_eq!(kinds(&events), [Decode, Miss, MemWrite], "{write_policy:?}");
        assert_eq!(
            shape(event(&events, MemWrite)),
            (
                Some(addr(0x0A4)),
                Some(set(2)),
                None,
                &[memory(0x0A4), counter(Counter::Writearounds)][..],
                Some(Cause::AllocatePolicy)
            )
        );
        assert_eq!(run.machine.cache, before, "the cache is unchanged");
        assert_eq!(run.memory(0x0A4), 7);
        assert!(run.machine.memory.is_touched(line_addr(0x0A0)));
        let counters = run.machine.counters;
        assert_eq!(
            (
                counters.misses,
                counters.writearounds,
                counters.writethroughs,
                counters.writebacks,
                counters.fills
            ),
            (1, 1, 0, 0, 0),
            "{write_policy:?}"
        );
        // A write-around of the value memory holds changes no cell.
        assert_eq!(
            event(&run.store(0x0A8, 66), MemWrite).changed,
            [counter(Counter::Writearounds)]
        );
    }
}

// ---------------------------------------------------------------------------
// Allocate and write policies.
// ---------------------------------------------------------------------------

#[test]
fn write_no_allocate_changes_only_store_misses() {
    // A load miss still fills, and a store that hits still writes the cache,
    // as the write policy says.
    for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
        let mut run = Run::new(direct_mapped(write_policy, AllocatePolicy::WriteNoAllocate));
        assert_eq!(
            kinds(&run.load(0x0A4)),
            [Decode, Miss, Victim, Fill, Complete]
        );
        let expected: &[EventKind] = match write_policy {
            WritePolicy::WriteBack => &[Decode, Hit, Write],
            WritePolicy::WriteThrough => &[Decode, Hit, Write, WriteThrough],
        };
        assert_eq!(kinds(&run.store(0x0A4, 7)), expected);
        let counters = run.machine.counters;
        assert_eq!(counters.writearounds, 0, "{write_policy:?}");
    }
}

#[test]
fn write_through_and_write_around_counts_stay_apart() {
    // Under write-through and write-no-allocate, store hits count as
    // write-throughs and store misses as write-arounds, never both.
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteThrough,
        AllocatePolicy::WriteNoAllocate,
    ));
    run.store(0x0A4, 1); // miss: around
    run.load(0x0A4); // miss: fill
    run.store(0x0A4, 2); // hit: through
    run.store(0x0A8, 3); // hit: through
    run.store(0x1A4, 4); // miss: around
    let counters = run.machine.counters;
    assert_eq!(
        (
            counters.writethroughs,
            counters.writearounds,
            counters.writebacks
        ),
        (2, 2, 0)
    );
    // Under write-back and write-allocate neither counter moves.
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    ));
    for (index, at) in [0x0A4, 0x1A4, 0x0A4, 0x0A8].into_iter().enumerate() {
        run.store(at, index as u64);
    }
    let counters = run.machine.counters;
    assert_eq!(
        (
            counters.writethroughs,
            counters.writearounds,
            counters.writebacks
        ),
        (0, 0, 2)
    );
}

#[test]
fn a_no_allocate_store_then_a_load_of_its_line_both_miss() {
    // Nothing is filled by the store, so the load misses too.
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteBack,
        AllocatePolicy::WriteNoAllocate,
    ));
    run.store(0x0A4, 7);
    let events = run.load(0x0A4);
    assert_eq!(kinds(&events), [Decode, Miss, Victim, Fill, Complete]);
    assert_eq!(run.machine.registers.get(Register::R1), 7);
    assert_eq!(run.machine.counters.misses, 2);
    // The store filled no line, so both misses are compulsory.
    assert_eq!(three_c(&run), (2, 0, 0));
}

#[test]
fn a_store_keeps_the_low_bytes_of_its_value() {
    let cases = [
        (WordBytes::One, 0x1FF_u64, 0xFF_u64),
        (WordBytes::Two, 0x1_2345, 0x2345),
        (WordBytes::Four, u64::MAX, 0xFFFF_FFFF),
        (WordBytes::Eight, u64::MAX, u64::MAX),
    ];
    for (word_bytes, value, stored) in cases {
        let config = Config {
            word_bytes,
            ..direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate)
        };
        let mut run = Run::new(config);
        let at = 0x0A0 + word_bytes.bytes();
        run.store(at, value);
        assert_eq!(run.memory(at), stored, "{word_bytes:?}");
        run.events(load(at, Register::R4));
        assert_eq!(
            run.machine.registers.get(Register::R4),
            stored,
            "{word_bytes:?}"
        );
    }
}

#[test]
fn eight_byte_words_are_little_endian_in_the_line() {
    let config = Config {
        word_bytes: WordBytes::Eight,
        ..direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate)
    };
    let mut run = Run::new(config);
    let events = run.store(0x0A8, 0x0102_0304_0506_0708);
    // 16 B lines of 8-byte words: two words, and the store writes word 1.
    assert_eq!(
        event(&events, Fill).changed,
        [
            valid(2, 0),
            tag(2, 0),
            word(2, 0, 0),
            word(2, 0, 1),
            counter(Counter::Fills)
        ]
    );
    match &run.way(2, 0).slot {
        Slot::Valid { data, .. } => assert_eq!(&data[8..], [8, 7, 6, 5, 4, 3, 2, 1]),
        Slot::Invalid => panic!("the store filled way 0"),
    }
}

#[test]
fn a_load_into_r0_changes_no_register() {
    let mut run = Run::new(preset(Preset::DirectMapped8));
    let events = run.events(load(0x0A4, Register::R0));
    let complete = event(&events, Complete);
    assert!(complete.changed.is_empty());
    assert_eq!(
        complete.explain,
        "Read word 1 of way 0 (53); r0 always reads 0, so the value is dropped."
    );
    assert_eq!(run.machine.registers, Registers::default());
}

// ---------------------------------------------------------------------------
// Replacement.
// ---------------------------------------------------------------------------

/// The ways the misses of `addrs` evicted, in order, as `(way, reason)`.
fn victims(run: &mut Run, addrs: &[u32]) -> Vec<(u32, VictimReason)> {
    let mut chosen = Vec::new();
    for &at in addrs {
        let trace = run.trace(load(at, Register::R1));
        for (event, after) in &trace {
            if event.kind == Victim {
                let choice = after.access.as_ref().unwrap().victim.unwrap();
                chosen.push((choice.way.get(), choice.reason));
            }
        }
    }
    chosen
}

#[test]
fn lru_evicts_the_least_recently_used_way() {
    // Lines A, B and C all map to set 2 of the two-way cache.
    let (a, b, c) = (0x0A0, 0x1A0, 0x2A0);
    let mut run = Run::new(two_way(Replacement::Lru));
    let chosen = victims(&mut run, &[a, b, a, c, b]);
    assert_eq!(
        chosen,
        [
            (0, VictimReason::Empty),
            (1, VictimReason::Empty),
            (1, VictimReason::LeastRecentlyUsed { rank: 1 }),
            (0, VictimReason::LeastRecentlyUsed { rank: 1 }),
        ]
    );
}

#[test]
fn fifo_evicts_the_way_filled_earliest_whatever_hit_since() {
    let (a, b, c) = (0x0A0, 0x1A0, 0x2A0);
    let mut run = Run::new(two_way(Replacement::Fifo));
    run.load(a);
    run.load(b);
    // A hit on A, filled first, changes nothing: it is still first out.
    let hit = run.load(a);
    assert_eq!(
        kinds(&hit),
        [Decode, Hit, Complete],
        "FIFO does not rank hits"
    );
    let chosen = victims(&mut run, &[c, a]);
    assert_eq!(
        chosen,
        [
            (0, VictimReason::FirstIn { rank: 1 }),
            (1, VictimReason::FirstIn { rank: 1 }),
        ]
    );
}

#[test]
fn random_replacement_draws_from_the_seeded_generator_and_never_ranks() {
    // Eight lines of set 0 of a four-way cache, round and round: once the set
    // is full every miss draws, and the draws are the seed's stream.
    let config = config(
        128,
        4,
        Replacement::Random,
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    );
    let lines: Vec<u32> = (0..24).map(|i| (i % 8) * 0x40).collect();
    let mut run = Run::new(config.clone());
    let mut expected = Prng::new(config.seed);
    for &at in &lines {
        let trace = run.trace(load(at, Register::R1));
        assert!(trace.iter().all(|(event, _)| event.kind != Rank));
        if let Some((_, after)) = trace.iter().find(|(event, _)| event.kind == Victim) {
            let choice = after.access.as_ref().unwrap().victim.unwrap();
            if choice.reason == VictimReason::Random {
                assert_eq!(choice.way.get(), expected.draw_way(4));
                let victim = trace
                    .iter()
                    .find(|(event, _)| event.kind == Victim)
                    .unwrap();
                assert_eq!(victim.0.cause, Some(Cause::Replacement));
            }
        }
    }
    // The same seed gives the same run; the ranks never moved.
    let mut again = Run::new(config);
    for &at in &lines {
        again.load(at);
    }
    assert_eq!(again.machine, run.machine);
    let ranks: Vec<u32> = run
        .machine
        .cache
        .set(set(0))
        .unwrap()
        .ways()
        .iter()
        .map(|way| way.rank)
        .collect();
    assert_eq!(ranks, [0, 1, 2, 3]);
}

// ---------------------------------------------------------------------------
// Hit and miss sequences and miss counts.
// ---------------------------------------------------------------------------

/// Whether each access of `requests` hit.
fn outcomes(run: &mut Run, requests: &[Request]) -> Vec<bool> {
    requests
        .iter()
        .map(|&request| {
            let events = run.events(request);
            events[1].kind == Hit
        })
        .collect()
}

#[test]
fn a_direct_mapped_sequence_hits_within_a_line_and_misses_across_conflicts() {
    let mut run = Run::new(preset(Preset::DirectMapped8));
    let requests = [
        load(0x000, Register::R1),
        load(0x004, Register::R1), // same line
        load(0x010, Register::R1), // next line, next set
        store(0x000, 1),           // still there
        load(0x080, Register::R1), // set 0 again: evicts 0x000
        load(0x000, Register::R1), // and back
        load(0x01C, Register::R1), // 0x010's line is untouched
    ];
    assert_eq!(
        outcomes(&mut run, &requests),
        [false, true, false, true, false, false, true]
    );
    let counters = run.machine.counters;
    assert_eq!(
        (counters.accesses, counters.hits, counters.misses),
        (7, 3, 4)
    );
    // The dirty 0x000 was written back when 0x080 replaced it.
    assert_eq!(counters.writebacks, 1);
    assert_eq!(run.memory(0x000), 1);
    // Three lines are new; 0x000 came back while a cache of eight lines
    // would still hold it.
    assert_eq!(three_c(&run), (3, 0, 1));
}

#[test]
fn an_alternation_that_conflicts_direct_mapped_hits_in_two_ways() {
    // 0x000 and 0x080 share set 0: in the direct-mapped cache every access
    // misses (2 of the misses compulsory, 6 conflict); with two ways only
    // the first two miss.
    let requests: Vec<Request> = (0..8)
        .map(|i| load(if i % 2 == 0 { 0x000 } else { 0x080 }, Register::R1))
        .collect();
    let mut run = Run::new(preset(Preset::DirectMapped8));
    outcomes(&mut run, &requests);
    assert_eq!(
        (run.machine.counters.misses, run.machine.counters.hits),
        (8, 0)
    );
    assert_eq!(three_c(&run), (2, 0, 6));
    let mut run = Run::new(preset(Preset::TwoWayLru));
    outcomes(&mut run, &requests);
    assert_eq!(
        (run.machine.counters.misses, run.machine.counters.hits),
        (2, 6)
    );
    assert_eq!(three_c(&run), (2, 0, 0));
}

#[test]
fn a_loop_one_line_larger_than_a_fully_associative_lru_cache_always_misses() {
    // Four lines, fully associative, LRU: cycling through four lines misses
    // only the first time round; through five, it misses every time, the
    // capacity case.
    let config = config(
        64,
        4,
        Replacement::Lru,
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    );
    let rounds = |lines: u32| -> Vec<Request> {
        (0..3)
            .flat_map(|_| (0..lines).map(|line| load(line * 0x10, Register::R1)))
            .collect()
    };
    let mut run = Run::new(config.clone());
    outcomes(&mut run, &rounds(4));
    assert_eq!(
        (run.machine.counters.misses, run.machine.counters.hits),
        (4, 8)
    );
    assert_eq!(three_c(&run), (4, 0, 0));
    let mut run = Run::new(config);
    outcomes(&mut run, &rounds(5));
    assert_eq!(
        (run.machine.counters.misses, run.machine.counters.hits),
        (15, 0)
    );
    assert_eq!(run.machine.counters.evictions, 11);
    // After the first round every miss is one the cache's size causes.
    assert_eq!(three_c(&run), (5, 10, 0));
}

// ---------------------------------------------------------------------------
// The classes of misses.
// ---------------------------------------------------------------------------

/// The run's compulsory, capacity and conflict misses.
fn three_c(run: &Run) -> (u32, u32, u32) {
    let counters = run.machine.counters;
    (counters.compulsory, counters.capacity, counters.conflict)
}

/// The counters of the three classes of miss, in the order they are
/// declared.
const CLASSES: [Counter; 3] = [Counter::Compulsory, Counter::Capacity, Counter::Conflict];

/// The class of each access of `requests`, as the counter its `Miss` event
/// moved; `None` for a hit.
fn classes(run: &mut Run, requests: &[Request]) -> Vec<Option<Counter>> {
    requests
        .iter()
        .map(|&request| {
            let events = run.events(request);
            let moved: Vec<Counter> = events
                .iter()
                .flat_map(|event| &event.changed)
                .filter_map(|change| match change {
                    ChangeRef::Counter { counter } if CLASSES.contains(counter) => Some(*counter),
                    _ => None,
                })
                .collect();
            match events[1].kind {
                Hit => assert!(moved.is_empty(), "a hit has no class"),
                _ => assert_eq!(moved.len(), 1, "a miss has one class"),
            }
            moved.first().copied()
        })
        .collect()
}

const COMPULSORY: Option<Counter> = Some(Counter::Compulsory);
const CAPACITY: Option<Counter> = Some(Counter::Capacity);
const CONFLICT: Option<Counter> = Some(Counter::Conflict);

#[test]
fn a_class_moves_at_the_miss_alone() {
    // The class is known at the tag compare and moves there, with the miss;
    // no other event of the access changes it.
    let mut run = Run::new(preset(Preset::DirectMapped8));
    run.load(0x000);
    run.load(0x080);
    let trace = run.trace(load(0x000, Register::R1));
    let conflicts: Vec<(EventKind, u32)> = trace
        .iter()
        .map(|(event, after)| (event.kind, after.counters.conflict))
        .collect();
    assert_eq!(
        conflicts,
        [
            (Decode, 0),
            (Miss, 1),
            (Victim, 1),
            (Evict, 1),
            (Fill, 1),
            (Complete, 1)
        ]
    );
    assert!(trace[1].0.changed.contains(&counter(Counter::Conflict)));
}

#[test]
fn a_direct_mapped_sweep_of_twice_its_lines_misses_for_capacity() {
    // Sixteen lines through an eight-line cache, twice: the first round is
    // compulsory, and in the second each line has been out of a fully
    // associative cache of eight lines for eight other lines.
    let mut run = Run::new(preset(Preset::DirectMapped8));
    let sweep: Vec<Request> = (0..2)
        .flat_map(|_| (0..16).map(|line| load(line * 0x10, Register::R1)))
        .collect();
    let classes = classes(&mut run, &sweep);
    assert_eq!(classes[..16], [COMPULSORY; 16]);
    assert_eq!(classes[16..], [CAPACITY; 16]);
    assert_eq!(three_c(&run), (16, 16, 0));
}

#[test]
fn one_run_can_have_all_three_classes() {
    // 0x000 and 0x080 share set 0 of direct-mapped-8, so 0x000's return is
    // a conflict. Eight new lines then push it out of a cache of eight
    // lines of any shape, so its next return is for capacity.
    let mut run = Run::new(preset(Preset::DirectMapped8));
    let mut requests = vec![
        load(0x000, Register::R1),
        load(0x080, Register::R1),
        load(0x000, Register::R1),
    ];
    requests.extend((0..8).map(|line| load(0x100 + line * 0x10, Register::R1)));
    requests.push(load(0x000, Register::R1));
    let mut expected = vec![COMPULSORY, COMPULSORY, CONFLICT];
    expected.extend([COMPULSORY; 8]);
    expected.push(CAPACITY);
    assert_eq!(classes(&mut run, &requests), expected);
    assert_eq!(three_c(&run), (10, 1, 1));
}

#[test]
fn a_store_miss_under_write_no_allocate_fills_nothing_so_the_next_miss_on_its_line_is_compulsory() {
    // The same three accesses under each allocate policy. A store that
    // allocates fills its line, so when a conflict has pushed it out, the
    // load's miss is a conflict; a store that does not allocate fills
    // nothing, not even the shadow, so the load is the line's first fill.
    let requests = [
        store(0x0A4, 7),
        load(0x1A4, Register::R1),
        load(0x0A4, Register::R1),
    ];
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    ));
    assert_eq!(
        classes(&mut run, &requests),
        [COMPULSORY, COMPULSORY, CONFLICT]
    );
    for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
        let mut run = Run::new(direct_mapped(write_policy, AllocatePolicy::WriteNoAllocate));
        assert_eq!(classes(&mut run, &requests[..1]), [COMPULSORY]);
        assert!(run.machine.filled.is_empty(), "{write_policy:?}");
        assert!(run.machine.shadow.lines().is_empty(), "{write_policy:?}");
        assert_eq!(
            classes(&mut run, &requests[1..]),
            [COMPULSORY, COMPULSORY],
            "{write_policy:?}"
        );
    }
}

#[test]
fn a_store_miss_under_write_no_allocate_is_classed_like_any_other_miss() {
    // On a line filled before, it is a conflict if a fully associative
    // cache would still hold the line. It allocates nowhere, so the load
    // after it misses too, and is a conflict as well, not a compulsory
    // miss, since the line was filled before...
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteBack,
        AllocatePolicy::WriteNoAllocate,
    ));
    let requests = [
        load(0x0A4, Register::R1),
        load(0x1A4, Register::R1),
        store(0x0A4, 7),
        load(0x0A4, Register::R1),
    ];
    assert_eq!(
        classes(&mut run, &requests),
        [COMPULSORY, COMPULSORY, CONFLICT, CONFLICT]
    );
    // ...and for capacity if it would not. Five lines through a fully
    // associative cache of four push out the first; the store to it misses
    // for capacity and allocates nowhere, so the load after it misses for
    // capacity too.
    let mut run = Run::new(config(
        64,
        4,
        Replacement::Lru,
        WritePolicy::WriteBack,
        AllocatePolicy::WriteNoAllocate,
    ));
    let mut requests: Vec<Request> = (0..5).map(|line| load(line * 0x10, Register::R1)).collect();
    requests.extend([store(0x000, 7), load(0x000, Register::R1)]);
    let mut expected = vec![COMPULSORY; 5];
    expected.extend([CAPACITY, CAPACITY]);
    assert_eq!(classes(&mut run, &requests), expected);
}

#[test]
fn the_shadow_brings_in_a_line_the_cache_hit_on() {
    // A direct-mapped cache of four lines keeps 0x000 in set 0 while four
    // lines of other sets pass, and a fully associative LRU cache of four
    // lines loses it. The load of 0x000 then hits in the cache and misses
    // in the shadow, which brings the line in as its own miss would. So
    // when 0x040 has evicted it from set 0, its return is a conflict: a
    // fully associative cache of four lines would have held it.
    let mut run = Run::new(config(
        64,
        1,
        Replacement::Lru,
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    ));
    let requests: Vec<Request> = [0x000, 0x010, 0x020, 0x030, 0x050, 0x000, 0x040, 0x000]
        .into_iter()
        .map(|at| load(at, Register::R1))
        .collect();
    assert_eq!(
        classes(&mut run, &requests[..6]),
        [COMPULSORY, COMPULSORY, COMPULSORY, COMPULSORY, COMPULSORY, None]
    );
    assert_eq!(
        run.machine.shadow.lines(),
        [
            line_addr(0x000),
            line_addr(0x050),
            line_addr(0x030),
            line_addr(0x020)
        ]
    );
    assert_eq!(classes(&mut run, &requests[6..]), [COMPULSORY, CONFLICT]);
}

#[test]
fn a_fifo_miss_that_lru_would_have_avoided_is_a_conflict() {
    // In a fully associative cache of two lines under FIFO, a hit on 0x000
    // does not save it: 0x020 evicts it, filled earliest, where LRU would
    // have evicted 0x010. Its return is therefore a conflict, a miss the
    // policy caused rather than the cache's size.
    let requests: Vec<Request> = [0x000, 0x010, 0x000, 0x020, 0x000]
        .into_iter()
        .map(|at| load(at, Register::R1))
        .collect();
    let two_lines = |replacement| {
        config(
            32,
            2,
            replacement,
            WritePolicy::WriteBack,
            AllocatePolicy::WriteAllocate,
        )
    };
    let mut run = Run::new(two_lines(Replacement::Fifo));
    assert_eq!(
        classes(&mut run, &requests),
        [COMPULSORY, COMPULSORY, None, COMPULSORY, CONFLICT]
    );
    let mut run = Run::new(two_lines(Replacement::Lru));
    assert_eq!(
        classes(&mut run, &requests),
        [COMPULSORY, COMPULSORY, None, COMPULSORY, None]
    );
}

/// A fully associative LRU cache of `capacity` lines kept beside a run, by
/// last use, to check the shadow against: `line` → the access that last
/// used it.
struct Beside {
    capacity: usize,
    lines: std::collections::BTreeMap<u32, usize>,
}

impl Beside {
    /// Whether it holds `line`; either way the access uses the line, and a
    /// miss that `allocate`s brings it in, evicting the line used least
    /// recently when it is full.
    fn access(&mut self, line: u32, allocate: bool, now: usize) -> bool {
        let held = self.lines.contains_key(&line);
        if held || allocate {
            if !held && self.lines.len() == self.capacity {
                let oldest = self
                    .lines
                    .iter()
                    .min_by_key(|(_, used)| **used)
                    .map(|(line, _)| *line)
                    .unwrap();
                self.lines.remove(&oldest);
            }
            self.lines.insert(line, now);
        }
        held
    }
}

#[test]
fn every_miss_is_classed_by_the_lines_filled_and_a_fully_associative_cache_beside_the_run() {
    // Every placement, policy and allocate policy, over streams with hits,
    // conflicts and evictions: a miss is compulsory exactly when no `Fill`
    // has brought its line in before, and otherwise a conflict exactly when
    // a fully associative LRU cache of as many lines, fed the same accesses
    // and allocating as the cache does, holds the line.
    let mut ways = 1;
    while ways <= 8 {
        for replacement in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
            for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
                for allocate_policy in [
                    AllocatePolicy::WriteAllocate,
                    AllocatePolicy::WriteNoAllocate,
                ] {
                    let config = config(128, ways, replacement, write_policy, allocate_policy);
                    let geometry = config.validate().unwrap();
                    let mut run = Run::new(config);
                    let mut filled = std::collections::BTreeSet::new();
                    let mut beside = Beside {
                        capacity: geometry.lines() as usize,
                        lines: std::collections::BTreeMap::new(),
                    };
                    let mut seen = [0; 3];
                    for (now, request) in request_stream(&geometry, u64::from(ways) + 7, 200)
                        .into_iter()
                        .enumerate()
                    {
                        let line = geometry.locate(request.addr()).line.get();
                        let allocate = matches!(request, Request::Load { .. })
                            || allocate_policy == AllocatePolicy::WriteAllocate;
                        let held = beside.access(line, allocate, now);
                        let events = run.events(request);
                        let class = events
                            .iter()
                            .filter(|event| event.kind == Miss)
                            .flat_map(|event| &event.changed)
                            .find_map(|change| match change {
                                ChangeRef::Counter { counter } if CLASSES.contains(counter) => {
                                    Some(*counter)
                                }
                                _ => None,
                            });
                        let expected = (events[1].kind == Miss).then(|| {
                            if !filled.contains(&line) {
                                Counter::Compulsory
                            } else if held {
                                Counter::Conflict
                            } else {
                                Counter::Capacity
                            }
                        });
                        assert_eq!(class, expected, "{request:?}");
                        if let Some(class) = class {
                            seen[CLASSES.iter().position(|c| *c == class).unwrap()] += 1;
                        }
                        if events.iter().any(|event| event.kind == Fill) {
                            filled.insert(line);
                        }
                    }
                    let shadow: Vec<u32> = run
                        .machine
                        .shadow
                        .lines()
                        .iter()
                        .map(|line| line.get())
                        .collect();
                    let mut by_use: Vec<(usize, u32)> = beside
                        .lines
                        .iter()
                        .map(|(line, used)| (*used, *line))
                        .collect();
                    by_use.sort_unstable_by(|a, b| b.cmp(a));
                    let expected: Vec<u32> = by_use.into_iter().map(|(_, line)| line).collect();
                    assert_eq!(shadow, expected);
                    // Each stream has misses of the first two classes; the
                    // direct-mapped and two-way caches have conflicts too.
                    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
                    if ways <= 2 {
                        assert!(seen[2] > 0, "{ways}-way: {seen:?}");
                    }
                }
            }
        }
        ways *= 2;
    }
}

#[test]
fn a_line_an_edit_takes_out_is_compulsory_again() {
    // An edit that invalidates a line takes it out of the filled lines and
    // the shadow, so its next miss is compulsory rather than a conflict the
    // cache did not cause.
    let mut run = Run::new(preset(Preset::DirectMapped8));
    run.load(0x0A4);
    let invalidate = way_edit(
        2,
        0,
        WayPatch {
            valid: Some(false),
            ..WayPatch::default()
        },
    );
    assert_eq!(apply_op(&mut run.machine, &invalidate, 0).0, Ok(()));
    assert_eq!(
        classes(&mut run, &[load(0x0A4, Register::R1)]),
        [COMPULSORY]
    );
}

// ---------------------------------------------------------------------------
// The sparkline's outcomes.
// ---------------------------------------------------------------------------

#[test]
fn an_outcome_joins_the_sparkline_at_the_tag_compare() {
    use crate::stats::Outcome;
    let mut run = Run::new(preset(Preset::DirectMapped8));
    let outcomes = |machine: &Machine| -> Vec<Outcome> { machine.recent.outcomes().collect() };
    let trace = run.trace(load(0x0A4, Register::R1));
    // Not yet at the decode, as the access is not yet counted.
    assert!(outcomes(&trace[0].1).is_empty());
    for (_, after) in &trace[1..] {
        assert_eq!(outcomes(after), [Outcome::Miss]);
    }
    run.load(0x0A8);
    assert_eq!(outcomes(&run.machine), [Outcome::Miss, Outcome::Hit]);
    // The sparkline keeps the latest RECENT_OUTCOMES, oldest first.
    for _ in 0..crate::limits::RECENT_OUTCOMES {
        run.load(0x0AC);
    }
    let latest = outcomes(&run.machine);
    assert_eq!(latest.len(), crate::limits::RECENT_OUTCOMES as usize);
    assert!(latest.iter().all(|outcome| *outcome == Outcome::Hit));
}

#[test]
fn the_stats_of_a_run_add_up_its_counters() {
    use crate::stats::{Outcome, StatKey, Stats, Traffic};
    // Write-through and write-no-allocate: a write-around, a fill, a
    // write-through, a clean eviction and a conflict.
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteThrough,
        AllocatePolicy::WriteNoAllocate,
    ));
    run.store(0x0A4, 1);
    run.load(0x0A4);
    run.store(0x0A8, 2);
    run.load(0x1A4);
    let last = run.load(0x0A4);
    let mut highlight: Vec<ChangeRef> = last.iter().flat_map(|e| e.changed.clone()).collect();
    highlight.sort_unstable();
    highlight.dedup();

    let stats = Stats::new(&run.machine, true, &highlight);
    let counters = stats.counters;
    assert_eq!(
        (counters.accesses, counters.hits, counters.misses),
        (5, 1, 4)
    );
    assert_eq!(three_c(&run), (3, 0, 1));
    assert_eq!(
        stats.traffic,
        Traffic {
            line_reads: 3,
            line_writes: 0,
            word_writes: 2,
            bytes_read: 3 * 16,
            bytes_written: 2 * 4,
        }
    );
    let rows: Vec<(StatKey, &str, bool)> = stats
        .rows
        .iter()
        .map(|row| (row.key, row.text.as_str(), row.changed))
        .collect();
    assert_eq!(
        rows,
        [
            (StatKey::Accesses, "5", true),
            (StatKey::Hits, "1", false),
            (StatKey::Misses, "4", true),
            (StatKey::HitRate, "20%", false),
            (StatKey::Evictions, "2", true),
            (StatKey::Writebacks, "0", false),
            (StatKey::Writethroughs, "1", false),
            (StatKey::Writearounds, "1", false),
            (StatKey::Compulsory, "3", false),
            (StatKey::Capacity, "0", false),
            (StatKey::Conflict, "1", true),
        ]
    );
    assert_eq!(
        stats.recent,
        [
            Outcome::Miss,
            Outcome::Miss,
            Outcome::Hit,
            Outcome::Miss,
            Outcome::Miss
        ]
    );
    assert_eq!(stats.recent_text.as_deref(), Some("1 hit, 4 misses"));
    assert_eq!(stats.amat, None);
}

// ---------------------------------------------------------------------------
// Staleness.
// ---------------------------------------------------------------------------

#[test]
fn a_word_is_stale_only_while_a_dirty_way_holds_a_different_value() {
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    ));
    assert_eq!(run.machine.stale(addr(0x0A4)), None, "nothing cached");
    run.load(0x0A4);
    assert_eq!(run.machine.stale(addr(0x0A4)), None, "clean");
    // Storing what memory holds dirties the line but leaves nothing stale.
    run.store(0x0A4, 53);
    assert!(run.way(2, 0).is_dirty());
    assert_eq!(run.machine.stale(addr(0x0A4)), None);
    run.store(0x0A4, 7);
    let stale = Some(StaleInfo {
        set: set(2),
        way: way(0),
        cached: Word::new_unchecked(7),
        memory: Word::new_unchecked(53),
    });
    assert_eq!(run.machine.stale(addr(0x0A4)), stale);
    // Any byte of the word finds it; the line's other words are not stale.
    assert_eq!(run.machine.stale(addr(0x0A6)), stale);
    assert_eq!(run.machine.stale(addr(0x0A8)), None);
    assert_eq!(run.machine.stale(addr(0x1A4)), None, "another line");
    // Storing memory's value back makes it current again.
    run.store(0x0A4, 53);
    assert_eq!(run.machine.stale(addr(0x0A4)), None);
    // Evicting the line writes it back.
    run.store(0x0A4, 9);
    run.load(0x1A4);
    assert_eq!(run.machine.stale(addr(0x0A4)), None);
    assert_eq!(run.memory(0x0A4), 9);
}

#[test]
fn nothing_is_stale_under_write_through() {
    let mut run = Run::new(direct_mapped(
        WritePolicy::WriteThrough,
        AllocatePolicy::WriteAllocate,
    ));
    for (at, value) in [(0x0A4, 7), (0x0A8, 8), (0x1A4, 9)] {
        run.store(at, value);
        assert_eq!(run.machine.stale(addr(at)), None);
    }
}

// ---------------------------------------------------------------------------
// Numbering, the access in flight, and stopping.
// ---------------------------------------------------------------------------

#[test]
fn events_are_numbered_by_log_position_entry_and_step() {
    let mut run = Run::new(preset(Preset::TwoWayLru));
    let first = run.load(0x0A4);
    let second = run.store(0x1A4, 5);
    let ids: Vec<u64> = first.iter().chain(&second).map(|event| event.id).collect();
    assert_eq!(ids, (0..ids.len() as u64).collect::<Vec<_>>());
    for (entry, events) in [(1, &first), (2, &second)] {
        for (seq, event) in (1..).zip(events.iter()) {
            assert_eq!(event.op_index, OpIndex::new_unchecked(entry - 1));
            assert_eq!(event.run, 1);
            assert_eq!(event.entry, Some(entry));
            assert_eq!(event.seq, seq);
            assert_eq!(event.label, format!("{entry}.{seq}"));
            assert_eq!(event.cycle, None);
        }
    }
    assert_eq!(run.machine.entries_in_run, 2);
}

#[test]
fn the_access_context_follows_the_access() {
    let mut run = Run::new(preset(Preset::TwoWayLru));
    run.load(0x0A4);
    run.store(0x1A4, 5);
    run.load(0x0A4);
    let trace = run.trace(load(0x2A4, Register::R3));
    let contexts: Vec<&crate::machine::AccessContext> = trace
        .iter()
        .map(|(_, after)| after.access.as_ref().unwrap())
        .collect();
    for context in &contexts {
        assert_eq!(context.request, load(0x2A4, Register::R3));
        assert_eq!(context.location, run.machine.geometry.locate(addr(0x2A4)));
    }
    // Decode: nothing looked up yet.
    assert_eq!(contexts[0].lookup, None);
    // Miss: the tags compared, before the access changes them.
    let lookup = contexts[1].lookup.as_ref().unwrap();
    assert_eq!(lookup.hit, None);
    assert_eq!(
        lookup.tags,
        [
            Some(Tag::new_unchecked(0x02)),
            Some(Tag::new_unchecked(0x06))
        ]
    );
    assert_eq!(contexts.last().unwrap().lookup.as_ref(), Some(lookup));
    // The victim from Victim until Fill; the way from Fill on.
    let victims: Vec<Option<u32>> = contexts
        .iter()
        .map(|context| context.victim.map(|choice| choice.way.get()))
        .collect();
    assert_eq!(
        victims,
        [None, None, Some(1), Some(1), Some(1), None, None, None]
    );
    let ways: Vec<Option<WayIndex>> = contexts.iter().map(|context| context.way).collect();
    assert_eq!(ways[..5], [None; 5]);
    assert_eq!(ways[5..], [Some(way(1)); 3]);
    // In flight until the last event.
    let done: Vec<bool> = contexts.iter().map(|context| context.done).collect();
    assert_eq!(
        done,
        [false, false, false, false, false, false, false, true]
    );

    // A hit knows its way from the compare on.
    let trace = run.trace(load(0x0A4, Register::R1));
    let context = trace[1].1.access.as_ref().unwrap();
    assert_eq!(context.lookup.as_ref().unwrap().hit, Some(way(0)));
    assert_eq!(context.way, Some(way(0)));
}

#[test]
fn the_last_event_ends_the_access_whichever_it_is() {
    // Complete, Write, WriteThrough, MemWrite and Rank can each be last.
    let cases: [(Config, Vec<Request>, EventKind); 5] = [
        (
            preset(Preset::DirectMapped8),
            vec![load(0x0A4, Register::R1)],
            Complete,
        ),
        (
            direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate),
            vec![store(0x0A4, 1)],
            Write,
        ),
        (
            direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate),
            vec![store(0x0A4, 1)],
            WriteThrough,
        ),
        (
            direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteNoAllocate),
            vec![store(0x0A4, 1)],
            MemWrite,
        ),
        (
            preset(Preset::TwoWayLru),
            vec![load(0x0A4, Register::R1), load(0x1A4, Register::R1)],
            Rank,
        ),
    ];
    for (config, requests, last) in cases {
        let mut run = Run::new(config);
        let mut trace = Vec::new();
        for request in requests {
            trace = run.trace(request);
        }
        let (event, after) = trace.last().unwrap();
        assert_eq!(event.kind, last);
        assert!(!after.in_flight());
        assert!(trace[..trace.len() - 1]
            .iter()
            .all(|(_, after)| after.in_flight()));
    }
}

#[test]
fn stopping_after_any_event_leaves_the_machine_as_the_full_run_has_it_there() {
    for timing in [None, Some(timing(1, 20))] {
        stop_after_each_event_of_frame_3b_access_5(Config {
            timing,
            ..preset(Preset::TwoWayLru)
        });
    }
}

/// Stops frame 3b's access #5 after each of its events in turn, with a
/// recording and a quiet sink, on `config`.
fn stop_after_each_event_of_frame_3b_access_5(config: Config) {
    let mut run = Run::new(config);
    run.load(0x0A4);
    run.store(0x1A4, 5);
    run.load(0x0A4);
    let base = run.machine.clone();
    let full = run.trace(load(0x2A4, Register::R3));
    for stop in 1..=full.len() {
        let mut machine = base.clone();
        let mut recorder = Recorder {
            stop_after: Some(stop),
            ..Recorder::default()
        };
        let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(3), 0);
        assert_eq!(
            access(&mut machine, load(0x2A4, Register::R3), &mut out),
            Flow::Stop
        );
        assert_eq!(recorder.events.len(), stop);
        assert_eq!(machine, full[stop - 1].1, "stopped after {stop}");

        // A quiet sink stops in the same state.
        let mut quiet_machine = base.clone();
        let mut quiet = Quiet {
            stop_after: Some(stop),
            ..Quiet::default()
        };
        let mut out = Emitter::new(&mut quiet, OpIndex::new_unchecked(3), 0);
        access(&mut quiet_machine, load(0x2A4, Register::R3), &mut out);
        assert_eq!(quiet_machine, machine);
    }
}

#[test]
fn quiet_and_recording_runs_reach_equal_machines() {
    for timing in [None, Some(timing(1, 20))] {
        let config = Config {
            timing,
            ..config(
                128,
                2,
                Replacement::Random,
                WritePolicy::WriteBack,
                AllocatePolicy::WriteAllocate,
            )
        };
        let requests = request_stream(&config.validate().unwrap(), 7, 200);
        let mut recording = Machine::start(config.clone()).unwrap();
        let mut quiet_machine = Machine::start(config).unwrap();
        let mut quiet = Quiet::default();
        let mut recorder = Recorder::default();
        for request in requests {
            let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(0), 0);
            access(&mut recording, request, &mut out);
            let mut out = Emitter::new(&mut quiet, OpIndex::new_unchecked(0), 0);
            access(&mut quiet_machine, request, &mut out);
        }
        assert_eq!(quiet_machine, recording);
        assert_eq!(quiet.seen, recorder.events.len());
    }
}

// ---------------------------------------------------------------------------
// Invariants at entry ends.
// ---------------------------------------------------------------------------

#[test]
fn i2_is_checked_between_accesses_only() {
    // A full four-way LRU set whose way 3 and then way 0 are invalidated by
    // edits, each sent to rank last; the next miss fills way 0, which ranks
    // behind invalid way 3 until the Rank event moves it to the front.
    let config = config(
        64,
        4,
        Replacement::Lru,
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    );
    let mut run = Run::new(config);
    for at in [0x000, 0x010, 0x020, 0x030] {
        run.load(at);
    }
    for index in [3, 0] {
        let invalidate = way_edit(
            0,
            index,
            WayPatch {
                valid: Some(false),
                ..WayPatch::default()
            },
        );
        assert_eq!(apply_op(&mut run.machine, &invalidate, 0).0, Ok(()));
    }
    let ranks: Vec<u32> = run
        .machine
        .cache
        .set(set(0))
        .unwrap()
        .ways()
        .iter()
        .map(|way| way.rank)
        .collect();
    assert_eq!(ranks, [3, 1, 0, 2]);
    assert_eq!(run.machine.check_invariants(), Ok(()));

    let trace = run.trace(load(0x040, Register::R1));
    let kinds: Vec<EventKind> = trace.iter().map(|(event, _)| event.kind).collect();
    assert_eq!(kinds, [Decode, Miss, Victim, Fill, Complete, Rank]);
    let (_, filled) = &trace[3];
    assert!(filled.in_flight());
    assert_eq!(filled.check_invariants(), Ok(()));
    let mut boundary = filled.clone();
    boundary.access.as_mut().unwrap().done = true;
    assert_eq!(
        boundary.check_invariants(),
        Err(InvariantViolation::ValidRankedAfterInvalid { set: set(0) })
    );
    let (_, ranked) = trace.last().unwrap();
    assert!(!ranked.in_flight());
    assert_eq!(ranked.check_invariants(), Ok(()));
}

#[test]
fn a_broken_invariant_at_an_entry_end_ends_the_op_with_an_error() {
    let mut machine = Machine::start(preset(Preset::TwoWayLru)).unwrap();
    // Two ways of set 1 claim the same line.
    for index in [0, 1] {
        machine.cache.way_mut(set(1), way(index)).unwrap().slot = Slot::Valid {
            tag: Tag::new_unchecked(1),
            dirty: Dirtiness::Clean,
            data: vec![0; 16],
        };
    }
    let mut recorder = Recorder::default();
    let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(0), 10);
    assert_eq!(
        access(&mut machine, load(0x0A4, Register::R1), &mut out),
        Flow::Stop
    );
    let events = recorder.events;
    assert_eq!(
        kinds(&events),
        [Decode, Miss, Victim, Fill, Complete, EventKind::Error]
    );
    let error = events.last().unwrap();
    assert_eq!((error.id, error.entry, error.seq), (15, Some(2), 1));
    assert_eq!(error.label, "2.1");
    assert_eq!((error.addr, error.set, error.way), (None, None, None));
    assert!(error.changed.is_empty());
    assert_eq!(error.cause, None);
    assert_eq!(
        error.explain,
        "Internal error: invariant I3 (no two valid ways in set 1 hold the same tag) no longer \
         holds. The run stopped here; please report it with a share link."
    );
    assert_eq!(machine.access, None);
    assert_eq!(machine.entries_in_run, 2);
}

#[test]
fn a_lookup_that_finds_no_way_abandons_the_access_with_an_error() {
    // The cache of a 64 B two-way geometry has only sets 0 and 1, so the
    // lookup of 0x0A4 finds no set 2 where the configuration promises one.
    let smaller = config(
        64,
        2,
        Replacement::Lru,
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    )
    .validate()
    .unwrap();
    let start = || {
        let mut machine = Machine::start(two_way(Replacement::Lru)).unwrap();
        machine.cache = Cache::new(&smaller);
        machine
    };
    let rule = |invariant: &str| {
        format!(
            "Internal error: invariant {invariant} no longer holds. The run stopped here; please \
             report it with a share link."
        )
    };
    let cases = [
        // Set 2 is missing, so it has no ranks to permute: I1.
        (
            start(),
            rule("I1 (the ranks in set 2 are a permutation of 0 to 1)"),
        ),
        // An earlier set's broken invariant is named instead.
        (
            {
                let mut machine = start();
                for index in [0, 1] {
                    machine.cache.way_mut(set(0), way(index)).unwrap().slot = Slot::Valid {
                        tag: Tag::new_unchecked(1),
                        dirty: Dirtiness::Clean,
                        data: vec![0; 16],
                    };
                }
                machine
            },
            rule("I3 (no two valid ways in set 0 hold the same tag)"),
        ),
    ];
    for (mut machine, explain) in cases {
        let before = machine.clone();
        let mut recorder = Recorder::default();
        let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(0), 10);
        assert_eq!(
            access(&mut machine, load(0x0A4, Register::R1), &mut out),
            Flow::Stop
        );
        let events = recorder.events;
        assert_eq!(kinds(&events), [Decode, EventKind::Error]);
        // The access's entry ends at its Decode; the error is an entry of
        // its own.
        assert_eq!(events[0].label, "1.1");
        let error = &events[1];
        assert_eq!((error.id, error.entry, error.seq), (11, Some(2), 1));
        assert_eq!(error.label, "2.1");
        assert_eq!((error.addr, error.set, error.way), (None, None, None));
        assert!(error.changed.is_empty());
        assert_eq!(error.cause, None);
        assert_eq!(error.explain, explain);
        // The access is abandoned before its compare: nothing counted,
        // nothing cached.
        assert_eq!(machine.access, None);
        assert_eq!(machine.entries_in_run, 2);
        assert_eq!(machine.counters, before.counters);
        assert_eq!(machine.cache, before.cache);
    }
}

// ---------------------------------------------------------------------------
// Every case, every policy: changed lists against the state.
// ---------------------------------------------------------------------------

/// A pseudo-random stream of `count` loads and stores over three times as
/// many lines as the cache holds, so hits, conflicts and dirty evictions all
/// happen.
fn request_stream(geometry: &Geometry, seed: u64, count: usize) -> Vec<Request> {
    let mut stream = Prng::new(seed);
    let lines = geometry.lines() * 3;
    let registers = [Register::R0, Register::R1, Register::R2, Register::R7];
    (0..count)
        .map(|_| {
            let line = stream.draw_way(1 << 16) % lines;
            let word = stream.draw_way(geometry.words_per_line());
            let at = line * geometry.line_bytes() + word * geometry.word_bytes();
            if stream.draw_way(2) == 0 {
                let register = registers[stream.draw_way(4) as usize];
                load(at, register)
            } else {
                // Small values, so a store often writes what is there.
                store(at, u64::from(stream.draw_way(4)))
            }
        })
        .collect()
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
        if before.registers.get(register) != after.registers.get(register) {
            changed.push(self::register(register));
        }
    }
    if before.cpu.pc != after.cpu.pc {
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

/// The 23 event sequences of the ten access cases, untimed, once each
/// optional event is counted.
fn case_sequences() -> Vec<Vec<EventKind>> {
    vec![
        // 1: load hit.
        vec![Decode, Hit, Complete],
        vec![Decode, Hit, Complete, Rank],
        // 2: load miss, invalid way.
        vec![Decode, Miss, Victim, Fill, Complete],
        vec![Decode, Miss, Victim, Fill, Complete, Rank],
        // 3: load miss, clean victim.
        vec![Decode, Miss, Victim, Evict, Fill, Complete],
        vec![Decode, Miss, Victim, Evict, Fill, Complete, Rank],
        // 4: load miss, dirty victim.
        vec![Decode, Miss, Victim, WriteBack, Evict, Fill, Complete],
        vec![Decode, Miss, Victim, WriteBack, Evict, Fill, Complete, Rank],
        // 5: store hit, write-back.
        vec![Decode, Hit, Write],
        vec![Decode, Hit, Write, Rank],
        // 6: store hit, write-through.
        vec![Decode, Hit, Write, WriteThrough],
        vec![Decode, Hit, Write, WriteThrough, Rank],
        // 7: store miss, write-allocate and write-back: an invalid way, a
        // clean victim or a dirty one.
        vec![Decode, Miss, Victim, Fill, Write],
        vec![Decode, Miss, Victim, Fill, Write, Rank],
        vec![Decode, Miss, Victim, Evict, Fill, Write],
        vec![Decode, Miss, Victim, Evict, Fill, Write, Rank],
        vec![Decode, Miss, Victim, WriteBack, Evict, Fill, Write],
        vec![Decode, Miss, Victim, WriteBack, Evict, Fill, Write, Rank],
        // 8: store miss, write-allocate and write-through: an invalid way or
        // a clean victim, since no line is dirty.
        vec![Decode, Miss, Victim, Fill, Write, WriteThrough],
        vec![Decode, Miss, Victim, Fill, Write, WriteThrough, Rank],
        vec![Decode, Miss, Victim, Evict, Fill, Write, WriteThrough],
        vec![Decode, Miss, Victim, Evict, Fill, Write, WriteThrough, Rank],
        // 9 and 10: store miss, write-no-allocate.
        vec![Decode, Miss, MemWrite],
    ]
}

/// `sequences` in timed mode: each of the 17 that miss has a `Stall`, after
/// `Victim` when the miss allocates and after `Miss` when it writes around
/// the cache.
fn with_stalls(sequences: &[Vec<EventKind>]) -> Vec<Vec<EventKind>> {
    let timed: Vec<Vec<EventKind>> = sequences
        .iter()
        .map(|sequence| {
            let after = if sequence.contains(&Victim) {
                Victim
            } else {
                Miss
            };
            let mut timed = sequence.clone();
            if let Some(at) = sequence.iter().position(|kind| *kind == after) {
                timed.insert(at + 1, Stall);
            }
            timed
        })
        .collect();
    assert_eq!(
        timed
            .iter()
            .filter(|sequence| sequence.contains(&Stall))
            .count(),
        17
    );
    timed
}

#[test]
fn every_event_lists_exactly_the_cells_and_counters_it_changed() {
    // The sequences seen, untimed and timed.
    let mut checked = [
        std::collections::BTreeSet::new(),
        std::collections::BTreeSet::new(),
    ];
    let mut ways = 1;
    while ways <= 4 {
        for replacement in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
            for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
                for allocate_policy in [
                    AllocatePolicy::WriteAllocate,
                    AllocatePolicy::WriteNoAllocate,
                ] {
                    for (timed, timing) in [(0, None), (1, Some(timing(2, 5)))] {
                        let config = Config {
                            timing,
                            ..config(64, ways, replacement, write_policy, allocate_policy)
                        };
                        let geometry = config.validate().unwrap();
                        let mut run = Run::new(config);
                        for request in request_stream(&geometry, u64::from(ways), 150) {
                            let mut before = run.machine.clone();
                            let trace = run.trace(request);
                            let sequence: Vec<EventKind> =
                                trace.iter().map(|(event, _)| event.kind).collect();
                            checked[timed].insert(format!("{sequence:?}"));
                            for (event, after) in &trace {
                                assert_eq!(
                                    event.changed,
                                    diff(&before, after),
                                    "{} {:?}",
                                    event.label,
                                    event.kind
                                );
                                before = after.clone();
                            }
                        }
                    }
                }
            }
        }
        ways *= 2;
    }
    // The ten cases have 23 sequences once each optional event is counted,
    // and the untimed streams produced every one of them and nothing else;
    // so did the timed streams, with their stalls.
    let untimed = case_sequences();
    let timed = with_stalls(&untimed);
    for (index, expected) in [untimed, timed].iter().enumerate() {
        let expected: std::collections::BTreeSet<String> = expected
            .iter()
            .map(|sequence| format!("{sequence:?}"))
            .collect();
        assert_eq!(expected.len(), 23);
        let missing: Vec<&String> = expected.difference(&checked[index]).collect();
        let unexpected: Vec<&String> = checked[index].difference(&expected).collect();
        assert!(
            missing.is_empty() && unexpected.is_empty(),
            "never came up: {missing:?}; not a case's sequence: {unexpected:?}"
        );
    }
}

#[test]
fn every_placement_up_to_the_largest_keeps_its_invariants() {
    // Every associativity up to a fully associative cache of `MAX_LINES`
    // lines, under every policy, at the widest addresses.
    let mut ways = 1;
    while ways <= MAX_LINES {
        for replacement in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
            for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
                for allocate_policy in [
                    AllocatePolicy::WriteAllocate,
                    AllocatePolicy::WriteNoAllocate,
                ] {
                    let config = Config {
                        address_bits: MAX_ADDRESS_BITS,
                        ..config(
                            MAX_LINES * 16,
                            ways,
                            replacement,
                            write_policy,
                            allocate_policy,
                        )
                    };
                    let geometry = config.validate().unwrap();
                    let mut run = Run::new(config);
                    for request in request_stream(&geometry, u64::from(ways), 60) {
                        run.trace(request);
                    }
                }
            }
        }
        ways *= 2;
    }
}

// ---------------------------------------------------------------------------
// Timed mode.
// ---------------------------------------------------------------------------

/// An event's kind, its cycle stamp less the stamp of the access's `Decode`,
/// and the stall countdown after it, `(remaining, total)`.
type Timed = (EventKind, u64, Option<(u32, u32)>);

/// The kind, relative stamp and countdown of each event of `trace`.
fn timed_trace(trace: &[(MicroEvent, Machine)]) -> Vec<Timed> {
    let start = trace[0].0.cycle.unwrap().get();
    trace
        .iter()
        .map(|(event, after)| {
            let countdown = after
                .stall()
                .map(|Stalled { remaining, total }| (remaining, total));
            (event.kind, event.cycle.unwrap().get() - start, countdown)
        })
        .collect()
}

#[test]
fn case_4_timed_is_stamped_as_the_design_example_reads() {
    // Two-way LRU at 1/20, with the run's count at 100 when access #5
    // starts: Decode@100 · Miss@101 · Victim@101 · Stall@101 (total 40) ·
    // WriteBack@121 · Evict@121 · Fill@141 · Complete@141 · Rank@141.
    let mut run = Run::new(timed(two_way(Replacement::Lru), 1, 20));
    run.load(0x0A4);
    run.store(0x1A4, 5);
    run.load(0x0A4);
    run.machine.counters.cycles = Cycle::new_unchecked(100);
    let trace = run.trace(load(0x2A4, Register::R3));
    let stamps: Vec<(EventKind, u64)> = trace
        .iter()
        .map(|(event, _)| (event.kind, event.cycle.unwrap().get()))
        .collect();
    assert_eq!(
        stamps,
        [
            (Decode, 100),
            (Miss, 101),
            (Victim, 101),
            (Stall, 101),
            (WriteBack, 121),
            (Evict, 121),
            (Fill, 141),
            (Complete, 141),
            (Rank, 141),
        ]
    );
    // The stall is row 4.4, and the rest move down by one.
    let labels: Vec<&str> = trace
        .iter()
        .map(|(event, _)| event.label.as_str())
        .collect();
    assert_eq!(
        labels,
        ["4.1", "4.2", "4.3", "4.4", "4.5", "4.6", "4.7", "4.8", "4.9"]
    );
    let events: Vec<MicroEvent> = trace.iter().map(|(event, _)| event.clone()).collect();
    let stall = event(&events, Stall);
    assert_eq!(
        shape(stall),
        (None, Some(set(2)), None, &[][..], Some(Cause::Timing))
    );
    assert_eq!(
        stall.explain,
        "The CPU stalls for 40 cycles: 20 to write line 0x1A0 back, then 20 to fetch line 0x2A0."
    );
    // The access costs 1 + 20 + 20 = 41.
    assert_eq!(run.machine.counters.cycles.get(), 141);
}

/// The countdown of a stall for one transfer, and for two, at 20-cycle
/// misses: `(remaining, total)` right after the `Stall` event.
const ONE_TRANSFER: Option<(u32, u32)> = Some((20, 20));
const TWO_TRANSFERS: Option<(u32, u32)> = Some((40, 40));

/// Runs `setup`, then `request` as access case `case`, on `config` timed with
/// 2-cycle hits and 20-cycle misses, and checks the request's events against
/// `expected`: kinds, stamps from the access's first, and the countdown
/// after each. The events that move the cycle counter, and only those, list
/// it among their changes: the compare, the write-back, the fill and the
/// write-around.
fn check_timed(case: u8, config: Config, setup: &[Request], request: Request, expected: &[Timed]) {
    let mut run = Run::new(timed(config, 2, 20));
    for earlier in setup {
        run.events(*earlier);
    }
    let trace = run.trace(request);
    assert_eq!(timed_trace(&trace), expected, "case {case}");
    for (event, _) in &trace {
        let charges = matches!(event.kind, Hit | Miss | WriteBack | Fill | MemWrite);
        assert_eq!(
            event.changed.contains(&counter(Counter::Cycles)),
            charges,
            "case {case}: {:?}",
            event.kind
        );
    }
}

#[test]
fn timed_hits_cost_the_hit_time_alone() {
    // Cases 1, 5 and 6: the compare is the only charge and nothing stalls,
    // though a write-through store writes memory too.
    let back = direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate);
    let through = direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate);
    let warm = [load(0x0A4, Register::R1)];
    check_timed(
        1,
        back.clone(),
        &warm,
        load(0x0A8, Register::R2),
        &[(Decode, 0, None), (Hit, 2, None), (Complete, 2, None)],
    );
    check_timed(
        5,
        back,
        &warm,
        store(0x0A4, 7),
        &[(Decode, 0, None), (Hit, 2, None), (Write, 2, None)],
    );
    check_timed(
        6,
        through,
        &warm,
        store(0x0A4, 7),
        &[
            (Decode, 0, None),
            (Hit, 2, None),
            (Write, 2, None),
            (WriteThrough, 2, None),
        ],
    );
}

#[test]
fn timed_misses_stall_for_the_fill() {
    // Cases 2, 3 and 8: the compare, then one miss penalty at the fill; the
    // stall counts it down from the Stall event until the Fill.
    let back = direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate);
    let through = direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate);
    check_timed(
        2,
        back.clone(),
        &[],
        load(0x0A4, Register::R1),
        &[
            (Decode, 0, None),
            (Miss, 2, None),
            (Victim, 2, None),
            (Stall, 2, ONE_TRANSFER),
            (Fill, 22, None),
            (Complete, 22, None),
        ],
    );
    check_timed(
        3,
        back,
        &[load(0x0A4, Register::R1)],
        load(0x1A4, Register::R1),
        &[
            (Decode, 0, None),
            (Miss, 2, None),
            (Victim, 2, None),
            (Stall, 2, ONE_TRANSFER),
            (Evict, 2, ONE_TRANSFER),
            (Fill, 22, None),
            (Complete, 22, None),
        ],
    );
    check_timed(
        8,
        through,
        &[],
        store(0x0A4, 7),
        &[
            (Decode, 0, None),
            (Miss, 2, None),
            (Victim, 2, None),
            (Stall, 2, ONE_TRANSFER),
            (Fill, 22, None),
            (Write, 22, None),
            (WriteThrough, 22, None),
        ],
    );
}

#[test]
fn timed_dirty_victims_stall_for_the_write_back_and_the_fill() {
    // Cases 4 and 7: two miss penalties, the write-back's first, so the
    // countdown reads 40, then 20 once the line is back in memory.
    let back = direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate);
    let dirty = [store(0x0A4, 7)];
    let written_back = Some((20, 40));
    for (case, request, result) in [
        (4, load(0x1A4, Register::R1), Complete),
        (7, store(0x1A4, 3), Write),
    ] {
        check_timed(
            case,
            back.clone(),
            &dirty,
            request,
            &[
                (Decode, 0, None),
                (Miss, 2, None),
                (Victim, 2, None),
                (Stall, 2, TWO_TRANSFERS),
                (WriteBack, 22, written_back),
                (Evict, 22, written_back),
                (Fill, 42, None),
                (result, 42, None),
            ],
        );
    }
}

#[test]
fn timed_write_arounds_stall_right_after_the_compare() {
    // Cases 9 and 10: no victim, so the stall follows the miss, and the
    // write to memory costs one miss penalty under either write policy.
    for (case, write_policy) in [(9, WritePolicy::WriteBack), (10, WritePolicy::WriteThrough)] {
        check_timed(
            case,
            direct_mapped(write_policy, AllocatePolicy::WriteNoAllocate),
            &[],
            store(0x0A4, 7),
            &[
                (Decode, 0, None),
                (Miss, 2, None),
                (Stall, 2, ONE_TRANSFER),
                (MemWrite, 22, None),
            ],
        );
    }
}

#[test]
fn every_kind_of_stall_says_what_the_cpu_waits_for() {
    let cases = [
        (
            direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate),
            vec![],
            load(0x0A4, Register::R1),
            "The CPU stalls for 20 cycles while memory supplies line 0x0A0.",
        ),
        (
            direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate),
            vec![store(0x0A4, 7)],
            load(0x1A4, Register::R1),
            "The CPU stalls for 20 cycles while memory supplies line 0x1A0.",
        ),
        (
            direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate),
            vec![store(0x0A4, 7)],
            store(0x1A4, 3),
            "The CPU stalls for 40 cycles: 20 to write line 0x0A0 back, then 20 to fetch line \
             0x1A0.",
        ),
        (
            direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteNoAllocate),
            vec![],
            store(0x0A4, 7),
            "The CPU stalls for 20 cycles while the store goes to memory.",
        ),
    ];
    for (config, setup, request, line) in cases {
        let mut run = Run::new(timed(config, 1, 20));
        for earlier in setup {
            run.events(earlier);
        }
        let events = run.events(request);
        let stall = event(&events, Stall);
        assert_eq!(stall.explain, line);
        assert_eq!(
            shape(stall),
            (None, Some(set(2)), None, &[][..], Some(Cause::Timing))
        );
    }
}

#[test]
fn the_access_context_holds_the_stall_window() {
    let mut run = Run::new(timed(two_way(Replacement::Lru), 1, 20));
    run.load(0x0A4);
    run.store(0x1A4, 5);
    run.load(0x0A4);
    let trace = run.trace(load(0x2A4, Register::R3));
    let windows: Vec<Option<(u32, u64)>> = trace
        .iter()
        .map(|(_, after)| {
            let window = after.access.as_ref().unwrap().stall;
            window.map(|window| (window.total, window.end.get()))
        })
        .collect();
    // From the stall on, the window names its length and the cycle count
    // at which the fill is done: 21 + 21 + 1 before the access, 1 for its
    // compare, then 40 for the write-back and the fill.
    let from_stall = Some((40, 84));
    assert_eq!(
        windows,
        [
            None, None, None, from_stall, from_stall, from_stall, from_stall, from_stall,
            from_stall
        ]
    );
    // After the access, and on a hit, nothing is stalled.
    assert_eq!(run.machine.stall(), None);
    let trace = run.trace(load(0x2A8, Register::R1));
    for (_, after) in &trace {
        assert_eq!(after.access.as_ref().unwrap().stall, None);
        assert_eq!(after.stall(), None);
    }
}

#[test]
fn untimed_runs_stamp_no_cycles_and_never_stall() {
    let mut run = Run::new(two_way(Replacement::Lru));
    let mut events = run.store(0x0A4, 7);
    events.extend(run.load(0x1A4));
    events.extend(run.load(0x2A4));
    assert!(events.iter().any(|event| event.kind == WriteBack));
    assert!(events
        .iter()
        .all(|event| event.cycle.is_none() && event.kind != Stall));
    assert!(events
        .iter()
        .all(|event| !event.changed.contains(&counter(Counter::Cycles))));
    assert_eq!(run.machine.counters.cycles.get(), 0);
    assert_eq!(run.machine.stall(), None);
}

#[test]
fn cycles_add_up_at_every_entry_boundary_and_only_there() {
    // Between accesses, cycles = accesses × hit + misses × miss + writebacks ×
    // miss, under every policy. In the middle of a miss they do not: the
    // miss is counted at Miss, and its penalty charged at the transfers.
    let mut short_mid_access = false;
    for (hit, miss) in [(1, 20), (3, 1), (2, 7)] {
        for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
            for allocate_policy in [
                AllocatePolicy::WriteAllocate,
                AllocatePolicy::WriteNoAllocate,
            ] {
                let config = timed(
                    config(64, 2, Replacement::Lru, write_policy, allocate_policy),
                    hit,
                    miss,
                );
                let geometry = config.validate().unwrap();
                let mut run = Run::new(config);
                let identity = |counters: &crate::stats::Counters| {
                    u64::from(counters.accesses) * u64::from(hit)
                        + u64::from(counters.misses + counters.writebacks) * u64::from(miss)
                };
                for request in request_stream(&geometry, u64::from(hit + miss), 120) {
                    let trace = run.trace(request);
                    for (_, after) in &trace {
                        short_mid_access |= after.counters.cycles.get() < identity(&after.counters);
                    }
                    let counters = run.machine.counters;
                    assert_eq!(counters.cycles.get(), identity(&counters));
                    assert_eq!(trace.last().unwrap().0.cycle, Some(counters.cycles));
                }
            }
        }
    }
    assert!(short_mid_access);
}

#[test]
fn amat_and_the_measured_cost_of_fixed_traces() {
    let figures = |run: &Run| {
        let timing = run.machine.config.timing.unwrap();
        let counters = run.machine.counters;
        (
            counters.amat(timing).unwrap().text,
            counters.measured(timing).unwrap().text,
        )
    };

    // Frame 3b's five accesses at 1/20: 4 misses in 5 accesses and one
    // write-back, 105 cycles.
    let mut run = Run::new(timed(two_way(Replacement::Lru), 1, 20));
    run.store(0x0B0, 5);
    run.load(0x0A4);
    run.store(0x1A4, 7);
    run.load(0x0A8);
    assert_eq!(run.machine.stall(), None);
    run.load(0x2A4);
    assert_eq!(run.machine.counters.cycles.get(), 105);
    assert_eq!(
        figures(&run),
        (
            "AMAT = 1 + 0.80 \u{d7} 20 = 17.0 cycles".to_owned(),
            "Measured: 21.0 cycles per access (AMAT plus 4.0 for 1 write-back).".to_owned()
        )
    );

    // Ten accesses on direct-mapped-8 at 1/20 with 4 misses and 1
    // write-back: 110 cycles, the figures the AMAT line is designed around.
    let mut run = Run::new(timed(preset(Preset::DirectMapped8), 1, 20));
    run.store(0x0A4, 7);
    for at in [
        0x0A8, 0x0AC, 0x0A0, 0x1A4, 0x1A8, 0x0B0, 0x0B4, 0x0C0, 0x0C4,
    ] {
        run.load(at);
    }
    let counters = run.machine.counters;
    assert_eq!(
        (counters.accesses, counters.misses, counters.writebacks),
        (10, 4, 1)
    );
    assert_eq!(counters.cycles.get(), 110);
    assert_eq!(
        figures(&run),
        (
            "AMAT = 1 + 0.40 \u{d7} 20 = 9.0 cycles".to_owned(),
            "Measured: 11.0 cycles per access (AMAT plus 2.0 for 1 write-back).".to_owned()
        )
    );

    // One miss in three accesses: the printed rate is 0.33, so AMAT reads
    // 7.6, while the 23 cycles spent are 7.67 per access.
    let mut run = Run::new(timed(preset(Preset::DirectMapped8), 1, 20));
    for at in [0x0A4, 0x0A8, 0x0AC] {
        run.load(at);
    }
    assert_eq!(run.machine.counters.cycles.get(), 23);
    assert_eq!(
        figures(&run),
        (
            "AMAT = 1 + 0.33 \u{d7} 20 = 7.6 cycles".to_owned(),
            "Measured: 7.7 cycles per access (AMAT plus 0.0 for 0 write-backs).".to_owned()
        )
    );
    let amat = run
        .machine
        .counters
        .amat(run.machine.config.timing.unwrap())
        .unwrap();
    assert!((amat.value - (1.0 + 20.0 / 3.0)).abs() < 1e-12);
}

// ---------------------------------------------------------------------------
// Programs.
// ---------------------------------------------------------------------------

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
            assert_eq!(counters.hits + counters.misses, counters.accesses);
            let length = after.program.as_deref().map_or(0, Program::len);
            assert!(after.cpu.pc <= length);
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
    fn load_as(&mut self, source: &str, format: ProgramFormat) -> Vec<(MicroEvent, Machine)> {
        let program = Program::parse(&self.run.machine.geometry, source, format).unwrap();
        let (flow, trace) = self.apply(|machine, out| load_program(machine, program, out));
        assert_eq!(flow, Flow::Continue);
        trace
    }

    /// Executes the next instruction, which there is.
    fn step(&mut self) -> Vec<(MicroEvent, Machine)> {
        assert_eq!(ready(&self.run.machine), Ok(()));
        self.apply(step).1
    }

    /// Steps until the program cannot go on, and returns every event.
    fn finish(&mut self) -> Vec<(MicroEvent, Machine)> {
        let mut trace = Vec::new();
        while ready(&self.run.machine).is_ok() {
            trace.extend(self.step());
        }
        trace
    }

    /// A load or store issued by hand.
    fn issue(&mut self, request: Request) -> Vec<(MicroEvent, Machine)> {
        let (flow, trace) = self.apply(|machine, out| access(machine, request, out));
        assert_eq!(flow, Flow::Continue);
        trace
    }

    fn machine(&self) -> &Machine {
        &self.run.machine
    }
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

/// A loop over six words 0x44 apart, within the first 24 lines: each is
/// loaded, incremented and stored back through a register, beside a store
/// of an immediate and a load into r0.
const STRIDED: &str = concat!(
    "        add r1, r0, #0\n",
    "        add r2, r0, #6\n",
    "loop:   ld r3, [r1]\n",
    "        add r3, r3, #1\n",
    "        st [r1], r3\n",
    "        st [0x040], #-1\n",
    "        add r1, r1, #0x44\n",
    "        add r2, r2, #-1\n",
    "        bnez r2, loop\n",
    "        ld r0, [0x040]",
);

#[test]
fn a_program_load_resets_the_cpu_and_keeps_the_cache() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    let trace = driven.load("ld r1, [0x0A4]\nadd r2, r1, #1\nadd r3, r0, #7");
    let (setup, after) = &trace[0];
    assert_eq!(trace.len(), 1);
    assert_eq!(
        (setup.kind, setup.entry, setup.seq, setup.label.as_str()),
        (EventKind::Setup, None, 1, "")
    );
    // Nothing held a value and the pc was 0: nothing changed.
    assert!(setup.changed.is_empty());
    assert_eq!(
        setup.explain,
        "Loaded a program of 3 instructions. Registers and the pc are reset; the cache and \
         memory keep their contents."
    );
    assert_eq!(
        (setup.cause, setup.addr, setup.set, setup.way),
        (None, None, None, None)
    );
    assert_eq!(after.cpu, Cpu::default());
    assert_eq!(after.entries_in_run, 0);

    driven.step();
    driven.step();
    let before = driven.machine().clone();
    assert_eq!(before.registers.get(Register::R2), 54);
    assert_eq!(before.cpu.pc, 2);

    let trace = driven.load("ld r4, [0x0A8]");
    let (setup, after) = &trace[0];
    assert_eq!(
        setup.changed,
        [
            register(Register::R1),
            register(Register::R2),
            ChangeRef::Pc
        ]
    );
    assert_eq!(after.registers, Registers::default());
    assert_eq!(after.cpu, Cpu::default());
    assert_eq!(after.access, None);
    // The cache, memory, counters and entries are kept.
    assert_eq!(after.cache, before.cache);
    assert_eq!(after.memory, before.memory);
    assert_eq!(after.counters, before.counters);
    assert_eq!(after.entries_in_run, before.entries_in_run);
    assert_eq!(after.program.as_deref().map(Program::len), Some(1));

    // The new program's load hits the line the old one brought in.
    let kinds = kinds(
        &driven
            .step()
            .into_iter()
            .map(|(event, _)| event)
            .collect::<Vec<_>>(),
    );
    assert_eq!(kinds, [Decode, Hit, Complete]);
}

#[test]
fn each_instruction_retires_at_its_last_event() {
    for (config, source) in [
        (preset(Preset::TwoWayLru), FRAME_3B),
        (two_way(Replacement::Fifo), STRIDED),
        (
            direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteNoAllocate),
            STRIDED,
        ),
        (timed(preset(Preset::TwoWayLru), 1, 20), FRAME_3B),
    ] {
        let mut driven = Driven::new(config);
        driven.load(source);
        let mut entries = 0;
        while ready(driven.machine()).is_ok() {
            let before = driven.machine().clone();
            let trace = driven.step();
            entries += 1;
            let (last, after) = trace.last().unwrap();
            // In flight, the instruction is the one at the pc.
            for (event, during) in &trace[..trace.len() - 1] {
                assert_eq!(during.cpu, before.cpu, "{}", event.label);
                assert!(!event.changed.contains(&ChangeRef::Pc), "{}", event.label);
            }
            assert_eq!(after.cpu.executed, before.cpu.executed + 1);
            assert_eq!(
                last.changed.contains(&ChangeRef::Pc),
                after.cpu.pc != before.cpu.pc
            );
            assert!(!after.in_flight());
            assert_eq!(after.entries_in_run, entries);
            assert!(trace.iter().all(|(event, _)| event.entry == Some(entries)));
        }
        let program = driven.machine().program.as_ref().unwrap();
        assert_eq!(driven.machine().cpu.pc, program.len());
        assert_eq!(ready(driven.machine()), Err(OpError::Finished));
    }
}

#[test]
fn every_program_event_lists_exactly_what_it_changed() {
    let mut kinds_seen = std::collections::BTreeSet::new();
    for replacement in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
        for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
            for allocate_policy in [
                AllocatePolicy::WriteAllocate,
                AllocatePolicy::WriteNoAllocate,
            ] {
                for timing in [None, Some(timing(2, 5))] {
                    let config = Config {
                        timing,
                        ..config(128, 2, replacement, write_policy, allocate_policy)
                    };
                    let mut driven = Driven::new(config);
                    let mut before = driven.machine().clone();
                    let mut trace = driven.load(STRIDED);
                    for _ in 0..7 {
                        trace.extend(driven.step());
                    }
                    // A second load, midway, clears registers that hold values.
                    trace.extend(driven.load(STRIDED));
                    trace.extend(driven.finish());
                    for (event, after) in &trace {
                        kinds_seen.insert(format!("{:?}", event.kind));
                        assert_eq!(
                            event.changed,
                            diff(&before, after),
                            "{} {:?}",
                            event.label,
                            event.kind
                        );
                        before = after.clone();
                    }
                }
            }
        }
    }
    for kind in [
        "Setup",
        "Complete",
        "Write",
        "WriteThrough",
        "MemWrite",
        "Rank",
    ] {
        assert!(kinds_seen.contains(kind), "{kind} never came up");
    }
}

#[test]
fn stopping_after_any_event_of_an_instruction_leaves_the_machine_as_the_full_step_has_it() {
    for timing in [None, Some(timing(1, 20))] {
        let mut driven = Driven::new(Config {
            timing,
            ..preset(Preset::TwoWayLru)
        });
        driven.load(FRAME_3B);
        for _ in 0..4 {
            driven.step();
        }
        // Access #5, `ld r3, [0x2A4]`, with its dirty eviction and rank.
        let base = driven.machine().clone();
        let full = driven.step();
        for stop in 1..=full.len() {
            let mut machine = base.clone();
            let mut recorder = Recorder {
                stop_after: Some(stop),
                ..Recorder::default()
            };
            let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(9), 0);
            // The sink stops the step, even at its last event.
            assert_eq!(step(&mut machine, &mut out), Flow::Stop);
            assert_eq!(recorder.events.len(), stop);
            assert_eq!(machine, full[stop - 1].1, "stopped after {stop}");

            let mut quiet_machine = base.clone();
            let mut quiet = Quiet {
                stop_after: Some(stop),
                ..Quiet::default()
            };
            let mut out = Emitter::new(&mut quiet, OpIndex::new_unchecked(9), 0);
            step(&mut quiet_machine, &mut out);
            assert_eq!(quiet.seen, stop);
            assert_eq!(quiet_machine, machine, "quietly stopped after {stop}");
        }
    }
}

#[test]
fn a_register_address_that_does_not_fit_faults_where_it_is() {
    let cases = [
        // r2 holds 4096, one past the last 12-bit address.
        ("add r2, r0, #0xFFF\nadd r2, r2, #1\nld r1, [r2]", None),
        // r2 holds 166, within the width but not a multiple of 4.
        ("add r2, r0, #0x0A6\nst [r2], #1", Some(addr(0x0A6))),
        // A register wider than every address width.
        ("add r3, r0, #-1\nst [r3], r3", None),
    ];
    for (source, at) in cases {
        let mut driven = Driven::new(preset(Preset::DirectMapped8));
        driven.load(source);
        let lines = source.lines().count();
        for _ in 1..lines {
            driven.step();
        }
        let before = driven.machine().clone();
        let (flow, trace) = driven.apply(step);
        assert_eq!(flow, Flow::Stop);
        let [(error, after)] = trace.as_slice() else {
            panic!("one event: {trace:?}");
        };
        assert_eq!(error.kind, EventKind::Error);
        assert_eq!(error.entry, Some(before.entries_in_run + 1));
        assert_eq!((error.addr, error.cause), (at, None));
        assert!(error.changed.is_empty());
        assert!(
            error.explain.starts_with(&format!("Line {lines}: ")),
            "{}",
            error.explain
        );
        assert!(
            error.explain.ends_with(" The program stopped."),
            "{}",
            error.explain
        );
        // Only the program's status and the entry count change: the pc stays
        // at the instruction, which does not count against the budget.
        let expected = Machine {
            cpu: Cpu {
                halted: Some(Halted::Faulted),
                ..before.cpu
            },
            entries_in_run: before.entries_in_run + 1,
            ..before.clone()
        };
        assert_eq!(after, &expected);
        let line = u32::try_from(lines).unwrap();
        assert_eq!(ready(after), Err(OpError::Stopped { line }));
    }
}

#[test]
fn the_budget_replaces_the_instruction_after_its_last() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load("add r1, r0, #1\nloop: bnez r1, loop");
    driven.step();
    // As if the loop had run until one instruction of the budget was left.
    driven.run.machine.cpu.executed = INSTRUCTION_BUDGET - 1;
    let trace = driven.step();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].0.kind, Complete);
    assert_eq!(driven.machine().cpu.executed, INSTRUCTION_BUDGET);
    let before = driven.machine().clone();

    let (flow, trace) = driven.apply(step);
    assert_eq!(flow, Flow::Stop);
    let [(error, after)] = trace.as_slice() else {
        panic!("one event: {trace:?}");
    };
    assert_eq!(error.kind, EventKind::Error);
    assert_eq!(error.explain, explain::out_of_budget());
    assert_eq!((error.addr, error.cause), (None, None));
    assert_eq!(
        after.cpu,
        Cpu {
            halted: Some(Halted::OutOfBudget),
            ..before.cpu
        }
    );
    assert_eq!(after.registers, before.registers);
    assert_eq!(ready(after), Err(OpError::Stopped { line: 2 }));

    // Loading the program again gives it a fresh budget.
    driven.load("add r1, r0, #1\nloop: bnez r1, loop");
    assert_eq!(driven.machine().cpu, Cpu::default());
    assert_eq!(driven.step()[0].0.kind, Complete);
}

#[test]
fn a_program_that_finishes_on_its_last_budgeted_instruction_does_not_run_out() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load("add r1, r0, #1\nadd r1, r1, #1");
    driven.step();
    driven.run.machine.cpu.executed = INSTRUCTION_BUDGET - 1;
    let trace = driven.step();
    assert_eq!(trace[0].0.kind, Complete);
    assert_eq!(driven.machine().cpu.halted, None);
    assert_eq!(ready(driven.machine()), Err(OpError::Finished));
}

#[test]
fn ready_says_why_there_is_nothing_to_execute() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    assert_eq!(ready(driven.machine()), Err(OpError::NoProgram));
    // With nothing to execute, a step does nothing.
    let before = driven.machine().clone();
    let (flow, trace) = driven.apply(step);
    assert_eq!((flow, trace.len()), (Flow::Stop, 0));
    assert_eq!(driven.machine(), &before);

    driven.load("; one instruction\nadd r1, r0, #1\ndone:");
    assert_eq!(ready(driven.machine()), Ok(()));
    driven.step();
    assert_eq!(ready(driven.machine()), Err(OpError::Finished));
    let before = driven.machine().clone();
    let (flow, trace) = driven.apply(step);
    assert_eq!((flow, trace.len()), (Flow::Stop, 0));
    assert_eq!(driven.machine(), &before);
}

#[test]
fn a_recorded_event_keeps_no_spare_room() {
    // A program's load of the least recently used line of a fully
    // associative set of `MAX_LINES` ways moves every rank: its `Rank` event
    // lists every way and the pc, the longest list an access makes, and its
    // line names every way. The session keeps every event for as long as it
    // holds the log, so no event keeps room it does not use.
    let mut driven = Driven::new(config(
        MAX_LINES * 16,
        MAX_LINES,
        Replacement::Lru,
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    ));
    let mut trace = Vec::new();
    for line in 0..MAX_LINES {
        trace.extend(driven.issue(load(line * 0x10, Register::R1)));
    }
    trace.extend(driven.load("ld r1, [0x000]"));
    trace.extend(driven.step());
    let (rank, _) = trace.last().unwrap();
    assert_eq!(rank.kind, Rank);
    assert_eq!(rank.changed.len(), MAX_LINES as usize + 1);
    assert_eq!(rank.changed.last(), Some(&ChangeRef::Pc));
    for (event, _) in &trace {
        assert_eq!(
            (event.changed.capacity(), event.explain.capacity()),
            (event.changed.len(), event.explain.len()),
            "{}",
            event.explain
        );
    }
}

#[test]
fn a_branch_moves_the_pc_to_its_label_or_on() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load(
        "add r1, r0, #2\nloop: add r1, r1, #-1\nbnez r1, loop\nbnez r0, loop\nbnez r1, end\nend:",
    );
    let pcs: Vec<(u32, String)> = driven
        .finish()
        .into_iter()
        .map(|(event, after)| (after.cpu.pc, event.explain))
        .collect();
    assert_eq!(
        pcs,
        [
            (1, "add: r1 \u{2190} 2. No memory access.".to_owned()),
            (2, "add: r1 \u{2190} 1. No memory access.".to_owned()),
            (1, "bnez: r1 is 1, branch taken to loop.".to_owned()),
            (2, "add: r1 \u{2190} 0. No memory access.".to_owned()),
            (3, "bnez: r1 is 0, branch not taken.".to_owned()),
            (4, "bnez: r0 is 0, branch not taken.".to_owned()),
            (5, "bnez: r1 is 0, branch not taken.".to_owned()),
        ]
    );
    // A branch taken to the label after the last instruction finishes the
    // program, and one to itself leaves the pc where it was.
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load("add r1, r0, #1\nbnez r1, end\nadd r2, r0, #5\nend:");
    driven.finish();
    assert_eq!(
        driven.machine().cpu,
        Cpu {
            pc: 3,
            executed: 2,
            halted: None
        }
    );
    assert_eq!(driven.machine().registers.get(Register::R2), 0);
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load("add r1, r0, #1\nloop: bnez r1, loop");
    driven.step();
    let trace = driven.step();
    assert!(trace[0].0.changed.is_empty());
    assert_eq!(driven.machine().cpu.pc, 1);
}

#[test]
fn a_hand_issued_access_leaves_the_program_where_it_is() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    driven.load("add r1, r0, #5\nadd r2, r1, #0");
    driven.step();
    let cpu = driven.machine().cpu;
    let trace = driven.issue(load(0x0A4, Register::R1));
    assert_eq!(driven.machine().cpu, cpu);
    assert!(trace
        .iter()
        .all(|(event, _)| !event.changed.contains(&ChangeRef::Pc)));
    assert_eq!(
        trace.last().unwrap().0.explain,
        "Read word 1 of way 0 (53) into r1. A program is loaded, so its r1 now holds 53."
    );
    driven.issue(store(0x0A8, 9));
    assert_eq!(driven.machine().cpu, cpu);
    // The program reads the r1 the hand-issued load wrote.
    driven.step();
    assert_eq!(driven.machine().registers.get(Register::R2), 53);
    // Once the program has finished, a hand-issued load says nothing of it.
    let trace = driven.issue(load(0x0A4, Register::R1));
    assert_eq!(
        trace.last().unwrap().0.explain,
        "Read word 1 of way 0 (53) into r1."
    );
}

#[test]
fn a_store_from_a_register_names_the_bits_it_keeps_on_its_result_line_only() {
    let config = Config {
        word_bytes: WordBytes::Two,
        ..direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate)
    };
    let mut driven = Driven::new(config);
    driven
        .load("add r2, r0, #70000\nst [0x0A4], r2\nst [0x0A4], #7\nadd r2, r0, #7\nst [0x0A4], r2");
    let explains = |trace: Vec<(MicroEvent, Machine)>| -> Vec<(EventKind, String)> {
        trace
            .into_iter()
            .filter(|(event, _)| matches!(event.kind, Write | WriteThrough))
            .map(|(event, _)| (event.kind, event.explain))
            .collect()
    };
    driven.step();
    assert_eq!(
        explains(driven.step()),
        [
            (
                Write,
                "r2 = 70,000 does not fit a 2-byte word, so its low 16 bits, 4464, are stored. \
                 Write 4464 into word 2 of way 0."
                    .to_owned()
            ),
            (
                WriteThrough,
                "Write-through: 4464 also goes to memory at 0x0A4.".to_owned()
            ),
        ]
    );
    // An immediate, and a register that fits, say nothing more.
    for _ in 0..2 {
        let written = explains(driven.step());
        assert!(
            written
                .iter()
                .all(|(_, explain)| explain.starts_with("Write")),
            "{written:?}"
        );
    }
    let written = explains(driven.step());
    assert!(
        written
            .iter()
            .all(|(_, explain)| explain.starts_with("Write")),
        "{written:?}"
    );
}

#[test]
fn a_trace_reads_into_r1_and_writes_its_values() {
    let mut driven = Driven::new(preset(Preset::DirectMapped8));
    let setup = driven.load_as("R 0x0A4\nW 0x0A8 9\nR 0x0A8", ProgramFormat::Trace);
    assert_eq!(
        setup[0].0.explain,
        "Loaded a trace of 3 accesses. Registers and the pc are reset; the cache and memory \
         keep their contents."
    );
    let last: Vec<String> = (0..3)
        .map(|_| driven.step().pop().unwrap().0.explain)
        .collect();
    assert_eq!(
        last,
        [
            "Read word 1 of way 0 (53) into r1.",
            "Write 9 into word 2 of way 0 and set its dirty bit. Memory at 0x0A8 is now stale.",
            "Read word 2 of way 0 (9) into r1.",
        ]
    );
    assert_eq!(ready(driven.machine()), Err(OpError::Finished));
}

/// Keeps every event, and every boundary inside an op with the machine
/// there, its progress and the id of the next event.
#[derive(Default)]
struct Boundaries {
    events: Vec<MicroEvent>,
    boundaries: Vec<(Machine, OpProgress, u64)>,
}

impl EventSink for Boundaries {
    fn records(&self) -> bool {
        true
    }

    fn event(&mut self, event: Option<MicroEvent>, _after: &Machine) -> Flow {
        self.events.extend(event);
        Flow::Continue
    }

    fn boundary(&mut self, machine: &Machine, progress: OpProgress) -> Flow {
        let next_id = self.events.len() as u64;
        self.boundaries.push((machine.clone(), progress, next_id));
        Flow::Continue
    }
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

/// A machine on `direct-mapped-8` with the assembly program `source` loaded.
fn loaded(source: &str) -> Machine {
    let mut machine = Machine::start(preset(Preset::DirectMapped8)).unwrap();
    let op = Op::Program {
        source: source.to_owned(),
        format: ProgramFormat::Asm,
    };
    assert_eq!(apply_op(&mut machine, &op, 0).0, Ok(()));
    machine
}

/// A loop without an exit or a memory access.
const ENDLESS: &str = "        add r1, r0, #1\nloop:   bnez r1, loop";

#[test]
fn an_op_needs_room_for_one_more_entry_in_the_log() {
    let limit = u64::from(SESSION_EVENT_LIMIT);
    let room = u64::from(MAX_EVENTS_PER_ENTRY);
    let issue = Op::Issue {
        kind: AccessKind::Read,
        addr: addr(0x0A4),
        value: None,
    };
    let mut machine = Machine::start(preset(Preset::DirectMapped8)).unwrap();
    let before = machine.clone();
    for op in [issue.clone(), Op::Step, Op::Run { n: 0 }] {
        let (result, events) = apply_op(&mut machine, &op, limit - room + 1);
        assert_eq!(result, Err(OpError::SessionFull), "{op:?}");
        assert!(events.is_empty());
        assert_eq!(machine, before);
    }
    let (result, events) = apply_op(&mut machine, &issue, limit - room);
    assert_eq!(result, Ok(()));
    assert_eq!(events.first().map(|event| event.id), Some(limit - room));
    assert!(events.len() < MAX_EVENTS_PER_ENTRY as usize);
}

#[test]
fn an_op_needs_room_for_one_more_op_a_link_can_hold() {
    let limit = u64::from(SESSION_EVENT_LIMIT);
    let room = u64::from(MAX_EVENTS_PER_ENTRY);
    let issue = Op::Issue {
        kind: AccessKind::Read,
        addr: addr(0x0A4),
        value: None,
    };
    let apply_as = |machine: &mut Machine, op: &Op, index: u32, first_id: u64| {
        let mut recorder = Recorder::default();
        let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(index), first_id);
        (apply(machine, op, &mut out), recorder.events)
    };
    let mut machine = Machine::start(preset(Preset::DirectMapped8)).unwrap();
    let before = machine.clone();
    // Op MAX_SHARE_OPS, from 0, would be one more than a link can hold,
    // whatever the op; the room for events is checked first.
    for op in [issue.clone(), Op::Step, Op::Run { n: 0 }] {
        let (result, events) = apply_as(&mut machine, &op, MAX_SHARE_OPS, 0);
        assert_eq!(result, Err(OpError::LinkFull), "{op:?}");
        assert!(events.is_empty());
        assert_eq!(machine, before);
        let (result, _) = apply_as(&mut machine, &op, MAX_SHARE_OPS, limit - room + 1);
        assert_eq!(result, Err(OpError::SessionFull), "{op:?}");
        assert_eq!(machine, before);
    }
    let (result, events) = apply_as(&mut machine, &issue, MAX_SHARE_OPS - 1, 0);
    assert_eq!(result, Ok(()));
    assert!(!events.is_empty());
}

#[test]
fn a_loop_that_runs_out_of_room_stops_with_an_error() {
    let limit = u64::from(SESSION_EVENT_LIMIT);
    let room = u64::from(MAX_EVENTS_PER_ENTRY);
    for op in [Op::StepAccess, Op::Run { n: 5 }] {
        let mut machine = loaded(ENDLESS);
        // Room for the first instruction's entry, and no more.
        let (result, events) = apply_op(&mut machine, &op, limit - room);
        assert_eq!(result, Ok(()));
        let kinds: Vec<EventKind> = events.iter().map(|event| event.kind).collect();
        assert_eq!(kinds, [Complete, EventKind::Error], "{op:?}");
        let error = events.last().unwrap();
        assert_eq!(error.explain, explain::session_limit());
        assert_eq!((error.entry, error.seq), (Some(2), 1));
        assert!(error.changed.is_empty());
        assert_eq!(machine.cpu.halted, Some(Halted::OutOfBudget));
        assert_eq!(
            machine.cpu.pc, 1,
            "the pc stays at the instruction not executed"
        );
        assert_eq!(ready(&machine), Err(OpError::Stopped { line: 2 }));
    }
}

#[test]
fn a_run_stops_at_its_count_and_a_step_access_after_an_access() {
    let source = "add r1, r0, #1\nadd r1, r1, #1\nld r2, [0x0A4]\nld r3, [0x0A8]";
    let mut machine = loaded(source);
    let (result, events) = apply_op(&mut machine, &Op::StepAccess, 1);
    assert_eq!(result, Ok(()));
    let labels: Vec<&str> = events.iter().map(|event| event.label.as_str()).collect();
    assert_eq!(labels, ["1.1", "2.1", "3.1", "3.2", "3.3", "3.4", "3.5"]);
    assert_eq!(machine.cpu.pc, 3);
    let (result, events) = apply_op(&mut machine, &Op::Run { n: 1 }, 8);
    assert_eq!(result, Ok(()));
    assert_eq!(events.last().map(|event| event.label.as_str()), Some("4.3"));
    // A run of more than is left stops when the program finishes.
    let mut machine = loaded(source);
    let (result, events) = apply_op(&mut machine, &Op::Run { n: 10 }, 1);
    assert_eq!(result, Ok(()));
    assert!(events.iter().all(|event| event.kind != EventKind::Error));
    assert_eq!(ready(&machine), Err(OpError::Finished));
    // A run's count is checked before anything else about the program.
    let mut empty = Machine::start(preset(Preset::DirectMapped8)).unwrap();
    for n in [0, MAX_RUN_STEPS + 1] {
        assert_eq!(
            apply_op(&mut empty, &Op::Run { n }, 0).0,
            Err(OpError::RunSteps)
        );
    }
    assert_eq!(
        apply_op(&mut empty, &Op::Run { n: MAX_RUN_STEPS }, 0).0,
        Err(OpError::NoProgram)
    );
}

#[test]
fn an_op_resumed_from_a_boundary_inside_it_goes_on_as_it_would_have() {
    let source = concat!(
        "        add r3, r0, #3\n",
        "loop:   ld r1, [0x0A4]\n",
        "        add r3, r3, #-1\n",
        "        bnez r3, loop\n",
        "        add r1, r0, #1\n",
        "        add r1, r1, #1\n",
        "        st [0x1A4], r1",
    );
    for op in [Op::Run { n: 9 }, Op::StepAccess] {
        let mut whole = loaded(source);
        if op == Op::StepAccess {
            // Past the loop, to the two adds before the store.
            let (result, _) = apply_op(&mut whole, &Op::Run { n: 10 }, 1);
            assert_eq!(result, Ok(()));
        }
        let start = whole.clone();
        let mut sink = Boundaries::default();
        let mut out = Emitter::new(&mut sink, OpIndex::new_unchecked(1), 0);
        assert_eq!(apply(&mut whole, &op, &mut out), Ok(()));
        assert!(sink.boundaries.len() >= 2, "{op:?}");
        for (machine, progress, next_id) in &sink.boundaries {
            let mut resumed = machine.clone();
            let mut recorder = Recorder::default();
            let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(1), *next_id);
            assert_eq!(resume(&mut resumed, &op, *progress, &mut out), Ok(()));
            assert_eq!(resumed, whole, "{op:?} from {}", progress.get());
            let before = sink
                .events
                .get(..usize::try_from(*next_id).unwrap())
                .unwrap();
            assert_eq!(
                [before, recorder.events.as_slice()].concat(),
                sink.events,
                "{op:?} from {}",
                progress.get()
            );
        }
        // From the op's start, resuming is applying.
        let mut again = start.clone();
        let mut recorder = Recorder::default();
        let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(1), 0);
        assert_eq!(resume(&mut again, &op, OpProgress::START, &mut out), Ok(()));
        assert_eq!(again, whole);
    }
}

/// Keeps every event, and stops the op at its `stop_at`-th boundary,
/// counting from 0, noting the progress there.
struct StopAtBoundary {
    events: Vec<MicroEvent>,
    stop_at: usize,
    seen: usize,
    stopped: Option<OpProgress>,
}

impl EventSink for StopAtBoundary {
    fn records(&self) -> bool {
        true
    }

    fn event(&mut self, event: Option<MicroEvent>, _after: &Machine) -> Flow {
        self.events.extend(event);
        Flow::Continue
    }

    fn boundary(&mut self, _machine: &Machine, progress: OpProgress) -> Flow {
        self.seen += 1;
        if self.seen == self.stop_at + 1 {
            self.stopped = Some(progress);
            Flow::Stop
        } else {
            Flow::Continue
        }
    }
}

#[test]
fn an_op_stopped_at_a_boundary_goes_on_from_there_when_resumed() {
    let source = concat!(
        "        add r3, r0, #3\n",
        "loop:   ld r1, [0x0A4]\n",
        "        add r3, r3, #-1\n",
        "        bnez r3, loop\n",
        "        st [0x1A4], r3",
    );
    let op = Op::Run { n: 11 };
    let mut whole = loaded(source);
    let start = whole.clone();
    let (result, events) = apply_op(&mut whole, &op, 0);
    assert_eq!(result, Ok(()));
    let entries = events.iter().filter(|event| event.seq == 1).count();
    assert_eq!(entries, 11);
    // A boundary lies between every two of the op's entries.
    for stop_at in 0..entries - 1 {
        let mut machine = start.clone();
        let mut sink = StopAtBoundary {
            events: Vec::new(),
            stop_at,
            seen: 0,
            stopped: None,
        };
        let mut out = Emitter::new(&mut sink, OpIndex::new_unchecked(1), 0);
        assert_eq!(apply(&mut machine, &op, &mut out), Ok(()));
        let progress = sink.stopped.unwrap();
        assert_eq!(progress.get() as usize, stop_at + 1);
        // Stopped at an entry boundary: its last event ends an entry.
        let next = events.get(sink.events.len()).unwrap();
        assert_eq!(next.seq, 1, "stopped at boundary {stop_at}");
        let mut recorder = Recorder::default();
        let first = sink.events.len() as u64;
        let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(1), first);
        assert_eq!(resume(&mut machine, &op, progress, &mut out), Ok(()));
        assert_eq!(machine, whole, "stopped at boundary {stop_at}");
        assert_eq!(
            [sink.events, recorder.events].concat(),
            events,
            "stopped at boundary {stop_at}"
        );
    }
}

#[test]
fn a_new_run_starts_over_with_the_program_kept_and_the_next_run_number() {
    let mut machine = loaded("ld r1, [0x0A4]\nst [0x0A8], #7");
    for op in [Op::Step, Op::Step] {
        assert_eq!(apply_op(&mut machine, &op, 0).0, Ok(()));
    }
    assert_eq!(machine.entries_in_run, 2);
    let config = preset(Preset::TwoWayLru);
    let (result, events) = apply_op(
        &mut machine,
        &Op::Config {
            config: config.clone(),
        },
        0,
    );
    assert_eq!(result, Ok(()));
    let [setup] = events.as_slice() else {
        panic!("one setup: {events:?}");
    };
    assert_eq!(
        (setup.kind, setup.run, setup.entry),
        (EventKind::Setup, 2, None)
    );
    assert!(setup.changed.is_empty());
    let mut expected = Machine::start(config.clone()).unwrap();
    expected.run = 2;
    expected.program = Program::parse(
        &expected.geometry,
        "ld r1, [0x0A4]\nst [0x0A8], #7",
        ProgramFormat::Asm,
    )
    .ok()
    .map(Arc::new);
    assert_eq!(machine, expected);
    // An invalid configuration is refused and changes nothing.
    let invalid = Config {
        address_bits: 0,
        ..config
    };
    let (result, events) = apply_op(
        &mut machine,
        &Op::Config {
            config: invalid.clone(),
        },
        0,
    );
    assert_eq!(
        result,
        Err(OpError::Config(invalid.validate().unwrap_err()))
    );
    assert!(events.is_empty());
    assert_eq!(machine, expected);
}

// ---------------------------------------------------------------------------
// Edits.
// ---------------------------------------------------------------------------

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

/// An edit of the memory word at `at` to `value`.
fn memory_edit(at: u32, value: u64) -> Op {
    Op::Edit {
        target: EditTarget::Memory { addr: addr(at) },
        patch: EditPatch::Memory {
            value: Word::new_unchecked(value),
        },
    }
}

/// `two-way-lru` after frame 3b's first four accesses: set 2 holds 0x0A0,
/// clean, and 0x1A0, dirty; set 3 holds 0x0B0, dirty with 5.
fn frame_3b_at_access_4() -> Machine {
    let mut run = Run::new(preset(Preset::TwoWayLru));
    run.store(0x0B0, 5);
    run.load(0x0A4);
    run.store(0x1A4, 7);
    run.load(0x0A8);
    run.machine
}

#[test]
fn an_edit_lists_every_cell_it_changes_and_moves_no_counter() {
    let mut machine = frame_3b_at_access_4();
    let words = |values: [u64; 4]| Some(values.map(Word::new_unchecked).to_vec());
    let edits = [
        // Memory words: under a clean line, under no line, under a dirty
        // line with another value, then with its own.
        memory_edit(0x0A8, 12),
        memory_edit(0x100, 1),
        memory_edit(0x0B0, 12),
        memory_edit(0x0B0, 5),
        // Ways: a word, the dirty bit and the rank; a dirty way made empty;
        // an empty way made valid with data and the dirty bit; a dirty way
        // given another line; a rank moved back.
        way_edit(
            2,
            0,
            WayPatch {
                data: words([40, 9, 12, 79]),
                dirty: Some(true),
                rank: Some(1),
                ..WayPatch::default()
            },
        ),
        way_edit(
            2,
            1,
            WayPatch {
                valid: Some(false),
                ..WayPatch::default()
            },
        ),
        way_edit(
            0,
            1,
            WayPatch {
                valid: Some(true),
                line: Some(addr(0x0C4)),
                data: words([1, 2, 3, 4]),
                dirty: Some(true),
                ..WayPatch::default()
            },
        ),
        way_edit(
            3,
            0,
            WayPatch {
                line: Some(addr(0x130)),
                ..WayPatch::default()
            },
        ),
        way_edit(
            0,
            1,
            WayPatch {
                rank: Some(0),
                data: words([1, 2, 3, 5]),
                ..WayPatch::default()
            },
        ),
    ];
    for op in edits {
        let before = machine.clone();
        let (result, events) = apply_op(&mut machine, &op, 0);
        assert_eq!(result, Ok(()), "{op:?}");
        let [event] = events.as_slice() else {
            panic!("one event: {events:?}");
        };
        assert_eq!(event.kind, EventKind::Edit);
        assert_eq!(event.cause, None);
        assert_eq!(event.changed, diff(&before, &machine), "{op:?}");
        assert_eq!(machine.check_invariants(), Ok(()), "{op:?}");
        assert_eq!(machine.counters, before.counters);
        assert_eq!(machine.prng, before.prng);
        assert_eq!(machine.access, None);
        assert_eq!(machine.entries_in_run, before.entries_in_run + 1);
        // A quiet sink reaches the same machine, and gets no event.
        let mut quiet_machine = before.clone();
        let mut quiet = Quiet::default();
        let mut out = Emitter::new(&mut quiet, OpIndex::new_unchecked(1), 0);
        assert_eq!(apply(&mut quiet_machine, &op, &mut out), Ok(()));
        assert_eq!((quiet_machine, quiet.seen), (machine.clone(), 1));
    }
}

#[test]
fn an_edit_that_moves_a_line_updates_what_the_classification_of_misses_remembers() {
    let mut machine = frame_3b_at_access_4();
    let a0 = LineAddr::new_unchecked(0x0A0);
    let c0 = LineAddr::new_unchecked(0x0C0);
    assert!(machine.filled.contains(&a0) && machine.shadow.lines().contains(&a0));
    let (result, _) = apply_op(
        &mut machine,
        &way_edit(
            2,
            0,
            WayPatch {
                valid: Some(false),
                ..WayPatch::default()
            },
        ),
        0,
    );
    assert_eq!(result, Ok(()));
    assert!(!machine.filled.contains(&a0) && !machine.shadow.lines().contains(&a0));
    assert!(!machine.memory.is_touched(c0));
    let (result, _) = apply_op(
        &mut machine,
        &way_edit(
            0,
            0,
            WayPatch {
                valid: Some(true),
                line: Some(addr(0x0C0)),
                ..WayPatch::default()
            },
        ),
        0,
    );
    assert_eq!(result, Ok(()));
    assert!(machine.filled.contains(&c0));
    assert_eq!(machine.shadow.lines().first(), Some(&c0));
    // The line the way takes is one memory has touched, as a fill's is.
    assert!(machine.memory.is_touched(c0));
    // An edit of data only moves nothing there.
    let shadow = machine.shadow.clone();
    let (result, _) = apply_op(
        &mut machine,
        &way_edit(
            0,
            0,
            WayPatch {
                dirty: Some(true),
                ..WayPatch::default()
            },
        ),
        0,
    );
    assert_eq!(result, Ok(()));
    assert_eq!(machine.shadow, shadow);
}

#[test]
fn a_refused_edit_changes_nothing_and_says_why() {
    let mut machine = frame_3b_at_access_4();
    let before = machine.clone();
    let (result, events) = apply_op(
        &mut machine,
        &way_edit(
            3,
            0,
            WayPatch {
                line: Some(addr(0x120)),
                ..WayPatch::default()
            },
        ),
        0,
    );
    let Err(OpError::Edit(refusal)) = result else {
        panic!("refused: {result:?}");
    };
    assert_eq!(refusal.reason, RefusalReason::WrongSetFull);
    assert!(events.is_empty());
    assert_eq!(machine, before);
}
