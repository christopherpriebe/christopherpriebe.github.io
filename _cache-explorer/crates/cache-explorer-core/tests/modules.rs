//! Guided modules, on the fixture modules in `tests/fixtures/modules/`,
//! which between them have every step kind, every kind of predicate, a
//! configuration change in the middle of a module and an observe step that
//! waits for a micro-event. Each parses, serializes back, and completes end
//! to end through its "Show me" paths. Every rule of `Module::parse` has its
//! error, a predicate checked against the configuration in force after a
//! `config` op included; a predicate nests at most `MAX_PREDICATE_DEPTH`
//! deep, a limit module JSON never meets before `serde_json`'s; the
//! predicates mean what they say, `event`, `lastAccess` across a truncation
//! and a new run, and `config` included, and a search for `lastAccess` along
//! a full session that finds the load or finds nothing;
//! "Show me" on a prediction comes only after a wrong answer, and a right
//! or revealed answer applies the `then` ops where the prediction began;
//! ops the state refuses leave the step waiting; replacing the session ends
//! the module, its gate going with the old session; the module index is
//! read; and the panel's view says where the run is.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::limits::{
    MAX_EVENTS_PER_ENTRY, MAX_PREDICATE_DEPTH, MAX_SHARE_OPS, SESSION_EVENT_LIMIT,
};
use cache_explorer_core::{
    parse_module_index, Boundary, Callout, CalloutArea, CellRef, Config, EventKind, FeedbackTone,
    Module, ModuleActionKind, ModuleError, ModuleRun, ModuleStepKind, Op, OpError, OptionState,
    Predicate, Preset, ProgramFormat, Replacement, Session, Step, StepOutcome, TextSegment,
    TextStyle,
};
use serde_json::{json, Value};

use common::{load, pos};
use support::{
    actions, check_show_me_completion, edited, fill, predicate, refused_with, session,
    shape_refused, show_me_path, started, text, walk, with,
};

#[cfg(test)]
mod common;

/// Flow A's module: explain, instruct, an observe step waiting for the tag
/// compare of access 1, a prediction, instruct, explain.
const MISS_THEN_HIT: &str = include_str!("fixtures/modules/01-fixture-miss-then-hit.json");

/// A trace on `direct-mapped-8`, a prediction whose `then` ops switch to two
/// ways and run the trace again, then a "Show me" that switches to FIFO.
const SWITCH_CONFIG: &str = include_str!("fixtures/modules/02-fixture-switch-config.json");

/// An advanced, timed module: a store, an edit of the stale word, a
/// prediction whose `then` ops evict the line, an observe step waiting for
/// the write-back.
const STALE_MEMORY: &str = include_str!("fixtures/modules/03-fixture-stale-memory.json");

/// Every fixture module.
const FIXTURES: [&str; 3] = [MISS_THEN_HIT, SWITCH_CONFIG, STALE_MEMORY];

/// A loop without an exit and without a memory access: one event per
/// instruction until the budget or the session's limit stops it.
const ENDLESS: &str = "        add r1, r0, #1\nloop:   bnez r1, loop";

/// The tests' helpers. They sit in a module compiled for tests only, where a
/// failed `unwrap` is a failed test, as in the tests themselves.
#[cfg(test)]
mod support {
    use cache_explorer_core::{
        Config, FeedbackTone, Module, ModuleAction, ModuleActionKind, ModuleError, ModuleRun,
        ModuleStepKind, ModuleView, Predicate, Session, Step, TextSegment,
    };
    use serde_json::Value;

    use super::common::load;

    /// A session with an empty log on the first visit's configuration.
    pub(crate) fn session() -> Session {
        Session::with_config(Config::first_visit()).unwrap()
    }

    /// A run of the module `json` started on a new session.
    pub(crate) fn started(json: &str) -> (ModuleRun, Session) {
        let mut session = session();
        let mut run = ModuleRun::new(Module::parse(json).unwrap());
        run.start(&mut session).unwrap();
        (run, session)
    }

    /// `json` with `edit` applied to its value.
    pub(crate) fn edited(json: &str, edit: impl FnOnce(&mut Value)) -> String {
        let mut value: Value = serde_json::from_str(json).unwrap();
        edit(&mut value);
        value.to_string()
    }

    /// The message `Module::parse` gives for `json`.
    #[track_caller]
    pub(crate) fn parse_error(json: &str) -> String {
        match Module::parse(json) {
            Ok(_) => panic!("the module parses"),
            Err(error) => error.to_string(),
        }
    }

    /// A predicate read from JSON, as the page sends one.
    pub(crate) fn predicate(value: Value) -> Predicate {
        serde_json::from_value(value).unwrap()
    }

    /// The text of `segments`, styles dropped.
    pub(crate) fn text(segments: &[TextSegment]) -> String {
        segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }

    /// The actions of `view`, as (action, enabled).
    pub(crate) fn actions(view: &ModuleView) -> Vec<(ModuleActionKind, bool)> {
        view.actions
            .iter()
            .map(|&ModuleAction { action, enabled }| (action, enabled))
            .collect()
    }

    /// Fills `session`'s log with loads of `addr` by hand until it holds
    /// `count` ops. `addr` should be one no fixture step's predicate checks,
    /// so filling a running module's session does not complete its step.
    pub(crate) fn fill(session: &mut Session, addr: u32, count: u32) {
        while session.op_count() < count {
            session.apply(load(addr)).unwrap();
        }
    }

    /// Takes `run` through every step by its "Show me" path: Next on an
    /// explanation, "Show me" on an instruct or observe step, and on a
    /// prediction a wrong answer checked, then "Show me" and Next. Returns the
    /// view of every step on the way, the done screen last.
    #[track_caller]
    pub(crate) fn show_me_path(run: &mut ModuleRun, session: &mut Session) -> Vec<ModuleView> {
        let mut views = Vec::new();
        let mut view = run.sync(session);
        for _ in 0..50 {
            views.push(view.clone());
            view = match view.kind {
                ModuleStepKind::Done => return views,
                ModuleStepKind::Explain => run.next(session),
                ModuleStepKind::Instruct | ModuleStepKind::Observe => {
                    let before = view.step_index;
                    let after = run.show_me(session);
                    assert_eq!(after.step_index, before + 1, "{} {view:#?}", view.title);
                    after
                }
                ModuleStepKind::Predict => {
                    let Some(Step::Predict { answer, .. }) =
                        run.module().steps().get(view.step_index as usize).cloned()
                    else {
                        panic!("a predict step");
                    };
                    run.select(u32::from(answer == 0));
                    let wrong = run.check(session);
                    assert_eq!(
                        wrong.feedback.unwrap().tone,
                        FeedbackTone::Incorrect,
                        "{}",
                        view.title
                    );
                    let shown = run.show_me(session);
                    assert_eq!(
                        shown.feedback.unwrap().tone,
                        FeedbackTone::Shown,
                        "{}",
                        view.title
                    );
                    assert!(session.gate().is_none(), "{}", view.title);
                    run.next(session)
                }
            };
        }
        panic!("the module did not finish");
    }

    /// Checks that the fixture module `json` completes through its "Show me"
    /// path: a view for every step and the done screen last, in order, and
    /// the done screen's text, its one action, and no gate left on the
    /// session.
    pub(crate) fn check_show_me_completion(json: &str) {
        let (mut run, mut session) = started(json);
        let views = show_me_path(&mut run, &mut session);
        let id = run.module().id();
        let count = run.module().steps().len();
        assert_eq!(views.len(), count + 1, "{id}");
        for (index, view) in views.iter().enumerate() {
            assert_eq!(view.step_index as usize, index, "{id}");
        }
        let done = views.last().unwrap();
        assert_eq!(done.kind, ModuleStepKind::Done, "{id}");
        assert_eq!(done.kind_label, "Done", "{id}");
        assert_eq!(done.progress_text, format!("{count} of {count}"), "{id}");
        assert_eq!(
            text(&done.text),
            "Module complete. The workbench stays as you left it; keep experimenting or pick \
             another module.",
            "{id}"
        );
        assert_eq!(actions(done), [(ModuleActionKind::Restart, true)], "{id}");
        assert!(done.callout.is_none() && done.options.is_none(), "{id}");
        assert!(done.drawer_summary.ends_with(" \u{b7} complete"), "{id}");
        assert!(session.gate().is_none(), "{id}");
    }

    /// The fixture `json` with the value at `pointer` set to `value`, added if
    /// the object there does not have it yet.
    #[track_caller]
    pub(crate) fn with(json: &str, pointer: &str, value: Value) -> String {
        edited(json, |module| {
            let (parent, key) = pointer.rsplit_once('/').unwrap();
            match module.pointer_mut(parent).unwrap() {
                Value::Object(object) => {
                    object.insert(key.to_owned(), value);
                }
                Value::Array(array) => array[key.parse::<usize>().unwrap()] = value,
                other => panic!("{other}"),
            }
        })
    }

    /// Checks that each module of `cases` is refused with a message that
    /// starts as given.
    #[track_caller]
    pub(crate) fn refused_with(cases: &[(String, &str)]) {
        for (json, expected) in cases {
            let message = parse_error(json);
            assert!(
                message.starts_with(expected),
                "{message}\n  expected\n{expected}"
            );
        }
    }

    /// Checks that the module `json` is refused for its JSON's shape, with
    /// exactly the message `expected`: the reason and serde's place, its line
    /// and column, the JSON being compact and so all on line 1.
    #[track_caller]
    pub(crate) fn shape_refused(json: &str, expected: &str) {
        let Err(ModuleError::Json { message }) = Module::parse(json) else {
            panic!("{json}");
        };
        assert_eq!(message, expected);
    }

