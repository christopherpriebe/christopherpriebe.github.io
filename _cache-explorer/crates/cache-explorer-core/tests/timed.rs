//! Timed mode as a run's events show it, through the golden harness: the
//! cycle stamps of each access case, the totals of fixed traces, the stall
//! countdown the stamps give, the identity between cycles and counts at
//! entry boundaries, timing that adds only its stalls, and untimed runs that
//! are never stamped and never stall. The machine's own countdown, and AMAT
//! with the measured cost, are unit tests of the core until the session
//! exposes them.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::golden::{self, Fixture, Issue};
use cache_explorer_core::{
    AllocatePolicy, CacheConfig, ChangeRef, Config, Counter, Cycle, EventKind, MicroEvent, Preset,
    Replacement, Timing, Word, WritePolicy,
};

use EventKind::{
    Complete, Decode, Evict, Fill, Hit, MemWrite, Miss, Rank, Stall, Victim, WriteBack,
};

/// An access issued by hand: a load of an address, or a store of a value to
/// one.
#[derive(Clone, Copy, Debug)]
enum By {
    Load(u32),
    Store(u32, u64),
}

/// `config` in timed mode, with `hit`-cycle hits and `miss`-cycle misses.
fn timed(config: Config, hit: u32, miss: u32) -> Config {
    Config {
        timing: Some(Timing {
            hit_cycles: hit,
            miss_cycles: miss,
        }),
        ..config
    }
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

/// The events of `accesses`, issued one after another on `config` from the
/// run's start.
fn run(config: &Config, accesses: &[By]) -> Result<Vec<MicroEvent>, String> {
    let geometry = config.validate().map_err(|error| error.to_string())?;
    let addr = |at: u32| {
        geometry
            .addr(at)
            .ok_or_else(|| format!("{at:#X} does not fit the address width"))
    };
    let issues = accesses
        .iter()
        .map(|access| {
            Ok(match *access {
                By::Load(at) => Issue::Load { addr: addr(at)? },
                By::Store(at, value) => Issue::Store {
                    addr: addr(at)?,
                    value: serde_json::from_value::<Word>(serde_json::json!(value))
                        .map_err(|error| error.to_string())?,
                },
            })
        })
        .collect::<Result<Vec<Issue>, String>>()?;
    let fixture = Fixture {
        name: "timed",
        description: "",
        config: config.clone(),
        issues,
    };
    fixture.events().map_err(|error| error.to_string())
}

/// The events of one access each, in order.
fn entries(events: &[MicroEvent]) -> Vec<&[MicroEvent]> {
    events.chunk_by(|a, b| a.entry == b.entry).collect()
}

/// Each event's kind and cycle stamp.
fn stamps(events: &[MicroEvent]) -> Vec<(EventKind, Option<u64>)> {
    events
        .iter()
        .map(|event| (event.kind, event.cycle.map(Cycle::get)))
        .collect()
}

/// A pseudo-random stream of `count` loads and stores over three times as
/// many 16 B lines as `lines`, so hits, conflicts and dirty evictions all
/// happen; a linear congruential generator seeded with `seed`.
fn stream(lines: u32, seed: u64, count: usize) -> Vec<By> {
    let mut state = seed;
    let mut next = move |bound: u32| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        u32::try_from((state >> 33) % u64::from(bound)).unwrap_or(0)
    };
    (0..count)
        .map(|_| {
            let at = next(lines * 3) * 16 + next(4) * 4;
            if next(2) == 0 {
                By::Load(at)
            } else {
                By::Store(at, u64::from(next(4)))
            }
        })
        .collect()
}

#[test]
fn every_event_is_stamped_with_what_it_charged() -> Result<(), String> {
    // The compare charges the hit time, and each transfer to or from memory
    // the miss penalty: the write-back, the fill and the write-around. No
    // other event moves the count, the write-through included. Every case
    // comes up in these streams, under every policy.
    let (hit, miss) = (3, 10);
    let mut sequences = std::collections::BTreeSet::new();
    for associativity in [1, 2] {
        for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
            for allocate_policy in [
                AllocatePolicy::WriteAllocate,
                AllocatePolicy::WriteNoAllocate,
            ] {
                let config = Config {
                    cache: CacheConfig {
                        associativity,
                        ..direct_mapped(write_policy, allocate_policy).cache
                    },
                    ..timed(Preset::DirectMapped8.config(), hit, miss)
                };
                let events = run(&config, &stream(8, 7, 150))?;
                let mut count = 0;
                for event in &events {
                    count += match event.kind {
                        Hit | Miss => u64::from(hit),
                        WriteBack | Fill | MemWrite => u64::from(miss),
                        _ => 0,
                    };
                    assert_eq!(event.cycle.map(Cycle::get), Some(count), "{}", event.label);
                }
                for entry in entries(&events) {
                    let kinds: Vec<EventKind> = entry.iter().map(|event| event.kind).collect();
                    sequences.insert(format!("{kinds:?}"));
                }
            }
        }
    }
    // The ten cases' sequences with their stalls, once each optional event
    // is counted.
    assert_eq!(sequences.len(), 23, "{sequences:#?}");
    Ok(())
}

