//! The session: the log of ops and its event cursor. Positions and their
//! boundaries, position 0 of a non-empty log included; every cursor move
//! (undo and redo between undo stops, which skip a `Program` op that another
//! op follows, scrubbing, seeking, Space and Shift+Space with the drawer's
//! program); appending, which drops the undone ops only once the op is
//! accepted; a refused op of each kind leaving the log, the undone ops and
//! the cursor as they were; configuration changes, applied at once as ops
//! of their own or asked about first, and Reset; switching between Basics
//! and Advanced; new runs keeping the program; the session's event limit,
//! the ops a link can hold and the bytes a saved file can hold, which the
//! editors report before planning and the configuration bar before asking,
//! and the first two of which refuse a press of two ops whole; a file at its
//! limit to the byte, within it wherever the cursor is and in either mode,
//! whose size the session keeps as the file's own; every state the cursor
//! reaches equal to the fold's, from checkpoints or not; and the gate a
//! guided module's unanswered prediction sets, which refuses every op,
//! holds the cursor at or before where the prediction began, and shows in
//! the controls.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::limits::{
    CHECKPOINT_INTERVAL, INSTRUCTION_BUDGET, MAX_EVENTS_PER_ENTRY, MAX_RUN_STEPS,
    MAX_SHARE_JSON_BYTES, MAX_SHARE_OPS, SESSION_EVENT_LIMIT,
};
use cache_explorer_core::{
    AccessKind, Boundary, CacheConfig, Config, ConfigChange, ConfigError, ControlAction,
    EditOutcome, EventKind, LoadError, MicroEvent, Op, OpError, OpIndex, Preset, ProgramFormat,
    ProgramSource, RefusalReason, Replacement, ReplayError, Session, StepOutcome, WordBytes,
    WritePolicy,
};

use common::{addr, apply_all, grouped, load, pos, program, store, word};
use support::{
    as_large_as_a_file, as_long_as_a_link, change_refused, config, drawer, edit, edit_word,
    editors_refuse_for_room, end_of, gated_at_end, gated_mid_log, hand_loads, log, log_end, padded,
    refused, session, table, varied,
};

#[cfg(test)]
mod common;

/// An `add` then a load: two entries, the second an access.
const ADD_THEN_LOAD: &str = "add r1, r0, #1\nld r2, [0x0B0]";

/// A loop without an exit and without a memory access: one event per
/// instruction until the budget or the session's limit stops it.
const ENDLESS: &str = "        add r1, r0, #1\nloop:   bnez r1, loop";

/// A loop that loads a word a line at a time and counts `r3` down from 40,
/// then finishes: hits, misses and evictions, several entries per `Run`.
const WALK: &str = concat!(
    "        add r3, r0, #40\n",
    "        add r2, r0, #0x0A0\n",
    "loop:   ld r1, [r2]\n",
    "        st [r2], r3\n",
    "        add r2, r2, #16\n",
    "        add r3, r3, #-1\n",
    "        bnez r3, loop",
);

/// The tests' helpers. They sit in a module compiled for tests only, where a
/// failed `unwrap` is a failed test, as in the tests themselves.
#[cfg(test)]
mod support {
    use cache_explorer_core::limits::{MAX_PROGRAM_BYTES, MAX_SHARE_OPS};
    use cache_explorer_core::{
        Boundary, Config, EditOutcome, EventPos, MicroEvent, Module, ModuleRun, Op, OpError,
        Preset, ProgramFormat, ProgramSource, RefusalReason, Session, Tone, WayForm,
    };

    use super::common::{addr, apply_all, load, pos, program, store};
    use super::{ADD_THEN_LOAD, WALK};

    /// The tests' configuration: `direct-mapped-8`, whose 12-bit
    /// addresses, 4-byte words and pattern memory the expected events and
    /// lines are written for.
    pub(crate) fn config() -> Config {
        Preset::DirectMapped8.config()
    }

    /// A session with an empty log on `config()`.
    pub(crate) fn session() -> Session {
        Session::with_config(config()).unwrap()
    }

    /// A session whose log holds `count` loads by hand of one word, a miss
    /// and then hits.
    pub(crate) fn hand_loads(count: u32) -> Session {
        let mut session = session();
        for _ in 0..count {
            session.apply(load(0x0A4)).unwrap();
        }
        session
    }

    /// A session whose log holds as many ops as a link can, loads by hand.
    pub(crate) fn as_long_as_a_link() -> Session {
        hand_loads(MAX_SHARE_OPS)
    }

    /// The instruction a padded program starts with, and the comment that
    /// follows it.
    const PADDED: &str = "add r1, r0, #1\n; ";

    /// A load of a program of one instruction and a comment of `letters`
    /// letters, whose JSON grows by exactly one byte a letter.
    pub(crate) fn padded(letters: usize) -> Op {
        program(&format!("{PADDED}{}", "x".repeat(letters)))
    }

    /// A session whose saved file is as large as a file can hold, to the
    /// byte: padded programs, the last of them the longest the session
    /// accepts, found by bisection, so that no op of any kind fits after it.
    #[track_caller]
    pub(crate) fn as_large_as_a_file() -> Session {
        let most = MAX_PROGRAM_BYTES as usize - PADDED.len();
        let fits = |session: &Session, letters| session.clone().apply(padded(letters)).is_ok();
        let mut session = session();
        // Half the longest program at a time: while the longest fits, what
        // is left after half of it still has room for the shortest.
        while fits(&session, most) {
            session.apply(padded(most / 2)).unwrap();
        }
        let (mut low, mut high) = (0, most);
        assert!(fits(&session, low));
        while high - low > 1 {
            let mid = low.midpoint(high);
            if fits(&session, mid) {
                low = mid;
            } else {
                high = mid;
            }
        }
        assert_eq!(session.clone().apply(padded(high)), Err(OpError::FileFull));
        session.apply(padded(low)).unwrap();
        session
    }

    /// Checks that a change from the configuration bar to `two-way-lru` is
    /// refused on `session` with `error`, rather than asked about, and
    /// changes nothing: agreeing would append a `Config` op, which `error`
    /// refuses.
    #[track_caller]
    pub(crate) fn change_refused(session: &mut Session, error: &OpError) {
        let before = session.clone();
        assert_eq!(
            session.propose_config(Preset::TwoWayLru.config()).as_ref(),
            Err(error)
        );
        assert_eq!(session.ops(), before.ops());
        assert_eq!(session.position(), before.position());
        assert_eq!(session.machine(), before.machine());
    }

    /// An edit of `target` with `patch`, each as the UI sends it.
    pub(crate) fn edit(target: &serde_json::Value, patch: &serde_json::Value) -> Op {
        serde_json::from_value(serde_json::json!({"op": "edit", "target": target, "patch": patch}))
            .unwrap()
    }

    /// An edit of the memory word at 0x0A4 to 1.
    pub(crate) fn edit_word() -> Op {
        edit(
            &serde_json::json!({"kind": "memory", "addr": 0x0A4}),
            &serde_json::json!({"kind": "memory", "value": 1}),
        )
    }

    /// Checks that the editors refuse every edit on `session` for want of
    /// room in its log, for `reason`, saying `message` and offering nothing:
    /// the plan of [`edit_word`], and the drafts of that word and of way 0
    /// of set 0 made valid.
    #[track_caller]
    pub(crate) fn editors_refuse_for_room(session: &Session, reason: RefusalReason, message: &str) {
        let Op::Edit { target, patch } = edit_word() else {
            panic!("an edit op");
        };
        let validate = WayForm {
            valid: true,
            line_text: "0x000".to_owned(),
            word_texts: Vec::new(),
            dirty: false,
            rank: None,
        };
        let set = serde_json::from_value(0.into()).unwrap();
        let way = serde_json::from_value(0.into()).unwrap();
        for (editor, outcome) in [
            ("the plan", session.plan_edit(&target, &patch).into()),
            (
                "the memory draft",
                session.draft_memory_edit(addr(0x0A4), "1").outcome,
            ),
            (
                "the way draft",
                session.draft_edit(set, way, &validate).outcome,
            ),
        ] {
            let EditOutcome::Refusal(refusal) = outcome else {
                panic!("a full session plans no edit, {editor}: {outcome:?}");
            };
            assert_eq!(
                (
                    refusal.reason,
                    refusal.tone,
                    refusal.title.as_str(),
                    refusal.text.as_str()
                ),
                (reason, Tone::Refused, "Refused", message),
                "{editor}"
            );
            assert_eq!(refusal.suggestion, None, "{editor}");
        }
    }

    /// The drawer holding the assembly program `source`.
    pub(crate) fn drawer(source: &str) -> ProgramSource {
        ProgramSource {
            source: source.to_owned(),
            format: ProgramFormat::Asm,
        }
    }

    /// The session of the cursor table: two loads by hand, a program load,
    /// and a `StepAccess` that executes its `add` and then its load. Fifteen
    /// events.
    pub(crate) fn table() -> Session {
        let mut session = session();
        apply_all(
            &mut session,
            [
                load(0x0A4),
                load(0x0A8),
                program(ADD_THEN_LOAD),
                Op::StepAccess,
            ],
        );
        session
    }