    /// Adds the kind of `predicate`, and of each predicate inside it, to
    /// `found`.
    pub(crate) fn walk(predicate: &Predicate, found: &mut Vec<&'static str>) {
        let name = match predicate {
            Predicate::Way(_) => "way",
            Predicate::Line(_) => "line",
            Predicate::Memory(_) => "memory",
            Predicate::Counter(_) => "counter",
            Predicate::Register(_) => "register",
            Predicate::All(p) => {
                p.all.iter().for_each(|p| walk(p, found));
                "all"
            }
            Predicate::Any(p) => {
                p.any.iter().for_each(|p| walk(p, found));
                "any"
            }
            Predicate::Not(p) => {
                walk(&p.not, found);
                "not"
            }
            Predicate::Event(_) => "event",
            Predicate::LastAccess(_) => "lastAccess",
            Predicate::Config(_) => "config",
        };
        found.push(name);
    }
}

#[test]
fn every_fixture_parses_and_serializes_back() {
    for json in FIXTURES {
        let module = Module::parse(json).unwrap();
        let written = serde_json::to_string(&module).unwrap();
        assert_eq!(Module::parse(&written).unwrap(), module, "{}", module.id());
    }
    // Addresses written as hex strings serialize back as numbers.
    let module = Module::parse(MISS_THEN_HIT).unwrap();
    let written = serde_json::to_value(&module).unwrap();
    assert_eq!(
        written["steps"][1]["expect"],
        json!({"lastAccess": {"kind": "read", "addr": 164}})
    );
    assert_eq!(
        written["steps"][1]["showMe"],
        json!([{"op": "issue", "kind": "read", "addr": 164}])
    );
    assert_eq!(written["steps"][5]["callout"], json!({"set": 2, "way": 0}));
    assert_eq!(written["steps"][0]["callout"], json!("addressStrip"));
    assert_eq!(module.number(), "01");
    assert_eq!(module.steps().len(), 6);
    assert_eq!(
        module.steps()[5].callout(),
        Some(Callout::Cell(CellRef {
            set: serde_json::from_value(json!(2)).unwrap(),
            way: Some(serde_json::from_value(json!(0)).unwrap()),
        }))
    );
    assert_eq!(
        module.steps()[0].callout(),
        Some(Callout::Area(CalloutArea::AddressStrip))
    );
}

cases! {
    check_show_me_completion, SHOW_ME_FIXTURES: &str;
    the_miss_then_hit_fixture_completes_through_its_show_me_path => (MISS_THEN_HIT),
    the_switch_config_fixture_completes_through_its_show_me_path => (SWITCH_CONFIG),
    the_stale_memory_fixture_completes_through_its_show_me_path => (STALE_MEMORY),
}

#[test]
fn every_fixture_has_a_show_me_test() {
    let ids = |fixtures: &[&str]| -> Vec<String> {
        fixtures
            .iter()
            .map(|json| Module::parse(json).unwrap().id().to_owned())
            .collect()
    };
    assert_eq!(ids(SHOW_ME_FIXTURES), ids(&FIXTURES));
}

#[test]
fn flow_a_completes_with_the_state_its_steps_check() {
    let (mut run, mut session) = started(MISS_THEN_HIT);
    let views = show_me_path(&mut run, &mut session);
    // The observe step's "Show me" moved the cursor back to 1.2, where the
    // prediction began; the load of 0x0A8 was then appended after access 1.
    assert_eq!(views[3].kind, ModuleStepKind::Predict);
    let events: Vec<(String, EventKind)> = session
        .events()
        .iter()
        .filter(|event| event.run == 2)
        .map(|event| (event.label.clone(), event.kind))
        .collect();
    assert_eq!(events.first().unwrap(), &(String::new(), EventKind::Setup));
    assert!(events.contains(&("1.2".to_owned(), EventKind::Miss)));
    assert!(events.contains(&("2.2".to_owned(), EventKind::Hit)));
    assert!(session.eval_predicate(&predicate(json!({"register": "r1", "equals": 66}))));
    assert!(session.eval_predicate(&predicate(
        json!({"counter": "hits", "op": "==", "value": 1})
    )));
}

#[test]
fn an_observe_step_shows_the_micro_event_it_waits_for() {
    let (mut run, mut session) = started(MISS_THEN_HIT);
    run.next(&mut session);
    let observe = run.show_me(&mut session);
    assert_eq!(observe.kind, ModuleStepKind::Observe);
    assert_eq!(observe.kind_label, "Watch");
    assert_eq!(observe.callout, Some(Callout::Area(CalloutArea::Log)));
    // Not yet: the cursor rests on the access's last event.
    assert_eq!(run.sync(&mut session).kind, ModuleStepKind::Observe);
    let end = session.position();
    assert_ne!(end.label, "1.2");
    let predict = run.show_me(&mut session);
    assert_eq!(predict.kind, ModuleStepKind::Predict);
    let position = session.position();
    assert_eq!(position.label, "1.2");
    assert_eq!(session.state().event.unwrap().kind, EventKind::Miss);
    // The prediction began there, and the gate holds the cursor to it.
    assert_eq!(session.gate().unwrap().seek_limit(), position.pos);
    session.seek(end.pos);
    assert_eq!(session.position().pos, position.pos);
    // Dragging back to 1.2 by hand completes the step as well.
    let (mut run, mut session) = started(MISS_THEN_HIT);
    run.next(&mut session);
    run.show_me(&mut session);
    let target = session.find_event(&predicate(json!({"event": {"kind": "miss", "entry": 1}})));
    assert_eq!(target, Some(position.pos));
    session.seek(pos(position.pos.get() + 1));
    assert_eq!(run.sync(&mut session).kind, ModuleStepKind::Observe);
    session.seek(position.pos);
    assert_eq!(run.sync(&mut session).kind, ModuleStepKind::Predict);
}

#[test]
fn an_observe_step_s_show_me_counts_only_ops_made_since_it_began() {
    // The observe step waits for a load of 0x0A4; the step before it made
    // one, then a load of 0x1A4.
    let json = edited(MISS_THEN_HIT, |module| {
        module["steps"][1]["expect"] = json!({"lastAccess": {"addr": "0x1A4"}});
        module["steps"][1]["showMe"] = json!([
            {"op": "issue", "kind": "read", "addr": "0x0A4"},
            {"op": "issue", "kind": "read", "addr": "0x1A4"}
        ]);
        module["steps"][2]["until"] = json!({"lastAccess": {"addr": "0x0A4"}});
    });
    let (mut run, mut session) = started(&json);
    run.next(&mut session);
    assert_eq!(run.show_me(&mut session).kind, ModuleStepKind::Observe);
    // "Show me" does not go back to the load made before the step began,
    // where the step could not complete: the cursor stays, and the step
    // waits.
    let before = session.position();
    assert_eq!(run.show_me(&mut session).kind, ModuleStepKind::Observe);
    assert_eq!(session.position(), before);
    // A load of 0x0A4 made since is found, although another op followed
    // it, and the step completes there.
    session.apply(load(0x0A4)).unwrap();
    let made = session.position().pos;
    session.apply(load(0x1A4)).unwrap();
    assert_eq!(run.sync(&mut session).kind, ModuleStepKind::Observe);
    assert_eq!(run.show_me(&mut session).kind, ModuleStepKind::Predict);
    assert_eq!(session.position().pos, made);
}

#[test]
fn a_configuration_change_in_the_middle_of_a_module() {
    let (mut run, mut session) = started(SWITCH_CONFIG);
    assert_eq!(session.config().cache.associativity, 1);
    // The trace, run by "Show me".
    run.show_me(&mut session);
    let predict = run.sync(&mut session);
    assert_eq!(predict.kind, ModuleStepKind::Predict);
    // A right answer applies the `then` ops: a new run with two ways, and
    // the trace again, which the new run kept.
    run.select(0);
    let correct = run.check(&mut session);
    let feedback = correct.feedback.clone().unwrap();
    assert_eq!(feedback.tone, FeedbackTone::Correct);
    assert_eq!(feedback.head, "Correct.");
    assert_eq!(
        text(&feedback.text),
        "Both lines fit in set 0, so only the first load of each misses."
    );
    assert_eq!(actions(&correct), [(ModuleActionKind::Next, true)]);
    assert_eq!(session.config().cache.associativity, 2);
    assert!(session.eval_predicate(&predicate(
        json!({"counter": "hits", "op": "==", "value": 2})
    )));
    // The observe step waits for the new run's first hit, entry 3's.
    run.next(&mut session);
    let instruct = run.show_me(&mut session);
    // Run 1 is the session's first, run 2 the module's preset, run 3 the
    // switch to two ways.
    assert_eq!(session.position().label, "3.2");
    assert_eq!(session.position().run, 3);
    // The switch to FIFO made by hand, as the configuration bar makes it,
    // completes the step as "Show me" would.
    assert_eq!(instruct.kind, ModuleStepKind::Instruct);
    assert_eq!(
        instruct.text[0].text,
        "Switch Replacement to FIFO, then reset."
    );
    let mut fifo = *session.config();
    fifo.cache.replacement = Replacement::Fifo;
    session.apply(Op::Config { config: fifo }).unwrap();
    let next = run.sync(&mut session);
    assert_eq!(next.step_index, 4);
    assert_eq!(session.config().cache.replacement, Replacement::Fifo);
    // The trace by hand, one Space at a time: the step completes at the end
    // of the fourth access, and never inside an entry.
    let mut accesses = 0;
    loop {
        assert_eq!(session.step_event(None), StepOutcome::Applied);
        while session.position().boundary < Boundary::EntryEnd {
            assert_eq!(run.sync(&mut session).step_index, 4, "{accesses}");
            assert_eq!(session.step_event(None), StepOutcome::Moved);
        }
        accesses += 1;
        if run.sync(&mut session).step_index != 4 {
            break;
        }
        assert!(accesses < 4);
    }
    assert_eq!(accesses, 4);
    assert_eq!(run.view(&session).kind, ModuleStepKind::Explain);
}

