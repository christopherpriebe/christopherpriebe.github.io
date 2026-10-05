#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use super::*;
use serde_json::json;

#[test]
fn counters_start_at_zero_and_serialize_in_camel_case() {
    let counters = Counters::default();
    assert_eq!(
        serde_json::to_value(counters).unwrap(),
        json!({
            "accesses": 0, "loads": 0, "stores": 0, "hits": 0, "misses": 0,
            "evictions": 0, "writebacks": 0, "writethroughs": 0, "writearounds": 0,
            "fills": 0, "cycles": 0, "compulsory": 0, "capacity": 0, "conflict": 0
        })
    );
}

#[test]
fn add_moves_only_the_named_counter() {
    let all = [
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
    for (index, counter) in all.into_iter().enumerate() {
        let mut counters = Counters::default();
        counters.add(counter, 3);
        let value = serde_json::to_value(counters).unwrap();
        let name = serde_json::to_value(counter).unwrap();
        let fields = value.as_object().unwrap();
        assert_eq!(fields.len(), all.len());
        for (field, count) in fields {
            let expected = if json!(field) == name { 3 } else { 0 };
            assert_eq!(
                count,
                &json!(expected),
                "{counter:?} moved {field} ({index})"
            );
        }
    }
}

/// Hit time `hit`, miss penalty `miss`.
fn timing(hit: u32, miss: u32) -> Timing {
    Timing {
        hit_cycles: hit,
        miss_cycles: miss,
    }
}

/// Counters after `accesses` accesses with `misses` misses and
/// `writebacks` write-backs, which spent `cycles` cycles.
fn run(accesses: u32, misses: u32, writebacks: u32, cycles: u64) -> Counters {
    Counters {
        accesses,
        hits: accesses - misses,
        misses,
        writebacks,
        cycles: Cycle::new_unchecked(cycles),
        ..Counters::default()
    }
}

fn text(figure: Option<Figure>) -> Option<String> {
    figure.map(|figure| figure.text)
}

#[test]
fn amat_is_true_as_printed() {
    let one_twenty = timing(1, 20);
    let lines = [
        ((10, 4), "AMAT = 1 + 0.40 \u{d7} 20 = 9.0 cycles"),
        // The rate is printed to two places, and the result is computed
        // from the printed rate: 1 + 0.33 × 20 = 7.6, not 7.67.
        ((3, 1), "AMAT = 1 + 0.33 \u{d7} 20 = 7.6 cycles"),
        ((3, 2), "AMAT = 1 + 0.67 \u{d7} 20 = 14.4 cycles"),
        ((1, 0), "AMAT = 1 + 0.00 \u{d7} 20 = 1.0 cycles"),
        ((5, 5), "AMAT = 1 + 1.00 \u{d7} 20 = 21.0 cycles"),
        // 1 / 8 = 0.125 rounds half up to 0.13.
        ((8, 1), "AMAT = 1 + 0.13 \u{d7} 20 = 3.6 cycles"),
    ];
    for ((accesses, misses), line) in lines {
        let counters = run(accesses, misses, 0, 0);
        assert_eq!(
            text(counters.amat(one_twenty)).as_deref(),
            Some(line),
            "{misses} in {accesses}"
        );
    }
}

#[test]
fn the_amat_result_rounds_half_up_to_one_place() {
    // 1 + 0.25 × 3 = 1.75, which rounds up to 1.8.
    assert_eq!(
        text(run(4, 1, 0, 0).amat(timing(1, 3))).as_deref(),
        Some("AMAT = 1 + 0.25 \u{d7} 3 = 1.8 cycles")
    );
    // 2 + 0.33 × 7 = 4.31, which rounds down to 4.3.
    assert_eq!(
        text(run(3, 1, 0, 0).amat(timing(2, 7))).as_deref(),
        Some("AMAT = 2 + 0.33 \u{d7} 7 = 4.3 cycles")
    );
    // Long costs are grouped like settings.
    assert_eq!(
        text(run(2, 1, 0, 0).amat(timing(3, 2_500))).as_deref(),
        Some("AMAT = 3 + 0.50 \u{d7} 2,500 = 1,253.0 cycles")
    );
}

#[test]
fn the_amat_value_is_exact() {
    let figure = run(3, 1, 0, 0).amat(timing(1, 20)).unwrap();
    assert!((figure.value - (1.0 + 20.0 / 3.0)).abs() < 1e-12);
    let figure = run(10, 4, 0, 0).amat(timing(1, 20)).unwrap();
    assert!((figure.value - 9.0).abs() < 1e-12);
}

#[test]
fn the_measured_cost_names_the_write_backs() {
    // 10 accesses, 4 misses and 1 write-back at 1/20 spend
    // 10 + 4 × 20 + 20 = 110 cycles: AMAT 9.0 plus 2.0.
    let counters = run(10, 4, 1, 110);
    assert_eq!(
        text(counters.amat(timing(1, 20))).as_deref(),
        Some("AMAT = 1 + 0.40 \u{d7} 20 = 9.0 cycles")
    );
    let measured = counters.measured(timing(1, 20)).unwrap();
    assert_eq!(
        measured.text,
        "Measured after access #10: 11.0 cycles per access, 2.0 of them for write-backs."
    );
    assert!((measured.value - 11.0).abs() < 1e-12);
    assert_eq!(
        text(run(5, 4, 2, 125).measured(timing(1, 20))).as_deref(),
        Some("Measured after access #5: 25.0 cycles per access, 8.0 of them for write-backs.")
    );
    assert_eq!(
        text(run(2, 1, 0, 22).measured(timing(1, 20))).as_deref(),
        Some("Measured after access #2: 11.0 cycles per access, 0.0 of them for write-backs.")
    );
}

#[test]
fn the_measured_numbers_round_half_up_on_their_own() {
    // 3 accesses, 1 miss and 1 write-back at 1/20 spend 3 + 20 + 20 =
    // 43 cycles: 14.33 rounds down to 14.3, and 20 / 3 = 6.67 up to 6.7.
    assert_eq!(
        text(run(3, 1, 1, 43).measured(timing(1, 20))).as_deref(),
        Some("Measured after access #3: 14.3 cycles per access, 6.7 of them for write-backs.")
    );
    // At 1/5: 4 + 3 × 5 + 2 × 5 = 29 cycles in 4 accesses, 7.25, rounds
    // up to 7.3; 4 + 5 + 5 = 14 leaves 5 / 4 = 1.25, which rounds up to
    // 1.3.
    assert_eq!(
        text(run(4, 3, 2, 29).measured(timing(1, 5))).as_deref(),
        Some("Measured after access #4: 7.3 cycles per access, 2.5 of them for write-backs.")
    );
    assert_eq!(
        text(run(4, 1, 1, 14).measured(timing(1, 5))).as_deref(),
        Some("Measured after access #4: 3.5 cycles per access, 1.3 of them for write-backs.")
    );
    let measured = run(3, 1, 1, 43).measured(timing(1, 20)).unwrap();
    assert!((measured.value - 43.0 / 3.0).abs() < 1e-12);
}

#[test]
fn the_printed_rate_can_move_amat_by_half_a_hundredth_of_the_miss_penalty() {
    // 1 miss at 1/1,000 spends 1,000 cycles more than the accesses' hit
    // time. In 201 accesses, 1,201 cycles are 5.98 per access, but the
    // rate 0.004975 prints as 0.00; in 200, 1,200 cycles are 6.0 per
    // access, but the rate 0.005 prints as 0.01, and AMAT reads 5 cycles
    // above the exact 6.0. Each line is still true as printed.
    let slow = timing(1, 1_000);
    for (accesses, amat) in [
        (201, "AMAT = 1 + 0.00 \u{d7} 1,000 = 1.0 cycles"),
        (200, "AMAT = 1 + 0.01 \u{d7} 1,000 = 11.0 cycles"),
    ] {
        let counters = run(accesses, 1, 0, u64::from(accesses) + 1_000);
        assert_eq!(text(counters.amat(slow)).as_deref(), Some(amat));
        assert_eq!(
            text(counters.measured(slow)).as_deref(),
            Some(
                format!(
                    "Measured after access #{accesses}: 6.0 cycles per access, 0.0 of them for write-backs."
                )
                .as_str()
            )
        );
    }
}

#[test]
fn neither_figure_exists_before_the_first_access() {
    let counters = Counters::default();
    assert_eq!(counters.amat(timing(1, 20)), None);
    assert_eq!(counters.measured(timing(1, 20)), None);
}

#[test]
fn a_figure_serializes_its_value_and_text() {
    let figure = run(10, 4, 0, 0).amat(timing(1, 20)).unwrap();
    assert_eq!(
        serde_json::to_value(figure).unwrap(),
        json!({ "value": 9.0, "text": "AMAT = 1 + 0.40 \u{d7} 20 = 9.0 cycles" })
    );
}

#[test]
fn counts_saturate() {
    let mut counters = Counters {
        hits: u32::MAX - 1,
        cycles: Cycle::new_unchecked(u64::MAX - 1),
        ..Counters::default()
    };
    counters.add(Counter::Hits, 5);
    counters.add(Counter::Cycles, 5);
    assert_eq!(counters.hits, u32::MAX);
    assert_eq!(counters.cycles.get(), u64::MAX);
}

/// Every counter, in the order the `Counter` enum declares them.
const ALL: [Counter; 14] = [
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

#[test]
fn get_reads_back_what_add_moved() {
    for counter in ALL {
        let mut counters = Counters::default();
        counters.add(counter, 7);
        for other in ALL {
            let expected = if other == counter { 7 } else { 0 };
            assert_eq!(
                counters.get(other),
                expected,
                "{counter:?} read as {other:?}"
            );
        }
    }
    let counters = Counters {
        cycles: Cycle::new_unchecked(u64::MAX),
        ..Counters::default()
    };
    assert_eq!(counters.get(Counter::Cycles), u64::MAX);
}

#[test]
fn the_hit_rate_is_a_whole_percent_of_an_exact_share() {
    assert_eq!(Counters::default().hit_rate(), None);
    let rate = run(3, 1, 0, 0).hit_rate().unwrap();
    assert_eq!(rate.text, "67%");
    assert!((rate.value - 2.0 / 3.0).abs() < 1e-12);
    // 1 hit in 8 is 12.5 %, which rounds up.
    assert_eq!(text(run(8, 7, 0, 0).hit_rate()).as_deref(), Some("13%"));
    assert_eq!(text(run(4, 4, 0, 0).hit_rate()).as_deref(), Some("0%"));
    assert_eq!(text(run(4, 0, 0, 0).hit_rate()).as_deref(), Some("100%"));
}

/// The geometry of `Config::default()`: 16 B lines of 4-byte words.
fn geometry() -> Geometry {
    crate::config::Config::default().validate().unwrap()
}

#[test]
fn traffic_counts_lines_and_words_and_their_bytes() {
    let counters = Counters {
        fills: 3,
        writebacks: 2,
        writethroughs: 5,
        writearounds: 1,
        ..Counters::default()
    };
    let geometry = geometry();
    let (line, word) = (
        u64::from(geometry.line_bytes()),
        u64::from(geometry.word_bytes()),
    );
    assert_eq!(
        Traffic::new(&counters, &geometry),
        Traffic {
            line_reads: 3,
            line_writes: 2,
            word_writes: 6,
            bytes_read: 3 * line,
            bytes_written: 2 * line + 6 * word,
        }
    );
    assert_eq!(
        serde_json::to_value(Traffic::new(&Counters::default(), &geometry)).unwrap(),
        json!({
            "lineReads": 0, "lineWrites": 0, "wordWrites": 0, "bytesRead": 0,
            "bytesWritten": 0
        })
    );
}

#[test]
fn a_shadow_keeps_its_lines_in_lru_order() {
    let line = |at: u32| LineAddr::new_unchecked(at);
    let mut shadow = Shadow::new(3);
    // Misses that do not allocate leave it empty.
    assert!(!shadow.access(line(0x00), false));
    assert!(shadow.lines().is_empty());
    for at in [0x00, 0x10, 0x20] {
        assert!(!shadow.access(line(at), true));
    }
    assert_eq!(shadow.lines(), [line(0x20), line(0x10), line(0x00)]);
    // A hit makes its line the most recent, whether or not a miss there
    // would allocate.
    assert!(shadow.access(line(0x00), false));
    assert_eq!(shadow.lines(), [line(0x00), line(0x20), line(0x10)]);
    // A fourth line pushes out the least recently used.
    assert!(!shadow.access(line(0x30), true));
    assert_eq!(shadow.lines(), [line(0x30), line(0x00), line(0x20)]);
    assert!(!shadow.access(line(0x10), true));
    assert_eq!(shadow.lines(), [line(0x10), line(0x30), line(0x00)]);
    // An edit's fill and removal: a filled line becomes the most recent,
    // and a forgotten one is gone, so the shadow misses on it.
    shadow.fill(line(0x00));
    assert_eq!(shadow.lines(), [line(0x00), line(0x10), line(0x30)]);
    shadow.forget(line(0x10));
    shadow.forget(line(0x70));
    assert_eq!(shadow.lines(), [line(0x00), line(0x30)]);
    assert!(!shadow.access(line(0x10), false));
}

#[test]
fn a_miss_is_compulsory_until_its_line_is_filled_then_the_shadow_decides() {
    assert_eq!(miss_class(false, false), Counter::Compulsory);
    // A line the shadow holds was filled; if an edit removed it from
    // the filled lines, it was removed from the shadow too, so this
    // only arises by construction.
    assert_eq!(miss_class(false, true), Counter::Compulsory);
    assert_eq!(miss_class(true, false), Counter::Capacity);
    assert_eq!(miss_class(true, true), Counter::Conflict);
}

#[test]
fn the_recent_outcomes_keep_the_latest() {
    let mut recent = Recent::default();
    assert_eq!(recent.outcomes().count(), 0);
    let limit = RECENT_OUTCOMES as usize;
    // One more miss than fits, then a hit: the first miss and the one
    // after it are dropped.
    for _ in 0..=limit {
        recent.push(Outcome::Miss);
    }
    recent.push(Outcome::Hit);
    let outcomes: Vec<Outcome> = recent.outcomes().collect();
    assert_eq!(outcomes.len(), limit);
    let (last, older) = outcomes.split_last().unwrap();
    assert_eq!(*last, Outcome::Hit);
    assert!(older.iter().all(|&o| o == Outcome::Miss));
}

/// A machine at the start of a run of `config`, with `outcomes` recorded
/// as its accesses' outcomes and its counters moved to match.
fn after(config: crate::config::Config, outcomes: &[Outcome]) -> Machine {
    let mut machine = Machine::start(config).unwrap();
    for &outcome in outcomes {
        machine.recent.push(outcome);
        machine.counters.add(Counter::Accesses, 1);
        machine.counters.add(Counter::Loads, 1);
        let counter = match outcome {
            Outcome::Hit => Counter::Hits,
            Outcome::Miss => Counter::Misses,
        };
        machine.counters.add(counter, 1);
    }
    machine
}

fn recent_text(outcomes: &[Outcome]) -> Option<String> {
    Stats::new(
        &after(crate::config::Config::default(), outcomes),
        Mode::Basics,
        &[],
    )
    .recent_text
}

#[test]
fn the_sparkline_text_counts_its_outcomes_in_the_singular_and_plural() {
    use Outcome::{Hit, Miss};
    let stats = Stats::new(
        &after(crate::config::Config::default(), &[]),
        Mode::Basics,
        &[],
    );
    assert!(stats.recent.is_empty());
    assert_eq!(stats.recent_text, None);
    assert_eq!(recent_text(&[Miss]).as_deref(), Some("0 hits, 1 miss"));
    assert_eq!(recent_text(&[Hit]).as_deref(), Some("1 hit, 0 misses"));
    assert_eq!(
        recent_text(&[Miss, Hit, Miss]).as_deref(),
        Some("1 hit, 2 misses")
    );
    assert_eq!(
        recent_text(&[Miss, Miss, Hit, Hit]).as_deref(),
        Some("2 hits, 2 misses")
    );
}

#[test]
fn the_sparkline_says_last_only_once_the_run_has_more_accesses_than_it_shows() {
    use Outcome::{Hit, Miss};
    let limit = RECENT_OUTCOMES as usize;
    // Exactly as many accesses as it shows: every one is counted, with
    // no suffix. One miss, the rest hits.
    let mut outcomes = vec![Miss];
    outcomes.resize(limit, Hit);
    // Saturating, so that a limit lowered to 0 still builds and fails
    // in limits.rs, which says why.
    let hit_count = RECENT_OUTCOMES.saturating_sub(1);
    let hits = format::data(hit_count);
    let stats = Stats::new(
        &after(crate::config::Config::default(), &outcomes),
        Mode::Basics,
        &[],
    );
    assert_eq!(stats.recent, outcomes);
    assert_eq!(
        stats.recent_text,
        Some(format!(
            "{hits} {}, 1 miss",
            format::plural(hit_count, "hit", "hits")
        ))
    );
    // One more, a miss: the first access drops out of the counts, which
    // are of the last RECENT_OUTCOMES, and the text says so.
    outcomes.push(Miss);
    let stats = Stats::new(
        &after(crate::config::Config::default(), &outcomes),
        Mode::Basics,
        &[],
    );
    assert_eq!(stats.recent, outcomes[1..]);
    assert_eq!(stats.counters.accesses, RECENT_OUTCOMES + 1);
    assert_eq!(
        stats.recent_text,
        Some(format!(
            "{hits} {}, 1 miss (last {})",
            format::plural(hit_count, "hit", "hits"),
            format::setting(RECENT_OUTCOMES)
        ))
    );
}

/// The keys of `rows`, in order.
fn keys(rows: &[StatRow]) -> Vec<StatKey> {
    rows.iter().map(|row| row.key).collect()
}

#[test]
fn basics_shows_the_writes_to_memory_and_advanced_each_kind_and_the_classes() {
    use StatKey::{
        Accesses, Capacity, Compulsory, Conflict, Cycles, Evictions, HitRate, Hits, MemoryWrites,
        Misses, Writearounds, Writebacks, Writethroughs,
    };
    let untimed = crate::config::Config::default();
    let machine = after(untimed, &[]);
    assert_eq!(
        keys(&Stats::new(&machine, Mode::Basics, &[]).rows),
        [Accesses, Hits, Misses, HitRate, Evictions, MemoryWrites]
    );
    assert_eq!(
        keys(&Stats::new(&machine, Mode::Advanced, &[]).rows),
        [
            Accesses,
            Hits,
            Misses,
            HitRate,
            Evictions,
            Writebacks,
            Writethroughs,
            Writearounds,
            Compulsory,
            Capacity,
            Conflict
        ]
    );
    // Timed mode adds the cycles, in Advanced, where timing lives.
    let machine = after(
        crate::config::Config {
            timing: Some(timing(1, 20)),
            ..untimed
        },
        &[],
    );
    assert_eq!(
        keys(&Stats::new(&machine, Mode::Advanced, &[]).rows),
        [
            Accesses,
            Hits,
            Misses,
            HitRate,
            Evictions,
            Writebacks,
            Writethroughs,
            Writearounds,
            Cycles,
            Compulsory,
            Capacity,
            Conflict
        ]
    );
}

#[test]
fn rows_give_counts_as_data_and_the_hit_rate_as_a_percent() {
    let mut machine = after(crate::config::Config::default(), &[]);
    let texts = |stats: &Stats| -> Vec<(StatKey, String)> {
        stats
            .rows
            .iter()
            .map(|row| (row.key, row.text.clone()))
            .collect()
    };
    // Before the first access the hit rate has nothing to show.
    let stats = Stats::new(&machine, Mode::Basics, &[]);
    assert_eq!(
        stats.rows[3],
        StatRow {
            key: StatKey::HitRate,
            text: format::NO_FIGURE.to_owned(),
            changed: false
        }
    );
    machine.counters = Counters {
        accesses: 12_000,
        hits: 8_000,
        misses: 4_000,
        evictions: 3_990,
        writebacks: 7,
        writethroughs: 5,
        writearounds: 2,
        compulsory: 10,
        capacity: 3_000,
        conflict: 990,
        ..Counters::default()
    };
    assert_eq!(
        texts(&Stats::new(&machine, Mode::Basics, &[])),
        [
            (StatKey::Accesses, "12,000".to_owned()),
            (StatKey::Hits, "8000".to_owned()),
            (StatKey::Misses, "4000".to_owned()),
            (StatKey::HitRate, "67%".to_owned()),
            (StatKey::Evictions, "3990".to_owned()),
            // Write-backs, write-throughs and write-arounds together.
            (StatKey::MemoryWrites, "14".to_owned()),
        ]
    );
    let advanced = texts(&Stats::new(&machine, Mode::Advanced, &[]));
    assert_eq!(
        advanced[5..],
        [
            (StatKey::Writebacks, "7".to_owned()),
            (StatKey::Writethroughs, "5".to_owned()),
            (StatKey::Writearounds, "2".to_owned()),
            (StatKey::Compulsory, "10".to_owned()),
            (StatKey::Capacity, "3000".to_owned()),
            (StatKey::Conflict, "990".to_owned()),
        ]
    );
}

#[test]
fn a_row_is_changed_when_the_entry_moved_one_of_its_counters() {
    let config = crate::config::Config {
        timing: Some(timing(1, 20)),
        ..crate::config::Config::default()
    };
    let machine = after(config, &[Outcome::Miss]);
    let moved = |counters: &[Counter]| -> Vec<ChangeRef> {
        counters
            .iter()
            .map(|&counter| ChangeRef::Counter { counter })
            .collect()
    };
    let changed = |mode: Mode, highlight: &[ChangeRef]| -> Vec<StatKey> {
        Stats::new(&machine, mode, highlight)
            .rows
            .iter()
            .filter(|row| row.changed)
            .map(|row| row.key)
            .collect()
    };
    // A miss that evicts a dirty line, as far as its write-back. The
    // hit rate, which no event names, is never changed; loads and fills
    // have no row.
    let highlight = moved(&[
        Counter::Accesses,
        Counter::Loads,
        Counter::Misses,
        Counter::Compulsory,
        Counter::Cycles,
        Counter::Writebacks,
    ]);
    assert_eq!(
        changed(Mode::Advanced, &highlight),
        [
            StatKey::Accesses,
            StatKey::Misses,
            StatKey::Writebacks,
            StatKey::Cycles,
            StatKey::Compulsory
        ]
    );
    assert_eq!(
        changed(Mode::Basics, &highlight),
        [StatKey::Accesses, StatKey::Misses, StatKey::MemoryWrites]
    );
    // Any of the three writes changes the writes to memory; cells that
    // are not counters change no row.
    for counter in [
        Counter::Writebacks,
        Counter::Writethroughs,
        Counter::Writearounds,
    ] {
        let mut highlight = moved(&[counter]);
        highlight.push(ChangeRef::Pc);
        assert_eq!(changed(Mode::Basics, &highlight), [StatKey::MemoryWrites]);
    }
    assert!(changed(Mode::Advanced, &[ChangeRef::Pc]).is_empty());
}

#[test]
fn amat_and_the_measured_cost_are_there_in_timed_mode_only() {
    let untimed = crate::config::Config::default();
    let mut machine = after(untimed, &[Outcome::Miss, Outcome::Hit]);
    let stats = Stats::new(&machine, Mode::Advanced, &[]);
    assert_eq!((stats.amat, stats.measured), (None, None));
    assert_eq!(stats.hit_rate.map(|rate| rate.text).as_deref(), Some("50%"));

    let config = crate::config::Config {
        timing: Some(timing(1, 20)),
        ..untimed
    };
    machine.config = config;
    machine.counters.cycles = Cycle::new_unchecked(22);
    let stats = Stats::new(&machine, Mode::Advanced, &[]);
    assert_eq!(stats.amat, machine.counters.amat(timing(1, 20)));
    assert_eq!(stats.measured, machine.counters.measured(timing(1, 20)));
    assert_eq!(
        text(stats.amat).as_deref(),
        Some("AMAT = 1 + 0.50 \u{d7} 20 = 11.0 cycles")
    );
    // Before the run's first access there is nothing to average.
    let stats = Stats::new(&after(config, &[]), Mode::Advanced, &[]);
    assert_eq!(
        (stats.hit_rate, stats.amat, stats.measured),
        (None, None, None)
    );
}

#[test]
fn stats_serialize_in_camel_case_without_absent_figures() {
    let machine = after(crate::config::Config::default(), &[]);
    let value = serde_json::to_value(Stats::new(&machine, Mode::Basics, &[])).unwrap();
    let fields: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert_eq!(fields, ["counters", "recent", "rows", "traffic"]);
    assert_eq!(value["recent"], json!([]));
    assert_eq!(
        value["rows"][3],
        json!({ "key": "hitRate", "text": "\u{2014}", "changed": false })
    );

    let config = crate::config::Config {
        timing: Some(timing(1, 20)),
        ..crate::config::Config::default()
    };
    let machine = after(config, &[Outcome::Miss, Outcome::Hit]);
    let value = serde_json::to_value(Stats::new(&machine, Mode::Advanced, &[])).unwrap();
    let fields: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert_eq!(
        fields,
        [
            "amat",
            "counters",
            "hitRate",
            "measured",
            "recent",
            "recentText",
            "rows",
            "traffic"
        ]
    );
    assert_eq!(value["recent"], json!(["miss", "hit"]));
    assert_eq!(value["recentText"], json!("1 hit, 1 miss"));
    assert_eq!(value["hitRate"], json!({ "value": 0.5, "text": "50%" }));
}

#[test]
fn keys_and_outcomes_serialize_by_camel_case_name() {
    let keys = [
        (StatKey::Accesses, "accesses"),
        (StatKey::Hits, "hits"),
        (StatKey::Misses, "misses"),
        (StatKey::HitRate, "hitRate"),
        (StatKey::Evictions, "evictions"),
        (StatKey::MemoryWrites, "memoryWrites"),
        (StatKey::Writebacks, "writebacks"),
        (StatKey::Writethroughs, "writethroughs"),
        (StatKey::Writearounds, "writearounds"),
        (StatKey::Cycles, "cycles"),
        (StatKey::Compulsory, "compulsory"),
        (StatKey::Capacity, "capacity"),
        (StatKey::Conflict, "conflict"),
    ];
    for (key, name) in keys {
        assert_eq!(serde_json::to_value(key).unwrap(), json!(name));
    }
    for (outcome, name) in [(Outcome::Hit, "hit"), (Outcome::Miss, "miss")] {
        assert_eq!(serde_json::to_value(outcome).unwrap(), json!(name));
        assert_eq!(
            serde_json::from_value::<Outcome>(json!(name)).unwrap(),
            outcome
        );
    }
    assert!(serde_json::from_value::<Outcome>(json!("Hit")).is_err());
}