    /// A log of every kind of op: loads and stores by hand, with a dirty
    /// eviction; a program stepped, run and stepped to an access; a new run
    /// that keeps it; and more of it under the new configuration.
    pub(crate) fn varied() -> Session {
        let mut session = session();
        let two_way = Preset::TwoWayLru.config();
        apply_all(
            &mut session,
            [
                load(0x0A4),
                store(0x0A8, 7),
                load(0x1A4),
                program(WALK),
                Op::Step,
                Op::Run { n: 3 },
                Op::StepAccess,
                Op::StepAccess,
                Op::Config { config: two_way },
                Op::Run { n: 12 },
                store(0x2A4, 9),
                Op::StepAccess,
                Op::Config { config: config() },
                Op::Run { n: 5 },
            ],
        );
        session
    }

    /// The position after the last event of `session`.
    pub(crate) fn log_end(session: &Session) -> EventPos {
        let mut end = session.clone();
        end.scrub_to(end.op_count());
        end.position().pos
    }

    /// Every event of `session`'s log, the undone ones included.
    pub(crate) fn log(session: &Session) -> Vec<MicroEvent> {
        let mut end = session.clone();
        end.scrub_to(end.op_count());
        end.events().to_vec()
    }

    /// Checks that `op`, applied to `session`, is refused with `error` and
    /// changes nothing: the log, the undone ops, the cursor and the machine.
    #[track_caller]
    pub(crate) fn refused(session: &mut Session, op: &Op, error: &OpError) {
        let before = session.clone();
        assert_eq!(session.apply(op.clone()).as_ref(), Err(error), "{op:?}");
        assert_eq!(session.ops(), before.ops(), "{op:?}");
        assert_eq!(session.op_count(), before.op_count());
        assert_eq!(session.position(), before.position());
        assert_eq!(session.machine(), before.machine());
        assert_eq!(log(session), log(&before), "{op:?}");
    }

    /// The module with a prediction in the middle of the log, taken to its
    /// prediction on a new session.
    #[track_caller]
    pub(crate) fn gated_mid_log() -> (ModuleRun, Session) {
        let mut session = session();
        let mut run = ModuleRun::new(Module::parse(super::PREDICT_MID_LOG).unwrap());
        run.start(&mut session).unwrap();
        run.show_me(&mut session);
        run.show_me(&mut session);
        assert_eq!(session.position().label, "1.2");
        assert!(session.gate().is_some());
        (run, session)
    }

    /// The module whose first step is a prediction, started on a new session:
    /// the prediction begins at the end of the log.
    #[track_caller]
    pub(crate) fn gated_at_end() -> (ModuleRun, Session) {
        let mut session = session();
        let mut run = ModuleRun::new(Module::parse(super::PREDICT_AT_END).unwrap());
        run.start(&mut session).unwrap();
        assert!(session.gate().is_some());
        assert_eq!(session.position().boundary, Boundary::LogEnd);
        (run, session)
    }

    /// The position after the last event of `session`'s log, which a gate
    /// does not clamp.
    pub(crate) fn end_of(session: &Session) -> EventPos {
        pos(u32::try_from(session.events_range(0..u32::MAX).len()).unwrap())
    }
}

#[test]
fn a_new_session_rests_at_position_0_of_an_empty_log() {
    let mut session = session();
    let position = session.position();
    assert_eq!(position.pos, pos(0));
    assert_eq!(position.ops_done, 0);
    assert_eq!(position.op, None);
    assert_eq!(position.run, 1);
    assert_eq!(position.entry, None);
    assert_eq!((position.seq, position.entry_length), (0, 0));
    assert_eq!(position.label, "");
    assert_eq!(position.boundary, Boundary::LogEnd);
    assert!(position.at_run_origin);
    assert_eq!(session.op_count(), 0);
    assert_eq!(session.cursor(), 0);
    assert!(session.events().is_empty());
    assert!(!session.undo());
    assert!(!session.redo());
    assert_eq!(session.initial(), &config());
    assert_eq!(session.config(), &config());
    assert_eq!(
        Session::with_config(Config {
            address_bits: 4,
            ..config()
        })
        .err(),
        Some(ConfigError::AddressBits { got: 4 })
    );
}

#[test]
fn an_applied_op_is_appended_and_the_cursor_rests_on_its_last_event() {
    let mut session = session();
    let events = session.apply(load(0x0A4)).unwrap().to_vec();
    let kinds: Vec<EventKind> = events.iter().map(|event| event.kind).collect();
    assert_eq!(
        kinds,
        [
            EventKind::Decode,
            EventKind::Miss,
            EventKind::Victim,
            EventKind::Fill,
            EventKind::Complete
        ]
    );
    assert_eq!(session.events(), events.as_slice());
    let position = session.position();
    assert_eq!(position.pos, pos(5));
    assert_eq!(position.ops_done, 1);
    assert_eq!(session.cursor(), 1);
    assert_eq!(position.label, "1.5");
    assert_eq!(position.boundary, Boundary::LogEnd);
    let events = session.apply(load(0x0A8)).unwrap();
    let ids: Vec<u64> = events.iter().map(|event| event.id).collect();
    assert_eq!(ids, [5, 6, 7]);
    assert_eq!(session.position().label, "2.3");
    assert_eq!(session.ops(), [load(0x0A4), load(0x0A8)]);
}

#[test]
fn an_applied_op_reports_the_events_the_log_holds_over_its_positions() {
    let mut session = session();
    for op in [
        load(0x0A4),
        store(0x0A8, 7),
        program(WALK),
        Op::Run { n: 4 },
        Op::StepAccess,
    ] {
        let from = end_of(&session);
        let reported = session.apply(op.clone()).unwrap().to_vec();
        let to = end_of(&session);
        assert!(!reported.is_empty(), "{op:?}");
        assert_eq!(
            reported,
            session.events_range(from.get()..to.get()),
            "{op:?}"
        );
    }
}

#[test]
fn an_op_applied_after_undo_reports_the_events_that_replace_the_undone_ones() {
    let mut session = table();
    assert!(session.undo());
    assert!(session.undo());
    let from = session.position().pos;
    assert_eq!(from, pos(5));
    let reported = session.apply(store(0x0A4, 7)).unwrap().to_vec();
    let to = end_of(&session);
    assert_eq!(
        to.get(),
        from.get() + u32::try_from(reported.len()).unwrap()
    );
    assert_eq!(reported, session.events_range(from.get()..to.get()));
}

#[test]
fn every_position_has_its_op_entry_event_and_boundary() {
    // pos, ops done, op, entry, seq, entry length, label, boundary
    type Row = (
        u32,
        u32,
        Option<u32>,
        Option<u32>,
        u32,
        u32,
        &'static str,
        Boundary,
    );
    let mut session = table();
    let rows: [Row; 16] = [
        (0, 0, None, None, 0, 0, "", Boundary::OpEnd),
        (1, 0, Some(0), Some(1), 1, 5, "1.1", Boundary::MidEntry),
        (2, 0, Some(0), Some(1), 2, 5, "1.2", Boundary::MidEntry),
        (3, 0, Some(0), Some(1), 3, 5, "1.3", Boundary::MidEntry),
        (4, 0, Some(0), Some(1), 4, 5, "1.4", Boundary::MidEntry),
        (5, 1, Some(0), Some(1), 5, 5, "1.5", Boundary::OpEnd),
        (6, 1, Some(1), Some(2), 1, 3, "2.1", Boundary::MidEntry),
        (7, 1, Some(1), Some(2), 2, 3, "2.2", Boundary::MidEntry),
        (8, 2, Some(1), Some(2), 3, 3, "2.3", Boundary::OpEnd),
        (9, 3, Some(2), None, 1, 1, "", Boundary::OpEnd),
        (10, 3, Some(3), Some(3), 1, 1, "3.1", Boundary::EntryEnd),
        (11, 3, Some(3), Some(4), 1, 5, "4.1", Boundary::MidEntry),
        (12, 3, Some(3), Some(4), 2, 5, "4.2", Boundary::MidEntry),
        (13, 3, Some(3), Some(4), 3, 5, "4.3", Boundary::MidEntry),
        (14, 3, Some(3), Some(4), 4, 5, "4.4", Boundary::MidEntry),
        (15, 4, Some(3), Some(4), 5, 5, "4.5", Boundary::LogEnd),
    ];
    for (at, ops_done, op, entry, seq, entry_length, label, boundary) in rows {
        session.seek(pos(at));
        let position = session.position();
        assert_eq!(position.pos, pos(at), "at {at}");
        assert_eq!(position.ops_done, ops_done, "at {at}");
        assert_eq!(session.cursor(), ops_done, "at {at}");
        assert_eq!(position.op.map(OpIndex::get), op, "at {at}");
        assert_eq!(position.run, 1, "at {at}");
        assert_eq!(position.entry, entry, "at {at}");
        assert_eq!(position.seq, seq, "at {at}");
        assert_eq!(position.entry_length, entry_length, "at {at}");
        assert_eq!(position.label, label, "at {at}");
        assert_eq!(position.boundary, boundary, "at {at}");
        assert_eq!(position.at_run_origin, at == 0, "at {at}");
        assert_eq!(session.events().len(), at as usize, "at {at}");
    }
    // A seek past the end rests at the end.
    session.seek(pos(99));
    assert_eq!(session.position().pos, pos(15));
}