#[test]
fn an_instruct_step_waits_for_an_op_made_since_it_began() {
    let (mut run, mut session) = started(MISS_THEN_HIT);
    // A load made while the explanation is showing does not count.
    session.apply(load(0x0A4)).unwrap();
    let instruct = run.next(&mut session);
    assert_eq!(instruct.kind, ModuleStepKind::Instruct);
    assert_eq!(instruct.kind_label, "Do this");
    assert_eq!(run.sync(&mut session).kind, ModuleStepKind::Instruct);
    // Undone and made again, it does: its serial is new.
    assert!(session.undo());
    assert_eq!(run.sync(&mut session).kind, ModuleStepKind::Instruct);
    session.apply(load(0x0A4)).unwrap();
    // Inside the access the step waits for its end.
    let end = session.position().pos;
    session.seek(pos(end.get() - 2));
    assert_eq!(run.sync(&mut session).kind, ModuleStepKind::Instruct);
    session.seek(end);
    assert_eq!(run.sync(&mut session).kind, ModuleStepKind::Observe);
}

#[test]
fn a_prediction_offers_show_me_only_after_a_wrong_answer() {
    let (mut run, mut session) = started(MISS_THEN_HIT);
    run.next(&mut session);
    run.show_me(&mut session);
    let predict = run.show_me(&mut session);
    assert_eq!(predict.kind, ModuleStepKind::Predict);
    assert_eq!(predict.kind_label, "Predict");
    assert_eq!(predict.progress_text, "Step 4 of 6");
    // Check is shown, disabled until an option is selected; Show me is not.
    assert_eq!(actions(&predict), [(ModuleActionKind::Check, false)]);
    let options = predict.options.unwrap();
    assert!(options
        .iter()
        .all(|option| option.state == OptionState::Idle));
    assert_eq!(
        options[0].text,
        [
            TextSegment {
                text: "Hit: it is in the same 16-byte line as ".to_owned(),
                style: TextStyle::Plain,
            },
            TextSegment {
                text: "0x0A4".to_owned(),
                style: TextStyle::Code,
            },
        ]
    );
    // "Show me" before a wrong answer reveals nothing.
    let unchanged = run.show_me(&mut session);
    assert!(unchanged.feedback.is_none());
    assert!(session.gate().is_some());
    let limit = session.gate().unwrap().seek_limit();
    run.select(2);
    let selected = run.view(&session);
    assert_eq!(actions(&selected), [(ModuleActionKind::Check, true)]);
    assert_eq!(selected.options.unwrap()[2].state, OptionState::Selected);
    // A wrong answer: "Not quite." and the hint, and now Show me.
    let wrong = run.check(&mut session);
    let feedback = wrong.feedback.clone().unwrap();
    assert_eq!(feedback.tone, FeedbackTone::Incorrect);
    assert_eq!(feedback.head, "Not quite.");
    assert_eq!(
        text(&feedback.text),
        "Compare the tag and index of 0x0A8 with those of 0x0A4."
    );
    assert_eq!(wrong.options.clone().unwrap()[2].state, OptionState::Wrong);
    assert_eq!(
        actions(&wrong),
        [
            (ModuleActionKind::Check, true),
            (ModuleActionKind::ShowMe, true)
        ]
    );
    assert!(session.gate().is_some());
    // Another selection clears the feedback; Show me stays.
    run.select(1);
    let again = run.view(&session);
    assert!(again.feedback.is_none());
    assert_eq!(
        again.options.clone().unwrap()[1].state,
        OptionState::Selected
    );
    assert!(actions(&again).contains(&(ModuleActionKind::ShowMe, true)));
    // An option out of range changes nothing.
    run.select(3);
    assert_eq!(
        run.view(&session).options.unwrap()[1].state,
        OptionState::Selected
    );
    // Show me reveals the answer and lifts the gate.
    let shown = run.show_me(&mut session);
    let feedback = shown.feedback.clone().unwrap();
    assert_eq!(feedback.tone, FeedbackTone::Shown);
    assert_eq!(feedback.head, "Shown.");
    assert_eq!(
        text(&feedback.text),
        "The answer is highlighted. 0x0A8 has the tag and index of 0x0A4; only the offset \
         differs."
    );
    let states: Vec<OptionState> = shown
        .options
        .clone()
        .unwrap()
        .iter()
        .map(|option| option.state)
        .collect();
    assert_eq!(
        states,
        [OptionState::Shown, OptionState::Selected, OptionState::Idle]
    );
    assert_eq!(actions(&shown), [(ModuleActionKind::Next, true)]);
    assert!(session.gate().is_none());
    // Answered, it no longer takes a selection or a check.
    run.select(2);
    assert_eq!(
        run.check(&mut session).feedback.unwrap().tone,
        FeedbackTone::Shown
    );
    assert!(session.position().pos <= limit);
    assert_eq!(run.next(&mut session).kind, ModuleStepKind::Instruct);
}

#[test]
fn the_gate_refuses_ops_and_holds_the_cursor_until_the_prediction_is_answered() {
    let (mut run, mut session) = started(SWITCH_CONFIG);
    run.show_me(&mut session);
    let limit = session.position().pos;
    assert_eq!(session.gate().unwrap().seek_limit(), limit);
    let ops = session.op_count();
    assert_eq!(session.apply(load(0x0A4)), Err(OpError::Gated));
    assert_eq!(
        OpError::Gated.to_string(),
        "Answer the prediction in the module panel first."
    );
    assert_eq!(
        session.propose_config(Preset::TwoWayLru.config()),
        Err(OpError::Gated)
    );
    assert_eq!(session.op_count(), ops);
    // Undo is allowed; Redo only as far as the limit.
    assert!(session.undo());
    assert!(session.position().pos < limit);
    assert!(session.redo());
    assert_eq!(session.position().pos, limit);
    // The correct answer lifts the gate and appends the `then` ops.
    run.select(0);
    run.check(&mut session);
    assert!(session.gate().is_none());
    assert_eq!(session.op_count(), ops + 2);
    // Exit lifts a gate too, and leaves the workbench as it is.
    let (mut run, mut session) = started(SWITCH_CONFIG);
    run.show_me(&mut session);
    assert!(session.gate().is_some());
    let before = session.position();
    run.exit(&mut session);
    assert!(session.gate().is_none());
    assert_eq!(session.position(), before);
    session.apply(load(0x0A4)).unwrap();
}

#[test]
fn start_appends_the_preset_and_the_program_and_sets_the_mode() {
    let mut session = session();
    session.apply(load(0x0A4)).unwrap();
    let mut run = ModuleRun::new(Module::parse(STALE_MEMORY).unwrap());
    assert!(!session.advanced());
    let events = run.start(&mut session).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, EventKind::Setup);
    assert!(session.advanced());
    assert_eq!(session.config().timing.unwrap().miss_cycles, 20);
    // The work before stays reachable with Undo.
    assert!(session.undo());
    assert_eq!(session.config(), &Config::first_visit());
    // A module with a program loads it after the preset.
    let mut session = Session::with_config(Config::first_visit()).unwrap();
    let mut run = ModuleRun::new(Module::parse(SWITCH_CONFIG).unwrap());
    run.start(&mut session).unwrap();
    let ops = session.ops();
    assert!(matches!(ops[0], Op::Config { .. }));
    assert!(matches!(&ops[1], Op::Program { source, .. } if source.starts_with("R 0x000")));
    assert!(!session.advanced());
    // On the done screen, Next starts the module again: two more ops.
    show_me_path(&mut run, &mut session);
    let count = session.op_count();
    let restarted = run.next(&mut session);
    assert_eq!(restarted.step_index, 0);
    assert_eq!(restarted.kind, ModuleStepKind::Instruct);
    assert_eq!(session.op_count(), count + 2);
    assert_eq!(session.config().cache.associativity, 1);
}

#[test]
fn start_reports_the_events_of_the_ops_it_appended_over_an_undone_op() {
    let mut session = session();
    session.apply(load(0x0A4)).unwrap();
    session.apply(load(0x0A8)).unwrap();
    assert!(session.undo());
    let from = session.position().pos;
    let mut run = ModuleRun::new(Module::parse(SWITCH_CONFIG).unwrap());
    let reported = run.start(&mut session).unwrap().to_vec();
    let kinds: Vec<EventKind> = reported.iter().map(|event| event.kind).collect();
    assert_eq!(kinds, [EventKind::Setup, EventKind::Setup], "{reported:?}");
    let to = u32::try_from(session.events_range(0..u32::MAX).len()).unwrap();
    assert_eq!(reported, session.events_range(from.get()..to));
}

#[test]
fn the_view_says_where_the_run_is() {
    let (mut run, mut session) = started(MISS_THEN_HIT);
    let first = run.view(&session);
    assert_eq!(first.id, "01-fixture-miss-then-hit");
    assert_eq!(first.title, "Fixture: miss then hit");
    assert_eq!(first.step_index, 0);
    assert_eq!(first.step_count, 6);
    assert_eq!(first.kind, ModuleStepKind::Explain);
    assert_eq!(first.kind_label, "Explain");
    assert_eq!(first.progress_text, "Step 1 of 6");
    assert_eq!(
        first.drawer_summary,
        "Running 01 \u{b7} Fixture: miss then hit \u{b7} step 1 of 6"
    );
    assert_eq!(
        first.callout,
        Some(Callout::Area(CalloutArea::AddressStrip))
    );
    assert_eq!(actions(&first), [(ModuleActionKind::Next, true)]);
    assert_eq!(
        first.description,
        [
            TextSegment {
                text: "A first access ".to_owned(),
                style: TextStyle::Plain,
            },
            TextSegment {
                text: "misses".to_owned(),
                style: TextStyle::Emphasis,
            },
            TextSegment {
                text: " and brings in the whole line, so ".to_owned(),
                style: TextStyle::Plain,
            },
            TextSegment {
                text: "0x0A8".to_owned(),
                style: TextStyle::Code,
            },
            TextSegment {
                text: " hits after ".to_owned(),
                style: TextStyle::Plain,
            },
            TextSegment {
                text: "0x0A4".to_owned(),
                style: TextStyle::Code,
            },
            TextSegment {
                text: ".".to_owned(),
                style: TextStyle::Plain,
            },
        ]
    );
    let instruct = run.next(&mut session);
    assert_eq!(instruct.progress_text, "Step 2 of 6");
    assert_eq!(
        text(instruct.hint.as_deref().unwrap()),
        "Type 0x0A4 in the address box and press Issue."
    );
    assert_eq!(actions(&instruct), [(ModuleActionKind::ShowMe, true)]);
    assert!(instruct.options.is_none() && instruct.feedback.is_none());
    // Next does nothing on an instruct step.
    assert_eq!(run.next(&mut session).step_index, 1);
    let json = serde_json::to_value(&instruct).unwrap();
    assert_eq!(json["kind"], "instruct");
    assert_eq!(json["kindLabel"], "Do this");
    assert_eq!(
        json["actions"],
        json!([{"action": "showMe", "enabled": true}])
    );
    assert_eq!(json["callout"], "transport");
    assert!(json.get("options").is_none() && json.get("feedback").is_none());
}

