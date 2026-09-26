//! The views the page draws without computing, on fixed runs: the snapshot
//! at the cursor (the cache with its ranks, victim, addressed word and
//! comparators; memory with its stale marks and watched lines; the lanes; the
//! CPU and the program; the net highlight of the cursor's entry and its
//! summary; the controls and the labels), the timeline (the displayed run's
//! entries and the current one's events, ticks, the log's header and the live
//! region's sentence), the issue draft, and windows of the log. Two runs are
//! pinned whole as golden snapshots: frame 3b's pause on its write-back, and
//! the first visit before anything is issued.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::limits::MAX_RUN_STEPS;
use cache_explorer_core::{
    AccessKind, AddressInput, AllocatePolicy, CacheConfig, ChangeRef, Config, ControlAction,
    Counter, CpuStatus, EntryKind, EntryOutcome, EventKind, HitMarker, Op, Preset, RankHeader,
    RankRole, Register, Replacement, RequestStatus, Session, SetIndex, State, Tense, Timing,
    Transfer, WayCompare, WordIndex, WritePolicy,
};
use insta::{Comparator, DefaultComparator, Snapshot};

use support::{
    controls_text, first_visit, frame_3b, json, line, load, pos, program, session, store, way,
    FRAME_3B,
};

/// The tests' helpers. They sit in a module compiled for tests only, where a
/// failed `unwrap` is a failed test, as in the tests themselves.
#[cfg(test)]
mod support {
    use std::fmt::Write as _;

    use cache_explorer_core::{
        AccessKind, Addr, Config, ConfigControls, EventPos, LineAddr, Op, Preset, ProgramFormat,
        Session, State, WayView, Word,
    };

    /// Frame 3b's program, as the design writes it.
    pub(crate) const FRAME_3B: &str = concat!(
        "  st  [0x0B0], #5\n",
        "  ld  r1, [0x0A4]\n",
        "  st  [0x1A4], #7\n",
        "  ld  r2, [0x0A8]\n",
        "  ld  r3, [0x2A4]\n",
        "  ld  r4, [0x1A4]",
    );

    /// `value` as an address, as one read from JSON is.
    pub(crate) fn addr(value: u32) -> Addr {
        serde_json::from_value(value.into()).unwrap()
    }

    /// `value` as a line address, as one read from JSON is.
    pub(crate) fn line(value: u32) -> LineAddr {
        serde_json::from_value(value.into()).unwrap()
    }

    /// `value` as a data value, as one read from JSON is.
    pub(crate) fn word(value: u64) -> Word {
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
        Op::Issue {
            kind: AccessKind::Write,
            addr: addr(at),
            value: Some(word(value)),
        }
    }

