//! The machine's behaviour as the page sees it, through the session's
//! snapshot and stats: the ten access cases with their events in order, the
//! counters each moves, and the cache and memory they leave; victims under
//! LRU, FIFO and seeded random replacement; a dirty victim still dirty at
//! its write-back and gone at its eviction; write-through and write-around
//! counts kept apart; allocating against not allocating; and the classes of
//! misses in known examples. The engine's unit tests check the same cases on
//! the machine itself, with its every cell diffed event by event.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::{
    AllocatePolicy, CacheConfig, Config, EventKind, Preset, Replacement, WritePolicy,
};

use support::{cases, cells, load, memory, moved, pos, session, store, two_way, victims};
use EventKind::{Evict, Miss, WriteBack};

/// The tests' helpers. They sit in a module compiled for tests only, where a
/// failed `unwrap` is a failed test, as in the tests themselves.
#[cfg(test)]
mod support {
    use cache_explorer_core::{
        AccessKind, Addr, AllocatePolicy, CacheConfig, Config, EventKind, EventPos, Op, Preset,
        Session, State, Word, WritePolicy,
    };

    use EventKind::{
        Complete, Decode, Evict, Fill, Hit, MemWrite, Miss, Rank, Victim, Write, WriteBack,
        WriteThrough,
    };

    /// `value` as an address, as one read from JSON is.
    fn addr(value: u32) -> Addr {
        serde_json::from_value(value.into()).unwrap()
    }

    /// Position `value`, as one read from JSON is.
    pub(crate) fn pos(value: u32) -> EventPos {
        serde_json::from_value(value.into()).unwrap()
    }

    /// A load of `at` issued by hand.
    pub(crate) fn load(at: u32) -> Op {
        Op::Issue {
            kind: AccessKind::Read,
            addr: addr(at),
            value: None,
        }
    }

    /// A store of `value` to `at` issued by hand.
    pub(crate) fn store(at: u32, value: u64) -> Op {
        let value: Word = serde_json::from_value(value.into()).unwrap();
        Op::Issue {
            kind: AccessKind::Write,
            addr: addr(at),
            value: Some(value),
        }
    }

    /// A session on `config` with `ops` applied.
    pub(crate) fn session(config: Config, ops: impl IntoIterator<Item = Op>) -> Session {
        let mut session = Session::with_config(config).unwrap();
        for op in ops {
            session.apply(op).unwrap();
        }
        session
    }

    /// `two-way-lru` with the write and allocate policies given: 128 B of 16 B
    /// lines in four sets of two ways, 12-bit addresses, pattern memory, where
    /// 0x0A4 reads 53.
    pub(crate) fn two_way(write_policy: WritePolicy, allocate_policy: AllocatePolicy) -> Config {
        Config {
            cache: CacheConfig {
                write_policy,
                allocate_policy,
                ..Preset::TwoWayLru.config().cache
            },
            ..Preset::TwoWayLru.config()
        }
    }

    /// The counters that differ between two snapshots, by name.
    pub(crate) fn moved(before: &State, after: &State) -> Vec<String> {
        let before = serde_json::to_value(before.stats.counters).unwrap();
        let after = serde_json::to_value(after.stats.counters).unwrap();
        let names = [
            "accesses",
            "loads",
            "stores",
            "hits",
            "misses",
            "evictions",
            "writebacks",
            "writethroughs",
            "writearounds",
            "fills",
            "cycles",
            "compulsory",
            "capacity",
            "conflict",
        ];
        names
            .into_iter()
            .filter(|name| before[name] != after[name])
            .map(str::to_owned)
            .collect()
    }