#[test]
fn frame_3b_charges_the_miss_at_its_transfers() -> Result<(), String> {
    // The entry totals are the design's; within the dirty eviction, the
    // write-back row reads 85 where the design reads 105, and the fill 105.
    let events = run(
        &timed(Preset::TwoWayLru.config(), 1, 20),
        &[
            By::Store(0x0B0, 5),
            By::Load(0x0A4),
            By::Store(0x1A4, 7),
            By::Load(0x0A8),
            By::Load(0x2A4),
        ],
    )?;
    let totals: Vec<Option<u64>> = entries(&events)
        .iter()
        .map(|entry| entry.last().and_then(|event| event.cycle).map(Cycle::get))
        .collect();
    assert_eq!(totals, [Some(21), Some(42), Some(63), Some(64), Some(105)]);
    let last = entries(&events).last().copied().unwrap_or_default();
    assert_eq!(
        stamps(last),
        [
            (Decode, Some(64)),
            (Miss, Some(65)),
            (Victim, Some(65)),
            (Stall, Some(65)),
            (WriteBack, Some(85)),
            (Evict, Some(85)),
            (Fill, Some(105)),
            (Complete, Some(105)),
            (Rank, Some(105)),
        ]
    );
    let labels: Vec<&str> = last.iter().map(|event| event.label.as_str()).collect();
    assert_eq!(
        labels,
        ["5.1", "5.2", "5.3", "5.4", "5.5", "5.6", "5.7", "5.8", "5.9"]
    );
    Ok(())
}

#[test]
fn the_stamps_count_each_stall_down_to_its_last_transfer() -> Result<(), String> {
    // A stall lasts one miss penalty per transfer: two with a write-back,
    // one otherwise. The event that ends the last transfer, the fill or the
    // write-around, is stamped exactly that long after the stall, and the
    // cycles left shrink in between, the write-back's miss penalty first.
    let (hit, miss) = (2, 7);
    let mut stalls = [0_usize; 2];
    for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
        for allocate_policy in [
            AllocatePolicy::WriteAllocate,
            AllocatePolicy::WriteNoAllocate,
        ] {
            let config = timed(direct_mapped(write_policy, allocate_policy), hit, miss);
            let events = run(&config, &stream(8, 3, 200))?;
            for entry in entries(&events) {
                let kinds: Vec<EventKind> = entry.iter().map(|event| event.kind).collect();
                let count = |kind| kinds.iter().filter(|k| **k == kind).count();
                assert_eq!(count(Stall), count(Miss), "{kinds:?}");
                let Some(at) = kinds.iter().position(|kind| *kind == Stall) else {
                    continue;
                };
                // After Victim when the miss allocates, after Miss otherwise.
                let before = if kinds.contains(&MemWrite) {
                    Miss
                } else {
                    Victim
                };
                assert_eq!(kinds.get(at - 1), Some(&before), "{kinds:?}");
                let transfers = 1 + count(WriteBack);
                stalls[transfers - 1] += 1;
                let total = u64::from(miss) * transfers as u64;
                let start = entry[at].cycle.map(Cycle::get).unwrap_or_default();
                let end = entry
                    .iter()
                    .position(|event| matches!(event.kind, Fill | MemWrite))
                    .and_then(|last| entry[last].cycle)
                    .map(Cycle::get);
                assert_eq!(end, Some(start + total), "{kinds:?}");
                let remaining: Vec<u64> = entry[at..]
                    .iter()
                    .take_while(|event| !matches!(event.kind, Fill | MemWrite))
                    .filter_map(|event| event.cycle)
                    .map(|cycle| start + total - cycle.get())
                    .collect();
                let expected: Vec<u64> = if transfers == 2 {
                    vec![total, total - u64::from(miss), total - u64::from(miss)]
                } else if kinds.contains(&Evict) {
                    vec![total, total]
                } else {
                    vec![total]
                };
                assert_eq!(remaining, expected, "{kinds:?}");
            }
        }
    }
    // The stream stalled for one transfer and for two.
    assert!(stalls.iter().all(|count| *count > 0), "{stalls:?}");
    Ok(())
}