    /// A load of the assembly program `source`.
    pub(crate) fn program(source: &str) -> Op {
        Op::Program {
            source: source.to_owned(),
            format: ProgramFormat::Asm,
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

    /// The page's first visit: `direct-mapped-8` with pattern memory,
    /// nothing issued.
    pub(crate) fn first_visit() -> Session {
        Session::with_config(Config::first_visit()).unwrap()
    }

    /// Frame 3b's first five accesses issued by hand on `two-way-lru`: the
    /// store to 0x0B0, the loads of 0x0A4, the store to 0x1A4, the load of
    /// 0x0A8, and the load of 0x2A4 that evicts the dirty line 0x1A0. Its
    /// events are at positions 1–5, 6–10, 11–16, 17–20 and 21–28.
    pub(crate) fn frame_3b() -> Session {
        session(
            Preset::TwoWayLru.config(),
            [
                store(0x0B0, 5),
                load(0x0A4),
                store(0x1A4, 7),
                load(0x0A8),
                load(0x2A4),
            ],
        )
    }

    /// `value` as pretty JSON, the text a golden view snapshot holds.
    pub(crate) fn json(value: &impl serde::Serialize) -> String {
        serde_json::to_string_pretty(value).unwrap()
    }

    /// The configuration bar as text, one option a line: its group, label, marks
    /// and title, and the configuration a click produces, as its Advanced
    /// summary, address width, word size and timing.
    pub(crate) fn controls_text(controls: &ConfigControls) -> String {
        let mut text = format!(
            "summary: {}\ntiming: on {}, hit {}, miss {}, toggle {:?}\nerror: {:?}\n",
            controls.summary,
            controls.timing.on,
            controls.timing.hit_text,
            controls.timing.miss_text,
            controls.timing.toggle.timing,
            controls.error
        );
        for group in &controls.groups {
            for option in &group.options {
                let config = &option.config;
                let _ = writeln!(
                    text,
                    "{:?}{} | {}{}{}{} | {} | {}-bit, {} B words",
                    group.group,
                    if group.advanced_only {
                        " (Advanced)"
                    } else {
                        ""
                    },
                    option.label,
                    if option.pressed { " | pressed" } else { "" },
                    if option.disabled { " | disabled" } else { "" },
                    option
                        .title
                        .as_ref()
                        .map(|title| format!(": {title}"))
                        .unwrap_or_default(),
                    config.summary(true),
                    config.address_bits,
                    u8::from(config.word_bytes),
                );
            }
        }
        text
    }

    /// The way `way` of set `set` in `state`'s cache.
    pub(crate) fn way(state: &State, set: usize, way: usize) -> &WayView {
        &state.cache.sets[set].ways[way]
    }
}

/// Compares a snapshot's header as well as its body, so an edited
/// description is reviewed again.
#[derive(Clone)]
struct WithHeader;

impl Comparator for WithHeader {
    fn matches(&self, reference: &Snapshot, test: &Snapshot) -> bool {
        DefaultComparator.matches_fully(reference, test)
    }

    fn dyn_clone(&self) -> Box<dyn Comparator> {
        Box::new(self.clone())
    }
}

/// Checks `text` against the snapshot `name`, whose header carries
/// `description`.
fn golden(name: &str, description: &str, text: &str) {
    insta::with_settings!({
        description => description,
        omit_expression => true,
        comparator => Box::new(WithHeader),
    }, {
        insta::assert_snapshot!(name, text);
    });
}

#[test]
fn frame_3b_paused_on_its_write_back_reads_as_the_design_draws_it() {
    let mut session = frame_3b();
    session.seek(pos(24));
    let state = session.state();
    let text = format!(
        "state = {}\ntimeline = {}\nentries = {}\nticks = {:?}\nmemory = {}\n",
        json(&state),
        json(&session.timeline()),
        json(&session.entries(0, 5)),
        session.ticks(),
        json(&session.memory_lines(0, 10)),
    );
    golden(
        "frame-3b-write-back",
        "Frame 3b's first five accesses issued by hand on two-way-lru, paused on access #5's \
         write-back, event 5.4: the victim, way 1 of set 2, is still valid and dirty and its word 7 \
         has reached memory, so nothing is stale there; the store to 0x0B0 is still stale. The \
         lane shows the write-back, the CPU step 4 of 8, the comparators a miss, and the \
         changed line the design's `memory 0x1A4 · misses, write-backs`.",
        &text,
    );
    // The frame's pause, as the design draws it.
    let victim = way(&state, 2, 1);
    assert!(victim.valid && victim.dirty && victim.victim);
    assert_eq!(victim.words, ["96", "7", "25", "38"]);
    assert_eq!(
        state.changed_summary,
        "memory 0x1A4 \u{b7} misses, write-backs"
    );
    assert_eq!(state.bus.transfer, Transfer::WriteBack);
    assert_eq!(
        state.cpu.status,
        CpuStatus::Busy {
            entry: 5,
            seq: 4,
            of: 8
        }
    );
    assert_eq!(state.cpu.status_text, "Access #5 \u{b7} step 4 of 8");
    assert_eq!(
        state.cache.compare,
        Some(vec![WayCompare::Mismatch, WayCompare::Mismatch])
    );
    assert_eq!(state.cpu.hit_marker, HitMarker::Miss);
}

#[test]
fn the_first_visit_shows_an_empty_cache_a_ready_cpu_and_the_typed_address() {
    let session = first_visit();
    let state = session.state();
    let draft = session.draft_issue(AccessKind::Read, "0x0A4", "");
    let text = format!(
        "state = {}\ntimeline = {}\ndraft = {}\nconfig =\n{}",
        json(&state),
        json(&session.timeline()),
        json(&draft),
        controls_text(&session.config_controls(session.config())),
    );
    golden(
        "first-visit",
        "The first visit (frame 5a) on direct-mapped-8 with pattern memory, before anything is \
         issued: an empty cache without a rank column, no lines touched, no request and a ready \
         CPU, `no steps yet`, Undo, Redo and Reset disabled; 0x0A4 typed, decoded to set 2 with \
         one pending comparator; and the configuration bar's options.",
        &text,
    );
    assert_eq!(draft.selected_set.map(SetIndex::get), Some(2));
    assert_eq!(draft.compare, Some(vec![WayCompare::Pending]));
    assert_eq!(draft.request_text, "Load 0x0A4");
    assert_eq!(draft.op, Some(load(0x0A4)));
    assert!(!state.controls.enabled.contains(&ControlAction::Undo));
    assert!(!state.controls.enabled.contains(&ControlAction::Redo));
    assert!(!state.controls.enabled.contains(&ControlAction::Reset));
    assert_eq!(session.timeline().header_text, "no steps yet");
}

#[test]
fn the_changed_line_reads_as_the_design_writes_it() {
    // Flow A: the miss brings in the line and r1 changes; the hit changes
    // only r1 and the hit count.
    let mut session = session(Config::first_visit(), [load(0x0A4)]);
    assert_eq!(
        session.state().changed_summary,
        "set 2 way 0 valid, tag, data \u{b7} r1 \u{b7} misses"
    );
    session.apply(load(0x0A8)).unwrap();
    assert_eq!(session.state().changed_summary, "r1 \u{b7} hits");
    // A write-around changes memory and the write-around count alone.
    let no_allocate = Config {
        cache: CacheConfig {
            allocate_policy: AllocatePolicy::WriteNoAllocate,
            ..Config::first_visit().cache
        },
        ..Config::first_visit()
    };
    let session = self::session(no_allocate, [store(0x0A4, 7)]);
    assert_eq!(
        session.state().changed_summary,
        "memory 0x0A4 \u{b7} misses, write-arounds"
    );
    // At a run's origin nothing has changed.
    assert_eq!(first_visit().state().changed_summary, "nothing changed");
}

#[test]
fn the_highlight_is_the_net_change_since_the_entry_began() {
    let mut session = frame_3b();
    let (s, w) = (
        |n: u32| serde_json::from_value(n.into()).unwrap(),
        |n: u32| serde_json::from_value(n.into()).unwrap(),
    );
    let valid = ChangeRef::Valid {
        set: s(2),
        way: w(1),
    };
    let tag = ChangeRef::Tag {
        set: s(2),
        way: w(1),
    };
    let dirty = ChangeRef::Dirty {
        set: s(2),
        way: w(1),
    };
    // The eviction clears the way: valid, tag, dirty and every word.
    session.seek(pos(25));
    let state = session.state();
    assert!(state.highlight.contains(&valid) && state.highlight.contains(&dirty));
    assert_eq!(
        state.changed_summary,
        "set 2 way 1 valid, tag, data, dirty \u{b7} memory 0x1A4 \u{b7} misses, evictions, \
         write-backs"
    );
    // The fill makes it valid again: the valid bit went 1, 0, 1 and is not
    // highlighted, but the new line's tag and words are, and the dirty bit
    // is now clear.
    session.seek(pos(26));
    let state = session.state();
    assert!(!state.highlight.contains(&valid));
    assert!(state.highlight.contains(&tag) && state.highlight.contains(&dirty));
    assert_eq!(
        state.changed_summary,
        "set 2 way 1 tag, data, dirty \u{b7} memory 0x1A4 \u{b7} misses, evictions, write-backs"
    );
    // By the entry's end the ranks and r1 have moved too, and the stats rows
    // that changed are marked.
    session.seek(pos(28));
    let state = session.state();
    assert_eq!(
        state.changed_summary,
        "set 2 way 0 rank \u{b7} set 2 way 1 tag, data, dirty, rank \u{b7} memory 0x1A4 \u{b7} \
         r1 \u{b7} misses, evictions, write-backs"
    );
    let changed: Vec<bool> = state.stats.rows.iter().map(|row| row.changed).collect();
    // Accesses, hits, misses, hit rate, evictions, writes to memory.
    assert_eq!(changed, [true, false, true, false, true, true]);
    assert!(state.highlight.contains(&ChangeRef::Counter {
        counter: Counter::Compulsory
    }));
    assert_eq!(session.stats(), state.stats);
    // At the entry's last event, the highlight is its summary in the log.
    let current = session.entries(4, 1);
    assert_eq!(
        current[0].changed_summary.as_deref(),
        Some(state.changed_summary.as_str())
    );
}

#[test]
fn the_victim_and_the_addressed_word_follow_the_access() {
    let mut session = frame_3b();
    // From the victim choice until the fill, the victim is marked.
    for at in 23..=25 {
        session.seek(pos(at));
        assert!(way(&session.state(), 2, 1).victim, "at {at}");
    }
    for at in [22, 26, 27, 28] {
        session.seek(pos(at));
        assert!(!way(&session.state(), 2, 1).victim, "at {at}");
    }
    // Once the way holds the line, the word the access reads is marked.
    session.seek(pos(25));
    assert_eq!(way(&session.state(), 2, 1).addressed_word, None);
    session.seek(pos(26));
    let state = session.state();
    assert_eq!(
        way(&state, 2, 1).addressed_word.map(WordIndex::get),
        Some(1)
    );
    assert_eq!(way(&state, 2, 0).addressed_word, None);
    // A hit marks its word from the compare on.
    session.seek(pos(18));
    assert_eq!(
        way(&session.state(), 2, 0)
            .addressed_word
            .map(WordIndex::get),
        Some(2)
    );
}

#[test]
fn the_comparators_wait_for_the_compare_then_show_what_each_way_held() {
    let mut session = frame_3b();
    session.seek(pos(17));
    let state = session.state();
    assert_eq!(state.cache.selected_set.map(SetIndex::get), Some(2));
    assert_eq!(
        state.cache.compare,
        Some(vec![WayCompare::Pending, WayCompare::Pending])
    );
    assert_eq!(state.cpu.hit_marker, HitMarker::None);
    session.seek(pos(18));
    let state = session.state();
    assert_eq!(
        state.cache.compare,
        Some(vec![WayCompare::Match, WayCompare::Mismatch])
    );
    assert_eq!(state.cpu.hit_marker, HitMarker::Hit);
    // The first load of set 2 compared against two invalid ways.
    session.seek(pos(7));
    assert_eq!(
        session.state().cache.compare,
        Some(vec![WayCompare::Invalid, WayCompare::Invalid])
    );
    // At the origin there is no access: the page shows the typed address.
    session.seek(pos(0));
    let state = session.state();
    assert_eq!(state.cache.selected_set, None);
    assert_eq!(state.cache.compare, None);
}

#[test]
fn the_rank_and_dirty_columns_are_shown_only_where_they_can_change() {
    let config = |replacement, ways, write_policy| Config {
        cache: CacheConfig {
            size_bytes: 128,
            line_bytes: 16,
            associativity: ways,
            replacement,
            write_policy,
            allocate_policy: AllocatePolicy::WriteAllocate,
        },
        ..Config::first_visit()
    };
    let lru = session(config(Replacement::Lru, 2, WritePolicy::WriteBack), []).state();
    assert_eq!(lru.cache.rank_header, Some(RankHeader::Lru));
    let ranks: Vec<_> = way(&lru, 0, 0)
        .rank
        .iter()
        .chain(&way(&lru, 0, 1).rank)
        .map(|rank| (rank.value, rank.role))
        .collect();
    assert_eq!(ranks, [(0, Some(RankRole::Mru)), (1, Some(RankRole::Lru))]);
    assert!(lru.cache.show_dirty && !lru.cache.wrap);
    let fifo = session(config(Replacement::Fifo, 8, WritePolicy::WriteBack), []).state();
    assert_eq!(fifo.cache.rank_header, Some(RankHeader::Age));
    let roles: Vec<_> = fifo.cache.sets[0]
        .ways
        .iter()
        .map(|way| way.rank.unwrap().role)
        .collect();
    assert_eq!(roles.first(), Some(&Some(RankRole::New)));
    assert_eq!(roles.last(), Some(&Some(RankRole::Old)));
    assert!(roles[1..7].iter().all(Option::is_none));
    // More than four ways wrap.
    assert!(fifo.cache.wrap);
    let random = session(
        config(Replacement::Random, 2, WritePolicy::WriteThrough),
        [],
    )
    .state();
    assert!(!random.cache.show_rank && random.cache.rank_header.is_none());
    assert!(way(&random, 0, 0).rank.is_none());
    // Write-through lines are never dirty.
    assert!(!random.cache.show_dirty);
    let direct = session(config(Replacement::Lru, 1, WritePolicy::WriteBack), []).state();
    assert!(!direct.cache.show_rank);
    assert_eq!(direct.cache.word_heads, ["+0", "+4", "+8", "+C"]);
    assert_eq!(
        direct.cache.header,
        "L1 cache \u{b7} 8 sets \u{d7} 1 way \u{d7} 16 B"
    );
}

#[test]
fn a_way_describes_itself_and_shows_its_line() {
    let mut session = frame_3b();
    session.seek(pos(24));
    let state = session.state();
    let held = way(&state, 2, 0);
    assert_eq!(
        (held.tag_text.as_deref(), held.line_text.as_deref()),
        (Some("0x02"), Some("0x0A0"))
    );
    assert_eq!(held.description, "Set 2, way 0: valid, tag 0x02, rank 0.");
    assert_eq!(
        way(&state, 2, 1).description,
        "Set 2, way 1: valid, tag 0x06, dirty, rank 1, victim."
    );
    let empty = way(&state, 0, 1);
    assert!(!empty.valid && empty.tag_text.is_none() && empty.words.is_empty());
    assert_eq!(empty.description, "Set 0, way 1: invalid, rank 1.");
}

#[test]
fn the_transfer_lane_is_lit_only_by_the_event_that_moves_data() {
    let timed = Config {
        cache: CacheConfig {
            write_policy: WritePolicy::WriteThrough,
            ..Preset::TwoWayLru.config().cache
        },
        timing: Some(Timing {
            hit_cycles: 1,
            miss_cycles: 20,
        }),
        ..Preset::TwoWayLru.config()
    };
    let mut through = session(timed, [load(0x0A4), store(0x0A4, 9)]);
    let lanes: Vec<(EventKind, Transfer, String, Option<String>)> = (1..=through.events().len())
        .map(|at| {
            through.seek(pos(u32::try_from(at).unwrap()));
            let state = through.state();
            let kind = state.event.unwrap().kind;
            (
                kind,
                state.bus.transfer,
                state.bus.text,
                state.bus.cycles_text,
            )
        })
        .filter(|(_, transfer, _, _)| *transfer != Transfer::Idle)
        .collect();
    assert_eq!(
        lanes,
        [
            (
                EventKind::Fill,
                Transfer::Fill,
                "Fill \u{b7} memory line 0x0A0 \u{2192} set 2, way 0".to_owned(),
                Some("20 cycles".to_owned())
            ),
            (
                EventKind::WriteThrough,
                Transfer::WriteThrough,
                "Write-through \u{b7} set 2, way 0 \u{2192} memory line 0x0A0".to_owned(),
                None
            ),
        ]
    );
    // A write-around is drawn as a write-through and named for what it is.
    let around = Config {
        cache: CacheConfig {
            allocate_policy: AllocatePolicy::WriteNoAllocate,
            ..Preset::TwoWayLru.config().cache
        },
        ..Preset::TwoWayLru.config()
    };
    let state = session(around, [store(0x0A4, 9)]).state();
    assert_eq!(state.bus.transfer, Transfer::WriteThrough);
    assert_eq!(
        state.bus.text,
        "Write-around \u{b7} CPU \u{2192} memory line 0x0A0"
    );
    assert_eq!(state.bus.cycles_text, None);
    assert_eq!(state.memory.bus_row, Some(0));
    // Between transfers the bus is idle.
    let state = first_visit().state();
    assert_eq!(
        (state.bus.transfer, state.bus.text.as_str(), state.bus.line),
        (Transfer::Idle, "Bus idle", None)
    );
}

#[test]
fn the_request_lane_follows_the_access_at_the_cursor() {
    let mut session = session(
        Preset::TwoWayLru.config(),
        [
            program("add r1, r0, #1\nld r2, [0x0A8]"),
            Op::Step,
            Op::Step,
        ],
    );
    let state = session.state();
    assert_eq!(state.request.status, RequestStatus::Issued);
    assert_eq!(state.request.kind, Some(AccessKind::Read));
    assert_eq!(state.request.text, "Load 0x0A8 \u{2192} cache");
    assert_eq!(state.request.source.as_deref(), Some("ld r2, [0x0A8]"));
    assert_eq!(state.request.source_text, "from access #2");
    assert_eq!(
        state
            .request
            .decoded
            .as_ref()
            .map(|decoded| decoded.addr_text.as_str()),
        Some("0x0A8")
    );
    // On the `add`, there is no request.
    session.seek(pos(2));
    let state = session.state();
    assert_eq!(state.request.status, RequestStatus::None);
    assert_eq!(state.request.text, "No request");
    assert!(state.request.source.is_none() && state.request.decoded.is_none());
    assert_eq!(state.request.source_text, "");
    assert_eq!(state.cpu.instruction, "add r1, r0, #1");
}

#[test]
fn the_cpu_names_its_entry_inside_one_and_the_programs_state_between_entries() {
    let mut session = session(Preset::TwoWayLru.config(), [program(FRAME_3B)]);
    let state = session.state();
    assert_eq!(state.cpu.status, CpuStatus::Ready);
    assert_eq!(state.cpu.instruction, "\u{2014}");
    let listing = state.program.unwrap();
    assert_eq!(listing.current_line, Some(1));
    assert_eq!(
        listing.summary,
        "6 instructions \u{b7} next: st [0x0B0], #5"
    );
    assert_eq!(listing.lines.len(), 6);
    assert_eq!(listing.lines[1].text, "  ld  r1, [0x0A4]");
    session.apply(Op::Step).unwrap();
    let state = session.state();
    assert_eq!(
        (state.cpu.status, state.cpu.status_text.as_str()),
        (CpuStatus::Ready, "Ready")
    );
    assert_eq!(state.cpu.instruction, "st [0x0B0], #5");
    // Inside the access, its steps are counted; the program's line is the
    // one in flight.
    session.seek(pos(2));
    let state = session.state();
    assert_eq!(state.cpu.status_text, "Access #1 \u{b7} step 1 of 5");
    let listing = state.program.unwrap();
    assert!(listing.executing);
    assert_eq!((listing.current_line, listing.executed), (Some(1), 0));
    // Run to the end: finished.
    session.seek(pos(6));
    session.apply(Op::Run { n: 5 }).unwrap();
    let state = session.state();
    assert_eq!(
        (state.cpu.status, state.cpu.status_text.as_str()),
        (CpuStatus::Finished, "Finished")
    );
    let listing = state.program.unwrap();
    assert!(listing.finished && !listing.executing && listing.current_line.is_none());
    assert_eq!(listing.summary, "6 instructions \u{b7} next: end");
    // Without a program the CPU is ready for an access issued by hand.
    let hand = self::session(Config::first_visit(), [load(0x0A4)]).state();
    assert_eq!(
        (hand.cpu.status, hand.cpu.status_text.as_str()),
        (CpuStatus::NoProgram, "Ready")
    );
    assert!(hand.program.is_none());
    let registers: Vec<(Register, &str)> = hand
        .cpu
        .registers
        .iter()
        .map(|view| (view.register, view.text.as_str()))
        .take(2)
        .collect();
    assert_eq!(registers, [(Register::R0, "0"), (Register::R1, "53")]);
}

#[test]
fn a_program_that_stops_says_where() {
    let faulted = session(
        Config::first_visit(),
        [program("add r2, r0, #4096\nld r1, [r2]"), Op::Run { n: 2 }],
    );
    let state = faulted.state();
    assert_eq!(
        (state.cpu.status, state.cpu.status_text.as_str()),
        (CpuStatus::Faulted, "Stopped at line 2")
    );
    assert_eq!(state.cpu.instruction, "ld r1, [r2]");
    let error = faulted.timeline().current.unwrap();
    assert_eq!(
        (
            error.kind,
            error.outcome,
            error.outcome_text.as_str(),
            error.number
        ),
        (EntryKind::Error, EntryOutcome::Error, "error", Some(2))
    );
    assert_eq!(error.label, "ld r1, [r2]");
    assert_eq!(error.changed_summary.as_deref(), Some("nothing changed"));
    // A loop without an exit uses its budget.
    let mut endless = session(
        Config::first_visit(),
        [program("add r1, r0, #1\nloop: bnez r1, loop")],
    );
    while endless.apply(Op::Run { n: MAX_RUN_STEPS }).is_ok() {}
    let state = endless.state();
    assert_eq!(
        (state.cpu.status, state.cpu.status_text.as_str()),
        (CpuStatus::OutOfBudget, "Stopped at line 2")
    );
}

#[test]
fn memory_lists_the_lines_touched_and_the_lines_watched() {
    let mut session = frame_3b();
    let rows = |session: &Session| -> Vec<(String, bool, bool)> {
        session
            .memory_lines(0, 100)
            .into_iter()
            .map(|row| (row.line_text, row.watched, row.on_bus))
            .collect()
    };
    let owned = |rows: &[(&str, bool, bool)]| -> Vec<(String, bool, bool)> {
        rows.iter()
            .map(|(text, watched, bus)| ((*text).to_owned(), *watched, *bus))
            .collect()
    };
    assert_eq!(
        rows(&session),
        owned(&[
            ("0x0A0", false, false),
            ("0x0B0", false, false),
            ("0x1A0", false, false),
            ("0x2A0", false, false),
        ])
    );
    // A watched line joins the rows in its place, from the start of its
    // line, once; a line already touched is marked watched; one beyond the
    // address width is left out.
    session.set_memory_watch(&[line(0x124), line(0x0A0), line(0x1000), line(0x124)]);
    assert_eq!(
        rows(&session),
        owned(&[
            ("0x0A0", true, false),
            ("0x0B0", false, false),
            ("0x120", true, false),
            ("0x1A0", false, false),
            ("0x2A0", false, false),
        ])
    );
    let state = session.state();
    assert_eq!(state.memory.caption, "4 lines touched");
    assert_eq!(state.memory.line_count, 5);
    assert_eq!(state.memory.bus_row, None);
    // The fill of 0x2A0 is on the bus; the rows are windowed.
    session.seek(pos(26));
    assert_eq!(session.state().memory.bus_row, Some(4));
    let window = session.memory_lines(4, 1);
    assert_eq!(window.len(), 1);
    assert!(window[0].on_bus && window[0].line_text == "0x2A0");
    assert_eq!(
        window[0]
            .resident
            .map(|resident| (resident.set.get(), resident.way.get())),
        Some((2, 1))
    );
    // A watched line nothing has touched shows memory's initial contents.
    let watched = &session.memory_lines(2, 1)[0];
    assert_eq!(watched.resident, None);
    assert_eq!(watched.words[1].text, "81");
    assert_eq!(watched.words[1].description, "0x124 = 81.");
    assert!(session.memory_lines(5, 10).is_empty());
}

#[test]
fn a_stale_word_names_the_way_that_holds_the_newer_value() {
    let session = session(Preset::TwoWayLru.config(), [store(0x0A4, 7)]);
    let row = &session.memory_lines(0, 1)[0];
    let stale = row.words[1].stale.as_ref().unwrap();
    assert_eq!((stale.set.get(), stale.way.get()), (2, 0));
    assert_eq!(stale.text, "Stale: set 2, way 0 holds the newer value 7.");
    assert_eq!(
        row.words[1].description,
        "0x0A4 = 53. Stale: set 2, way 0 holds the newer value 7."
    );
    assert!(row.words[0].stale.is_none());
}

#[test]
fn the_timeline_counts_the_steps_of_the_displayed_run() {
    let mut session = frame_3b();
    let timeline = session.timeline();
    assert_eq!(
        (timeline.run, timeline.runs, timeline.origin, timeline.end),
        (1, 1, pos(0), pos(28))
    );
    assert_eq!((timeline.step, timeline.step_count), (28, 28));
    assert_eq!(timeline.header_text, "step 28 of 28");
    assert_eq!(timeline.entry_count, 5);
    // A new run: its own origin, steps and entries.
    session
        .apply(Op::Config {
            config: Preset::FourWayWriteBack.config(),
        })
        .unwrap();
    session.apply(load(0x0A4)).unwrap();
    let timeline = session.timeline();
    assert_eq!(
        (timeline.run, timeline.runs, timeline.origin, timeline.end),
        (2, 2, pos(29), pos(34))
    );
    assert_eq!((timeline.step, timeline.step_count), (5, 5));
    assert_eq!(timeline.entry_count, 1);
    assert_eq!(session.entries(0, 10)[0].label, "ld r1, [0x0A4]");
    assert_eq!(session.ticks(), [3, 2, 2, 2, 2]);
    // At the new run's origin, nothing is current; its steps are ahead.
    session.seek(pos(29));
    let timeline = session.timeline();
    assert!(timeline.current.is_none());
    assert_eq!(
        (timeline.step, timeline.header_text.as_str()),
        (0, "step 0 of 5")
    );
    assert_eq!(session.entries(0, 1)[0].tense, Tense::Future);
    assert_eq!(timeline.live_text, session.state().event.unwrap().explain);
    // Back in the first run, the timeline is the first run's.
    session.seek(pos(10));
    let timeline = session.timeline();
    assert_eq!(
        (timeline.run, timeline.end, timeline.step),
        (1, pos(28), 10)
    );
}

#[test]
fn entries_are_windowed_and_only_the_current_one_brings_its_events() {
    let mut session = frame_3b();
    session.seek(pos(13));
    let entries = session.entries(1, 3);
    let seen: Vec<(u32, Option<u32>, Tense, usize)> = entries
        .iter()
        .map(|entry| (entry.index, entry.number, entry.tense, entry.events.len()))
        .collect();
    assert_eq!(
        seen,
        [
            (1, Some(2), Tense::Past, 0),
            (2, Some(3), Tense::Current, 6),
            (3, Some(4), Tense::Future, 0),
        ]
    );
    assert_eq!(
        entries[0].changed_summary.as_deref(),
        Some("set 2 way 0 valid, tag, data \u{b7} r1 \u{b7} misses")
    );
    assert_eq!(entries[2].changed_summary, None);
    let tenses: Vec<Tense> = entries[1].events.iter().map(|row| row.tense).collect();
    assert_eq!(
        tenses,
        [
            Tense::Past,
            Tense::Past,
            Tense::Current,
            Tense::Future,
            Tense::Future,
            Tense::Future
        ]
    );
    let row = &entries[1].events[2];
    assert_eq!((row.pos, row.label.as_str()), (pos(13), "3.3"));
    assert_eq!(row.text, "Way 1 chosen: it is invalid (empty).");
    assert_eq!(session.timeline().current.as_ref(), Some(&entries[1]));
    // Past the run's entries the window is empty.
    assert!(session.entries(5, 3).is_empty());
    assert_eq!(session.entries(4, 100).len(), 1);
}

#[test]
fn a_program_load_is_a_divider_without_a_step() {
    let mut session = session(Preset::TwoWayLru.config(), [program(FRAME_3B), Op::Step]);
    let entries = session.entries(0, 10);
    let divider = &entries[0];
    assert_eq!(
        (divider.kind, divider.outcome, divider.number),
        (EntryKind::Program, EntryOutcome::Program, None)
    );
    assert_eq!(divider.label, "Loaded a program of 6 instructions");
    assert_eq!(divider.outcome_text, "program");
    assert_eq!(divider.changed_summary, None);
    assert_eq!(entries[1].label, "st [0x0B0], #5");
    assert_eq!(entries[1].number, Some(1));
    // It has no tick, and does not count as a step.
    assert_eq!(session.ticks().len(), 5);
    assert_eq!(session.timeline().step_count, 5);
    // On it, nothing is highlighted.
    session.seek(pos(1));
    let state = session.state();
    assert!(state.highlight.is_empty());
    assert_eq!(session.timeline().current.unwrap().kind, EntryKind::Program);
}

#[test]
fn ticks_mark_the_first_step_of_each_entry_and_the_steps_of_a_miss() {
    let session = session(
        Preset::TwoWayLru.config(),
        [
            program("ld r1, [0x0A4]\nadd r2, r0, #1\nld r3, [0x0A8]"),
            Op::Step,
            Op::Step,
            Op::Step,
        ],
    );
    // A miss of 5 steps, an instruction of 1, a hit of 3.
    assert_eq!(session.ticks(), [3, 2, 2, 2, 2, 1, 1, 0, 0]);
    let outcomes: Vec<(EntryOutcome, String)> = session
        .entries(1, 3)
        .into_iter()
        .map(|entry| (entry.outcome, entry.outcome_text))
        .collect();
    assert_eq!(
        outcomes,
        [
            (EntryOutcome::Miss, "miss".to_owned()),
            (EntryOutcome::CpuOnly, "CPU only".to_owned()),
            (EntryOutcome::Hit, "hit".to_owned()),
        ]
    );
}

#[test]
fn the_live_sentence_opens_with_the_outcome_after_a_jump() {
    let mut session = session(Preset::TwoWayLru.config(), [program(FRAME_3B)]);
    for _ in 0..5 {
        session.apply(Op::Step).unwrap();
    }
    // A seek from afar lands on access #5's read: the design's example.
    session.seek(pos(0));
    session.seek(pos(28));
    assert_eq!(
        session.timeline().live_text,
        "#5 ld r3, [0x2A4]: miss \u{b7} evict \u{b7} write-back. Read word 1 of way 1 (68) into \
         r3."
    );
    // One event on, as Space moves, the log line alone.
    session.step_event(None);
    assert_eq!(
        session.timeline().live_text,
        "LRU ranks in set 2: way 1 becomes 0 (most recent); way 0 is 1."
    );
    // Space at the end of the log executes the next instruction and shows
    // its first event, still one at a time.
    session.step_event(None);
    assert_eq!(
        session.timeline().live_text,
        "Load 0x1A4: tag 0x06, set 2, offset 4 (word 1)."
    );
    // Shift+Space jumps to the access's end.
    session.step_access(None);
    assert!(session
        .timeline()
        .live_text
        .starts_with("#6 ld r4, [0x1A4]: miss \u{b7} evict. "));
    // At position 0 there is nothing to read.
    session.seek(pos(0));
    assert_eq!(session.timeline().live_text, "");
}

#[test]
fn the_controls_follow_the_cursor_and_the_log() {
    let enabled = |session: &Session, action| session.state().controls.enabled.contains(&action);
    let mut session = frame_3b();
    assert!(enabled(&session, ControlAction::Undo));
    assert!(!enabled(&session, ControlAction::Redo));
    assert!(enabled(&session, ControlAction::Reset));
    session.undo();
    assert!(enabled(&session, ControlAction::Redo));
    session.seek(pos(0));
    assert!(!enabled(&session, ControlAction::Undo));
    assert!(enabled(&session, ControlAction::Redo));
    // Nothing of the run is on screen at its origin, so nothing to reset.
    assert!(!enabled(&session, ControlAction::Reset));
    let state = session.state();
    for action in [
        ControlAction::Step,
        ControlAction::StepAccess,
        ControlAction::Run,
        ControlAction::Issue,
        ControlAction::Edit,
        ControlAction::Configure,
        ControlAction::Program,
        ControlAction::Load,
    ] {
        assert!(state.controls.enabled.contains(&action), "{action:?}");
    }
    assert_eq!(state.controls.seek_limit, None);
    assert_eq!(state.controls.guard, None);
}

#[test]
fn the_labels_give_the_mode_the_runs_steps_and_timing() {
    let mut session = frame_3b();
    session.seek(pos(3));
    let labels = session.state().labels;
    assert_eq!(labels.status_text, "Basics \u{b7} 28 steps in this run");
    assert_eq!(
        labels.config_summary,
        "128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU"
    );
    session.set_advanced(true);
    let labels = session.state().labels;
    assert_eq!(labels.status_text, "Advanced \u{b7} 28 steps in this run");
    assert_eq!(
        labels.config_summary,
        "128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU \u{b7} write-back \u{b7} allocate"
    );
    let timed = Config {
        timing: Some(Timing {
            hit_cycles: 1,
            miss_cycles: 20,
        }),
        ..Preset::TwoWayLru.config()
    };
    let state = self::session(timed, [load(0x0A4)]).state();
    assert!(state.advanced);
    assert_eq!(
        state.labels.status_text,
        "Advanced \u{b7} 6 steps in this run \u{b7} timed"
    );
}

#[test]
fn the_state_does_not_depend_on_how_the_cursor_got_there() {
    let mut session = frame_3b();
    session
        .apply(Op::Config {
            config: Preset::DirectMapped8.config(),
        })
        .unwrap();
    session.apply(store(0x0A4, 1)).unwrap();
    session.apply(load(0x1A4)).unwrap();
    let end = u32::try_from(session.events_range(0, u32::MAX).len()).unwrap();
    // Every position reached by a seek from the end of the log ...
    let sought: Vec<State> = (0..=end)
        .map(|at| {
            let mut fresh = session.clone();
            fresh.seek(pos(at));
            fresh.state()
        })
        .collect();
    // ... reads the same when reached one event at a time from the start ...
    session.seek(pos(0));
    for expected in sought.iter().skip(1) {
        assert_eq!(
            session.step_event(None),
            cache_explorer_core::StepOutcome::Moved
        );
        assert_eq!(&session.state(), expected);
    }
    // ... and backwards, one seek at a time.
    for at in (0..=end).rev() {
        session.seek(pos(at));
        assert_eq!(
            &session.state(),
            &sought[usize::try_from(at).unwrap()],
            "at {at}"
        );
    }
}

#[test]
fn the_issue_draft_reads_the_typed_address_and_value() {
    let session = frame_3b();
    let store_draft = session.draft_issue(AccessKind::Write, "1a4", " -3 ");
    assert_eq!(store_draft.op, Some(store(0x1A4, 4_294_967_293)));
    assert_eq!(store_draft.request_text, "Store 0x1A4");
    assert_eq!(
        store_draft.compare,
        Some(vec![WayCompare::Pending, WayCompare::Pending])
    );
    // A store needs a value that fits.
    let missing = session.draft_issue(AccessKind::Write, "0x1A4", "");
    assert_eq!(missing.op, None);
    assert!(missing.value.is_some());
    // A misaligned address decodes, so the strip follows it, but issues
    // nothing.
    let misaligned = session.draft_issue(AccessKind::Read, "0x0A6", "");
    assert!(matches!(
        misaligned.address,
        AddressInput::Misaligned { .. }
    ));
    assert_eq!(misaligned.op, None);
    assert_eq!(misaligned.selected_set.map(SetIndex::get), Some(2));
    assert_eq!(misaligned.request_text, "Load 0x0A6");
    // An address that does not decode has no request and no set.
    let invalid = session.draft_issue(AccessKind::Read, "0x1000", "");
    assert_eq!(invalid.request_text, "No request");
    assert_eq!((invalid.selected_set, invalid.compare), (None, None));
    assert!(invalid.value.is_none());
    assert_eq!(
        session.read_address("0x0A4"),
        session.draft_issue(AccessKind::Read, "0x0A4", "").address
    );
    assert_eq!(
        session.read_word("7"),
        session
            .draft_issue(AccessKind::Write, "0x0A4", "7")
            .value
            .unwrap()
    );
    assert_eq!(session.decode(0x0A4).unwrap().tag_text, "0x02");
}

#[test]
fn a_window_of_events_covers_the_log_undone_events_included() {
    let mut session = frame_3b();
    session.seek(pos(3));
    assert_eq!(session.events().len(), 3);
    let window = session.events_range(20, 24);
    let ids: Vec<u64> = window.iter().map(|event| event.id).collect();
    assert_eq!(ids, [20, 21, 22, 23]);
    assert_eq!(session.events_range(26, 100).len(), 2);
    assert!(session.events_range(30, 40).is_empty());
    assert!(session.events_range(5, 2).is_empty());
}

#[test]
fn the_snapshot_serializes_with_its_tags() {
    let mut session = frame_3b();
    session.seek(pos(24));
    let value = serde_json::to_value(session.state()).unwrap();
    assert_eq!(
        value["cpu"]["status"],
        serde_json::json!({"state": "busy", "entry": 5, "seq": 4, "of": 8})
    );
    assert_eq!(value["request"]["status"], "issued");
    assert_eq!(value["bus"]["transfer"], "writeBack");
    assert_eq!(value["cache"]["rankHeader"], "lru");
    assert_eq!(
        value["cache"]["compare"],
        serde_json::json!(["mismatch", "mismatch"])
    );
    assert_eq!(
        value["cache"]["sets"][2]["ways"][1]["rank"],
        serde_json::json!({"value": 1, "role": "lru"})
    );
    let timeline = serde_json::to_value(session.timeline()).unwrap();
    assert_eq!(timeline["current"]["kind"], "access");
    assert_eq!(timeline["current"]["tense"], "current");
    // Every entry carries its events, empty but for the current one.
    let entries = serde_json::to_value(session.entries(0, 1)).unwrap();
    assert_eq!(entries[0]["events"], serde_json::json!([]));
    assert_eq!(
        serde_json::to_value(CpuStatus::NoProgram).unwrap(),
        serde_json::json!({"state": "noProgram"})
    );
    assert_eq!(
        serde_json::to_value(CpuStatus::OutOfBudget).unwrap(),
        serde_json::json!({"state": "outOfBudget"})
    );
}