#[test]
fn predicates_read_the_machine_at_the_cursor() {
    let mut session = session();
    session
        .apply(
            serde_json::from_value(
                json!({"op": "issue", "kind": "write", "addr": 0x0A4, "value": 7}),
            )
            .unwrap(),
        )
        .unwrap();
    let holds = |session: &Session, value: Value| session.eval_predicate(&predicate(value));
    let cases = [
        (
            json!({"way": {"set": 2, "way": 0}, "valid": true, "tag": 1, "dirty": true, "rank": 0}),
            true,
        ),
        (json!({"way": {"set": 2, "way": 0}, "tag": 2}), false),
        (json!({"way": {"set": 3, "way": 0}, "valid": false}), true),
        (json!({"way": {"set": 8, "way": 0}}), false),
        (json!({"line": {"addr": "0x0AC"}}), true),
        (json!({"line": {"addr": "0x0A4"}, "dirty": true}), true),
        (json!({"line": {"addr": "0x0A4"}, "present": false}), false),
        (json!({"line": {"addr": "0x1A4"}, "present": false}), true),
        (json!({"line": {"addr": "0x1A4"}, "dirty": false}), false),
        (json!({"line": {"addr": "0x1000"}}), false),
        // Memory is stale: its word is the pattern's 53, not the 7 cached.
        (json!({"memory": {"addr": "0x0A4"}, "equals": 53}), true),
        (json!({"memory": {"addr": "0x0A4"}, "equals": 7}), false),
        (json!({"memory": {"addr": "0x0A6"}, "equals": 53}), false),
        (json!({"counter": "stores", "op": "==", "value": 1}), true),
        (json!({"counter": "misses", "op": ">=", "value": 1}), true),
        (json!({"counter": "misses", "op": "<=", "value": 0}), false),
        (
            json!({"counter": "writebacks", "op": "==", "value": 0}),
            true,
        ),
        (json!({"register": "r1", "equals": 0}), true),
        (
            json!({"all": [{"counter": "stores", "op": "==", "value": 1}, {"register": "r0", "equals": 0}]}),
            true,
        ),
        (
            json!({"all": [{"counter": "stores", "op": "==", "value": 1}, {"register": "r0", "equals": 1}]}),
            false,
        ),
        (
            json!({"any": [{"register": "r0", "equals": 1}, {"counter": "fills", "op": "==", "value": 1}]}),
            true,
        ),
        (
            json!({"not": {"counter": "fills", "op": "==", "value": 1}}),
            false,
        ),
        (
            json!({"config": {"sizeBytes": 128, "lineBytes": 16, "associativity": 1}}),
            true,
        ),
        (
            json!({"config": {"replacement": "lru", "writePolicy": "writeBack", "allocatePolicy": "writeAllocate"}}),
            true,
        ),
        (json!({"config": {"timed": false}}), true),
        (json!({"config": {"timed": true}}), false),
        (json!({"config": {"associativity": 2}}), false),
        (
            json!({"lastAccess": {"kind": "write", "addr": 164, "outcome": "miss"}}),
            true,
        ),
        (json!({"lastAccess": {"kind": "read"}}), false),
        (json!({"event": {"kind": "write"}}), true),
    ];
    for (value, expected) in cases {
        assert_eq!(holds(&session, value.clone()), expected, "{value}");
    }
    // A load of the stale word reads the cache's copy into r1.
    session.apply(load(0x0A4)).unwrap();
    assert!(holds(&session, json!({"register": "r1", "equals": 7})));
    // A register holds all 64 bits.
    assert!(!holds(
        &session,
        json!({"register": "r2", "equals": u64::MAX})
    ));
}

#[test]
fn an_event_predicate_names_the_event_at_the_cursor() {
    let mut session = session();
    let miss = predicate(json!({"event": {"kind": "miss", "entry": 1}}));
    assert!(!session.eval_predicate(&miss));
    session.apply(load(0x0A4)).unwrap();
    session.apply(load(0x0A8)).unwrap();
    // At the end, the cursor rests on access 2's last event; `misses == 1`
    // holds there too, which is why the event predicate exists.
    assert!(!session.eval_predicate(&miss));
    assert!(session.eval_predicate(&predicate(
        json!({"counter": "misses", "op": "==", "value": 1})
    )));
    let found = session.find_event(&miss).unwrap();
    session.seek(found);
    assert_eq!(session.position().label, "1.2");
    assert!(session.eval_predicate(&miss));
    assert!(!session.eval_predicate(&predicate(json!({"event": {"kind": "miss", "entry": 2}}))));
    assert!(session.eval_predicate(&predicate(json!({"event": {"kind": "miss"}}))));
    session.seek(pos(found.get() + 1));
    assert!(!session.eval_predicate(&miss));
    // Nothing matches: none; a predicate true at the run's origin: the
    // origin.
    assert_eq!(
        session.find_event(&predicate(json!({"event": {"kind": "writeBack"}}))),
        None
    );
    assert_eq!(
        session.find_event(&predicate(
            json!({"counter": "accesses", "op": "==", "value": 0})
        )),
        Some(pos(0))
    );
    let hit = session
        .find_event(&predicate(json!({"event": {"kind": "hit", "entry": 2}})))
        .unwrap();
    session.seek(hit);
    assert_eq!(session.position().label, "2.2");
}

#[test]
fn the_search_for_an_event_stays_in_the_displayed_run() {
    let mut session = session();
    session.apply(load(0x0A4)).unwrap();
    session
        .apply(Op::Config {
            config: Preset::TwoWayLru.config(),
        })
        .unwrap();
    let origin = session.position().pos;
    let miss = predicate(json!({"event": {"kind": "miss", "entry": 1}}));
    // The run shown has no access yet: the miss of run 1 is not found.
    assert_eq!(session.find_event(&miss), None);
    session.apply(load(0x0A4)).unwrap();
    let found = session.find_event(&miss).unwrap();
    assert!(found > origin);
    // Found, it is where a seek evaluates it true.
    session.seek(found);
    assert!(session.eval_predicate(&miss));
    assert_eq!(session.position().run, 2);
}

#[test]
fn the_last_access_follows_undo_truncation_and_new_runs() {
    let mut session = session();
    let last = |session: &Session, addr: u32| {
        session.eval_predicate(&predicate(json!({"lastAccess": {"addr": addr}})))
    };
    let any = predicate(json!({"lastAccess": {}}));
    assert!(!session.eval_predicate(&any));
    session.apply(load(0x0A4)).unwrap();
    session.apply(load(0x0A8)).unwrap();
    assert!(last(&session, 0x0A8));
    // Inside access 2, the last completed access is access 1.
    let end = session.position().pos;
    session.seek(pos(end.get() - 1));
    assert!(last(&session, 0x0A4));
    // Undone, the same.
    session.seek(end);
    assert!(session.undo());
    assert!(last(&session, 0x0A4) && !last(&session, 0x0A8));
    // A new op truncates the undone one and is the last access.
    session.apply(load(0x1A4)).unwrap();
    assert!(last(&session, 0x1A4));
    // A new run has no access yet, whatever the run before made.
    session
        .apply(Op::Config {
            config: Preset::TwoWayLru.config(),
        })
        .unwrap();
    assert!(!session.eval_predicate(&any));
    assert!(session.undo());
    assert!(last(&session, 0x1A4));
    // An edit is not an access.
    assert!(session.redo());
    session.apply(load(0x0B0)).unwrap();
    session
        .apply(
            serde_json::from_value(json!({
                "op": "edit",
                "target": {"kind": "memory", "addr": 0x0A4},
                "patch": {"kind": "memory", "value": 1}
            }))
            .unwrap(),
        )
        .unwrap();
    assert!(last(&session, 0x0B0));
    assert!(session.eval_predicate(&predicate(
        json!({"lastAccess": {"kind": "read", "outcome": "miss"}})
    )));
}

#[test]
fn the_search_for_the_last_access_along_a_full_run_finds_or_misses_it() {
    // One load, then a run as long as the session holds of instructions
    // that access nothing. The search evaluates `lastAccess` at every
    // position of the run; the work it takes is bounded by a test of the
    // session, which counts it.
    let mut session = session();
    session.apply(load(0x0A4)).unwrap();
    let loaded = session.position().pos;
    let endless = Op::Program {
        source: ENDLESS.to_owned(),
        format: ProgramFormat::Asm,
    };
    while session.apply(endless.clone()).is_ok() && session.apply(Op::StepAccess).is_ok() {}
    let full = session.position().pos.get();
    assert!(full + MAX_EVENTS_PER_ENTRY > SESSION_EVENT_LIMIT, "{full}");
    let found = |value| session.find_event(&predicate(value));
    assert_eq!(found(json!({"lastAccess": {"addr": "0x0A8"}})), None);
    assert_eq!(
        found(json!({"lastAccess": {"addr": "0x0A4"}})),
        Some(loaded)
    );
}