    /// One access case: the configuration, what comes before, the access, the
    /// kinds of its events, the counters it moves, and the state it leaves.
    pub(crate) struct Case {
        /// The case's number, 1 to 10.
        pub(crate) number: u8,
        /// The configuration.
        pub(crate) config: Config,
        /// The ops before the access.
        pub(crate) setup: Vec<Op>,
        /// The access.
        pub(crate) access: Op,
        /// Its events' kinds, in order.
        pub(crate) kinds: Vec<EventKind>,
        /// The counters it moves.
        pub(crate) counters: &'static [&'static str],
        /// A check of the state it leaves.
        pub(crate) check: fn(&State),
    }

    /// The cells of way `way` of set `set` in `state`'s cache: whether valid,
    /// its tag, whether dirty, and its words.
    pub(crate) fn cells(
        state: &State,
        set: usize,
        way: usize,
    ) -> (bool, Option<&str>, bool, Vec<&str>) {
        let view = &state.cache.sets[set].ways[way];
        (
            view.valid,
            view.tag_text.as_deref(),
            view.dirty,
            view.words.iter().map(String::as_str).collect(),
        )
    }

    /// The value memory holds at the word `word` of the line in row `row` of
    /// `session`'s memory table, and whether it is stale.
    pub(crate) fn memory(session: &Session, row: u32, word: usize) -> (String, bool) {
        let line = session.memory_lines(row, 1).remove(0);
        let word = &line.words[word];
        (word.text.clone(), word.stale.is_some())
    }

    /// The ten access cases, loads first.
    pub(crate) fn cases() -> Vec<Case> {
        let mut cases = load_cases();
        cases.extend(store_cases());
        cases
    }

    /// Cases 1 to 4: a load that hits, and loads that miss into an invalid way,
    /// over a clean victim and over a dirty one.
    fn load_cases() -> Vec<Case> {
        let write_back = two_way(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate);
        vec![
            Case {
                number: 1,
                config: write_back.clone(),
                setup: vec![load(0x0A4), load(0x1A4)],
                access: load(0x0A8),
                kinds: vec![Decode, Hit, Complete, Rank],
                counters: &["accesses", "loads", "hits"],
                check: |state| {
                    assert_eq!(state.cpu.registers[1].text, "66");
                    let ranks: Vec<u32> = state.cache.sets[2]
                        .ways
                        .iter()
                        .map(|way| way.rank.unwrap().value)
                        .collect();
                    assert_eq!(ranks, [0, 1]);
                },
            },
            Case {
                number: 2,
                config: write_back.clone(),
                setup: vec![],
                access: load(0x0A4),
                kinds: vec![Decode, Miss, Victim, Fill, Complete],
                counters: &["accesses", "loads", "misses", "fills", "compulsory"],
                check: |state| {
                    assert_eq!(
                        cells(state, 2, 0),
                        (true, Some("0x02"), false, vec!["40", "53", "66", "79"])
                    );
                    assert_eq!(state.cpu.registers[1].text, "53");
                },
            },
            Case {
                number: 3,
                config: write_back.clone(),
                setup: vec![load(0x0A4), load(0x1A4)],
                access: load(0x2A4),
                kinds: vec![Decode, Miss, Victim, Evict, Fill, Complete, Rank],
                counters: &[
                    "accesses",
                    "loads",
                    "misses",
                    "evictions",
                    "fills",
                    "compulsory",
                ],
                check: |state| {
                    // The least recently used line, 0x0A0 in way 0, made room.
                    assert_eq!(cells(state, 2, 0).1, Some("0x0A"));
                    assert_eq!(cells(state, 2, 1).1, Some("0x06"));
                },
            },
            Case {
                number: 4,
                config: write_back.clone(),
                setup: vec![store(0x0B0, 5), load(0x0A4), store(0x1A4, 7), load(0x0A8)],
                access: load(0x2A4),
                kinds: vec![Decode, Miss, Victim, WriteBack, Evict, Fill, Complete, Rank],
                counters: &[
                    "accesses",
                    "loads",
                    "misses",
                    "evictions",
                    "writebacks",
                    "fills",
                    "compulsory",
                ],
                check: |state| {
                    assert_eq!(
                        cells(state, 2, 1),
                        (true, Some("0x0A"), false, vec!["55", "68", "81", "94"])
                    );
                    assert_eq!(state.cpu.registers[1].text, "68");
                },
            },
        ]
    }

    /// Cases 5 to 10: stores that hit and miss under each write and allocate
    /// policy.
    fn store_cases() -> Vec<Case> {
        let write_back = two_way(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate);
        let write_through = two_way(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate);
        let around_back = two_way(WritePolicy::WriteBack, AllocatePolicy::WriteNoAllocate);
        let around_through = two_way(WritePolicy::WriteThrough, AllocatePolicy::WriteNoAllocate);
        vec![
            Case {
                number: 5,
                config: write_back.clone(),
                setup: vec![load(0x0A4)],
                access: store(0x0A4, 9),
                kinds: vec![Decode, Hit, Write],
                counters: &["accesses", "stores", "hits"],
                check: |state| {
                    assert_eq!(
                        cells(state, 2, 0),
                        (true, Some("0x02"), true, vec!["40", "9", "66", "79"])
                    );
                },
            },
            Case {
                number: 6,
                config: write_through.clone(),
                setup: vec![load(0x0A4)],
                access: store(0x0A4, 9),
                kinds: vec![Decode, Hit, Write, WriteThrough],
                counters: &["accesses", "stores", "hits", "writethroughs"],
                check: |state| {
                    assert_eq!(
                        cells(state, 2, 0),
                        (true, Some("0x02"), false, vec!["40", "9", "66", "79"])
                    );
                },
            },
            Case {
                number: 7,
                config: write_back,
                setup: vec![],
                access: store(0x0A4, 9),
                kinds: vec![Decode, Miss, Victim, Fill, Write],
                counters: &["accesses", "stores", "misses", "fills", "compulsory"],
                check: |state| {
                    assert_eq!(
                        cells(state, 2, 0),
                        (true, Some("0x02"), true, vec!["40", "9", "66", "79"])
                    );
                },
            },
            Case {
                number: 8,
                config: write_through,
                setup: vec![],
                access: store(0x0A4, 9),
                kinds: vec![Decode, Miss, Victim, Fill, Write, WriteThrough],
                counters: &[
                    "accesses",
                    "stores",
                    "misses",
                    "writethroughs",
                    "fills",
                    "compulsory",
                ],
                check: |state| {
                    assert_eq!(
                        cells(state, 2, 0),
                        (true, Some("0x02"), false, vec!["40", "9", "66", "79"])
                    );
                },
            },
            Case {
                number: 9,
                config: around_back,
                setup: vec![],
                access: store(0x0A4, 9),
                kinds: vec![Decode, Miss, MemWrite],
                counters: &["accesses", "stores", "misses", "writearounds", "compulsory"],
                check: |state| {
                    assert!(!cells(state, 2, 0).0 && !cells(state, 2, 1).0);
                },
            },
            Case {
                number: 10,
                config: around_through,
                setup: vec![],
                access: store(0x0A4, 9),
                kinds: vec![Decode, Miss, MemWrite],
                counters: &["accesses", "stores", "misses", "writearounds", "compulsory"],
                check: |state| {
                    assert!(!cells(state, 2, 0).0 && !cells(state, 2, 1).0);
                },
            },
        ]
    }

    /// The ways the victims of `session`'s run went to, in order.
    pub(crate) fn victims(session: &Session) -> Vec<u32> {
        session
            .events()
            .iter()
            .filter(|event| event.kind == Victim)
            .map(|event| event.way.unwrap().get())
            .collect()
    }
}