#[test]
fn undo_and_redo_move_between_stops_and_skip_a_program_that_another_op_follows() {
    let mut session = table();
    // The stops are 0, 5, 8 and 15: the program load at 9 undoes together
    // with the op that ran it.
    let mut undone = Vec::new();
    while session.undo() {
        undone.push(session.position().pos.get());
    }
    assert_eq!(undone, [8, 5, 0]);
    assert_eq!(session.position().boundary, Boundary::OpEnd);
    let mut redone = Vec::new();
    while session.redo() {
        redone.push(session.position().pos.get());
    }
    assert_eq!(redone, [5, 8, 15]);
    // From inside an op, undo goes to the end of the stop before it, and
    // redo to the op's end.
    session.seek(pos(12));
    assert!(session.undo());
    assert_eq!(session.position().pos, pos(8));
    session.seek(pos(3));
    assert!(session.redo());
    assert_eq!(session.position().pos, pos(5));
    // Resting on the program load itself, undo goes to the stop before it
    // and redo past it.
    session.seek(pos(9));
    assert!(session.undo());
    assert_eq!(session.position().pos, pos(8));
    session.seek(pos(9));
    assert!(session.redo());
    assert_eq!(session.position().pos, pos(15));
    // Undo and redo keep the log.
    assert_eq!(session.op_count(), 4);
}

#[test]
fn a_program_load_at_the_end_of_the_log_is_an_undo_stop() {
    let mut session = session();
    apply_all(&mut session, [load(0x0A4), program(ADD_THEN_LOAD)]);
    assert_eq!(session.position().pos, pos(6));
    assert!(session.undo());
    assert_eq!(session.position().pos, pos(5));
    assert!(session.redo());
    assert_eq!(session.position().pos, pos(6));
    // Once another op follows it, it is no longer a stop.
    apply_all(&mut session, [Op::Step]);
    assert!(session.undo());
    assert_eq!(session.position().pos, pos(5));
}

#[test]
fn scrubbing_moves_to_the_end_of_an_op() {
    let mut session = table();
    for (done, at) in [(0, 0), (1, 5), (2, 8), (3, 9), (4, 15), (9, 15)] {
        session.scrub_to(done);
        assert_eq!(session.position().pos, pos(at), "scrub to {done}");
    }
}

#[test]
fn space_moves_one_event_and_at_the_end_executes_one_instruction() {
    let mut session = session();
    let sample = drawer(ADD_THEN_LOAD);
    // The drawer's program is loaded with the first Space, and the cursor
    // rests on the first event of the instruction it executes.
    assert_eq!(session.step_event(Some(&sample)), StepOutcome::Applied);
    assert_eq!(session.ops(), [program(ADD_THEN_LOAD), Op::Step]);
    let position = session.position();
    assert_eq!((position.pos, position.label.as_str()), (pos(2), "1.1"));
    assert_eq!(position.boundary, Boundary::LogEnd);
    // The same program is not loaded again.
    assert_eq!(session.step_event(Some(&sample)), StepOutcome::Applied);
    assert_eq!(session.op_count(), 3);
    let position = session.position();
    assert_eq!((position.pos, position.label.as_str()), (pos(3), "2.1"));
    assert_eq!(position.boundary, Boundary::MidEntry);
    for at in 4..=7 {
        assert_eq!(
            session.step_event(Some(&sample)),
            StepOutcome::Moved,
            "to {at}"
        );
        assert_eq!(session.position().pos, pos(at), "to {at}");
    }
    assert_eq!(session.position().boundary, Boundary::LogEnd);
    assert_eq!(
        session.step_event(Some(&sample)),
        StepOutcome::Idle {
            message: "End of program.".to_owned()
        }
    );
    assert_eq!(session.op_count(), 3);
    // Other text in the drawer is loaded before the next step.
    assert_eq!(
        session.step_event(Some(&drawer("ld r3, [0x0A8]"))),
        StepOutcome::Applied
    );
    assert_eq!(&session.ops()[3..], [program("ld r3, [0x0A8]"), Op::Step]);
    // So is a trace with the same text as the program.
    let trace = ProgramSource {
        source: "R 0x0A4".to_owned(),
        format: ProgramFormat::Trace,
    };
    session.scrub_to(session.op_count());
    assert_eq!(session.step_event(Some(&trace)), StepOutcome::Applied);
    assert_eq!(
        &session.ops()[5..],
        [
            Op::Program {
                source: "R 0x0A4".to_owned(),
                format: ProgramFormat::Trace
            },
            Op::Step
        ]
    );
}

#[test]
fn one_undo_undoes_one_space_with_its_program_load() {
    let mut session = session();
    let sample = drawer(ADD_THEN_LOAD);
    assert_eq!(session.step_event(Some(&sample)), StepOutcome::Applied);
    assert!(session.undo());
    assert_eq!(session.position().pos, pos(0));
    assert!(!session.undo());
    // Space from position 0 of a non-empty log moves along it.
    assert_eq!(session.step_event(Some(&sample)), StepOutcome::Moved);
    assert_eq!(session.position().pos, pos(1));
    assert_eq!(session.op_count(), 2);
}

#[test]
fn space_with_nothing_to_execute_says_why_and_appends_nothing() {
    let mut session = session();
    assert_eq!(
        session.step_event(None),
        StepOutcome::Idle {
            message: OpError::NoProgram.to_string()
        }
    );
    // A drawer program that does not parse appends nothing, and says why.
    let outcome = session.step_event(Some(&drawer("lw r1, [0x0A4]")));
    assert_eq!(
        outcome,
        StepOutcome::Idle {
            message: "The program has an error on line 1: unknown instruction `lw`; use ld, st, \
                      add or bnez."
                .to_owned()
        }
    );
    assert_eq!(
        session.step_access(Some(&drawer("lw r1, [0x0A4]"))),
        outcome
    );
    assert_eq!(session.op_count(), 0);
    // Nor does an empty drawer with no program loaded.
    assert_eq!(
        session.step_access(None),
        StepOutcome::Idle {
            message: OpError::NoProgram.to_string()
        }
    );
    assert_eq!(session.op_count(), 0);
}

#[test]
fn shift_space_moves_to_the_end_of_an_op_and_at_the_end_executes_to_an_access() {
    let mut session = table();
    session.seek(pos(0));
    let mut ends = Vec::new();
    while session.position().boundary != Boundary::LogEnd {
        assert_eq!(session.step_access(None), StepOutcome::Moved);
        ends.push(session.position().pos.get());
    }
    // To the end of each next op, the program load included.
    assert_eq!(ends, [5, 8, 9, 15]);
    // From inside an op, to its end.
    session.seek(pos(11));
    assert_eq!(session.step_access(None), StepOutcome::Moved);
    assert_eq!(session.position().pos, pos(15));
    // At the end, an op executes instructions until an access completes.
    let mut session = self::session();
    let source = "add r1, r0, #1\nadd r1, r1, #1\nld r2, [0x0A4]\nld r3, [0x0A8]";
    assert_eq!(
        session.step_access(Some(&drawer(source))),
        StepOutcome::Applied
    );
    assert_eq!(session.ops(), [program(source), Op::StepAccess]);
    let labels: Vec<&str> = session
        .events()
        .iter()
        .map(|event| event.label.as_str())
        .collect();
    assert_eq!(
        labels,
        ["", "1.1", "2.1", "3.1", "3.2", "3.3", "3.4", "3.5"]
    );
    assert_eq!(session.position().boundary, Boundary::LogEnd);
    // One Undo takes back the load and the op together.
    assert!(session.undo());
    assert_eq!(session.position().pos, pos(0));
}

#[test]
fn an_accepted_op_drops_the_undone_ops_and_reuses_their_ids() {
    let mut session = table();
    assert!(session.undo());
    assert!(session.undo());
    assert_eq!(session.position().pos, pos(5));
    let events = session.apply(store(0x0A4, 7)).unwrap();
    assert_eq!(events.first().map(|event| event.id), Some(5));
    assert_eq!(
        events.first().map(|event| event.label.as_str()),
        Some("2.1")
    );
    assert_eq!(session.ops(), [load(0x0A4), store(0x0A4, 7)]);
    assert!(!session.redo());
    // An op applied inside another keeps that op whole.
    session.seek(pos(2));
    session.apply(load(0x1A4)).unwrap();
    assert_eq!(session.ops(), [load(0x0A4), load(0x1A4)]);
    assert_eq!(session.events().first().map(|event| event.id), Some(0));
    assert_eq!(
        session.events().get(5).map(|event| event.label.as_str()),
        Some("2.1")
    );
    // From position 0, it replaces the whole log.
    session.seek(pos(0));
    session.apply(load(0x0A8)).unwrap();
    assert_eq!(session.ops(), [load(0x0A8)]);
    assert_eq!(session.position().pos, pos(5));
}