#[test]
fn a_loaded_session_counts_its_ops_as_made_before_a_step_began() {
    // A module started on a session loaded from a share string: its ops are
    // numbered from 1, the module's own after them.
    let mut original = session();
    original.apply(load(0x0A4)).unwrap();
    let mut session = Session::from_share_string(&original.to_share_string().unwrap()).unwrap();
    let mut run = ModuleRun::new(Module::parse(MISS_THEN_HIT).unwrap());
    run.start(&mut session).unwrap();
    run.next(&mut session);
    // Undone back into the loaded op, the step still waits.
    session.undo();
    assert_eq!(session.position().run, 1);
    assert_eq!(run.sync(&mut session).kind, ModuleStepKind::Instruct);
    session.apply(load(0x0A4)).unwrap();
    assert_eq!(run.sync(&mut session).kind, ModuleStepKind::Observe);
}

#[test]
fn the_module_s_own_rules_have_their_errors() {
    // The id, title, description, preset, program and steps.
    refused_with(&[
        (
            with(SWITCH_CONFIG, "/id", json!("2-switch")),
            "id: an id is two digits, a hyphen, then lower-case letters, digits and hyphens, as \
             in `04-tracking-lru`.",
        ),
        (
            with(SWITCH_CONFIG, "/id", json!("02-Switch")),
            "id: an id is two digits, a hyphen, then lower-case letters, digits and hyphens, as \
             in `04-tracking-lru`.",
        ),
        (
            with(SWITCH_CONFIG, "/title", json!(" ")),
            "title: the title is empty.",
        ),
        (
            with(SWITCH_CONFIG, "/description", json!("**Conflicts**")),
            "description: strong emphasis (`**`) at character 1 is not supported; module text \
             allows only `code` and *emphasis*.",
        ),
        (
            with(SWITCH_CONFIG, "/preset/cache/associativity", json!(3)),
            "preset: Associativity 3 is not a power of two. It must be 1, 2, 4 or 8 (fully \
             associative) for an 8-line cache.",
        ),
        (
            with(SWITCH_CONFIG, "/preset/advanced", json!(true)),
            "preset.advanced: the preset's `advanced` must be false, as the module's is.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/preset/cache/writePolicy",
                json!("writeThrough"),
            ),
            "preset: a module that is not advanced uses Basics configurations only: \
             write-back, write-allocate and untimed.",
        ),
        (
            with(SWITCH_CONFIG, "/program/source", json!("R 0x1000")),
            "program: the module's program does not fit this configuration. The trace has an \
             error on line 1: ",
        ),
    ]);
}

#[test]
fn each_step_s_rules_have_their_errors() {
    // Text, options and answers, empty op lists and callouts.
    refused_with(&[
        (
            with(SWITCH_CONFIG, "/steps", json!([])),
            "steps: a module needs at least one step.",
        ),
        (
            with(SWITCH_CONFIG, "/steps/0/text", json!("Run the trace\nnow.")),
            "steps[0].text: a line break at character 14 is not supported; module text allows \
             only `code` and *emphasis*.",
        ),
        (
            with(SWITCH_CONFIG, "/steps/0/showMe", json!([])),
            "steps[0].showMe: list at least one op, or leave `showMe` out.",
        ),
        (
            with(SWITCH_CONFIG, "/steps/1/then", json!([])),
            "steps[1].then: list at least one op, or leave `then` out.",
        ),
        (
            with(SWITCH_CONFIG, "/steps/1/options", json!(["2"])),
            "steps[1].options: a prediction needs at least two options; it has 1.",
        ),
        (
            with(SWITCH_CONFIG, "/steps/1/options/1", json!("<b>4</b>")),
            "steps[1].options[1]: HTML at character 1 is not supported; module text allows only \
             `code` and *emphasis*.",
        ),
        (
            with(SWITCH_CONFIG, "/steps/1/answer", json!(2)),
            "steps[1].answer: answer 2 is not an option; the options are numbered from 0 to 1.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/1/explain",
                json!("see [the log](#log)"),
            ),
            "steps[1].explain: a link at character 5 is not supported; module text allows only \
             `code` and *emphasis*.",
        ),
        (
            with(SWITCH_CONFIG, "/steps/1/hint", json!("")),
            "steps[1].hint: the text is empty.",
        ),
        (
            with(SWITCH_CONFIG, "/steps/4/callout", json!({"set": 4})),
            "steps[4].callout.set: this configuration has no set 4; its sets are 0\u{2013}3.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/5/callout",
                json!({"set": 0, "way": 2}),
            ),
            "steps[5].callout.way: this configuration has no way 2; its ways are 0\u{2013}1.",
        ),
    ]);
}

#[test]
fn a_step_s_ops_have_their_errors() {
    // Each op as a run of the configuration in force would refuse it, and
    // each configuration a `config` op switches to.
    let run_steps = format!("steps[0].showMe[0]: {}", OpError::RunSteps);
    refused_with(&[
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/showMe/0",
                json!({"op": "run", "n": 0}),
            ),
            run_steps.as_str(),
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/showMe/0",
                json!({"op": "issue", "kind": "read", "addr": "0x0A6"}),
            ),
            "steps[0].showMe[0]: 0x0A6 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/showMe/0",
                json!({"op": "issue", "kind": "write", "addr": "0x0A4"}),
            ),
            "steps[0].showMe[0]: A store needs a value to write.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/showMe/0",
                json!({
                    "op": "edit",
                    "target": {"kind": "way", "set": 8, "way": 0},
                    "patch": {"kind": "way", "valid": false}
                }),
            ),
            "steps[0].showMe[0]: Refused. This cache has no set 8; its sets are 0\u{2013}7.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/showMe/0",
                json!({"op": "program", "source": "ld r1, [0x1000]", "format": "asm"}),
            ),
            "steps[0].showMe[0]: The program has an error on line 1: ",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/1/then/0/config/cache/sizeBytes",
                json!(100),
            ),
            "steps[1].then[0].config: Cache size must be a power of two, such as 64, 128 or 256 \
             bytes; got 100.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/1/then/0/config/timing",
                json!({"hitCycles": 1, "missCycles": 20}),
            ),
            "steps[1].then[0].config: a module that is not advanced uses Basics configurations \
             only: write-back, write-allocate and untimed.",
        ),
        (
            edited(SWITCH_CONFIG, |module| {
                module["program"]["source"] = json!("R 0x100");
                module["steps"][1]["then"][0]["config"]["addressBits"] = json!(8);
            }),
            "steps[1].then[0].config: the module's program does not fit this configuration. The \
             trace has an error on line 1: ",
        ),
        // A program an earlier step's ops load replaces the module's: the
        // module's fits 8-bit addresses, the one loaded does not.
        (
            edited(SWITCH_CONFIG, |module| {
                module["steps"][0]["showMe"] = json!([
                    {"op": "program", "source": "R 0x100", "format": "trace"},
                    {"op": "run", "n": 1}
                ]);
                module["steps"][1]["then"][0]["config"]["addressBits"] = json!(8);
            }),
            "steps[1].then[0].config: the module's program does not fit this configuration. The \
             trace has an error on line 1: ",
        ),
    ]);
}

#[test]
fn an_edit_s_form_is_checked_when_the_module_is_read() {
    // The target, the patch's kind and shape, a way's line and a memory
    // word's address and value, as a run of the configuration in force
    // would refuse them whatever the cache holds.
    refused_with(&[
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/showMe/0",
                json!({
                    "op": "edit",
                    "target": {"kind": "way", "set": 0, "way": 0},
                    "patch": {"kind": "memory", "value": 1}
                }),
            ),
            "steps[0].showMe[0]: Refused. A way takes a way's fields, not a memory value.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/showMe/0",
                json!({
                    "op": "edit",
                    "target": {"kind": "way", "set": 0, "way": 0},
                    "patch": {"kind": "way", "valid": true, "line": "0x000", "data": [1, 2, 3]}
                }),
            ),
            "steps[0].showMe[0]: Refused. Each line holds 4 words; this data has 3.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/showMe/0",
                json!({
                    "op": "edit",
                    "target": {"kind": "way", "set": 0, "way": 0},
                    "patch": {"kind": "way", "valid": true, "line": "0x1000"}
                }),
            ),
            "steps[0].showMe[0]: Refused. 0x1000 does not fit a 12-bit address",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/showMe/0",
                json!({
                    "op": "edit",
                    "target": {"kind": "memory", "addr": "0x1000"},
                    "patch": {"kind": "memory", "value": 1}
                }),
            ),
            "steps[0].showMe[0]: Refused. 0x1000 does not fit a 12-bit address",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/showMe/0",
                json!({
                    "op": "edit",
                    "target": {"kind": "memory", "addr": "0x0A4"},
                    "patch": {"kind": "memory", "value": 4_294_967_296_u64}
                }),
            ),
            "steps[0].showMe[0]: Refused. 4,294,967,296 does not fit a 4-byte word",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/showMe/0",
                json!({
                    "op": "edit",
                    "target": {"kind": "memory", "addr": "0x0A6"},
                    "patch": {"kind": "memory", "value": 1}
                }),
            ),
            "steps[0].showMe[0]: Refused. 0x0A6 is not aligned to a 4-byte word; use 0x0A4 or \
             0x0A8.",
        ),
    ]);
}

