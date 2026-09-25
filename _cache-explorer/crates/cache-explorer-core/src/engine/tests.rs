//! The access engine: the ten access cases with their events, order,
//! counters and changed cells; hit and miss sequences; victims under each
//! replacement policy; write-backs, write-throughs and write-arounds; miss
//! counts for compulsory, conflict and capacity examples; staleness; event
//! numbering; stopping mid-access; the invariant checks at entry ends; and
//! the internal error of a lookup that finds no way.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use super::access::access;
use super::*;
use crate::cache::{Cache, Dirtiness, Slot};
use crate::config::{
    AllocatePolicy, CacheConfig, Config, MemoryInit, Preset, Replacement, WordBytes, WritePolicy,
};
use crate::event::Counter;
use crate::limits::{MAX_ADDRESS_BITS, MAX_EVENTS_PER_ENTRY, MAX_LINES};
use crate::memory::StaleInfo;
use crate::prng::Prng;
use crate::replacement::VictimReason;
use crate::units::{LineAddr, Register, Tag, Word, WordIndex};

use EventKind::{
    Complete, Decode, Evict, Fill, Hit, MemWrite, Miss, Rank, Victim, Write, WriteBack,
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
    /// after every event, the counters add up, and no event is an error.
    fn trace(&mut self, request: Request) -> Vec<(MicroEvent, Machine)> {
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

fn load(at: u32, into: Register) -> Request {
    Request::Load {
        addr: addr(at),
        into,
    }
}

fn store(at: u32, value: u64) -> Request {
    Request::Store {
        addr: addr(at),
        value,
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
    assert_eq!(
        shape(&events[1]),
        (
            Some(addr(0x0A4)),
            Some(set(2)),
            None,
            &[
                counter(Counter::Accesses),
                counter(Counter::Loads),
                counter(Counter::Misses)
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
            assert_eq!(data, &run.machine.memory.line(line_addr(0x0A0)));
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
            counter(Counter::Misses)
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
    assert!(event(&events, Complete).changed.is_empty());
    assert_eq!(run.machine.registers, crate::cpu::Registers::default());
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
    let mut run = Run::new(preset(Preset::TwoWayLru));
    outcomes(&mut run, &requests);
    assert_eq!(
        (run.machine.counters.misses, run.machine.counters.hits),
        (2, 6)
    );
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
    let mut run = Run::new(config);
    outcomes(&mut run, &rounds(5));
    assert_eq!(
        (run.machine.counters.misses, run.machine.counters.hits),
        (15, 0)
    );
    assert_eq!(run.machine.counters.evictions, 11);
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
    let mut run = Run::new(preset(Preset::TwoWayLru));
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
    let config = config(
        128,
        2,
        Replacement::Random,
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    );
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

// ---------------------------------------------------------------------------
// Invariants at entry ends.
// ---------------------------------------------------------------------------

#[test]
fn i2_is_checked_between_accesses_only() {
    // A full four-way LRU set whose way 3 and then way 0 are invalidated, as
    // an edit does, each sent to rank last; the next miss fills way 0, which
    // ranks behind invalid way 3 until the Rank event moves it to the front.
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
    for (index, rank) in (0..).zip([3, 1, 0, 2]) {
        run.machine.cache.way_mut(set(0), way(index)).unwrap().rank = rank;
    }
    for index in [0, 3] {
        run.machine.cache.way_mut(set(0), way(index)).unwrap().slot = Slot::Invalid;
    }
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
fn request_stream(geometry: &crate::geometry::Geometry, seed: u64, count: usize) -> Vec<Request> {
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
/// every word.
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
    ];
    for (name, a, b) in counts {
        if a != b {
            changed.push(counter(name));
        }
    }
    changed.sort_unstable();
    changed
}

#[test]
fn every_event_lists_exactly_the_cells_and_counters_it_changed() {
    let mut checked = std::collections::BTreeSet::new();
    let mut ways = 1;
    while ways <= 4 {
        for replacement in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
            for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
                for allocate_policy in [
                    AllocatePolicy::WriteAllocate,
                    AllocatePolicy::WriteNoAllocate,
                ] {
                    let config = config(64, ways, replacement, write_policy, allocate_policy);
                    let geometry = config.validate().unwrap();
                    let mut run = Run::new(config);
                    for request in request_stream(&geometry, u64::from(ways), 150) {
                        let mut before = run.machine.clone();
                        let trace = run.trace(request);
                        let sequence: Vec<EventKind> =
                            trace.iter().map(|(event, _)| event.kind).collect();
                        checked.insert(format!("{sequence:?}"));
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
        ways *= 2;
    }
    // The ten cases have 23 sequences once each optional event is counted,
    // and the streams produced every one of them and nothing else.
    let expected: std::collections::BTreeSet<String> = [
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
    .iter()
    .map(|sequence| format!("{sequence:?}"))
    .collect();
    assert_eq!(expected.len(), 23);
    let missing: Vec<&String> = expected.difference(&checked).collect();
    let unexpected: Vec<&String> = checked.difference(&expected).collect();
    assert!(
        missing.is_empty() && unexpected.is_empty(),
        "never came up: {missing:?}; not a case's sequence: {unexpected:?}"
    );
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