#[test]
fn a_refused_op_of_each_kind_leaves_the_log_the_undone_ops_and_the_cursor_as_they_were() {
    let bad_config = Config {
        cache: CacheConfig {
            size_bytes: 100,
            ..config().cache
        },
        ..config()
    };
    let config_error = bad_config.validate().unwrap_err();
    let geometry = config().validate().unwrap();
    let too_far = 1_u32 << geometry.address_bits();
    let issue_errors = [
        (
            load(0x0A6),
            "0x0A6 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8.",
        ),
        (
            load(too_far),
            "0x1000 does not fit a 12-bit address (0x000\u{2013}0xFFF).",
        ),
        (
            Op::Issue {
                kind: AccessKind::Write,
                addr: addr(0x0A4),
                value: None,
            },
            "A store needs a value to write.",
        ),
        (
            Op::Issue {
                kind: AccessKind::Read,
                addr: addr(0x0A4),
                value: Some(word(7)),
            },
            "A load takes no value; only a store writes one.",
        ),
        (
            store(0x0A4, 1 << 32),
            "4,294,967,296 does not fit a 4-byte word (0\u{2013}4,294,967,295).",
        ),
    ];
    // With undone ops after the cursor, and with the cursor inside an op.
    for at in [pos(5), pos(12)] {
        let mut session = table();
        session.seek(at);
        refused(
            &mut session,
            &Op::Config { config: bad_config },
            &OpError::Config(config_error.clone()),
        );
        for (op, message) in &issue_errors {
            let error = session.apply(op.clone()).unwrap_err();
            assert_eq!(error.to_string(), *message, "{at:?}: {op:?}");
            refused(&mut session, op, &error);
        }
        let error = session.apply(program("lw r1, [0x0A4]")).unwrap_err();
        assert!(
            matches!(error, OpError::Program { errors: 1, .. }),
            "{error:?}"
        );
        refused(&mut session, &program("lw r1, [0x0A4]"), &error);
        for n in [0, MAX_RUN_STEPS + 1] {
            refused(&mut session, &Op::Run { n }, &OpError::RunSteps);
        }
        // The redo still reaches the same end.
        while session.redo() {}
        assert_eq!(session.position().pos, pos(15), "{at:?}");
    }
    // Nothing to execute: no program at the base, a finished one, and a
    // stopped one.
    let mut session = table();
    session.seek(pos(8));
    for op in [Op::Step, Op::StepAccess, Op::Run { n: 1 }] {
        refused(&mut session, &op, &OpError::NoProgram);
    }
    let mut session = table();
    apply_all(&mut session, [load(0x0A4)]);
    assert!(session.undo());
    for op in [Op::Step, Op::StepAccess, Op::Run { n: 1 }] {
        refused(&mut session, &op, &OpError::Finished);
    }
    let mut session = self::session();
    apply_all(
        &mut session,
        [
            program("add r2, r0, #0xA6\nld r1, [r2]"),
            Op::Run { n: 2 },
            load(0x0A4),
        ],
    );
    assert!(session.undo());
    for op in [Op::Step, Op::StepAccess, Op::Run { n: 1 }] {
        refused(&mut session, &op, &OpError::Stopped { line: 2 });
    }
}

#[test]
fn a_run_executes_up_to_its_count_and_stops_early_when_the_program_does() {
    let mut session = session();
    apply_all(&mut session, [program(WALK), Op::Run { n: 4 }]);
    let entries: Vec<Option<u32>> = session.events().iter().map(|event| event.entry).collect();
    // A setup, two adds, a load that misses, and a store that hits.
    assert_eq!(entries.iter().filter(|entry| entry.is_none()).count(), 1);
    assert_eq!(entries.last(), Some(&Some(4)));
    assert_eq!(session.position().boundary, Boundary::LogEnd);
    // A run of more instructions than are left stops when the program
    // finishes, with no error.
    let mut session = self::session();
    apply_all(&mut session, [program("add r1, r0, #1\nadd r1, r1, #1")]);
    let events = session.apply(Op::Run { n: 3 }).unwrap();
    let kinds: Vec<EventKind> = events.iter().map(|event| event.kind).collect();
    assert_eq!(kinds, [EventKind::Complete, EventKind::Complete]);
    assert_eq!(session.apply(Op::Step), Err(OpError::Finished));
    // A step access over a trace executes one line.
    let mut session = self::session();
    let trace = Op::Program {
        source: "R 0x0A4\nW 0x0A4 7".to_owned(),
        format: ProgramFormat::Trace,
    };
    apply_all(&mut session, [trace, Op::StepAccess]);
    assert_eq!(
        session.events().last().map(|event| event.label.as_str()),
        Some("1.5")
    );
}

#[test]
fn a_configuration_equal_to_the_displayed_runs_changes_nothing() {
    let mut session = table();
    let before = session.clone();
    let same = Config {
        advanced: true,
        ..config()
    };
    assert_eq!(session.propose_config(same), Ok(ConfigChange::Applied));
    assert_eq!(session.ops(), before.ops());
    assert_eq!(session.position(), before.position());
}

#[test]
fn a_change_on_an_empty_run_at_the_end_of_the_log_is_appended_at_once() {
    let mut session = session();
    // Sizes of at most 16 lines, the floor `MAX_LINES` may be lowered to.
    let sizes = [256, 64, 16, 32, 128];
    for size in sizes {
        let config = Config {
            cache: CacheConfig {
                size_bytes: size,
                ..config().cache
            },
            ..config()
        };
        assert_eq!(
            session.propose_config(config),
            Ok(ConfigChange::Applied),
            "{size} B"
        );
        assert_eq!(session.config(), &config, "{size} B");
        assert!(session.position().at_run_origin, "{size} B");
    }
    // Five changes are five ops, five runs and five undo stops; none
    // replaces another, and the first run's configuration is unchanged.
    assert_eq!(session.op_count(), 5);
    assert!(session
        .ops()
        .iter()
        .all(|op| matches!(op, Op::Config { .. })));
    assert_eq!(session.position().run, 6);
    assert_eq!(session.initial(), &config());
    let expected: Vec<Op> = sizes
        .iter()
        .map(|&size| Op::Config {
            config: Config {
                cache: CacheConfig {
                    size_bytes: size,
                    ..config().cache
                },
                ..config()
            },
        })
        .collect();
    assert_eq!(session.ops(), expected);
    // Every change is in the share document, and a link opens them all.
    let doc = session.share_doc();
    assert_eq!(doc.config, config());
    assert_eq!(doc.ops, expected);
    let reopened = Session::from_share_string(&session.to_share_string().unwrap()).unwrap();
    assert_eq!(reopened.ops(), expected);
    assert_eq!(reopened.position(), session.position());
    let mut runs = Vec::new();
    while session.undo() {
        runs.push(session.position().run);
    }
    assert_eq!(runs, [5, 4, 3, 2, 1]);
    assert_eq!(session.config(), &config());
    // With the changes undone, the empty run has undone ops after it, so a
    // change asks first.
    let change = session.propose_config(Preset::TwoWayLru.config()).unwrap();
    assert!(
        matches!(change, ConfigChange::NeedsReset { .. }),
        "{change:?}"
    );
    assert_eq!(session.op_count(), 5);
}

#[test]
fn a_change_to_a_run_with_steps_asks_first_with_the_count() {
    let two_way = Preset::TwoWayLru.config();
    let mut session = table();
    let before = session.clone();
    assert_eq!(
        session.propose_config(two_way),
        Ok(ConfigChange::NeedsReset {
            config: two_way,
            message: "Configuration is fixed for a run. Applying starts a new, empty run; this \
                      one has 14 steps and stays reachable with Undo."
                .to_owned()
        })
    );
    assert_eq!(session.ops(), before.ops());
    assert_eq!(session.position(), before.position());
    // With undone steps after the cursor, it says what a new run drops.
    session.seek(pos(5));
    assert_eq!(
        session.propose_config(two_way),
        Ok(ConfigChange::NeedsReset {
            config: two_way,
            message: "Configuration is fixed for a run. Applying starts a new, empty run here \
                      and drops the 9 undone steps after this point; this run's 5 steps stay \
                      reachable with Undo."
                .to_owned()
        })
    );
    // Inside an op, the op is kept whole.
    session.seek(pos(6));
    let Ok(ConfigChange::NeedsReset { message, .. }) = session.propose_config(two_way) else {
        panic!("a change inside an op asks first");
    };
    assert!(
        message.ends_with(
            "drops the 6 undone steps after this point; this run's 8 steps stay \
                           reachable with Undo."
        ),
        "{message}"
    );
    // One step reads in the singular.
    let mut session = self::session();
    apply_all(&mut session, [program("add r1, r0, #1"), Op::Step]);
    let Ok(ConfigChange::NeedsReset { message, .. }) = session.propose_config(two_way) else {
        panic!("a run with a step asks first");
    };
    assert!(message.ends_with("this one has 1 step and stays reachable with Undo."));
    session.seek(pos(1));
    let Ok(ConfigChange::NeedsReset { message, .. }) = session.propose_config(two_way) else {
        panic!("a run with a step asks first");
    };
    assert!(message.ends_with(
        "drops the 1 undone step after this point; this run's 0 steps stay \
                               reachable with Undo."
    ));
    // An invalid configuration is refused.
    let invalid = Config {
        address_bits: 4,
        ..config()
    };
    assert_eq!(
        session.propose_config(invalid),
        Err(OpError::Config(ConfigError::AddressBits { got: 4 }))
    );
}