#[test]
fn each_predicate_is_checked_against_the_configuration_in_force() {
    // Each against the configuration in force at its step: direct-mapped-8
    // in step 0, two-way LRU in step 2, after step 1's switch, and two-way
    // FIFO in step 4, after step 3's.
    refused_with(&[
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/expect",
                json!({"way": {"set": 0, "way": 1}}),
            ),
            "steps[0].expect.way.way: this configuration has no way 1; its only way is 0.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/expect",
                json!({"all": [{"way": {"set": 8, "way": 0}}]}),
            ),
            "steps[0].expect.all[0].way.set: this configuration has no set 8; its sets are \
             0\u{2013}7.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/expect",
                json!({"way": {"set": 0, "way": 0}, "rank": 0}),
            ),
            "steps[0].expect.rank: this configuration keeps no replacement order to check: \
             ranks need LRU or FIFO and more than one way.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/expect",
                json!({"way": {"set": 0, "way": 0}, "tag": 32}),
            ),
            "steps[0].expect.tag: tag 0x20 does not fit this configuration's 5-bit tags \
             (0x00\u{2013}0x1F).",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/expect",
                json!({"line": {"addr": "0x1000"}}),
            ),
            "steps[0].expect.line.addr: 0x1000 does not fit a 12-bit address \
             (0x000\u{2013}0xFFF).",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/expect",
                json!({"line": {"addr": "0x0A0"}, "present": false, "dirty": true}),
            ),
            "steps[0].expect.dirty: a line that is not present cannot be dirty.",
        ),
    ]);
}

#[test]
fn each_predicate_s_values_are_checked_against_the_configuration_in_force() {
    refused_with(&[
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/expect",
                json!({"memory": {"addr": "0x0A6"}, "equals": 1}),
            ),
            "steps[0].expect.memory.addr: 0x0A6 is not aligned to a 4-byte word; use 0x0A4 or \
             0x0A8.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/expect",
                json!({"memory": {"addr": "0x0A4"}, "equals": 4_294_967_296_u64}),
            ),
            "steps[0].expect.equals: 4,294,967,296 does not fit a 4-byte word \
             (0\u{2013}4,294,967,295).",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/expect",
                json!({"not": {"lastAccess": {"addr": 162}}}),
            ),
            "steps[0].expect.not.lastAccess.addr: 0x0A2 is not aligned to a 4-byte word; use \
             0x0A0 or 0x0A4.",
        ),
        (
            with(
                SWITCH_CONFIG,
                "/steps/0/expect",
                json!({"counter": "cycles", "op": ">=", "value": 1}),
            ),
            "steps[0].expect.counter: the `cycles` counter counts only in timed mode, and this \
             configuration is untimed.",
        ),
        (
            with(SWITCH_CONFIG, "/steps/0/expect", json!({"any": []})),
            "steps[0].expect.any: an `any` predicate needs at least one predicate.",
        ),
        (
            with(SWITCH_CONFIG, "/steps/2/until/event/entry", json!(0)),
            "steps[2].until.event.entry: entry must be at least 1.",
        ),
        (
            with(SWITCH_CONFIG, "/steps/4/expect/all/0/rank", json!(2)),
            "steps[4].expect.all[0].rank: rank 2 is out of range; with 2 ways the ranks are \
             0\u{2013}1.",
        ),
    ]);
}

#[test]
fn a_check_follows_the_configuration_in_force() {
    // A set checked against the configuration in force: set 4 exists before
    // the switch to two ways, not after it.
    Module::parse(&with(SWITCH_CONFIG, "/steps/0/callout", json!({"set": 4}))).unwrap();
    // Way 1 and a rank, refused under direct-mapped in step 0, are fine
    // under two-way FIFO in step 4.
    Module::parse(&with(
        SWITCH_CONFIG,
        "/steps/4/expect",
        json!({"way": {"set": 3, "way": 1}, "rank": 1}),
    ))
    .unwrap();
    // A program an earlier step's ops load replaces the module's for the
    // configurations after it: here one that fits 8-bit addresses, where
    // the module's does not.
    Module::parse(&edited(SWITCH_CONFIG, |module| {
        module["program"]["source"] = json!("R 0x100");
        module["steps"][0]["showMe"] = json!([
            {"op": "program", "source": "R 0x000\nR 0x080", "format": "trace"},
            {"op": "run", "n": 2}
        ]);
        module["steps"][1]["then"][0]["config"]["addressBits"] = json!(8);
    }))
    .unwrap();
}

#[test]
fn an_expect_must_tell_the_configuration_its_show_me_switches_to() {
    // Step 3's "Show me" switches two-way LRU to two-way FIFO. Its `expect`
    // needs `config` predicates it cannot hold without, which FIFO meets
    // and LRU does not, so the step waits for a switch made by hand too.
    const UNCHECKED: &str = "steps[3].expect: Show me changes the configuration, so `expect` \
                             needs a `config` predicate that the new configuration meets and the \
                             one before it does not.";
    let unchecked = |expect: Value| (with(SWITCH_CONFIG, "/steps/3/expect", expect), UNCHECKED);
    refused_with(&[
        // None, or one the new configuration does not meet.
        unchecked(json!({"counter": "accesses", "op": "==", "value": 0})),
        unchecked(json!({"config": {"replacement": "lru"}})),
        // Ones the configuration before the switch meets too: the step
        // would complete without the switch.
        unchecked(json!({"config": {}})),
        unchecked(json!({"config": {"associativity": 2}})),
        unchecked(json!({"all": [{"config": {"associativity": 2}},
                                 {"counter": "accesses", "op": ">=", "value": 0}]})),
        // One the step can hold without.
        unchecked(json!({"any": [{"config": {"replacement": "fifo"}},
                                 {"counter": "accesses", "op": ">=", "value": 0}]})),
    ]);
    // Together they tell the switch: two-way alone holds before it, and
    // with FIFO it does not.
    for expect in [
        json!({"all": [{"config": {"associativity": 2}}, {"config": {"replacement": "fifo"}}]}),
        json!({"all": [{"counter": "accesses", "op": ">=", "value": 0},
                       {"all": [{"config": {"replacement": "fifo"}}]}]}),
    ] {
        Module::parse(&with(SWITCH_CONFIG, "/steps/3/expect", expect)).unwrap();
    }
}

#[test]
fn a_predicate_nests_at_most_max_predicate_depth_deep() {
    let counter = json!({"counter": "accesses", "op": "==", "value": 0});
    let nest = |key: &str, depth: u32| {
        (0..depth).fold(counter.clone(), |inner, _| {
            if key == "not" {
                json!({ key: inner })
            } else {
                json!({ key: [inner] })
            }
        })
    };
    let session = session();
    // A `Value` sets no limit on nesting, as a value from JavaScript sets
    // none, so the predicate's own limit is what stops it.
    for key in ["all", "any", "not"] {
        let deepest = predicate(nest(key, MAX_PREDICATE_DEPTH));
        assert!(session.eval_predicate(&deepest), "{key}");
        let error =
            serde_json::from_value::<Predicate>(nest(key, MAX_PREDICATE_DEPTH + 1)).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("predicates nest at most {MAX_PREDICATE_DEPTH} deep in `all`, `any` and `not`"),
            "{key}"
        );
    }
    // A refusal leaves the depth as it found it, so the next predicate reads.
    assert!(session.eval_predicate(&predicate(nest("not", MAX_PREDICATE_DEPTH))));
    // In module JSON, serde_json's own limit comes first, as it did before
    // predicates had a limit.
    shape_refused(
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            nest("not", MAX_PREDICATE_DEPTH),
        ),
        "recursion limit exceeded at line 1 column 1442",
    );
}

cases! {
    shape_refused;
    a_misspelled_predicate_key_is_refused_with_the_keys_it_could_be => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!({"way": {"set": 0, "way": 0}, "vaild": true}),
        ),
        "unknown field `vaild`, expected one of `way`, `valid`, `tag`, `dirty`, `rank`, \
         `line`, `present`, `memory`, `equals`, `counter`, `op`, `value`, `register`, `all`, \
         `any`, `not`, `event`, `lastAccess`, `config` at line 1 column 581",
    ),
    a_predicate_of_two_kinds_is_refused_naming_both => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!({"way": {"set": 0, "way": 0}, "line": {"addr": 0}}),
        ),
        "a predicate needs exactly one of `way`, `line`, `memory`, `counter`, `register`, \
         `all`, `any`, `not`, `event`, `lastAccess`, `config`; found way, line at line 1 \
         column 616",
    ),
    an_empty_predicate_is_refused_naming_the_kinds_it_could_be => (
        &with(MISS_THEN_HIT, "/steps/1/expect", json!({})),
        "a predicate needs exactly one of `way`, `line`, `memory`, `counter`, `register`, \
         `all`, `any`, `not`, `event`, `lastAccess`, `config`; found none at line 1 column 575",
    ),
    a_field_not_allowed_in_a_line_predicate_is_refused => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!({"line": {"addr": 0}, "rank": 1}),
        ),
        "`rank` is not allowed in a `line` predicate (allowed: present, dirty) at line 1 \
         column 601",
    ),
    a_field_not_allowed_in_an_all_predicate_is_refused => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!({"all": [], "valid": true}),
        ),
        "`valid` is not allowed in an `all` predicate (allowed: none) at line 1 column 596",
    ),
    a_memory_predicate_without_equals_is_refused => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!({"memory": {"addr": 0}}),
        ),
        "a `memory` predicate needs `equals` at line 1 column 594",
    ),
    a_counter_predicate_without_a_value_is_refused => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!({"counter": "misses", "op": "==", "value": null}),
        ),
        "a `counter` predicate needs `value` at line 1 column 616",
    ),
    an_unknown_register_is_refused_with_the_registers => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!({"register": "r8", "equals": 0}),
        ),
        "unknown variant `r8`, expected one of `r0`, `r1`, `r2`, `r3`, `r4`, `r5`, `r6`, `r7` \
         at line 1 column 600",
    ),
    an_unknown_comparison_is_refused_with_the_comparisons => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!({"counter": "misses", "op": "!=", "value": 0}),
        ),
        "unknown variant `!=`, expected one of `==`, `>=`, `<=` at line 1 column 602",
    ),
    // A count, as the counters are: a module read back across the
    // boundary never holds a number JavaScript cannot represent.
    a_count_beyond_a_javascript_number_is_refused => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!({"counter": "misses", "op": "==", "value": 9_007_199_254_740_993_u64}),
        ),
        "invalid value: integer `9007199254740993`, expected u32 at line 1 column 627",
    ),
    a_malformed_hex_address_is_refused_with_the_forms_it_takes => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!({"lastAccess": {"addr": "0x0G4"}}),
        ),
        "invalid value: string \"0x0G4\", expected an address: a number, or hex after 0x such \
         as \"0x0A4\" at line 1 column 602",
    ),
    an_address_without_its_prefix_is_refused => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!({"line": {"addr": "0A4"}}),
        ),
        "invalid value: string \"0A4\", expected an address: a number, or hex after 0x such as \
         \"0x0A4\" at line 1 column 594",
    ),
}