#[test]
fn each_access_case_has_its_events_counters_and_result() {
    for case in cases() {
        let mut session = session(case.config, case.setup);
        let before = session.state();
        let events = session.apply(case.access).unwrap();
        let kinds: Vec<EventKind> = events.iter().map(|event| event.kind).collect();
        assert_eq!(kinds, case.kinds, "case {}", case.number);
        let after = session.state();
        assert_eq!(
            moved(&before, &after),
            case.counters,
            "case {}",
            case.number
        );
        (case.check)(&after);
        // Every access ends between entries, where hits and misses add up.
        let counters = after.stats.counters;
        assert_eq!(counters.hits + counters.misses, counters.accesses);
        assert_eq!(
            counters.compulsory + counters.capacity + counters.conflict,
            counters.misses
        );
    }
}

#[test]
fn stores_leave_memory_as_their_write_policy_says() {
    // Write-back: the cached word is newer, and memory's is stale.
    let back = session(
        two_way(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate),
        [load(0x0A4), store(0x0A4, 9)],
    );
    assert_eq!(memory(&back, 0, 1), ("53".to_owned(), true));
    // Write-through: memory has it too.
    let through = session(
        two_way(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate),
        [load(0x0A4), store(0x0A4, 9)],
    );
    assert_eq!(memory(&through, 0, 1), ("9".to_owned(), false));
    // Around the cache, under either write policy: only memory changes.
    for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
        let around = session(
            two_way(write_policy, AllocatePolicy::WriteNoAllocate),
            [store(0x0A4, 9)],
        );
        assert_eq!(memory(&around, 0, 1), ("9".to_owned(), false));
        let counters = around.stats().counters;
        assert_eq!((counters.writearounds, counters.writethroughs), (1, 0));
        // The next load of the line misses: nothing was allocated.
        let mut around = around;
        let events = around.apply(load(0x0A4)).unwrap();
        assert_eq!(events[1].kind, Miss);
        assert_eq!(around.state().cpu.registers[1].text, "9");
    }
    // A write-through store that hits counts as a write-through, never a
    // write-around.
    let counters = through.stats().counters;
    assert_eq!((counters.writethroughs, counters.writearounds), (1, 0));
}

#[test]
fn a_dirty_victim_is_written_back_while_still_dirty_and_cleared_at_its_eviction() {
    let mut session = session(
        two_way(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate),
        [
            store(0x0B0, 5),
            load(0x0A4),
            store(0x1A4, 7),
            load(0x0A8),
            load(0x2A4),
        ],
    );
    // 5.4, the write-back: memory has the word, the victim is still dirty.
    session.seek(pos(24));
    let state = session.state();
    assert_eq!(state.event.as_ref().unwrap().kind, WriteBack);
    assert_eq!(
        cells(&state, 2, 1),
        (true, Some("0x06"), true, vec!["96", "7", "25", "38"])
    );
    assert_eq!(memory(&session, 2, 1), ("7".to_owned(), false));
    // Before it, memory's copy was stale.
    session.seek(pos(23));
    assert_eq!(memory(&session, 2, 1), ("12".to_owned(), true));
    // 5.5, the eviction: valid, tag, dirty and data go together.
    session.seek(pos(25));
    let state = session.state();
    assert_eq!(state.event.as_ref().unwrap().kind, Evict);
    assert_eq!(cells(&state, 2, 1), (false, None, false, vec![]));
}