#[test]
fn applying_a_configuration_always_appends_a_new_run_that_undo_leaves() {
    let two_way = Preset::TwoWayLru.config();
    let mut session = table();
    let old = session.machine().clone();
    let events = session
        .apply(Op::Config { config: two_way })
        .unwrap()
        .to_vec();
    assert_eq!(session.op_count(), 5);
    assert_eq!(session.config(), &two_way);
    let [setup] = events.as_slice() else {
        panic!("a new run with no program is one event: {events:?}");
    };
    assert_eq!(setup.kind, EventKind::Setup);
    assert_eq!(
        (setup.run, setup.entry, setup.label.as_str()),
        (2, None, "")
    );
    assert!(setup.changed.is_empty());
    assert_eq!(
        setup.explain,
        "New run: 128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU. The cache is empty and \
         memory, registers and counters are reset. The program is kept and restarts at line 1."
    );
    let position = session.position();
    assert!(position.at_run_origin);
    assert_eq!(position.run, 2);
    // Undo returns to the old run, whose configuration the session shows.
    assert!(session.undo());
    assert_eq!(session.config(), &config());
    assert_eq!(session.machine(), &old);
    assert!(session.redo());
    assert_eq!(session.config(), &two_way);
    // Reset is the displayed run's configuration applied again: a new, empty
    // run, undoable, that keeps the program.
    let current = *session.config();
    session.apply(Op::Config { config: current }).unwrap();
    assert_eq!(session.position().run, 3);
    assert_eq!(session.config(), &current);
    assert_eq!(session.op_count(), 6);
    assert!(session.undo());
    assert_eq!(session.position().run, 2);
}

#[test]
fn a_new_run_restarts_the_program_it_keeps_or_unloads_one_that_no_longer_parses() {
    let mut session = session();
    apply_all(
        &mut session,
        [
            program("; first\nld r1, [0x0A4]\nld r2, [0x0A8]"),
            Op::Step,
            Op::Config {
                config: Preset::TwoWayLru.config(),
            },
        ],
    );
    assert!(session
        .events()
        .last()
        .is_some_and(|event| event.explain.ends_with("restarts at line 2.")));
    // It restarts at its first instruction, which misses on the new run's
    // empty cache.
    let events = session.apply(Op::Step).unwrap();
    assert_eq!(
        events.first().map(|event| event.label.as_str()),
        Some("1.1")
    );
    let kinds: Vec<EventKind> = events.iter().map(|event| event.kind).collect();
    assert_eq!(
        kinds,
        [
            EventKind::Decode,
            EventKind::Miss,
            EventKind::Victim,
            EventKind::Fill,
            EventKind::Complete
        ]
    );
    // Under 8-byte words its address is no longer aligned: it is unloaded,
    // with an error after the new run's setup.
    let wide = Config {
        word_bytes: WordBytes::Eight,
        ..config()
    };
    let events = session.apply(Op::Config { config: wide }).unwrap();
    let kinds: Vec<EventKind> = events.iter().map(|event| event.kind).collect();
    assert_eq!(kinds, [EventKind::Setup, EventKind::Error]);
    assert!(events
        .first()
        .is_some_and(|event| event.explain.ends_with("counters are reset.")));
    let error = events.last().unwrap();
    assert_eq!((error.entry, error.label.as_str()), (Some(1), "1.1"));
    assert_eq!(
        error.explain,
        "The program no longer fits this configuration (line 2: 0x0A4 is not aligned to an \
         8-byte word; use 0x0A0 or 0x0A8); it was unloaded."
    );
    assert_eq!(session.apply(Op::Step), Err(OpError::NoProgram));
    // The new run's origin is its setup, not the error after it.
    session.seek(pos(session.position().pos.get() - 1));
    assert!(session.position().at_run_origin);
    assert_eq!(session.position().boundary, Boundary::EntryEnd);
    // A trace the new run keeps is named as a trace.
    let mut session = self::session();
    let trace = Op::Program {
        source: "R 0x0A4\nR 0x0A8".to_owned(),
        format: ProgramFormat::Trace,
    };
    let two_way = Preset::TwoWayLru.config();
    apply_all(&mut session, [trace, Op::Config { config: two_way }]);
    assert_eq!(
        session.events().last().map(|event| event.explain.as_str()),
        Some(
            "New run: 128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU. The cache is empty and \
             memory, registers and counters are reset. The trace is kept and restarts at line 1."
        )
    );
}

#[test]
fn a_configuration_needing_advanced_names_its_policies_in_the_new_runs_line() {
    let mut session = session();
    let through = Config {
        cache: CacheConfig {
            write_policy: WritePolicy::WriteThrough,
            replacement: Replacement::Fifo,
            associativity: 2,
            ..config().cache
        },
        ..config()
    };
    let events = session.apply(Op::Config { config: through }).unwrap();
    assert_eq!(
        events.first().map(|event| event.explain.as_str()),
        Some(
            "New run: 128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} FIFO \u{b7} write-through \
             \u{b7} allocate. The cache is empty and memory, registers and counters are reset."
        )
    );
}

#[test]
fn switching_mode_is_not_an_op_and_basics_asks_first_while_the_run_needs_advanced() {
    let through = Config {
        cache: CacheConfig {
            write_policy: WritePolicy::WriteThrough,
            ..config().cache
        },
        ..config()
    };
    // The mode starts as the initial configuration names it.
    assert!(!session().advanced());
    let chosen = Config {
        advanced: true,
        ..config()
    };
    assert!(Session::with_config(chosen).unwrap().advanced());
    // On a run that fits Basics, either switch changes the mode and nothing
    // else.
    let mut session = table();
    let before = session.clone();
    assert_eq!(session.set_advanced(true), ConfigChange::Applied);
    assert!(session.advanced());
    assert_eq!(session.set_advanced(false), ConfigChange::Applied);
    assert!(!session.advanced());
    assert_eq!(session.set_advanced(true), ConfigChange::Applied);
    assert_eq!(session.ops(), before.ops());
    assert_eq!(session.position(), before.position());
    assert_eq!(session.machine(), before.machine());
    // On a run that needs Advanced, switching to Basics changes nothing and
    // asks first, offering the configuration's Basics form.
    apply_all(&mut session, [Op::Config { config: through }, load(0x0A4)]);
    let before = session.clone();
    assert_eq!(
        session.set_advanced(false),
        ConfigChange::NeedsReset {
            config: through.to_basics(),
            message: "Configuration is fixed for a run. Applying starts a new, empty run; this \
                      one has 5 steps and stays reachable with Undo."
                .to_owned()
        }
    );
    assert!(session.advanced());
    assert_eq!(session.ops(), before.ops());
    assert_eq!(session.position(), before.position());
    // "Reset and apply": the Basics run is appended, and the mode is still
    // Advanced until the switch is made again.
    apply_all(
        &mut session,
        [Op::Config {
            config: through.to_basics(),
        }],
    );
    assert!(session.advanced());
    assert_eq!(session.set_advanced(false), ConfigChange::Applied);
    assert!(!session.advanced());
    // Undoing back into the write-through run shows Advanced again, and
    // switching to Basics there asks first.
    assert!(session.undo());
    assert_eq!(session.config(), &through);
    assert!(session.advanced());
    assert!(matches!(
        session.set_advanced(false),
        ConfigChange::NeedsReset { .. }
    ));
    assert!(session.redo());
    assert!(!session.advanced());
}

#[test]
fn every_state_the_cursor_reaches_is_the_folds() {
    let mut session = varied();
    let initial = *session.initial();
    let ops = session.ops().to_vec();
    let replay = Session::replay(&initial, &ops).unwrap();
    assert_eq!(log(&session), replay.events);
    assert_eq!(session.machine(), &replay.machine);
    assert_eq!(
        Session::replay(&initial, &ops),
        Ok(replay.clone()),
        "the fold is pure"
    );
    // Every position, by seeking.
    let end = log_end(&session).get();
    for at in (0..=end).rev() {
        session.seek(pos(at));
        assert_eq!(
            Ok(session.machine()),
            Session::replay_to(&initial, &ops, pos(at)).as_ref(),
            "at {at}"
        );
    }
    // Scrubbing to k ops done is the fold of the first k ops.
    for done in 0..=session.op_count() {
        session.scrub_to(done);
        let prefix = &ops[..done as usize];
        assert_eq!(
            session.machine(),
            &Session::replay(&initial, prefix).unwrap().machine,
            "{done} ops done"
        );
        assert_eq!(session.cursor(), done, "{done} ops done");
    }
    // Undo from the end visits the stops in reverse, and redo restores them.
    session.scrub_to(session.op_count());
    let mut stops = vec![(session.position().pos, session.machine().clone())];
    while session.undo() {
        stops.push((session.position().pos, session.machine().clone()));
    }
    assert_eq!(session.position().pos, pos(0));
    for (at, machine) in stops.iter().rev().skip(1) {
        assert!(session.redo(), "to {at:?}");
        assert_eq!(session.position().pos, *at, "to {at:?}");
        assert_eq!(session.machine(), machine, "to {at:?}");
    }
    assert!(!session.redo());
    // Replaying past the end gives the last state.
    assert_eq!(
        Session::replay_to(&initial, &ops, pos(end + 10)).as_ref(),
        Ok(&replay.machine)
    );
}