#[test]
fn cycles_add_up_at_every_entry_boundary() -> Result<(), String> {
    // cycles = accesses × hit + misses × miss + writebacks × miss once each
    // access is over, counting from the events alone.
    for (hit, miss) in [(1, 20), (3, 1)] {
        for replacement in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
            for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
                for allocate_policy in [
                    AllocatePolicy::WriteAllocate,
                    AllocatePolicy::WriteNoAllocate,
                ] {
                    let config = Config {
                        cache: CacheConfig {
                            associativity: 2,
                            replacement,
                            write_policy,
                            allocate_policy,
                            ..Preset::DirectMapped8.config().cache
                        },
                        ..timed(Preset::DirectMapped8.config(), hit, miss)
                    };
                    let events = run(&config, &stream(8, 11, 150))?;
                    let (mut accesses, mut misses, mut writebacks) = (0_u64, 0_u64, 0_u64);
                    for entry in entries(&events) {
                        accesses += 1;
                        misses += entry.iter().filter(|event| event.kind == Miss).count() as u64;
                        writebacks +=
                            entry.iter().filter(|event| event.kind == WriteBack).count() as u64;
                        let expected =
                            accesses * u64::from(hit) + (misses + writebacks) * u64::from(miss);
                        let stamp = entry.last().and_then(|event| event.cycle);
                        assert_eq!(stamp.map(Cycle::get), Some(expected));
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn timing_adds_its_stalls_and_its_stamps_and_nothing_else() -> Result<(), String> {
    // The same accesses, untimed and timed: take away the stalls, the stamps
    // and the cycle counter, and the events are the same, wording included.
    let accesses = stream(8, 5, 150);
    for config in [
        Preset::TwoWayLru.config(),
        direct_mapped(WritePolicy::WriteThrough, AllocatePolicy::WriteNoAllocate),
    ] {
        let untimed = run(&config, &accesses)?;
        let with_timing = run(&timed(config, 1, 20), &accesses)?;
        let stripped: Vec<(EventKind, &str, Vec<ChangeRef>, u32)> = with_timing
            .iter()
            .filter(|event| event.kind != Stall)
            .map(|event| {
                let changed = event
                    .changed
                    .iter()
                    .copied()
                    .filter(|cell| {
                        *cell
                            != ChangeRef::Counter {
                                counter: Counter::Cycles,
                            }
                    })
                    .collect();
                (
                    event.kind,
                    event.explain.as_str(),
                    changed,
                    event.entry.unwrap_or(0),
                )
            })
            .collect();
        let plain: Vec<(EventKind, &str, Vec<ChangeRef>, u32)> = untimed
            .iter()
            .map(|event| {
                (
                    event.kind,
                    event.explain.as_str(),
                    event.changed.clone(),
                    event.entry.unwrap_or(0),
                )
            })
            .collect();
        assert_eq!(stripped, plain);
        assert!(with_timing.iter().all(|event| event.cycle.is_some()));
    }
    Ok(())
}

#[test]
fn the_fixtures_stamp_only_timed_runs_and_show_every_kind_of_stall() -> Result<(), String> {
    // Untimed fixtures are never stamped and never stall. The timed ones are
    // stamped throughout, and between them stall for a fill, for a
    // write-back and a fill, and for a write-around.
    let mut kinds = std::collections::BTreeSet::new();
    for fixture in golden::fixtures() {
        let events = fixture.events().map_err(|error| error.to_string())?;
        if fixture.config.timing.is_none() {
            assert!(
                events
                    .iter()
                    .all(|event| event.cycle.is_none() && event.kind != Stall),
                "{}",
                fixture.name
            );
            continue;
        }
        assert!(
            events.iter().all(|event| event.cycle.is_some()),
            "{}",
            fixture.name
        );
        for entry in entries(&events) {
            let has = |kind| entry.iter().any(|event| event.kind == kind);
            if has(Stall) {
                kinds.insert(if has(WriteBack) {
                    "write-back and fill"
                } else if has(MemWrite) {
                    "write-around"
                } else {
                    "fill"
                });
            }
        }
    }
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        ["fill", "write-around", "write-back and fill"]
    );
    Ok(())
}