#[test]
fn victims_follow_the_replacement_policy() {
    // Flow B: four lines of set 0, one of them used twice.
    let flow_b = [
        load(0x000),
        load(0x040),
        load(0x000),
        load(0x080),
        load(0x0C0),
    ];
    let config = |replacement, seed| Config {
        cache: CacheConfig {
            replacement,
            ..Preset::TwoWayLru.config().cache
        },
        seed,
        ..Preset::TwoWayLru.config()
    };
    // LRU keeps the line used again: 0x080 takes way 1, then 0x0C0 way 0.
    let lru = session(config(Replacement::Lru, 12_345), flow_b.clone());
    assert_eq!(victims(&lru), [0, 1, 1, 0]);
    // FIFO evicts the line filled first, whatever hit since.
    let fifo = session(config(Replacement::Fifo, 12_345), flow_b.clone());
    assert_eq!(victims(&fifo), [0, 1, 0, 1]);
    assert_eq!(
        fifo.state().cache.sets[0].ways[0].tag_text.as_deref(),
        Some("0x02")
    );
    // Random draws from the run's seed: the same seed, the same victims,
    // each way holding the line its victim event says.
    let random = session(config(Replacement::Random, 1), flow_b.clone());
    let again = session(config(Replacement::Random, 1), flow_b);
    assert_eq!(victims(&random), victims(&again));
    assert_eq!(&victims(&random)[..2], [0, 1]);
    let last = *victims(&random).last().unwrap();
    let held = &random.state().cache.sets[0].ways[usize::try_from(last).unwrap()];
    assert_eq!(held.tag_text.as_deref(), Some("0x03"));
    // Random keeps no order to show.
    assert!(random.state().cache.sets[0].ways[0].rank.is_none());
}

#[test]
fn misses_are_classed_compulsory_capacity_or_conflict() {
    // Two lines that share set 0 of a direct-mapped cache, alternated: the
    // first two misses are compulsory, and the six after them conflicts,
    // since a fully associative cache as large would hold both.
    let alternation = (0..8).map(|i| load(if i % 2 == 0 { 0x000 } else { 0x080 }));
    let counters = session(Preset::DirectMapped8.config(), alternation)
        .stats()
        .counters;
    assert_eq!(
        (
            counters.misses,
            counters.compulsory,
            counters.conflict,
            counters.capacity
        ),
        (8, 2, 6, 0)
    );
    // Twice as many lines as the cache holds, swept twice: the second sweep
    // misses for capacity, as a fully associative cache would.
    let lines = Preset::DirectMapped8.config().validate().unwrap().lines();
    let sweep = (0..2 * lines).map(|line| load(line * 16));
    let counters = session(Preset::DirectMapped8.config(), sweep.clone().chain(sweep))
        .stats()
        .counters;
    assert_eq!(
        (counters.compulsory, counters.capacity, counters.conflict),
        (2 * lines, 2 * lines, 0)
    );
    // A store around the cache fills nothing, so the load of its line after
    // it is compulsory too.
    let no_allocate = Config {
        cache: CacheConfig {
            allocate_policy: AllocatePolicy::WriteNoAllocate,
            ..Preset::DirectMapped8.config().cache
        },
        ..Preset::DirectMapped8.config()
    };
    let counters = session(no_allocate, [store(0x0A4, 7), load(0x0A4)])
        .stats()
        .counters;
    assert_eq!((counters.misses, counters.compulsory), (2, 2));
}

#[test]
fn hits_and_misses_accumulate_in_the_stats_and_the_sparkline() {
    let session = session(
        Preset::DirectMapped8.config(),
        [load(0x0A4), load(0x0A8), load(0x1A4), load(0x0A4)],
    );
    let stats = session.stats();
    assert_eq!(
        (
            stats.counters.hits,
            stats.counters.misses,
            stats.counters.accesses
        ),
        (1, 3, 4)
    );
    assert_eq!(stats.hit_rate.as_ref().unwrap().text, "25%");
    assert_eq!(stats.recent_text.as_deref(), Some("1 hit, 3 misses"));
    assert_eq!(stats.traffic.line_reads, 3);
    assert_eq!(stats.traffic.bytes_read, 48);
}