#[test]
fn event_ids_and_labels_follow_the_log() {
    let session = varied();
    let events = log(&session);
    let mut previous: Option<&MicroEvent> = None;
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.id, index as u64, "event {index}");
        if let Some(entry) = event.entry {
            assert_eq!(
                event.label,
                format!("{entry}.{}", event.seq),
                "event {index}"
            );
        } else {
            assert_eq!(event.kind, EventKind::Setup, "event {index}");
            assert_eq!(event.label, "", "event {index}");
        }
        if let Some(previous) = previous {
            assert!(event.op_index >= previous.op_index, "event {index}");
            assert!(event.run >= previous.run, "event {index}");
            // An event either continues its entry or starts the next one.
            if event.seq > 1 {
                assert_eq!(event.seq, previous.seq + 1, "event {index}");
                assert_eq!(event.entry, previous.entry, "event {index}");
                assert_eq!(event.op_index, previous.op_index, "event {index}");
            }
            if event.run > previous.run {
                assert_eq!(
                    (event.kind, event.entry),
                    (EventKind::Setup, None),
                    "event {index}"
                );
            }
        }
        previous = Some(event);
    }
}

#[test]
fn a_seek_across_checkpoints_reaches_the_folds_state() {
    // A log several checkpoint intervals long, of long ops: runs of a loop
    // that loads and stores, and a step access that uses a whole budget.
    let mut session = session();
    let target = (3 * CHECKPOINT_INTERVAL).min(SESSION_EVENT_LIMIT / 2);
    while session.position().pos.get() < target {
        if session.apply(Op::Run { n: MAX_RUN_STEPS }).is_err() {
            apply_all(&mut session, [program(WALK)]);
        }
    }
    apply_all(&mut session, [program(ENDLESS), Op::StepAccess]);
    let initial = *session.initial();
    let ops = session.ops().to_vec();
    let end = log_end(&session).get();
    let mut positions: Vec<u32> = (0..=end).step_by(251).collect();
    for multiple in (CHECKPOINT_INTERVAL..=end).step_by(CHECKPOINT_INTERVAL as usize) {
        positions.extend(multiple.saturating_sub(3)..=(multiple + 3).min(end));
    }
    positions.push(end);
    for at in positions {
        session.seek(pos(at));
        assert_eq!(
            Ok(session.machine()),
            Session::replay_to(&initial, &ops, pos(at)).as_ref(),
            "at {at}"
        );
    }
    // Ops appended after a seek back build on the right state too.
    session.seek(pos(end / 2));
    session.apply(load(0x0A4)).unwrap();
    let ops = session.ops().to_vec();
    assert_eq!(
        session.machine(),
        &Session::replay(&initial, &ops).unwrap().machine
    );
    assert_eq!(
        log(&session),
        Session::replay(&initial, &ops).unwrap().events
    );
}

#[test]
fn a_refused_edit_leaves_the_log_the_undone_ops_and_the_cursor_as_they_were() {
    let edits = [
        (
            edit(
                &serde_json::json!({"kind": "memory", "addr": 0x0A6}),
                &serde_json::json!({"kind": "memory", "value": 1}),
            ),
            "Refused. 0x0A6 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8.",
        ),
        (
            edit(
                &serde_json::json!({"kind": "way", "set": 9, "way": 0}),
                &serde_json::json!({"kind": "way", "valid": false}),
            ),
            "Refused. This cache has no set 9; its sets are 0\u{2013}7.",
        ),
    ];
    // With undone ops after the cursor, and with the cursor inside an op:
    // an edit is planned on the machine at the base, as every op is checked
    // there, wherever the cursor is.
    for at in [pos(5), pos(12)] {
        let mut session = table();
        session.seek(at);
        for (op, message) in &edits {
            let error = session.apply(op.clone()).unwrap_err();
            assert!(matches!(error, OpError::Edit(_)), "{error:?}");
            assert_eq!(error.to_string(), *message, "{at:?}: {op:?}");
            refused(&mut session, op, &error);
        }
        while session.redo() {}
        assert_eq!(session.position().pos, pos(15), "{at:?}");
    }
}

#[test]
fn a_full_session_refuses_every_op_until_undo_makes_room() {
    let mut session = session();
    // Each pair loads a loop without an exit and steps it to its budget: the
    // program load, the budget's instructions and the budget's error. The
    // last pair must find room for its program load, so a load by hand
    // shifts the pairs off the one alignment where it would not.
    let pair = INSTRUCTION_BUDGET + 2;
    if (SESSION_EVENT_LIMIT - MAX_EVENTS_PER_ENTRY + 1) % pair == 0 {
        apply_all(&mut session, [load(0x0A4)]);
    }
    let mut stopped = false;
    for _ in 0..=SESSION_EVENT_LIMIT / pair {
        apply_all(&mut session, [program(ENDLESS), Op::StepAccess]);
        let last = session.events().last().unwrap();
        if last.explain.starts_with("The session reached its limit") {
            stopped = true;
            break;
        }
    }
    assert!(stopped, "the last step access runs out of room");
    let full = session.position().pos.get();
    assert!(full <= SESSION_EVENT_LIMIT);
    assert!(full + MAX_EVENTS_PER_ENTRY > SESSION_EVENT_LIMIT);
    // Every op is refused, and Space and Shift+Space say why.
    for op in [
        load(0x0A4),
        program(ENDLESS),
        Op::Config { config: config() },
        Op::Run { n: 1 },
        edit_word(),
    ] {
        refused(&mut session, &op, &OpError::SessionFull);
    }
    let message = format!(
        "This session has reached its limit of {} steps. Undo to make room, or save it and start \
         fresh.",
        grouped(SESSION_EVENT_LIMIT)
    );
    // So do the editors, before any rule of their own, rather than plan an
    // edit that would then be refused, and so does the configuration bar,
    // rather than ask about a new run that would then be refused.
    editors_refuse_for_room(&session, RefusalReason::SessionFull, &message);
    change_refused(&mut session, &OpError::SessionFull);
    assert_eq!(
        session.step_event(Some(&drawer("add r1, r0, #1"))),
        StepOutcome::Idle {
            message: message.clone()
        }
    );
    assert_eq!(
        session.step_access(None),
        StepOutcome::Idle {
            message: message.clone()
        }
    );
    // Inside the last op, a new op would go at its end, where there is no
    // room either; the events after the cursor stay.
    session.seek(pos(full - 1));
    refused(&mut session, &load(0x0A4), &OpError::SessionFull);
    assert_eq!(session.position().pos, pos(full - 1));
    // The editors say so there too, not that the op should be finished
    // first, which would not make room.
    editors_refuse_for_room(&session, RefusalReason::SessionFull, &message);
    session.seek(pos(full));
    // The fold refuses the same op at the same place.
    let initial = *session.initial();
    let mut ops = session.ops().to_vec();
    assert!(Session::replay(&initial, &ops).is_ok());
    ops.push(load(0x0A4));
    let last = u32::try_from(ops.len() - 1).unwrap();
    let error = Session::replay(&initial, &ops).unwrap_err();
    let ReplayError::Op {
        index,
        error: refusal,
    } = &error
    else {
        panic!("the op is refused: {error:?}");
    };
    assert_eq!((index.get(), refusal), (last, &OpError::SessionFull));
    assert_eq!(error.to_string(), message);
    // Undo makes room: an edit plans there, a configuration change asks
    // first, and an op applied there drops what came after.
    assert!(session.undo());
    assert!(session.position().pos.get() < full);
    assert!(matches!(
        session.draft_memory_edit(addr(0x0A4), "1").outcome,
        EditOutcome::Plan(_)
    ));
    assert!(matches!(
        session.propose_config(Preset::TwoWayLru.config()),
        Ok(ConfigChange::NeedsReset { .. })
    ));
    session.apply(load(0x0A4)).unwrap();
    assert!(!session.redo());
}

