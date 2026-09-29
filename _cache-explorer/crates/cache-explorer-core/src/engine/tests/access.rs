//! The access engine: the ten access cases with their events, order,
//! counters and changed cells, and frame 3b's dirty eviction with its log
//! lines; hit and miss sequences; victims under each replacement policy;
//! write-backs, write-throughs and write-arounds; miss counts and the class
//! of each miss, compulsory, capacity or conflict, in known examples and
//! against a fully associative LRU cache run beside the engine; the
//! sparkline's outcomes and the stats of a run; staleness; event numbering;
//! stopping mid-access; the invariant checks at entry ends; the internal
//! error of a lookup that finds no way; and timed mode: where each case's
//! cycles are charged, the stall and its countdown, the cycle identity at
//! entry ends, and AMAT and the measured cost of fixed traces.

use super::*;

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
            (Some(way(0)), Some(Cause::Replacement)),
            "{kind:?}"
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
fn case_4_words_each_event_of_frame_3b_access_5() {
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

/// Checks that a store miss under `write_policy` and write-no-allocate
/// writes around the cache: the cache is left as it was, and memory takes the
/// value.
fn check_store_miss_writes_around(write_policy: WritePolicy) {
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

cases! {
    check_store_miss_writes_around;
    case_9_a_store_miss_under_write_back_no_allocate_writes_around_the_cache =>
        (WritePolicy::WriteBack),
    case_10_a_store_miss_under_write_through_no_allocate_writes_around_the_cache =>
        (WritePolicy::WriteThrough),
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
            [Decode, Miss, Victim, Fill, Complete],
            "{write_policy:?}"
        );
        let expected: &[EventKind] = match write_policy {
            WritePolicy::WriteBack => &[Decode, Hit, Write],
            WritePolicy::WriteThrough => &[Decode, Hit, Write, WriteThrough],
        };
        assert_eq!(kinds(&run.store(0x0A4, 7)), expected, "{write_policy:?}");
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
    let mut run = Run::new(config);
    let mut expected = Prng::new(config.seed);
    for &at in &lines {
        let trace = run.trace(load(at, Register::R1));
        assert!(
            trace.iter().all(|(event, _)| event.kind != Rank),
            "load of {at:#x}"
        );
        if let Some((_, after)) = trace.iter().find(|(event, _)| event.kind == Victim) {
            let choice = after.access.as_ref().unwrap().victim.unwrap();
            if choice.reason == VictimReason::Random {
                assert_eq!(choice.way.get(), expected.draw_way(4), "load of {at:#x}");
                let victim = trace
                    .iter()
                    .find(|(event, _)| event.kind == Victim)
                    .unwrap();
                assert_eq!(victim.0.cause, Some(Cause::Replacement), "load of {at:#x}");
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
    let mut run = Run::new(config);
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
#[track_caller]
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
        assert_eq!(
            classes(&mut run, &requests[..1]),
            [COMPULSORY],
            "{write_policy:?}"
        );
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
                    assert_eq!(shadow, expected, "{config:?}");
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
    for (index, (_, after)) in trace.iter().enumerate().skip(1) {
        assert_eq!(outcomes(after), [Outcome::Miss], "after event {index}");
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

    let stats = Stats::new(&run.machine, Mode::Advanced, &highlight);
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
        assert_eq!(
            run.machine.stale(addr(at)),
            None,
            "store of {value} to {at:#x}"
        );
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
            assert_eq!(
                event.op_index,
                OpIndex::new_unchecked(entry - 1),
                "{entry}.{seq}"
            );
            assert_eq!(event.run, 1, "{entry}.{seq}");
            assert_eq!(event.entry, Some(entry), "{entry}.{seq}");
            assert_eq!(event.seq, seq, "{entry}.{seq}");
            assert_eq!(event.label, format!("{entry}.{seq}"), "{entry}.{seq}");
            assert_eq!(event.cycle, None, "{entry}.{seq}");
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
    for (index, context) in contexts.iter().enumerate() {
        assert_eq!(context.request, load(0x2A4, Register::R3), "event {index}");
        assert_eq!(
            context.location,
            run.machine.geometry.locate(addr(0x2A4)),
            "event {index}"
        );
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

/// Checks that `last` is the last event of the access the `requests` make on
/// `config`, and that the access is in flight until then.
fn check_last_event(config: Config, requests: &[Request], last: EventKind) {
    let mut run = Run::new(config);
    let mut trace = Vec::new();
    for &request in requests {
        trace = run.trace(request);
    }
    let (event, after) = trace.last().unwrap();
    assert_eq!(event.kind, last);
    assert!(!after.in_flight());
    assert!(trace[..trace.len() - 1]
        .iter()
        .all(|(_, after)| after.in_flight()));
}

// Complete, Write, WriteThrough, MemWrite and Rank can each be last.
cases! {
    check_last_event;
    a_load_hit_ends_at_its_complete => (
        preset(Preset::DirectMapped8),
        &[load(0x0A4, Register::R1)],
        Complete,
    ),
    a_store_hit_under_write_back_ends_at_its_write => (
        direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate),
        &[store(0x0A4, 1)],
        Write,
    ),
    a_store_under_write_through_ends_at_its_write_through => (
        direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate),
        &[store(0x0A4, 1)],
        WriteThrough,
    ),
    a_store_around_the_cache_ends_at_its_memory_write => (
        direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteNoAllocate),
        &[store(0x0A4, 1)],
        MemWrite,
    ),
    a_load_that_reorders_a_two_way_set_ends_at_its_rank => (
        preset(Preset::TwoWayLru),
        &[load(0x0A4, Register::R1), load(0x1A4, Register::R1)],
        Rank,
    ),
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
#[track_caller]
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
            Flow::Stop,
            "stopped after {stop}"
        );
        assert_eq!(recorder.events.len(), stop, "stopped after {stop}");
        assert_eq!(machine, full[stop - 1].1, "stopped after {stop}");

        // A quiet sink stops in the same state.
        let mut quiet_machine = base.clone();
        let mut quiet = Quiet {
            stop_after: Some(stop),
            ..Quiet::default()
        };
        let mut out = Emitter::new(&mut quiet, OpIndex::new_unchecked(3), 0);
        access(&mut quiet_machine, load(0x2A4, Register::R3), &mut out);
        assert_eq!(quiet_machine, machine, "stopped after {stop}");
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
        let mut recording = Machine::start(config).unwrap();
        let mut quiet_machine = Machine::start(config).unwrap();
        let mut quiet = Quiet::default();
        let mut recorder = Recorder::default();
        for request in requests {
            let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(0), 0);
            access(&mut recording, request, &mut out);
            let mut out = Emitter::new(&mut quiet, OpIndex::new_unchecked(0), 0);
            access(&mut quiet_machine, request, &mut out);
        }
        assert_eq!(quiet_machine, recording, "{timing:?}");
        assert_eq!(quiet.seen, recorder.events.len(), "{timing:?}");
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
        assert_eq!(
            apply_op(&mut run.machine, &invalidate, 0).0,
            Ok(()),
            "invalidating way {index}"
        );
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

/// A machine on two-way LRU whose cache is that of a 64 B two-way geometry,
/// which has only sets 0 and 1: the lookup of 0x0A4 finds no set 2 where the
/// configuration promises one.
fn machine_with_a_smaller_cache() -> Machine {
    let smaller = config(
        64,
        2,
        Replacement::Lru,
        WritePolicy::WriteBack,
        AllocatePolicy::WriteAllocate,
    )
    .validate()
    .unwrap();
    let mut machine = Machine::start(two_way(Replacement::Lru)).unwrap();
    machine.cache = Cache::new(&smaller);
    machine
}

/// The explanation of an internal error that names `invariant`.
fn invariant_error(invariant: &str) -> String {
    format!(
        "Internal error: invariant {invariant} no longer holds. The run stopped here; please \
         report it with a share link."
    )
}

/// Checks that the load of 0x0A4 on `machine` abandons the access with an
/// error whose explanation is `explain`.
fn check_abandoned_access(mut machine: Machine, explain: &str) {
    let before = machine.clone();
    let mut recorder = Recorder::default();
    let mut out = Emitter::new(&mut recorder, OpIndex::new_unchecked(0), 10);
    assert_eq!(
        access(&mut machine, load(0x0A4, Register::R1), &mut out),
        Flow::Stop
    );
    let events = recorder.events;
    assert_eq!(kinds(&events), [Decode, EventKind::Error]);
    // The access's entry ends at its Decode; the error is an entry of its
    // own.
    assert_eq!(events[0].label, "1.1");
    let error = &events[1];
    assert_eq!((error.id, error.entry, error.seq), (11, Some(2), 1));
    assert_eq!(error.label, "2.1");
    assert_eq!((error.addr, error.set, error.way), (None, None, None));
    assert!(error.changed.is_empty());
    assert_eq!(error.cause, None);
    assert_eq!(error.explain, explain);
    // The access is abandoned before its compare: nothing counted, nothing
    // cached.
    assert_eq!(machine.access, None);
    assert_eq!(machine.entries_in_run, 2);
    assert_eq!(machine.counters, before.counters);
    assert_eq!(machine.cache, before.cache);
}

/// The smaller cache with two ways of set 0 valid and holding the same tag.
fn machine_with_a_repeated_tag_in_set_0() -> Machine {
    let mut machine = machine_with_a_smaller_cache();
    for index in [0, 1] {
        machine.cache.way_mut(set(0), way(index)).unwrap().slot = Slot::Valid {
            tag: Tag::new_unchecked(1),
            dirty: Dirtiness::Clean,
            data: vec![0; 16],
        };
    }
    machine
}

cases! {
    check_abandoned_access;
    // Set 2 is missing, so it has no ranks to permute: I1.
    a_lookup_in_a_missing_set_names_the_rank_invariant => (
        machine_with_a_smaller_cache(),
        &invariant_error("I1 (the ranks in set 2 are a permutation of 0 to 1)"),
    ),
    // An earlier set's broken invariant is named instead.
    a_lookup_after_an_earlier_broken_set_names_that_sets_invariant => (
        machine_with_a_repeated_tag_in_set_0(),
        &invariant_error("I3 (no two valid ways in set 0 hold the same tag)"),
    ),
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
#[track_caller]
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
        assert_eq!(expected.len(), 23, "sequences {index}");
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
fn case_4_timed_stamps_each_event_with_its_cycle() {
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
#[track_caller]
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
        back,
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
        back,
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
            back,
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

/// Checks that `request`, after the `setup` requests, stalls on `config`
/// timed 1/20 for the reason `line` gives, in set 2 and caused by the timing.
fn check_stall_line(config: Config, setup: &[Request], request: Request, line: &str) {
    let mut run = Run::new(timed(config, 1, 20));
    for &earlier in setup {
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

cases! {
    check_stall_line;
    a_fill_stalls_while_memory_supplies_the_line => (
        direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate),
        &[],
        load(0x0A4, Register::R1),
        "The CPU stalls for 20 cycles while memory supplies line 0x0A0.",
    ),
    a_fill_under_write_through_stalls_while_memory_supplies_the_line => (
        direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate),
        &[store(0x0A4, 7)],
        load(0x1A4, Register::R1),
        "The CPU stalls for 20 cycles while memory supplies line 0x1A0.",
    ),
    a_dirty_eviction_stalls_for_the_write_back_and_the_fetch => (
        direct_mapped(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate),
        &[store(0x0A4, 7)],
        store(0x1A4, 3),
        "The CPU stalls for 40 cycles: 20 to write line 0x0A0 back, then 20 to fetch line \
         0x1A0.",
    ),
    a_write_around_stalls_while_the_store_goes_to_memory => (
        direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteNoAllocate),
        &[],
        store(0x0A4, 7),
        "The CPU stalls for 20 cycles while the store goes to memory.",
    ),
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
    for (index, (_, after)) in trace.iter().enumerate() {
        assert_eq!(
            after.access.as_ref().unwrap().stall,
            None,
            "after event {index}"
        );
        assert_eq!(after.stall(), None, "after event {index}");
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
                    assert_eq!(counters.cycles.get(), identity(&counters), "{config:?}");
                    assert_eq!(
                        trace.last().unwrap().0.cycle,
                        Some(counters.cycles),
                        "{config:?}"
                    );
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