#[test]
fn a_predicate_given_as_a_sequence_reads_like_its_map_form() {
    assert_eq!(
        predicate(json!([{"set": 0, "way": 0}])),
        predicate(json!({"way": {"set": 0, "way": 0}})),
    );
}

cases! {
    shape_refused;
    an_empty_predicate_sequence_is_refused_as_an_empty_map_is => (
        &with(MISS_THEN_HIT, "/steps/1/expect", json!([])),
        "a predicate needs exactly one of `way`, `line`, `memory`, `counter`, `register`, \
         `all`, `any`, `not`, `event`, `lastAccess`, `config`; found none at line 1 column 575",
    ),
    a_not_of_an_empty_sequence_is_refused => (
        &with(MISS_THEN_HIT, "/steps/1/expect", json!({"not": []})),
        "a predicate needs exactly one of `way`, `line`, `memory`, `counter`, `register`, \
         `all`, `any`, `not`, `event`, `lastAccess`, `config`; found none at line 1 column 583",
    ),
    an_all_of_an_empty_sequence_is_refused => (
        &with(MISS_THEN_HIT, "/steps/1/expect", json!({"all": [[]]})),
        "a predicate needs exactly one of `way`, `line`, `memory`, `counter`, `register`, \
         `all`, `any`, `not`, `event`, `lastAccess`, `config`; found none at line 1 column 584",
    ),
    a_null_in_a_predicate_sequence_is_refused_as_a_way_without_its_way => (
        &with(MISS_THEN_HIT, "/steps/1/expect", json!([null])),
        "a `way` predicate needs `way` at line 1 column 579",
    ),
    a_counter_in_a_predicate_sequence_is_refused_as_an_unknown_way_field => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/expect",
            json!([{"counter": "misses", "op": "==", "value": 0}]),
        ),
        "unknown field `counter`, expected `set` or `way` at line 1 column 584",
    ),
}

#[test]
fn a_module_cut_short_is_refused_with_its_place() {
    match Module::parse("{") {
        Err(ModuleError::Json { message }) => assert!(message.contains("EOF while parsing")),
        other => panic!("{other:?}"),
    }
}

cases! {
    shape_refused;
    a_show_me_address_without_its_prefix_is_refused => (
        &with(MISS_THEN_HIT, "/steps/1/showMe/0/addr", json!("A4")),
        "invalid value: string \"A4\", expected an address: a number, or hex after 0x such as \
         \"0x0A4\" at line 1 column 746",
    ),
    an_unknown_show_me_op_is_refused_with_the_ops => (
        &with(MISS_THEN_HIT, "/steps/1/showMe/0", json!({"op": "stpe"})),
        "unknown variant `stpe`, expected one of `config`, `issue`, `program`, `step`, \
         `stepAccess`, `run`, `edit` at line 1 column 719",
    ),
    an_until_on_an_instruct_step_is_refused => (
        &with(
            MISS_THEN_HIT,
            "/steps/1/until",
            json!({"counter": "misses", "op": "==", "value": 0}),
        ),
        "unexpected field `until` for step `instruct` at line 1 column 834",
    ),
    an_instruct_step_without_its_expect_is_refused => (
        &with(MISS_THEN_HIT, "/steps/1/expect", Value::Null),
        "missing field `expect` for step `instruct` at line 1 column 744",
    ),
    a_hint_on_an_explain_step_is_refused => (
        &with(MISS_THEN_HIT, "/steps/0/hint", json!("a hint")),
        "unexpected field `hint` for step `explain` at line 1 column 557",
    ),
    an_unknown_step_kind_is_refused_with_the_kinds => (
        &with(MISS_THEN_HIT, "/steps/0/kind", json!("quiz")),
        "unknown variant `quiz`, expected one of `instruct`, `predict`, `observe`, `explain` \
         at line 1 column 432",
    ),
    an_unknown_callout_area_is_refused_with_the_areas => (
        &with(MISS_THEN_HIT, "/steps/0/callout", json!("table")),
        "unknown variant `table`, expected one of `addressStrip`, `cache`, `memory`, `log`, \
         `stats`, `config`, `cpu`, `transport` at line 1 column 411",
    ),
    an_unknown_callout_cell_field_is_refused => (
        &with(
            MISS_THEN_HIT,
            "/steps/0/callout",
            json!({"set": 1, "row": 2}),
        ),
        "unknown field `row`, expected `set` or `way` at line 1 column 410",
    ),
    a_callout_that_is_neither_area_nor_cell_is_refused => (
        &with(MISS_THEN_HIT, "/steps/0/callout", json!(3)),
        "invalid type: integer `3`, expected an area such as \"cache\", or a cell such as \
         {\"set\": 2, \"way\": 0} at line 1 column 405",
    ),
    an_unknown_module_field_is_refused_with_the_fields => (
        &with(MISS_THEN_HIT, "/theme", json!("dark")),
        "unknown field `theme`, expected one of `id`, `title`, `description`, `advanced`, \
         `preset`, `program`, `steps` at line 1 column 1856",
    ),
}

#[test]
fn optional_fields_may_be_null() {
    let module = Module::parse(&with(MISS_THEN_HIT, "/steps/1/hint", Value::Null)).unwrap();
    assert!(matches!(
        &module.steps()[1],
        Step::Instruct { hint: None, .. }
    ));
    let module = Module::parse(&with(
        MISS_THEN_HIT,
        "/steps/4/expect/all/3",
        json!({"way": {"set": 2, "way": 0}, "valid": true, "tag": null}),
    ))
    .unwrap();
    let written = serde_json::to_value(&module).unwrap();
    assert_eq!(
        written["steps"][4]["expect"]["all"][3],
        json!({"way": {"set": 2, "way": 0}, "valid": true})
    );
}

#[test]
fn predicates_serialize_keyed_by_which_field_they_have() {
    let shapes = [
        json!({"way": {"set": 2, "way": 0}, "valid": true, "tag": 1, "dirty": false, "rank": 0}),
        json!({"line": {"addr": 164}, "present": true}),
        json!({"memory": {"addr": 164}, "equals": 18_446_744_073_709_551_615_u64}),
        json!({"counter": "writearounds", "op": "<=", "value": 3}),
        json!({"counter": "cycles", "op": ">=", "value": u32::MAX}),
        json!({"register": "r7", "equals": 5}),
        json!({"all": [{"any": [{"not": {"event": {"kind": "memWrite", "entry": 2}}}]}]}),
        json!({"lastAccess": {"kind": "write", "addr": 164, "outcome": "hit"}}),
        json!({"config": {"sizeBytes": 256, "lineBytes": 32, "associativity": 4,
                          "replacement": "random", "writePolicy": "writeThrough",
                          "allocatePolicy": "writeNoAllocate", "timed": true}}),
    ];
    for shape in shapes {
        let read = predicate(shape.clone());
        assert_eq!(serde_json::to_value(&read).unwrap(), shape, "{shape}");
    }
    // Every counter name reads, the added ones included.
    for counter in [
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
    ] {
        predicate(json!({"counter": counter, "op": "==", "value": 0}));
    }
    // An address may be a hex string, in either case of `0x`.
    assert_eq!(
        predicate(json!({"line": {"addr": "0X0a4"}})),
        predicate(json!({"line": {"addr": 164}}))
    );
}

#[test]
fn the_index_lists_the_modules() {
    let index = json!([
        {"id": "01-fixture-miss-then-hit", "title": "Fixture: miss then hit",
         "description": "A first access *misses*.", "advanced": false},
        {"id": "02-fixture-switch-config", "title": "Fixture: conflicts, then two ways",
         "description": "`0x000` and `0x080` conflict.", "advanced": false},
        {"id": "03-fixture-stale-memory", "title": "Fixture: stale memory",
         "description": "Write-back leaves memory stale.", "advanced": true}
    ]);
    let view = parse_module_index(&index.to_string()).unwrap();
    assert_eq!(view.summary, "3 modules \u{b7} none running");
    assert_eq!(view.modules.len(), 3);
    let card = &view.modules[2];
    assert_eq!(card.number, "03");
    assert!(card.advanced);
    assert_eq!(card.title, "Fixture: stale memory");
    assert_eq!(
        serde_json::to_value(&view.modules[0]).unwrap(),
        json!({
            "id": "01-fixture-miss-then-hit", "number": "01", "title": "Fixture: miss then hit",
            "description": [
                {"text": "A first access ", "style": "plain"},
                {"text": "misses", "style": "emphasis"},
                {"text": ".", "style": "plain"}
            ],
            "advanced": false
        })
    );
    // The index agrees with the fixture files.
    for (row, json) in view.modules.iter().zip(FIXTURES) {
        let module = Module::parse(json).unwrap();
        assert_eq!(row.id, module.id(), "{}", row.id);
        assert_eq!(row.title, module.title(), "{}", row.id);
        assert_eq!(row.advanced, module.advanced(), "{}", row.id);
    }
    let one = parse_module_index(&json!([index[0].clone()]).to_string()).unwrap();
    assert_eq!(one.summary, "1 module \u{b7} none running");
    let refused = |value: Value| {
        parse_module_index(&value.to_string())
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        refused(json!([index[0].clone(), index[0].clone()])),
        "[1].id: `01-fixture-miss-then-hit` is listed more than once."
    );
    let mut bad = index.clone();
    bad[1]["id"] = json!("2-x");
    assert!(refused(bad).starts_with("[1].id: an id is two digits"));
    let mut bad = index.clone();
    bad[1]["title"] = json!(" ");
    assert_eq!(refused(bad), "[1].title: the title is empty.");
    let mut bad = index.clone();
    bad[2]["description"] = json!("_stale_");
    assert!(
        refused(bad).starts_with("[2].description: an underscore outside `code` at character 1")
    );
    let mut bad = index;
    bad[0]["order"] = json!(1);
    assert!(refused(bad).starts_with("unknown field `order`, expected one of `id`, `title`"));
}