#[test]
fn a_log_as_long_as_a_link_can_hold_refuses_every_op_until_undo_makes_room() {
    let mut session = as_long_as_a_link();
    assert_eq!(session.op_count(), MAX_SHARE_OPS);
    // The events leave room for more, so it is the link's limit that refuses.
    let end = session.position().pos.get();
    assert!(end + MAX_EVENTS_PER_ENTRY <= SESSION_EVENT_LIMIT);
    // Every op is refused, Reset's `Config` op included, and Space and
    // Shift+Space say why.
    for op in [
        load(0x0A4),
        program(ENDLESS),
        Op::Config { config: config() },
        Op::Run { n: 1 },
        edit_word(),
    ] {
        refused(&mut session, &op, &OpError::LinkFull);
    }
    let message = format!(
        "A link can hold at most {} actions, and this would pass that. Undo to make room, or save \
         the run and start fresh.",
        grouped(MAX_SHARE_OPS)
    );
    assert_eq!(OpError::LinkFull.to_string(), message);
    // So do the editors, before any rule of their own, and the
    // configuration bar, rather than ask about a new run.
    editors_refuse_for_room(&session, RefusalReason::LinkFull, &message);
    change_refused(&mut session, &OpError::LinkFull);
    assert_eq!(
        session.step_event(Some(&drawer("add r1, r0, #1"))),
        StepOutcome::Idle {
            message: message.clone()
        }
    );
    assert_eq!(
        session.step_access(None),
        StepOutcome::Idle {
            message: message.clone()
        }
    );
    // Inside the last op, a new op would go at its end, after as many ops
    // as a link can hold; the editors say so there too, not that the op
    // should be finished first, which would not make room.
    session.seek(pos(end - 1));
    refused(&mut session, &load(0x0A4), &OpError::LinkFull);
    editors_refuse_for_room(&session, RefusalReason::LinkFull, &message);
    session.seek(pos(end));
    // The fold refuses the same op at the same place.
    let initial = *session.initial();
    let mut ops = session.ops().to_vec();
    assert!(Session::replay(&initial, &ops).is_ok());
    ops.push(load(0x0A4));
    let error = Session::replay(&initial, &ops).unwrap_err();
    let ReplayError::Op {
        index,
        error: refusal,
    } = &error
    else {
        panic!("the op is refused: {error:?}");
    };
    assert_eq!((index.get(), refusal), (MAX_SHARE_OPS, &OpError::LinkFull));
    // Undo makes room: an edit plans there, a configuration change asks
    // first, and an op applied there drops what came after, so the log
    // stays as long as a link can hold.
    assert!(session.undo());
    assert!(matches!(
        session.draft_memory_edit(addr(0x0A4), "1").outcome,
        EditOutcome::Plan(_)
    ));
    assert!(matches!(
        session.propose_config(Preset::TwoWayLru.config()),
        Ok(ConfigChange::NeedsReset { .. })
    ));
    session.apply(load(0x0A4)).unwrap();
    assert_eq!(session.op_count(), MAX_SHARE_OPS);
    assert!(!session.redo());
}

#[test]
fn a_press_that_would_pass_the_link_limit_is_refused_whole() {
    // One op short of the limit, Space and Shift+Space with a new program in
    // the drawer would append two ops, its load and then the step, and the
    // second would pass the limit: neither is appended, and they say why
    // with the link limit's message.
    let mut session = hand_loads(MAX_SHARE_OPS - 1);
    let before = session.clone();
    let sample = drawer(ADD_THEN_LOAD);
    let idle = StepOutcome::Idle {
        message: OpError::LinkFull.to_string(),
    };
    assert_eq!(session.step_event(Some(&sample)), idle);
    assert_eq!(session.step_access(Some(&sample)), idle);
    assert_eq!(session.ops(), before.ops());
    assert_eq!(session.position(), before.position());
    // The load alone fits; then the step does not.
    apply_all(&mut session, [program(ADD_THEN_LOAD)]);
    assert_eq!(session.op_count(), MAX_SHARE_OPS);
    assert_eq!(session.step_event(Some(&sample)), idle);
    assert_eq!(session.step_access(Some(&sample)), idle);
    assert_eq!(session.op_count(), MAX_SHARE_OPS);
}

#[test]
fn a_log_as_large_as_a_file_can_hold_refuses_every_op_until_undo_makes_room() {
    let mut session = as_large_as_a_file();
    let end = session.position().pos.get();
    // The file is within its limit wherever the cursor is and in either
    // mode, at the size the session keeps without writing it.
    for advanced in [true, false] {
        assert_eq!(
            session.set_advanced(advanced),
            ConfigChange::Applied,
            "advanced {advanced}"
        );
        for at in [0, 1, end / 2, end] {
            session.seek(pos(at));
            let json = session.to_json();
            assert_eq!(session.to_json_len() as usize, json.len(), "at {at}");
            assert!(json.len() <= MAX_SHARE_JSON_BYTES as usize, "at {at}");
        }
    }
    // The events and the ops leave room for more, so it is the file's
    // limit that refuses, its room used to the byte.
    assert!(session.op_count() < MAX_SHARE_OPS);
    assert!(end + MAX_EVENTS_PER_ENTRY <= SESSION_EVENT_LIMIT);
    // Every op is refused, the shortest and Reset's `Config` op included,
    // and Space and Shift+Space say why.
    for op in [
        Op::Step,
        Op::StepAccess,
        Op::Run { n: 1 },
        load(0x0A4),
        padded(0),
        Op::Config { config: config() },
        edit_word(),
    ] {
        refused(&mut session, &op, &OpError::FileFull);
    }
    let message = format!(
        "A saved file can hold at most {} bytes, and this would pass that. Undo to make room, or \
         save the run and start fresh.",
        grouped(MAX_SHARE_JSON_BYTES)
    );
    assert_eq!(OpError::FileFull.to_string(), message);
    // So do the editors, before any rule of their own, and the
    // configuration bar, rather than ask about a new run.
    editors_refuse_for_room(&session, RefusalReason::FileFull, &message);
    change_refused(&mut session, &OpError::FileFull);
    let idle = StepOutcome::Idle {
        message: message.clone(),
    };
    assert_eq!(session.step_event(None), idle);
    assert_eq!(session.step_event(Some(&drawer(ADD_THEN_LOAD))), idle);
    assert_eq!(session.step_access(None), idle);
    // The fold refuses the same op at the same place, as far as it goes.
    let initial = *session.initial();
    let mut ops = session.ops().to_vec();
    assert!(Session::replay(&initial, &ops).is_ok());
    ops.push(Op::Step);
    let last = u32::try_from(ops.len() - 1).unwrap();
    let error = Session::replay(&initial, &ops).unwrap_err();
    let ReplayError::Op {
        index,
        error: refusal,
    } = &error
    else {
        panic!("the op is refused: {error:?}");
    };
    assert_eq!((index.get(), refusal), (last, &OpError::FileFull));
    assert!(Session::replay_to(&initial, &ops, pos(end)).is_ok());
    assert_eq!(
        Session::replay_to(&initial, &ops, pos(end + 1)).unwrap_err(),
        error
    );
    // So does a load of a document with that op, which no session writes,
    // though its JSON is within the limit without the cursor's place.
    let mut doc = session.share_doc();
    doc.ops.push(Op::Step);
    doc.at = None;
    let json = doc.to_json();
    assert!(json.len() <= MAX_SHARE_JSON_BYTES as usize);
    assert_eq!(
        Session::from_json(&json).unwrap_err(),
        LoadError::Op {
            index: last,
            error: OpError::FileFull,
            not_replayed: 0,
        }
    );
    // Moving back makes room for as much as the ops passed: one op back, a
    // load replaces the last program.
    let count = session.op_count();
    let mut back = session.clone();
    back.scrub_to(count - 1);
    back.apply(load(0x0A4)).unwrap();
    assert_eq!(back.op_count(), count);
    assert!(back.to_json_len() < session.to_json_len());
    // Undo makes room: an edit plans there, a configuration change asks
    // first, and an op applied there drops what came after.
    assert!(session.undo());
    assert!(matches!(
        session.draft_memory_edit(addr(0x0A4), "1").outcome,
        EditOutcome::Plan(_)
    ));
    assert!(matches!(
        session.propose_config(Preset::TwoWayLru.config()),
        Ok(ConfigChange::NeedsReset { .. })
    ));
    session.apply(load(0x0A4)).unwrap();
    assert!(session.op_count() < count);
    assert!(!session.redo());
}

/// A call on a session, as the page makes it.
type Call<'a> = Box<dyn Fn(&mut Session) + 'a>;

#[test]
fn no_call_rewrites_an_op_the_log_keeps() {
    let mut session = session();
    let two_way = Preset::TwoWayLru.config();
    let small = Config {
        cache: CacheConfig {
            size_bytes: 64,
            ..two_way.cache
        },
        ..two_way
    };
    let sample = drawer(ADD_THEN_LOAD);
    let calls: Vec<Call<'_>> = vec![
        Box::new(|session| apply_all(session, [load(0x0A4)])),
        Box::new(|session| {
            assert!(matches!(
                session.propose_config(two_way),
                Ok(ConfigChange::NeedsReset { .. })
            ));
        }),
        Box::new(|session| {
            apply_all(session, [Op::Config { config: two_way }]);
        }),
        Box::new(|session| {
            assert_eq!(session.propose_config(small), Ok(ConfigChange::Applied));
        }),
        Box::new(|session| {
            assert_eq!(session.set_advanced(true), ConfigChange::Applied);
            assert_eq!(session.set_advanced(false), ConfigChange::Applied);
        }),
        Box::new(|session| {
            assert_eq!(session.step_event(Some(&sample)), StepOutcome::Applied);
        }),
        Box::new(|session| {
            assert_eq!(session.step_event(Some(&sample)), StepOutcome::Applied);
        }),
        Box::new(|session| {
            session.undo();
        }),
        Box::new(|session| {
            assert_eq!(session.step_access(Some(&sample)), StepOutcome::Moved);
        }),
        Box::new(|session| session.seek(pos(2))),
        Box::new(|session| apply_all(session, [store(0x0A4, 3)])),
        Box::new(|session| {
            session.undo();
            session.undo();
        }),
        Box::new(|session| {
            let current = *session.config();
            apply_all(session, [Op::Config { config: current }]);
        }),
    ];
    for (index, call) in calls.iter().enumerate() {
        let before = session.ops().to_vec();
        let position = session.position();
        // A new op goes at the end of the op holding the cursor.
        let kept = match position.boundary {
            Boundary::MidEntry | Boundary::EntryEnd => position.op.map_or(0, |op| op.get() + 1),
            Boundary::OpEnd | Boundary::LogEnd => position.ops_done,
        } as usize;
        call(&mut session);
        assert_eq!(
            session.ops().get(..kept),
            before.get(..kept),
            "call {index}"
        );
        assert_eq!(session.initial(), &config(), "call {index}");
    }
    // Undone back to position 0, a Reset starts a new run of the first
    // run's configuration there, which drops everything after it.
    assert_eq!(session.ops(), [Op::Config { config: config() }]);
    assert_eq!(session.position().run, 2);
}

