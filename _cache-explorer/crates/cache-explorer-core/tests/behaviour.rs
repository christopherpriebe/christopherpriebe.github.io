//! The machine's behaviour as the page sees it, through the session's
//! snapshot and stats: the ten access cases with their events in order and
//! the cells and counters each event lists as changed, the counters each
//! case moves, and the cache and memory they leave; victims under
//! LRU, FIFO and seeded random replacement; a dirty victim still dirty at
//! its write-back and gone at its eviction; write-through and write-around
//! counts kept apart; allocating against not allocating; and the classes of
//! misses in known examples, edits that take lines in and out of the cache
//! included. The engine's unit tests check the same cases on the machine
//! itself, with its every cell diffed event by event.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::{
    AllocatePolicy, CacheConfig, Config, EventKind, Op, Preset, Replacement, WritePolicy,
};

use support::{cases, cells, load, memory, moved, pos, session, short, store, two_way, victims};
use EventKind::{Evict, Miss, WriteBack};

/// The tests' helpers. They sit in a module compiled for tests only, where a
/// failed `unwrap` is a failed test, as in the tests themselves.
#[cfg(test)]
mod support {
    use cache_explorer_core::{
        AccessKind, Addr, AllocatePolicy, CacheConfig, ChangeRef, Config, EventKind, EventPos, Op,
        Preset, Session, State, Word, WritePolicy,
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

    /// One access case: the configuration, what comes before, the access, its
    /// events with what each changed, the counters it moves, and the state it
    /// leaves.
    pub(crate) struct Case {
        /// The case's number, 1 to 10.
        pub(crate) number: u8,
        /// The configuration.
        pub(crate) config: Config,
        /// The ops before the access.
        pub(crate) setup: Vec<Op>,
        /// The access.
        pub(crate) access: Op,
        /// Its events' kinds, in order, each with the cells and counters it
        /// lists as changed, in short (see [`short`]).
        pub(crate) events: Vec<(EventKind, &'static [&'static str])>,
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

    /// What a fill of way 0 of set 2 changes: the way's valid bit, its tag and
    /// every word, as a different line now occupies it, and the fill count.
    const FILL_WAY_0: &[&str] = &[
        "valid 2.0",
        "tag 2.0",
        "word 2.0.0",
        "word 2.0.1",
        "word 2.0.2",
        "word 2.0.3",
        "fills",
    ];

    /// What a fill of way 1 of set 2 changes.
    const FILL_WAY_1: &[&str] = &[
        "valid 2.1",
        "tag 2.1",
        "word 2.1.0",
        "word 2.1.1",
        "word 2.1.2",
        "word 2.1.3",
        "fills",
    ];

    /// What the eviction of a clean line from way 0 of set 2 changes: valid,
    /// tag and every word go together.
    const EVICT_WAY_0: &[&str] = &[
        "valid 2.0",
        "tag 2.0",
        "word 2.0.0",
        "word 2.0.1",
        "word 2.0.2",
        "word 2.0.3",
        "evictions",
    ];

    /// What the eviction of a dirty line from way 1 of set 2 changes: its
    /// dirty bit too.
    const EVICT_DIRTY_WAY_1: &[&str] = &[
        "valid 2.1",
        "tag 2.1",
        "dirty 2.1",
        "word 2.1.0",
        "word 2.1.1",
        "word 2.1.2",
        "word 2.1.3",
        "evictions",
    ];

    /// What a rank event changes in set 2 when its two ways trade places.
    const RANKS: &[&str] = &["rank 2.0", "rank 2.1"];

    /// What a load's tag compare changes when it misses a line never filled.
    const LOAD_MISS: &[&str] = &["accesses", "loads", "misses", "compulsory"];

    /// What a store's tag compare changes when it misses a line never filled.
    const STORE_MISS: &[&str] = &["accesses", "stores", "misses", "compulsory"];

    /// The ten access cases, loads first.
    pub(crate) fn cases() -> Vec<Case> {
        let mut cases = loads_without_eviction();
        cases.extend(loads_with_eviction());
        cases.extend(store_hits());
        cases.extend(store_misses());
        cases
    }

    /// Cases 1 and 2: a load that hits, and one that misses into an invalid
    /// way.
    fn loads_without_eviction() -> Vec<Case> {
        let write_back = two_way(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate);
        vec![
            Case {
                number: 1,
                config: write_back,
                setup: vec![load(0x0A4), load(0x1A4)],
                access: load(0x0A8),
                events: vec![
                    (Decode, &[]),
                    (Hit, &["accesses", "loads", "hits"]),
                    (Complete, &["r1"]),
                    (Rank, RANKS),
                ],
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
                config: write_back,
                setup: vec![],
                access: load(0x0A4),
                events: vec![
                    (Decode, &[]),
                    (Miss, LOAD_MISS),
                    (Victim, &[]),
                    (Fill, FILL_WAY_0),
                    (Complete, &["r1"]),
                ],
                counters: &["accesses", "loads", "misses", "fills", "compulsory"],
                check: |state| {
                    assert_eq!(
                        cells(state, 2, 0),
                        (true, Some("0x02"), false, vec!["40", "53", "66", "79"])
                    );
                    assert_eq!(state.cpu.registers[1].text, "53");
                },
            },
        ]
    }

    /// Cases 3 and 4: loads that miss over a clean victim and over a dirty
    /// one.
    fn loads_with_eviction() -> Vec<Case> {
        let write_back = two_way(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate);
        vec![
            Case {
                number: 3,
                config: write_back,
                setup: vec![load(0x0A4), load(0x1A4)],
                access: load(0x2A4),
                events: vec![
                    (Decode, &[]),
                    (Miss, LOAD_MISS),
                    (Victim, &[]),
                    (Evict, EVICT_WAY_0),
                    (Fill, FILL_WAY_0),
                    (Complete, &["r1"]),
                    (Rank, RANKS),
                ],
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
                config: write_back,
                setup: vec![store(0x0B0, 5), load(0x0A4), store(0x1A4, 7), load(0x0A8)],
                access: load(0x2A4),
                events: vec![
                    (Decode, &[]),
                    (Miss, LOAD_MISS),
                    (Victim, &[]),
                    // Only the word the store made newer differs in memory.
                    (WriteBack, &["memory 0x1A4", "writebacks"]),
                    (Evict, EVICT_DIRTY_WAY_1),
                    (Fill, FILL_WAY_1),
                    (Complete, &["r1"]),
                    (Rank, RANKS),
                ],
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

    /// Cases 5 and 6: stores that hit, under each write policy.
    fn store_hits() -> Vec<Case> {
        vec![
            Case {
                number: 5,
                config: two_way(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate),
                setup: vec![load(0x0A4)],
                access: store(0x0A4, 9),
                events: vec![
                    (Decode, &[]),
                    (Hit, &["accesses", "stores", "hits"]),
                    (Write, &["dirty 2.0", "word 2.0.1"]),
                ],
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
                config: two_way(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate),
                setup: vec![load(0x0A4)],
                access: store(0x0A4, 9),
                events: vec![
                    (Decode, &[]),
                    (Hit, &["accesses", "stores", "hits"]),
                    (Write, &["word 2.0.1"]),
                    (WriteThrough, &["memory 0x0A4", "writethroughs"]),
                ],
                counters: &["accesses", "stores", "hits", "writethroughs"],
                check: |state| {
                    assert_eq!(
                        cells(state, 2, 0),
                        (true, Some("0x02"), false, vec!["40", "9", "66", "79"])
                    );
                },
            },
        ]
    }

    /// Cases 7 to 10: stores that miss, under each write and allocate policy.
    fn store_misses() -> Vec<Case> {
        vec![
            Case {
                number: 7,
                config: two_way(WritePolicy::WriteBack, AllocatePolicy::WriteAllocate),
                setup: vec![],
                access: store(0x0A4, 9),
                events: vec![
                    (Decode, &[]),
                    (Miss, STORE_MISS),
                    (Victim, &[]),
                    (Fill, FILL_WAY_0),
                    (Write, &["dirty 2.0", "word 2.0.1"]),
                ],
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
                config: two_way(WritePolicy::WriteThrough, AllocatePolicy::WriteAllocate),
                setup: vec![],
                access: store(0x0A4, 9),
                events: vec![
                    (Decode, &[]),
                    (Miss, STORE_MISS),
                    (Victim, &[]),
                    (Fill, FILL_WAY_0),
                    (Write, &["word 2.0.1"]),
                    (WriteThrough, &["memory 0x0A4", "writethroughs"]),
                ],
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
                config: two_way(WritePolicy::WriteBack, AllocatePolicy::WriteNoAllocate),
                setup: vec![],
                access: store(0x0A4, 9),
                events: vec![
                    (Decode, &[]),
                    (Miss, STORE_MISS),
                    (MemWrite, &["memory 0x0A4", "writearounds"]),
                ],
                counters: &["accesses", "stores", "misses", "writearounds", "compulsory"],
                check: |state| {
                    assert!(!cells(state, 2, 0).0 && !cells(state, 2, 1).0);
                },
            },
            Case {
                number: 10,
                config: two_way(WritePolicy::WriteThrough, AllocatePolicy::WriteNoAllocate),
                setup: vec![],
                access: store(0x0A4, 9),
                events: vec![
                    (Decode, &[]),
                    (Miss, STORE_MISS),
                    (MemWrite, &["memory 0x0A4", "writearounds"]),
                ],
                counters: &["accesses", "stores", "misses", "writearounds", "compulsory"],
                check: |state| {
                    assert!(!cells(state, 2, 0).0 && !cells(state, 2, 1).0);
                },
            },
        ]
    }

    /// The cells and counters of `changed` in short, in its order: `valid
    /// 2.0` for set 2, way 0, `word 2.0.1` for its word 1, `memory 0x0A4`,
    /// `r1`, `pc`, or a counter's name.
    pub(crate) fn short(changed: &[ChangeRef]) -> Vec<String> {
        changed
            .iter()
            .map(|cell| {
                let value = serde_json::to_value(cell).unwrap();
                let at = |key: &str| value[key].as_u64().unwrap();
                let name = |key: &str| value[key].as_str().unwrap().to_owned();
                match name("cell").as_str() {
                    "word" => format!("word {}.{}.{}", at("set"), at("way"), at("word")),
                    "memory" => format!("memory 0x{:03X}", at("addr")),
                    "register" => name("register"),
                    "pc" => "pc".to_owned(),
                    "counter" => name("counter"),
                    field => format!("{field} {}.{}", at("set"), at("way")),
                }
            })
            .collect()
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
fn each_access_case_has_its_events_what_each_changed_its_counters_and_result() {
    for case in cases() {
        let mut session = session(case.config, case.setup);
        let before = session.state();
        let events = session.apply(case.access).unwrap();
        let listed: Vec<(EventKind, Vec<String>)> = events
            .iter()
            .map(|event| (event.kind, short(&event.changed)))
            .collect();
        let expected: Vec<(EventKind, Vec<String>)> = case
            .events
            .iter()
            .map(|(kind, changed)| (*kind, changed.iter().map(|&cell| cell.to_owned()).collect()))
            .collect();
        assert_eq!(listed, expected, "case {}", case.number);
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
fn a_line_an_edit_takes_out_misses_as_compulsory_and_one_it_brings_in_as_filled() {
    let edit = |set: u32, fields: serde_json::Value| {
        let mut patch = fields;
        patch["kind"] = serde_json::json!("way");
        Op::Edit {
            target: serde_json::from_value(
                serde_json::json!({"kind": "way", "set": set, "way": 0}),
            )
            .unwrap(),
            patch: serde_json::from_value(patch).unwrap(),
        }
    };
    let invalidate = || edit(2, serde_json::json!({"valid": false}));
    // Without the edit, 0x0A4's return after 0x1A4 evicted it is a conflict.
    let evicted = session(
        Preset::DirectMapped8.config(),
        [load(0x0A4), load(0x1A4), load(0x0A4)],
    );
    let counters = evicted.stats().counters;
    assert_eq!((counters.compulsory, counters.conflict), (2, 1));
    // Made empty by an edit, the line is as good as never filled: its return
    // is compulsory, not a conflict the cache did not cause.
    let edited = session(
        Preset::DirectMapped8.config(),
        [load(0x0A4), invalidate(), load(0x0A4)],
    );
    let counters = edited.stats().counters;
    assert_eq!(
        (
            counters.misses,
            counters.compulsory,
            counters.conflict,
            counters.capacity
        ),
        (2, 2, 0, 0)
    );
    // Given to a way by an edit, a line counts as filled, as a fill would
    // make it: when 0x1A4 takes its place, its return is a conflict.
    let validated = session(
        Preset::DirectMapped8.config(),
        [
            edit(2, serde_json::json!({"valid": true, "line": 0x0A0})),
            load(0x0A4),
            load(0x1A4),
            load(0x0A4),
        ],
    );
    let counters = validated.stats().counters;
    assert_eq!(
        (
            counters.hits,
            counters.misses,
            counters.compulsory,
            counters.conflict
        ),
        (1, 2, 1, 1)
    );
    // Edits move no counter of their own.
    let before = session(Preset::DirectMapped8.config(), [load(0x0A4)])
        .stats()
        .counters;
    assert_eq!(
        session(Preset::DirectMapped8.config(), [load(0x0A4), invalidate()])
            .stats()
            .counters,
        before
    );
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