#[test]
fn the_fixtures_cover_every_step_kind_and_predicate() {
    let mut kinds = Vec::new();
    let mut predicates = Vec::new();
    let mut switched = false;
    for json in FIXTURES {
        for step in Module::parse(json).unwrap().steps() {
            match step {
                Step::Instruct {
                    expect, show_me, ..
                } => {
                    kinds.push("instruct");
                    walk(expect, &mut predicates);
                    switched |= show_me
                        .iter()
                        .flatten()
                        .any(|op| matches!(op, Op::Config { .. }));
                }
                Step::Predict { then, .. } => {
                    kinds.push("predict");
                    switched |= then
                        .iter()
                        .flatten()
                        .any(|op| matches!(op, Op::Config { .. }));
                }
                Step::Observe { until, .. } => {
                    kinds.push("observe");
                    walk(until, &mut predicates);
                }
                Step::Explain { .. } => kinds.push("explain"),
            }
        }
    }
    for kind in ["instruct", "predict", "observe", "explain"] {
        assert!(kinds.contains(&kind), "{kind}");
    }
    for name in [
        "way",
        "line",
        "memory",
        "counter",
        "register",
        "all",
        "any",
        "not",
        "event",
        "lastAccess",
        "config",
    ] {
        assert!(predicates.contains(&name), "{name}");
    }
    assert!(switched);
}

#[test]
fn replacing_the_session_ends_the_module() {
    // The gate lives in the session a prediction gated: a session opened
    // from its link or its file, or a fresh one, has none and takes ops.
    let (mut run, mut session) = started(SWITCH_CONFIG);
    run.show_me(&mut session);
    assert_eq!(session.apply(load(0x0A4)), Err(OpError::Gated));
    // Undo is allowed under the gate, and the cursor is then saved too.
    assert!(session.undo());
    let opened = [
        Session::from_share_string(&session.to_share_string().unwrap()).unwrap(),
        Session::from_json(&session.to_json()).unwrap(),
        Session::with_config(Config::first_visit()).unwrap(),
    ];
    for (index, mut replaced) in opened.into_iter().enumerate() {
        assert!(replaced.gate().is_none(), "session {index}");
        let controls = replaced.state().controls;
        assert!(
            controls.seek_limit.is_none() && controls.guard.is_none(),
            "session {index}"
        );
        replaced.apply(load(0x0A4)).unwrap();
    }
    // Progress is not in the share document: it holds the log alone.
    let document: Value = serde_json::from_str(&session.to_json()).unwrap();
    let mut keys: Vec<&str> = document
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["at", "config", "ops", "schema_version"]);
    // The old session stays gated until its run lifts the gate.
    assert_eq!(session.apply(load(0x0A4)), Err(OpError::Gated));
}

#[test]
fn a_right_answer_applies_the_then_ops_where_the_prediction_began() {
    let (mut run, mut session) = started(SWITCH_CONFIG);
    run.show_me(&mut session);
    let limit = session.gate().unwrap().seek_limit();
    let ops = session.ops().to_vec();
    // The student undoes the trace's run before answering.
    assert!(session.undo());
    assert!(session.position().pos < limit);
    run.select(0);
    run.check(&mut session);
    // The undone run is kept, and the `then` ops follow it rather than the
    // op the cursor had gone back to.
    assert_eq!(session.ops()[..ops.len()], ops[..]);
    assert!(matches!(session.ops()[ops.len()], Op::Config { .. }));
    assert_eq!(session.ops()[ops.len() + 1], Op::Run { n: 4 });
    assert_eq!(session.op_count() as usize, ops.len() + 2);
    assert_eq!(session.position().boundary, Boundary::LogEnd);
}

#[test]
fn ops_the_state_refuses_leave_the_step_waiting() {
    // "Show me" and `then` ops are checked for their form when the module
    // is read; one the state refuses when it is applied, here a step with
    // no program loaded, appends nothing.
    let json = with(
        &with(MISS_THEN_HIT, "/steps/1/showMe", json!([{"op": "step"}])),
        "/steps/3/then",
        json!([{"op": "run", "n": 2}]),
    );
    let (mut run, mut session) = started(&json);
    let instruct = run.next(&mut session);
    let ops = session.op_count();
    let waiting = run.show_me(&mut session);
    assert_eq!(waiting.step_index, instruct.step_index);
    assert_eq!(session.op_count(), ops);
    assert_eq!(waiting.notice, Some(OpError::NoProgram.to_string()));
    // Done by hand, the step completes.
    session.apply(load(0x0A4)).unwrap();
    run.sync(&mut session);
    run.show_me(&mut session);
    let predict = run.view(&session);
    assert_eq!(predict.kind, ModuleStepKind::Predict);
    // The new step clears the notice the refused "Show me" left.
    assert_eq!(predict.notice, None);
    let before = session.position();
    let ops = session.op_count();
    // A right answer lifts the gate; the refused `then` ops append nothing,
    // and the cursor stays where the student left it.
    run.select(0);
    let answered = run.check(&mut session);
    assert_eq!(answered.feedback.unwrap().tone, FeedbackTone::Correct);
    assert!(session.gate().is_none());
    assert_eq!(session.op_count(), ops);
    assert_eq!(session.position(), before);
    assert_eq!(answered.notice, Some(OpError::NoProgram.to_string()));
    let advanced = run.next(&mut session);
    assert_eq!(advanced.kind, ModuleStepKind::Instruct);
    // The new step clears the notice the refused `then` left.
    assert_eq!(advanced.notice, None);
}

#[test]
fn show_me_refused_at_the_link_limit_leaves_a_notice() {
    let (mut run, mut session) = started(MISS_THEN_HIT);
    let instruct = run.next(&mut session);
    fill(&mut session, 0x400, MAX_SHARE_OPS);
    let ops = session.op_count();
    let waiting = run.show_me(&mut session);
    assert_eq!(waiting.step_index, instruct.step_index);
    assert_eq!(session.op_count(), ops);
    assert_eq!(waiting.notice, Some(OpError::LinkFull.to_string()));
}

#[test]
fn restart_refused_at_the_link_limit_leaves_a_notice() {
    let (mut run, mut session) = started(MISS_THEN_HIT);
    show_me_path(&mut run, &mut session);
    assert_eq!(run.view(&session).kind, ModuleStepKind::Done);
    fill(&mut session, 0x400, MAX_SHARE_OPS);
    let ops = session.op_count();
    let restarted = run.next(&mut session);
    assert_eq!(restarted.kind, ModuleStepKind::Done);
    assert_eq!(session.op_count(), ops);
    assert_eq!(restarted.notice, Some(OpError::LinkFull.to_string()));
}

#[test]
fn a_correct_answers_then_ops_refused_at_the_link_limit_leave_a_notice() {
    // Room for the switch's opening `Config` and `Program`, the instruct
    // step's `run 4`, and one more op, but not the two the prediction's
    // `then` needs.
    let mut session = session();
    fill(&mut session, 0x400, MAX_SHARE_OPS - 4);
    let mut run = ModuleRun::new(Module::parse(SWITCH_CONFIG).unwrap());
    run.start(&mut session).unwrap();
    run.show_me(&mut session);
    let ops = session.op_count();
    run.select(0);
    let answered = run.check(&mut session);
    assert_eq!(answered.feedback.unwrap().tone, FeedbackTone::Correct);
    assert!(session.gate().is_none());
    assert_eq!(session.op_count(), ops);
    assert_eq!(answered.notice, Some(OpError::LinkFull.to_string()));
}

#[test]
fn a_wrong_answer_without_a_hint_reads_the_explanation() {
    let run = ModuleRun::from_json(STALE_MEMORY).unwrap();
    assert_eq!(run.module().id(), "03-fixture-stale-memory");
    let (mut run, mut session) = started(STALE_MEMORY);
    run.show_me(&mut session);
    run.show_me(&mut session);
    let predict = run.view(&session);
    assert_eq!(predict.kind, ModuleStepKind::Predict);
    assert_eq!(
        predict.callout,
        Some(Callout::Cell(CellRef {
            set: serde_json::from_value(json!(2)).unwrap(),
            way: Some(serde_json::from_value(json!(0)).unwrap()),
        }))
    );
    run.select(1);
    let wrong = run.check(&mut session).feedback.unwrap();
    assert_eq!(wrong.head, "Not quite.");
    assert_eq!(
        text(&wrong.text),
        "The dirty line is written back, and its 7 replaces the edited 9."
    );
    // Checking the same wrong answer again counts again; Show me stays.
    let again = run.check(&mut session);
    assert_eq!(
        again.feedback.clone().unwrap().tone,
        FeedbackTone::Incorrect
    );
    assert!(actions(&again).contains(&(ModuleActionKind::ShowMe, true)));
    assert!(matches!(
        ModuleRun::from_json("{}"),
        Err(ModuleError::Json { .. })
    ));
}