/// A module that runs a three-line trace, moves the cursor back to access
/// 1's tag compare, and asks a prediction there, so the prediction begins
/// in the middle of the log.
const PREDICT_MID_LOG: &str = r#"{
  "id": "90-gate-mid-log",
  "title": "Gate",
  "description": "A prediction in the middle of the log.",
  "advanced": false,
  "preset": {
    "addressBits": 12, "wordBytes": 4,
    "cache": { "sizeBytes": 128, "lineBytes": 16, "associativity": 1, "replacement": "lru",
               "writePolicy": "writeBack", "allocatePolicy": "writeAllocate" },
    "seed": 12345, "advanced": false, "memoryInit": "pattern"
  },
  "program": { "source": "R 0x0A4\nR 0x0A8\nR 0x1A4", "format": "trace" },
  "steps": [
    { "kind": "instruct", "text": "Run the trace.",
      "expect": { "counter": "accesses", "op": "==", "value": 3 },
      "showMe": [{ "op": "run", "n": 3 }] },
    { "kind": "observe", "text": "Go back to the first miss.",
      "until": { "event": { "kind": "miss", "entry": 1 } } },
    { "kind": "predict", "text": "Does the second load hit?", "options": ["Hit", "Miss"],
      "answer": 0, "explain": "It is in the same line.",
      "then": [{ "op": "issue", "kind": "read", "addr": "0x0A8" }] }
  ]
}"#;

/// A module whose first step is a prediction, which begins at the end of
/// the log.
const PREDICT_AT_END: &str = r#"{
  "id": "91-gate-at-end",
  "title": "Gate",
  "description": "A prediction at the end of the log.",
  "advanced": false,
  "preset": {
    "addressBits": 12, "wordBytes": 4,
    "cache": { "sizeBytes": 128, "lineBytes": 16, "associativity": 1, "replacement": "lru",
               "writePolicy": "writeBack", "allocatePolicy": "writeAllocate" },
    "seed": 12345, "advanced": false, "memoryInit": "pattern"
  },
  "steps": [
    { "kind": "predict", "text": "Will `0x0A4` miss?", "options": ["Yes", "No"],
      "answer": 0, "explain": "The cache is empty." }
  ]
}"#;

/// The guard's message.
const GUARD: &str = "Answer the prediction in the module panel first.";

#[test]
fn the_gate_refuses_every_op() {
    let (_run, mut session) = gated_mid_log();
    assert_eq!(OpError::Gated.to_string(), GUARD);
    assert_eq!(session.gate().unwrap().message(), GUARD);
    // Issue, an edit, Reset and "Reset and apply", a trace through Load,
    // a step: each refused, and nothing changes.
    let reset = Op::Config {
        config: *session.config(),
    };
    let trace = Op::Program {
        source: "R 0x0A4".to_owned(),
        format: ProgramFormat::Trace,
    };
    for op in [
        load(0x0A4),
        store(0x0A4, 3),
        edit_word(),
        reset,
        trace,
        Op::Step,
    ] {
        refused(&mut session, &op, &OpError::Gated);
    }
    let two_way = Config {
        cache: CacheConfig {
            associativity: 2,
            ..config().cache
        },
        ..config()
    };
    assert_eq!(session.propose_config(two_way), Err(OpError::Gated));
    // Even a configuration equal to the displayed run's.
    let same = *session.config();
    assert_eq!(session.propose_config(same), Err(OpError::Gated));
    // Space and Shift+Space at the end of the log append nothing, the
    // drawer's program included.
    let (_run, mut session) = gated_at_end();
    let ops = session.op_count();
    let idle = StepOutcome::Idle {
        message: GUARD.to_owned(),
    };
    assert_eq!(session.step_event(None), idle);
    assert_eq!(session.step_event(Some(&drawer("ld r1, [0x0A4]"))), idle);
    assert_eq!(session.step_access(None), idle);
    assert_eq!(session.step_access(Some(&drawer("ld r1, [0x0A4]"))), idle);
    assert_eq!(session.op_count(), ops);
    assert_eq!(session.position().boundary, Boundary::LogEnd);
}

#[test]
fn plans_and_drafts_refuse_under_the_gate_as_apply_does() {
    // Mid-op: without the gate this would be an "unfinished" refusal, but
    // apply refuses with the gate first, so the plan and the drafts must
    // say the same, not name an op that was never in question.
    let (_run, session) = gated_mid_log();
    assert_ne!(session.position().boundary, Boundary::OpEnd);
    editors_refuse_for_room(&session, RefusalReason::Gated, GUARD);

    // At an op end within the limit, where the rules on the machine would
    // otherwise plan an edit that apply then refuses with the gate: a plan
    // shown must be a plan apply honours.
    let (_run, mut at_an_op_end) = gated_mid_log();
    while at_an_op_end.undo() {}
    assert_eq!(at_an_op_end.position().pos, pos(0));
    assert_eq!(at_an_op_end.position().boundary, Boundary::OpEnd);
    editors_refuse_for_room(&at_an_op_end, RefusalReason::Gated, GUARD);

    // At the very end of the log, also an op end.
    let (_run, session) = gated_at_end();
    editors_refuse_for_room(&session, RefusalReason::Gated, GUARD);
}

#[test]
fn the_gate_holds_the_cursor_at_or_before_its_limit() {
    let (_run, mut session) = gated_mid_log();
    let limit = session.gate().unwrap().seek_limit();
    let end = end_of(&session);
    assert!(limit < end);
    let idle = StepOutcome::Idle {
        message: GUARD.to_owned(),
    };
    // Space and Shift+Space inside the log refuse a move past the limit.
    assert_eq!(session.step_event(None), idle);
    assert_eq!(session.step_access(None), idle);
    assert_eq!(session.position().pos, limit);
    // A seek and a scrub stop at the limit.
    session.seek(end);
    assert_eq!(session.position().pos, limit);
    session.scrub_to(session.op_count());
    assert_eq!(session.position().pos, limit);
    // Moves at or before the limit are allowed: a seek back, and Space up
    // to the limit.
    session.seek(pos(limit.get() - 1));
    assert_eq!(session.step_event(None), StepOutcome::Moved);
    assert_eq!(session.position().pos, limit);
    // Undo is allowed; Redo is refused when its stop, the end of the Run
    // op, lies past the limit.
    assert!(session.undo());
    let undone = session.position().pos;
    assert!(undone < limit);
    assert!(!session.redo());
    assert_eq!(session.position().pos, undone);
    // Shift+Space goes to the end of the program load, before the limit,
    // then refuses the end of the Run op, past it.
    assert_eq!(session.step_access(None), StepOutcome::Moved);
    let loaded = session.position().pos;
    assert!(undone < loaded && loaded < limit);
    assert_eq!(session.step_access(None), idle);
    assert_eq!(session.position().pos, loaded);
    // The log is as it was.
    assert_eq!(end_of(&session), end);
}

#[test]
fn the_controls_show_the_gate() {
    let (mut run, mut session) = gated_mid_log();
    let limit = session.gate().unwrap().seek_limit();
    let controls = session.state().controls;
    assert_eq!(controls.enabled, [ControlAction::Undo]);
    assert_eq!(controls.seek_limit, Some(limit));
    assert_eq!(controls.guard.as_deref(), Some(GUARD));
    assert_eq!(
        serde_json::to_value(&controls).unwrap(),
        serde_json::json!({"enabled": ["undo"], "seekLimit": limit.get(), "guard": GUARD})
    );
    // At position 0, Redo to the end of the preset's op, before the limit,
    // is live, and Undo is not.
    session.seek(pos(0));
    let controls = session.state().controls;
    assert_eq!(controls.enabled, [ControlAction::Redo]);
    // Once the prediction is answered, the gate goes and the `then` op is
    // appended at the cursor.
    session.seek(limit);
    run.select(0);
    run.check(&mut session);
    assert!(session.gate().is_none());
    let controls = session.state().controls;
    assert!(controls.seek_limit.is_none() && controls.guard.is_none());
    assert!(controls.enabled.contains(&ControlAction::Issue));
    assert!(controls.enabled.contains(&ControlAction::Load));
    assert!(session.eval_predicate(
        &serde_json::from_value(serde_json::json!({"lastAccess": {"addr": 0x0A8}})).unwrap()
    ));
}
