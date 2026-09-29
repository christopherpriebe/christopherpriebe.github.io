//! Edits: every rule that plans or refuses an edit of a way or a memory
//! word, in order, with its wording and its offer, each offer planning and
//! applying; the session's refusal while the cursor is inside an op, an
//! access, several instructions, an op that ended in an error, or a new
//! run's setup before its error; ranks under each policy and in one-way
//! sets; the editors' drafts, with their minimal patches, fields read, word
//! prefill and the way they would leave; applied edits as ops, events and
//! log entries, refused ops and replay; and the design's editor states 4a to
//! 4e, staged at the end of access #4 of frame 3b's program. The session's
//! refusal at its event limit is tested with that limit, in session.rs.

#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use cache_explorer_core::{
    AccessKind, CacheConfig, ChangeRef, Config, Consequence, EditOutcome, EntryKind, EntryOutcome,
    EventKind, FieldInput, Op, OpError, Preset, RefusalReason, Replacement, ReplayError, Session,
    StepOutcome, Tone, WayForm, WordMark, WritePolicy,
};

use common::{addr, line, program, session, word};
use support::{
    applied, draft, draft_of_line, draft_outcome, edit, form, frame_3b_at_access_4, memory, offer,
    plan, ranks, refusal, said, set, two_way, value, way, way_index, way_patch,
};

#[cfg(test)]
mod common;

/// The tests' helpers. They sit in a module compiled for tests only, where a
/// failed `unwrap` is a failed test, as in the tests themselves.
#[cfg(test)]
mod support {
    use cache_explorer_core::golden::FRAME_3B;
    use cache_explorer_core::{
        AccessKind, CacheConfig, Config, EditDraft, EditOutcome, EditPatch, EditPlan, EditRefusal,
        EditTarget, Op, Preset, Replacement, Session, SetIndex, WayForm, WayIndex, WritePolicy,
    };
    use serde_json::{json, Value};

    use super::common::{addr, program, session};

    /// `value` as a set index, as one read from JSON is.
    pub(crate) fn set(value: u32) -> SetIndex {
        serde_json::from_value(value.into()).unwrap()
    }

    /// Way `way` of set `set`, as the UI sends a target.
    pub(crate) fn way(set: u32, way: u32) -> EditTarget {
        serde_json::from_value(json!({"kind": "way", "set": set, "way": way})).unwrap()
    }

    /// The memory word at `at`, as the UI sends a target.
    pub(crate) fn memory(at: u32) -> EditTarget {
        serde_json::from_value(json!({"kind": "memory", "addr": at})).unwrap()
    }

    /// A way's patch with the fields of `fields`, a JSON object.
    pub(crate) fn way_patch(fields: Value) -> EditPatch {
        let mut fields = fields;
        fields["kind"] = json!("way");
        serde_json::from_value(fields).unwrap()
    }

    /// A memory word's patch to `value`.
    pub(crate) fn value(value: u64) -> EditPatch {
        serde_json::from_value(json!({"kind": "memory", "value": value})).unwrap()
    }

    /// The way editor's form.
    pub(crate) fn form(
        valid: bool,
        line_text: &str,
        word_texts: &[&str],
        dirty: bool,
        rank: Option<u32>,
    ) -> WayForm {
        WayForm {
            valid,
            line_text: line_text.to_owned(),
            word_texts: word_texts.iter().map(|text| (*text).to_owned()).collect(),
            dirty,
            rank,
        }
    }

    /// An edit op.
    pub(crate) fn edit(target: EditTarget, patch: EditPatch) -> Op {
        Op::Edit { target, patch }
    }

    /// Loads by hand of each of `at`, on `config`.
    pub(crate) fn loads(config: Config, at: &[u32]) -> Session {
        session(
            config,
            at.iter().map(|&at| Op::Issue {
                kind: AccessKind::Read,
                addr: addr(at),
                value: None,
            }),
        )
    }

    /// Frame 3b's program on `two-way-lru`, stepped to the end of access
    /// #4, where the design's editor states are staged: set 2 holds 0x0A0,
    /// clean, in way 0 at rank 0 and 0x1A0, dirty with 7 at 0x1A4 (memory
    /// 12), in way 1; set 3 holds 0x0B0, dirty with 5 at 0x0B0 (memory 92),
    /// in way 0; sets 0 and 1 are empty. Its events end at position 21.
    pub(crate) fn frame_3b_at_access_4() -> Session {
        let mut session = session(Preset::TwoWayLru.config(), [program(FRAME_3B)]);
        for _ in 0..4 {
            session.apply(Op::Step).unwrap();
        }
        session
    }

    /// The plan of `patch` on `target` at the cursor, which must plan. Its
    /// normalised patch plans the same way again.
    #[track_caller]
    pub(crate) fn plan(session: &Session, target: EditTarget, patch: &EditPatch) -> EditPlan {
        let plan = session.plan_edit(&target, patch).unwrap();
        let again = session.plan_edit(&plan.target, &plan.patch).unwrap();
        assert_eq!(again.changes, plan.changes, "{patch:?}");
        assert_eq!(again.consequences, plan.consequences, "{patch:?}");
        plan
    }

    /// The refusal of `patch` on `target` at the cursor, which must be
    /// refused. Its offer, if any, plans with the preview it shows, and
    /// applies, leaving the machine's invariants whole.
    #[track_caller]
    pub(crate) fn refusal(session: &Session, target: EditTarget, patch: &EditPatch) -> EditRefusal {
        let refusal = session.plan_edit(&target, patch).unwrap_err();
        if let Some(offer) = &refusal.suggestion {
            let plan = session.plan_edit(&offer.target, &offer.patch).unwrap();
            assert_eq!(plan.preview, offer.preview);
            let mut copy = session.clone();
            copy.apply(Op::Edit {
                target: offer.target,
                patch: offer.patch.clone(),
            })
            .unwrap();
            assert_eq!(copy.machine().check_invariants(), Ok(()));
        }
        refusal
    }

    /// `session` after `op`, which must be accepted.
    #[track_caller]
    pub(crate) fn applied(session: &Session, op: Op) -> Session {
        let mut session = session.clone();
        session.apply(op).unwrap();
        assert_eq!(session.machine().check_invariants(), Ok(()));
        session
    }

    /// The way editor's draft of `form` for way `way` of set `set`.
    #[track_caller]
    pub(crate) fn draft(session: &Session, set: u32, way: u32, form: &WayForm) -> EditDraft {
        let EditTarget::Way { set, way } = self::way(set, way) else {
            panic!("a way target is a way");
        };
        session.draft_edit(set, way, form)
    }

    /// The texts of a refusal: its title and text.
    pub(crate) fn said(refusal: &EditRefusal) -> (&str, &str) {
        (refusal.title.as_str(), refusal.text.as_str())
    }

    /// The label of a refusal's offer, if it makes one.
    pub(crate) fn offer(refusal: &EditRefusal) -> Option<&str> {
        refusal
            .suggestion
            .as_ref()
            .map(|suggestion| suggestion.action.as_str())
    }

    /// A two-way LRU cache like `two-way-lru` under `write_policy` and
    /// `replacement`.
    pub(crate) fn two_way(write_policy: WritePolicy, replacement: Replacement) -> Config {
        let base = Preset::TwoWayLru.config();
        Config {
            cache: CacheConfig {
                write_policy,
                replacement,
                ..base.cache
            },
            ..base
        }
    }

    /// The ranks of set `set` at the cursor, in way order.
    pub(crate) fn ranks(session: &Session, set: usize) -> Vec<u32> {
        session.state().cache.sets[set]
            .ways
            .iter()
            .map(|way| way.rank.map_or(u32::MAX, |rank| rank.value))
            .collect()
    }

    /// The draft of set 3, way 0 given the line `text`, the other fields as
    /// the editor opened them.
    pub(crate) fn draft_of_line(session: &Session, text: &str) -> EditDraft {
        draft(
            session,
            3,
            0,
            &form(true, text, &["", "", "", ""], true, None),
        )
    }

    /// `value` as a way index, as one read from JSON is.
    pub(crate) fn way_index(value: u32) -> WayIndex {
        serde_json::from_value(value.into()).unwrap()
    }

    /// The outcome of the draft of `form` for way `way` of set `set`.
    pub(crate) fn draft_outcome(
        session: &Session,
        set: u32,
        way: u32,
        form: &WayForm,
    ) -> EditOutcome {
        draft(session, set, way, form).outcome
    }
}

// ── Editor states 4a–4e ─────────────────────────────────────────────────

#[test]
fn state_4a_edits_a_word_the_dirty_bit_and_the_rank_of_a_clean_way() {
    let session = frame_3b_at_access_4();
    let form = form(true, "0x0A0", &["40", "9", "66", "79"], true, Some(1));
    let draft = draft(&session, 2, 0, &form);
    let patch = way_patch(serde_json::json!({"data": [40, 9, 66, 79], "dirty": true, "rank": 1}));
    assert_eq!(draft.patch.as_ref(), Some(&patch), "only the edited fields");
    let EditOutcome::Plan(plan) = &draft.outcome else {
        panic!("4a plans: {:?}", draft.outcome);
    };
    assert_eq!(plan, &self::plan(&session, way(2, 0), &patch));
    assert_eq!(plan.tone, Tone::Ready);
    assert_eq!(plan.title, "Ready to apply");
    assert_eq!(
        plan.text,
        "Ranks stay a permutation: the other ways shift to make room."
    );
    assert_eq!(
        plan.preview,
        [
            "word 1: 53 \u{2192} 9",
            "dirty 0 \u{2192} 1",
            "rank way 0: 0 \u{2192} 1",
            "rank way 1: 1 \u{2192} 0",
        ]
    );
    assert_eq!(
        plan.consequences,
        [
            Consequence::BecomesStale { addr: addr(0x0A4) },
            Consequence::Rerank {
                set: set(2),
                way: way_index(0),
                from: 0,
                to: 1
            },
            Consequence::Rerank {
                set: set(2),
                way: way_index(1),
                from: 1,
                to: 0
            },
        ]
    );
    let changes: Vec<(String, &str, &str)> = plan
        .changes
        .iter()
        .map(|change| {
            (
                serde_json::to_string(&change.cell).unwrap(),
                change.from.as_str(),
                change.to.as_str(),
            )
        })
        .collect();
    assert_eq!(
        changes,
        [
            (r#"{"cell":"dirty","set":2,"way":0}"#.to_owned(), "0", "1"),
            (r#"{"cell":"rank","set":2,"way":0}"#.to_owned(), "0", "1"),
            (r#"{"cell":"rank","set":2,"way":1}"#.to_owned(), "1", "0"),
            (
                r#"{"cell":"word","set":2,"way":0,"word":1}"#.to_owned(),
                "53",
                "9"
            ),
        ]
    );
    assert_eq!(plan.action, "Apply");
    assert_eq!(
        plan.word_marks,
        [
            WordMark::Same,
            WordMark::ChangedAndDiffers,
            WordMark::Same,
            WordMark::Same
        ]
    );
    // The way as it would be, and the inputs as the draft reads them.
    assert_eq!(draft.result.words, ["40", "9", "66", "79"]);
    assert!(draft.result.valid && draft.result.dirty);
    assert_eq!(draft.result.rank.map(|rank| rank.value), Some(1));
    assert_eq!(draft.word_texts, ["40", "53", "66", "79"]);
    assert_eq!(draft.tag_text.as_deref(), Some("0x02"));
    assert_eq!(draft.index_text.as_deref(), Some("2"));
    assert_eq!(
        draft.line,
        FieldInput::Valid {
            text: "0x0A0".to_owned()
        }
    );
}

#[test]
fn state_4a_applied_is_one_edit_event_that_leaves_a_word_stale() {
    let session = frame_3b_at_access_4();
    let patch = way_patch(serde_json::json!({"data": [40, 9, 66, 79], "dirty": true, "rank": 1}));
    let plan = plan(&session, way(2, 0), &patch);
    let after = applied(&session, edit(way(2, 0), patch));
    let event = after.events().last().unwrap();
    assert_eq!(event.kind, EventKind::Edit);
    assert_eq!(
        event.explain,
        "Edited set 2, way 0: word 1: 53 \u{2192} 9; dirty 0 \u{2192} 1; rank 0 \u{2192} 1 \
         (way 1: 1 \u{2192} 0)."
    );
    let cells: Vec<ChangeRef> = plan.changes.iter().map(|change| change.cell).collect();
    assert_eq!(event.changed, cells);
    assert_eq!(ranks(&after, 2), [1, 0]);
    let stale = after.machine().stale(addr(0x0A4)).unwrap();
    assert_eq!((stale.cached, stale.memory), (word(9), word(53)));
}

#[test]
fn state_4b_asks_for_the_dirty_bit_when_a_clean_line_would_differ_from_memory() {
    let session = frame_3b_at_access_4();
    let form = form(true, "0x0B0", &["5", "8", "40", "34"], false, None);
    let draft = draft(&session, 3, 0, &form);
    let patch = way_patch(serde_json::json!({"data": [5, 8, 40, 34], "dirty": false}));
    assert_eq!(draft.patch.as_ref(), Some(&patch));
    let refusal = refusal(&session, way(3, 0), &patch);
    assert_eq!(draft.outcome, EditOutcome::Refusal(refusal.clone()));
    assert_eq!(refusal.reason, RefusalReason::NeedsDirty);
    assert_eq!(refusal.tone, Tone::Caution);
    // It names the word edited, word 2, although word 0 differs too.
    assert_eq!(
        said(&refusal),
        (
            "Data differs from memory",
            "Word 2 is 40 here but 21 in memory. A clean line must match memory, so this edit \
             needs the dirty bit."
        )
    );
    let suggestion = refusal.suggestion.as_ref().unwrap();
    assert_eq!(suggestion.action, "Set dirty and apply");
    assert_eq!(suggestion.target, way(3, 0));
    assert_eq!(
        suggestion.patch,
        way_patch(serde_json::json!({"data": [5, 8, 40, 34], "dirty": true}))
    );
    assert_eq!(suggestion.preview, ["word 2: 21 \u{2192} 40"]);
    assert_eq!(
        refusal.word_marks,
        [
            WordMark::DiffersFromMemory,
            WordMark::Same,
            WordMark::ChangedAndDiffers,
            WordMark::Same
        ]
    );
}

#[test]
fn state_4c_offers_the_empty_way_of_the_lines_own_set_and_leaves_the_way_alone() {
    let session = frame_3b_at_access_4();
    // The data inputs the student has not typed in are sent empty, and read
    // as memory's copy of the line typed, which the draft shows.
    let form = form(true, "0x0C0", &["", "", "", ""], true, None);
    let draft = draft(&session, 3, 0, &form);
    assert_eq!(draft.word_texts, ["47", "60", "73", "86"]);
    assert_eq!(draft.tag_text.as_deref(), Some("0x03"));
    assert_eq!(draft.index_text.as_deref(), Some("0"));
    let patch = way_patch(serde_json::json!({"line": 0x0C0, "dirty": true}));
    assert_eq!(draft.patch.as_ref(), Some(&patch));
    let refusal = refusal(&session, way(3, 0), &patch);
    assert_eq!(draft.outcome, EditOutcome::Refusal(refusal.clone()));
    assert_eq!(refusal.reason, RefusalReason::WrongSet);
    assert_eq!(refusal.tone, Tone::Move);
    assert_eq!(
        said(&refusal),
        (
            "0x0C0 belongs in set 0",
            "Its index bits are 00, so it cannot live in set 3. Set 0 has an empty way 0."
        )
    );
    let suggestion = refusal.suggestion.as_ref().unwrap();
    assert_eq!(suggestion.action, "Move to set 0, way 0");
    assert_eq!(suggestion.target, way(0, 0));
    assert_eq!(
        suggestion.patch,
        way_patch(serde_json::json!({"valid": true, "line": 0x0C0, "dirty": true}))
    );
    assert_eq!(
        suggestion.preview,
        [
            "valid 0 \u{2192} 1",
            "tag \u{2014} \u{2192} 0x03",
            "dirty 0 \u{2192} 1"
        ]
    );
    // The suggestion is an edit of its own: the original way keeps its
    // dirty line, so nothing is dropped.
    let before = session.state().cache.sets[3].ways[0].clone();
    let after = applied(&session, edit(suggestion.target, suggestion.patch.clone()));
    assert_eq!(after.state().cache.sets[3].ways[0], before);
    let moved = &after.state().cache.sets[0].ways[0];
    assert_eq!(moved.line_text.as_deref(), Some("0x0C0"));
    assert!(moved.dirty);
    // The way the student opened is shown as it is, since the line typed
    // cannot live there.
    assert_eq!(draft.result, before);
}

#[test]
fn state_4d_refuses_a_line_whose_own_set_is_full() {
    let session = frame_3b_at_access_4();
    let form = form(true, "0x120", &["", "", "", ""], true, None);
    let draft = draft(&session, 3, 0, &form);
    let EditOutcome::Refusal(refusal) = &draft.outcome else {
        panic!("4d is refused: {:?}", draft.outcome);
    };
    assert_eq!(refusal.reason, RefusalReason::WrongSetFull);
    assert_eq!(refusal.tone, Tone::Refused);
    assert_eq!(
        said(refusal),
        (
            "Refused",
            "0x120 has index 2, so it can only live in set 2, and every way of set 2 is valid. \
             Evict a line there first, or choose an address with index 3."
        )
    );
    assert_eq!(refusal.suggestion, None);
    // The design's 0x1A0 is still in set 2, way 1 here, so another rule
    // speaks first.
    let design = draft_of_line(&session, "0x1A0");
    let EditOutcome::Refusal(design) = design.outcome else {
        panic!("0x1A0 is refused");
    };
    assert_eq!(design.reason, RefusalReason::DuplicateLine);
    assert_eq!(
        said(&design),
        (
            "0x1A0 belongs in set 2",
            "0x1A0 is already in set 2, way 1."
        )
    );
}

#[test]
fn state_4e_warns_that_a_word_behind_a_dirty_line_stays_stale() {
    let session = frame_3b_at_access_4();
    // As drawn: the editor opens on 0x0B0 with its value unchanged.
    let opened = session.draft_memory_edit(addr(0x0B0), "92");
    assert_eq!(
        opened.value,
        FieldInput::Valid {
            text: "92".to_owned()
        }
    );
    assert_eq!(opened.patch, Some(value(92)));
    let EditOutcome::Refusal(unchanged) = &opened.outcome else {
        panic!("an unchanged value applies nothing");
    };
    assert_eq!(unchanged.reason, RefusalReason::NoChange);
    assert_eq!(unchanged.tone, Tone::Caution);
    let warning = (
        "Set 3, way 0 holds a newer value",
        "The dirty line there has 5 for this word. Editing memory does not change that; the \
         word stays stale until the line is written back.",
    );
    assert_eq!(said(unchanged), warning);
    // With a new value it plans, to memory only.
    let typed = session.draft_memory_edit(addr(0x0B0), "12");
    let EditOutcome::Plan(plan) = &typed.outcome else {
        panic!("a new value plans");
    };
    assert_eq!(plan, &self::plan(&session, memory(0x0B0), &value(12)));
    assert_eq!(plan.tone, Tone::Caution);
    assert_eq!((plan.title.as_str(), plan.text.as_str()), warning);
    assert_eq!(plan.action, "Apply to memory");
    assert_eq!(
        plan.consequences,
        [
            Consequence::MemoryWrite {
                addr: addr(0x0B0),
                from: word(92),
                to: word(12)
            },
            Consequence::StaysStale {
                addr: addr(0x0B0),
                set: set(3),
                way: way_index(0)
            },
        ]
    );
    assert_eq!(plan.preview, ["memory 0x0B0: 92 \u{2192} 12"]);
    assert!(plan.word_marks.is_empty());
    let after = applied(&session, edit(memory(0x0B0), value(12)));
    assert_eq!(
        after.events().last().unwrap().explain,
        "Edited memory 0x0B0: 92 \u{2192} 12. Set 3, way 0 holds this line dirty, so the word \
         stays stale."
    );
    let stale = after.machine().stale(addr(0x0B0)).unwrap();
    assert_eq!((stale.cached, stale.memory), (word(5), word(12)));
}

// ── The cursor inside an op ─────────────────────────────────────────────

#[test]
fn an_edit_waits_for_the_end_of_the_access_the_cursor_is_in() {
    let mut session = frame_3b_at_access_4();
    session.apply(Op::Step).unwrap();
    // Paused on access #5's write-back, 5.4, as frame 3b is.
    session.seek(serde_json::from_value(25.into()).unwrap());
    assert_eq!(session.position().label, "5.4");
    let patch = way_patch(serde_json::json!({"dirty": true}));
    let refused = refusal(&session, way(2, 0), &patch);
    assert_eq!(refused.reason, RefusalReason::MidAccess);
    assert_eq!(refused.tone, Tone::Caution);
    assert_eq!(
        said(&refused),
        (
            "Finish this access first",
            "Step to the end of access #5 (Shift+Space), then edit."
        )
    );
    assert_eq!(refused.suggestion, None);
    // The drafts say the same, whatever is typed.
    let drafted = draft(&session, 2, 0, &form(true, "zz", &[], false, None));
    assert!(
        matches!(&drafted.outcome, EditOutcome::Refusal(refusal) if refusal.reason == RefusalReason::MidAccess)
    );
    let memory_draft = session.draft_memory_edit(addr(0x0A4), "9");
    assert!(
        matches!(&memory_draft.outcome, EditOutcome::Refusal(refusal) if refusal.reason == RefusalReason::MidAccess)
    );
    // Shift+Space goes to the end of the access, where the edit plans:
    // way 1 now holds the clean 0x2A0 the access brought in.
    assert_eq!(session.step_access(None), StepOutcome::Moved);
    assert!(session.plan_edit(&way(2, 1), &patch).is_ok());
}

#[test]
fn an_edit_waits_for_the_end_of_several_instructions_executed_together() {
    let source = "        add r1, r0, #1\n        bnez r0, skip\n        ld r2, [0x0A4]\nskip:   add r3, r0, #2";
    let mut session = session(
        Preset::DirectMapped8.config(),
        [program(source), Op::StepAccess],
    );
    let patch = value(9);
    assert!(session.plan_edit(&memory(0x0A4), &patch).is_ok());
    // At the end of the add, the first of the three entries.
    session.seek(serde_json::from_value(2.into()).unwrap());
    assert_eq!(session.position().label, "1.1");
    let refused = refusal(&session, memory(0x0A4), &patch);
    assert_eq!(refused.reason, RefusalReason::MidAccess);
    assert_eq!(
        said(&refused),
        (
            "Finish these instructions first",
            "Instructions #1 to #3 were executed together and undo together. Go to the end of \
             #3 (Shift+Space), then edit."
        )
    );
    // Inside the load, the op's last entry.
    session.seek(serde_json::from_value(5.into()).unwrap());
    assert_eq!(session.position().label, "3.2");
    assert_eq!(
        said(&refusal(&session, memory(0x0A4), &patch)),
        (
            "Finish this access first",
            "Step to the end of access #3 (Shift+Space), then edit."
        )
    );
    // Position 0 is the end of an op before the first.
    session.seek(serde_json::from_value(0.into()).unwrap());
    assert!(session.plan_edit(&memory(0x0A4), &patch).is_ok());
}

#[test]
fn inside_an_access_before_the_ops_last_entry_an_edit_waits_for_the_ops_end() {
    // Shift+Space goes to the end of the op, not of the access the cursor
    // is in, so the refusal names every instruction the op executed.
    let source = "ld r1, [0x0A4]\nld r2, [0x0A8]";
    let mut session = session(
        Preset::DirectMapped8.config(),
        [program(source), Op::Run { n: 2 }],
    );
    session.seek(serde_json::from_value(3.into()).unwrap());
    assert_eq!(session.position().label, "1.2");
    let patch = value(9);
    let refused = refusal(&session, memory(0x0A4), &patch);
    assert_eq!(refused.reason, RefusalReason::MidAccess);
    assert_eq!(
        said(&refused),
        (
            "Finish these instructions first",
            "Instructions #1 to #2 were executed together and undo together. Go to the end of \
             #2 (Shift+Space), then edit."
        )
    );
    assert_eq!(session.step_access(None), StepOutcome::Moved);
    assert_eq!(session.position().label, "2.3");
    assert!(session.plan_edit(&memory(0x0A4), &patch).is_ok());
}

#[test]
fn an_op_that_ended_in_an_error_names_the_error_among_what_to_finish() {
    let patch = value(9);
    // Two instructions, then the error of the third, whose address does
    // not fit.
    let walk = "; walk\nadd r2, r0, #0xFF0\nadd r2, r2, #16\nld r1, [r2]";
    let mut session = session(
        Preset::TwoWayLru.config(),
        [program(walk), Op::Run { n: 5 }],
    );
    let last = session.events().last().unwrap();
    assert_eq!((last.kind, last.label.as_str()), (EventKind::Error, "3.1"));
    for at in [2_u32, 3] {
        session.seek(serde_json::from_value(at.into()).unwrap());
        assert_eq!(
            said(&refusal(&session, memory(0x0A4), &patch)),
            (
                "Finish these instructions first",
                "Instructions #1 to #2 and error #3 were executed together and undo together. \
                 Go to the end of #3 (Shift+Space), then edit."
            ),
            "at {at}"
        );
    }
    // One instruction, then the error of a misaligned load.
    let misaligned = "add r2, r0, #2\nld r1, [r2]";
    let mut session = self::session(
        Preset::TwoWayLru.config(),
        [program(misaligned), Op::Run { n: 5 }],
    );
    let last = session.events().last().unwrap();
    assert_eq!((last.kind, last.label.as_str()), (EventKind::Error, "2.1"));
    session.seek(serde_json::from_value(2.into()).unwrap());
    assert_eq!(
        said(&refusal(&session, memory(0x0A4), &patch)),
        (
            "Finish these instructions first",
            "Instruction #1 and error #2 were executed together and undo together. Go to the \
             end of #2 (Shift+Space), then edit."
        )
    );
}

#[test]
fn a_new_run_whose_program_no_longer_fits_waits_for_the_error_that_unloads_it() {
    let mut session = session(Preset::DirectMapped8.config(), [program("ld r1, [0xFFC]")]);
    let narrow = Config {
        address_bits: 10,
        ..Preset::DirectMapped8.config()
    };
    session.apply(Op::Config { config: narrow }).unwrap();
    let kinds: Vec<EventKind> = session.events().iter().map(|event| event.kind).collect();
    assert_eq!(
        kinds,
        [EventKind::Setup, EventKind::Setup, EventKind::Error]
    );
    // On the new run's setup, before the error.
    session.seek(serde_json::from_value(2.into()).unwrap());
    let refused = refusal(&session, memory(0x0A4), &value(9));
    assert_eq!(refused.reason, RefusalReason::MidAccess);
    assert_eq!(
        said(&refused),
        (
            "Finish this error first",
            "Step to the end of error #1 (Shift+Space), then edit."
        )
    );
}

// ── The target and the patch's shape ────────────────────────────────────

#[test]
fn an_edit_of_a_way_or_word_that_does_not_exist_is_refused() {
    let session = frame_3b_at_access_4();
    let patch = way_patch(serde_json::json!({"dirty": true}));
    let cases = [
        (
            way(4, 0),
            patch.clone(),
            "This cache has no set 4; its sets are 0\u{2013}3.",
        ),
        (
            way(2, 2),
            patch.clone(),
            "Set 2 has no way 2; its ways are 0\u{2013}1.",
        ),
        (
            way(4, 0),
            value(1),
            "This cache has no set 4; its sets are 0\u{2013}3.",
        ),
        (
            memory(0x1000),
            value(1),
            "0x1000 does not fit a 12-bit address (0x000\u{2013}0xFFF).",
        ),
        (
            memory(0x1000),
            patch.clone(),
            "0x1000 does not fit a 12-bit address (0x000\u{2013}0xFFF).",
        ),
    ];
    for (target, patch, text) in cases {
        let refused = refusal(&session, target, &patch);
        assert_eq!(refused.reason, RefusalReason::OutOfRange);
        assert_eq!(refused.tone, Tone::Refused);
        assert_eq!(said(&refused), ("Refused", text));
        assert!(refused.word_marks.is_empty());
    }
    let direct = self::session(Preset::DirectMapped8.config(), []);
    assert_eq!(
        refusal(&direct, way(0, 1), &patch).text,
        "Set 0 has no way 1; its only way is 0."
    );
    let mismatched = refusal(&session, way(2, 0), &value(1));
    assert_eq!(mismatched.reason, RefusalReason::KindMismatch);
    assert_eq!(
        mismatched.text,
        "A way takes a way's fields, not a memory value."
    );
    let mismatched = refusal(&session, memory(0x0A4), &patch);
    assert_eq!(mismatched.reason, RefusalReason::KindMismatch);
    assert_eq!(
        mismatched.text,
        "A memory word takes a value, not a way's fields."
    );
}

#[test]
fn a_patch_of_the_wrong_shape_or_a_value_too_wide_is_refused() {
    let session = frame_3b_at_access_4();
    let short = refusal(
        &session,
        way(2, 0),
        &way_patch(serde_json::json!({"data": [1, 2, 3]})),
    );
    assert_eq!(short.reason, RefusalReason::BadShape);
    assert_eq!(short.text, "Each line holds 4 words; this data has 3.");
    let wide = way_patch(serde_json::json!({"data": [1, 2, 3, 4_294_967_296_u64], "dirty": true}));
    let wide = refusal(&session, way(2, 0), &wide);
    assert_eq!(wide.reason, RefusalReason::ValueTooWide);
    assert_eq!(
        wide.text,
        "4,294,967,296 does not fit a 4-byte word (0\u{2013}4,294,967,295)."
    );
    let rank = refusal(
        &session,
        way(2, 0),
        &way_patch(serde_json::json!({"rank": 2})),
    );
    assert_eq!(rank.reason, RefusalReason::BadShape);
    assert_eq!(rank.text, "Set 2 has 2 ways, so its ranks are 0\u{2013}1.");
    let direct = self::session(Preset::DirectMapped8.config(), []);
    assert_eq!(
        refusal(
            &direct,
            way(2, 0),
            &way_patch(serde_json::json!({"rank": 1}))
        )
        .text,
        "Set 2 has 1 way, so its only rank is 0."
    );
    let memory_wide = refusal(&session, memory(0x0A4), &value(1 << 32));
    assert_eq!(memory_wide.reason, RefusalReason::ValueTooWide);
    assert_eq!(
        memory_wide.text,
        "4,294,967,296 does not fit a 4-byte word (0\u{2013}4,294,967,295)."
    );
}

// ── Ways ────────────────────────────────────────────────────────────────

#[test]
fn an_edit_that_changes_nothing_is_idle() {
    let session = frame_3b_at_access_4();
    for patch in [
        serde_json::json!({}),
        serde_json::json!({"valid": true, "line": 0x0A4, "dirty": false, "rank": 0}),
        serde_json::json!({"data": [40, 53, 66, 79]}),
    ] {
        let refused = refusal(&session, way(2, 0), &way_patch(patch));
        assert_eq!(refused.reason, RefusalReason::NoChange);
        assert_eq!(refused.tone, Tone::Idle);
        assert_eq!(
            said(&refused),
            ("No changes", "Edit a field to preview what will change.")
        );
    }
    // An empty way kept empty changes nothing either, whatever rank comes
    // with it: an empty way has none to change.
    for (index, rank) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        for patch in [
            serde_json::json!({"rank": rank}),
            serde_json::json!({"valid": false, "rank": rank}),
        ] {
            let refused = refusal(&session, way(0, index), &way_patch(patch.clone()));
            assert_eq!(
                (refused.reason, refused.tone),
                (RefusalReason::NoChange, Tone::Idle),
                "way {index}: {patch}"
            );
        }
        let chosen = form(false, "", &["", "", "", ""], false, Some(rank));
        let drafted = draft(&session, 0, index, &chosen);
        assert!(
            matches!(&drafted.outcome, EditOutcome::Refusal(refusal) if refusal.reason == RefusalReason::NoChange),
            "way {index}: {:?}",
            drafted.outcome
        );
        assert!(!drafted.result.valid);
    }
}

#[test]
fn an_empty_way_takes_no_fields_and_invalidating_alone_is_offered() {
    let session = frame_3b_at_access_4();
    let refused = refusal(
        &session,
        way(2, 0),
        &way_patch(serde_json::json!({"valid": false, "dirty": false})),
    );
    assert_eq!(refused.reason, RefusalReason::FieldsOnInvalidWay);
    assert_eq!(refused.tone, Tone::Caution);
    assert_eq!(
        said(&refused),
        (
            "The way would be empty",
            "An empty way has no tag, data, dirty bit or rank."
        )
    );
    assert_eq!(offer(&refused), Some("Just invalidate it"));
    assert_eq!(
        refused.suggestion.as_ref().unwrap().patch,
        way_patch(serde_json::json!({"valid": false}))
    );
    // A rank sent with the invalidation is a field too.
    let ranked = refusal(
        &session,
        way(2, 0),
        &way_patch(serde_json::json!({"valid": false, "rank": 0})),
    );
    assert_eq!(
        (ranked.reason, ranked.tone, offer(&ranked)),
        (
            RefusalReason::FieldsOnInvalidWay,
            Tone::Caution,
            Some("Just invalidate it")
        )
    );
    // A way that is empty and stays empty changes nothing, whatever fields
    // come with it: that rule comes first.
    let refused = refusal(
        &session,
        way(0, 0),
        &way_patch(serde_json::json!({"dirty": true})),
    );
    assert_eq!(refused.reason, RefusalReason::NoChange);
}

#[test]
fn invalidating_a_way_sends_it_last_and_warns_of_the_dirty_words_it_drops() {
    let session = frame_3b_at_access_4();
    let invalid = way_patch(serde_json::json!({"valid": false}));
    let clean = plan(&session, way(2, 0), &invalid);
    assert_eq!(clean.tone, Tone::Ready);
    assert_eq!(
        (clean.title.as_str(), clean.text.as_str()),
        (
            "Ready to apply",
            "Ranks stay a permutation: the other ways shift to make room."
        )
    );
    assert_eq!(
        clean.preview,
        [
            "valid 1 \u{2192} 0",
            "rank way 0: 0 \u{2192} 1",
            "rank way 1: 1 \u{2192} 0"
        ]
    );
    assert_eq!(clean.patch, invalid);
    let dirty = plan(&session, way(2, 1), &invalid);
    assert_eq!(dirty.tone, Tone::Caution);
    assert_eq!(dirty.title, "Ready to apply");
    assert_eq!(
        dirty.text,
        "Way 1 is dirty: invalidating it discards 7 at 0x1A4; memory keeps 12."
    );
    assert_eq!(
        dirty.consequences,
        [Consequence::DiscardsDirtyData {
            set: set(2),
            way: way_index(1),
            addrs: vec![addr(0x1A4)]
        }]
    );
    assert_eq!(dirty.preview, ["valid 1 \u{2192} 0", "dirty 1 \u{2192} 0"]);
    let after = applied(&session, edit(way(2, 1), invalid));
    assert!(!after.state().cache.sets[2].ways[1].valid);
    assert_eq!(after.machine().stale(addr(0x1A4)), None);
    assert_eq!(
        after.events().last().unwrap().explain,
        "Edited set 2, way 1: valid 1 \u{2192} 0; dirty 1 \u{2192} 0."
    );
}

#[test]
fn a_way_made_valid_needs_a_line_that_fits_the_address_width() {
    let session = frame_3b_at_access_4();
    let missing = refusal(
        &session,
        way(0, 0),
        &way_patch(serde_json::json!({"valid": true})),
    );
    assert_eq!(missing.reason, RefusalReason::MissingLine);
    assert_eq!(missing.tone, Tone::Idle);
    assert_eq!(
        said(&missing),
        (
            "No line address",
            "Give the address of the line this way should hold."
        )
    );
    let wide = refusal(
        &session,
        way(2, 0),
        &way_patch(serde_json::json!({"line": 0x1000})),
    );
    assert_eq!(wide.reason, RefusalReason::LineOutOfRange);
    assert_eq!(
        said(&wide),
        (
            "Refused",
            "0x1000 does not fit a 12-bit address (0x000\u{2013}0xFFF)."
        )
    );
}

#[test]
fn a_line_another_set_already_holds_is_refused_with_that_way_offered() {
    let session = frame_3b_at_access_4();
    let session = applied(
        &session,
        edit(
            way(0, 1),
            way_patch(serde_json::json!({"valid": true, "line": 0x0C0})),
        ),
    );
    let bare = refusal(
        &session,
        way(3, 0),
        &way_patch(serde_json::json!({"line": 0x0C0})),
    );
    assert_eq!(bare.reason, RefusalReason::DuplicateLine);
    assert_eq!(
        said(&bare),
        (
            "0x0C0 belongs in set 0",
            "0x0C0 is already in set 0, way 1."
        )
    );
    // The rest of the patch would change nothing there, so nothing is
    // offered.
    assert_eq!((bare.tone, offer(&bare)), (Tone::Refused, None));
    let with_data = refusal(
        &session,
        way(3, 0),
        &way_patch(serde_json::json!({"line": 0x0C0, "data": [1, 60, 73, 86], "dirty": true})),
    );
    assert_eq!(with_data.tone, Tone::Move);
    assert_eq!(offer(&with_data), Some("Edit set 0, way 1 instead"));
    let suggestion = with_data.suggestion.unwrap();
    assert_eq!(suggestion.target, way(0, 1));
    assert_eq!(
        suggestion.patch,
        way_patch(serde_json::json!({"data": [1, 60, 73, 86], "dirty": true}))
    );
}

#[test]
fn a_line_another_way_of_the_same_set_holds_is_refused_with_that_way_offered() {
    let session = frame_3b_at_access_4();
    let bare = refusal(
        &session,
        way(2, 1),
        &way_patch(serde_json::json!({"line": 0x0A0})),
    );
    assert_eq!(bare.reason, RefusalReason::DuplicateLine);
    assert_eq!(
        said(&bare),
        (
            "0x0A0 is already cached",
            "Set 2 already holds 0x0A0 in way 0."
        )
    );
    assert_eq!((bare.tone, offer(&bare)), (Tone::Refused, None));
    let with_data = refusal(
        &session,
        way(2, 1),
        &way_patch(serde_json::json!({"line": 0x0A0, "data": [40, 53, 66, 1], "dirty": true})),
    );
    assert_eq!(
        (with_data.tone, offer(&with_data)),
        (Tone::Move, Some("Edit way 0 instead"))
    );
    assert_eq!(with_data.suggestion.unwrap().target, way(2, 0));
}

#[test]
fn under_write_through_no_line_is_dirty_or_differs_from_memory() {
    let session = support::loads(
        two_way(WritePolicy::WriteThrough, Replacement::Lru),
        &[0x0A4, 0x1A4],
    );
    let dirty = refusal(
        &session,
        way(2, 0),
        &way_patch(serde_json::json!({"dirty": true})),
    );
    assert_eq!(dirty.reason, RefusalReason::DirtyUnderWriteThrough);
    assert_eq!(
        said(&dirty),
        (
            "No dirty lines under write-through",
            "Under write-through lines are never dirty: every store already reached memory."
        )
    );
    // Without the dirty bit the patch changes nothing: nothing to offer.
    assert_eq!((dirty.tone, offer(&dirty)), (Tone::Refused, None));
    let with_rank = refusal(
        &session,
        way(2, 0),
        &way_patch(serde_json::json!({"dirty": true, "rank": 0})),
    );
    assert_eq!(
        (with_rank.tone, offer(&with_rank)),
        (Tone::Caution, Some("Clear dirty and apply"))
    );
    // A single word that differs: edit that memory word instead.
    let one = refusal(
        &session,
        way(2, 1),
        &way_patch(serde_json::json!({"data": [96, 9, 25, 38]})),
    );
    assert_eq!(one.reason, RefusalReason::WriteThroughMismatch);
    assert_eq!(
        said(&one),
        (
            "Data differs from memory",
            "Under write-through the cache and memory always agree."
        )
    );
    assert_eq!(
        (one.tone, offer(&one)),
        (Tone::Caution, Some("Edit memory 0x1A4 instead"))
    );
    let suggestion = one.suggestion.unwrap();
    assert_eq!(
        (suggestion.target, &suggestion.patch),
        (memory(0x1A4), &value(9))
    );
    assert_eq!(
        suggestion.preview,
        [
            "memory 0x1A4: 12 \u{2192} 9",
            "set 2 way 1 word 1: 12 \u{2192} 9"
        ]
    );
    // Two words: no single memory edit would do.
    let two = refusal(
        &session,
        way(2, 1),
        &way_patch(serde_json::json!({"data": [1, 9, 25, 38]})),
    );
    assert_eq!(two.reason, RefusalReason::WriteThroughMismatch);
    assert_eq!((two.tone, offer(&two)), (Tone::Refused, None));
    // Dirty and different: clearing the bit would still differ.
    let both = refusal(
        &session,
        way(2, 1),
        &way_patch(serde_json::json!({"data": [1, 12, 25, 38], "dirty": true})),
    );
    assert_eq!(both.reason, RefusalReason::DirtyUnderWriteThrough);
    assert_eq!(offer(&both), None);
}

#[test]
fn clearing_the_dirty_bit_of_a_stale_line_needs_it_back() {
    let session = frame_3b_at_access_4();
    let refused = refusal(
        &session,
        way(3, 0),
        &way_patch(serde_json::json!({"dirty": false})),
    );
    assert_eq!(refused.reason, RefusalReason::NeedsDirty);
    // No word was edited, so the first that differs is named; setting the
    // bit again would change nothing, so nothing is offered, and the
    // caution stays.
    assert_eq!(
        refused.text,
        "Word 0 is 5 here but 92 in memory. A clean line must match memory, so this edit needs \
         the dirty bit."
    );
    assert_eq!((refused.tone, offer(&refused)), (Tone::Caution, None));
}

#[test]
fn ranks_are_fixed_under_random_replacement_and_in_one_way_sets() {
    let random = support::loads(
        two_way(WritePolicy::WriteBack, Replacement::Random),
        &[0x0A4, 0x1A4],
    );
    // A rank alone changes nothing where ranks never change.
    let alone = refusal(
        &random,
        way(2, 0),
        &way_patch(serde_json::json!({"rank": 1})),
    );
    assert_eq!(alone.reason, RefusalReason::NoChange);
    let refused = refusal(
        &random,
        way(2, 0),
        &way_patch(serde_json::json!({"rank": 1, "dirty": true})),
    );
    assert_eq!(refused.reason, RefusalReason::RankNotEditable);
    assert_eq!(
        said(&refused),
        ("Refused", "Random replacement keeps no order to edit.")
    );
    let direct = support::loads(Preset::DirectMapped8.config(), &[0x0A4]);
    let rank_and_dirty = way_patch(serde_json::json!({"rank": 0, "dirty": true}));
    assert_eq!(
        refusal(&direct, way(2, 0), &rank_and_dirty).text,
        "A direct-mapped set has only rank 0."
    );
    let one_line = Config {
        cache: CacheConfig {
            size_bytes: 16,
            ..Preset::DirectMapped8.config().cache
        },
        ..Preset::DirectMapped8.config()
    };
    let one_line = support::loads(one_line, &[0x0A4]);
    assert_eq!(
        refusal(&one_line, way(0, 0), &rank_and_dirty).text,
        "A one-line cache has only rank 0."
    );
}

#[test]
fn a_valid_way_ranks_below_the_valid_ways_the_set_will_have() {
    let four_way = support::loads(Preset::FourWayWriteBack.config(), &[0x0A4, 0x1A4]);
    // Validating a third way of set 2: three valid ways after the edit, so
    // rank 2 is the highest it can take, and 3 is refused.
    let validate =
        |rank: u32| way_patch(serde_json::json!({"valid": true, "line": 0x2A0, "rank": rank}));
    let accepted = plan(&four_way, way(2, 2), &validate(2));
    assert!(accepted
        .consequences
        .contains(&Consequence::LoadsLine { line: line(0x2A0) }));
    let refused = refusal(&four_way, way(2, 2), &validate(3));
    assert_eq!(refused.reason, RefusalReason::RankOutOfRange);
    assert_eq!(refused.tone, Tone::Caution);
    assert_eq!(
        said(&refused),
        (
            "Rank out of range",
            "With 3 valid ways, ranks 0\u{2013}2 belong to valid ways; empty ways always rank \
             last."
        )
    );
    assert_eq!(offer(&refused), Some("Use rank 2"));
    assert_eq!(refused.suggestion.unwrap().patch, validate(2));
    // One valid way: only rank 0, which it has, so nothing is offered.
    let one = support::loads(Preset::TwoWayLru.config(), &[0x0A4]);
    let refused = refusal(&one, way(2, 0), &way_patch(serde_json::json!({"rank": 1})));
    assert_eq!(
        refused.text,
        "With 1 valid way, rank 0 belongs to it; empty ways always rank last."
    );
    assert_eq!((refused.tone, offer(&refused)), (Tone::Refused, None));
}

#[test]
fn giving_a_dirty_way_another_line_warns_of_the_words_it_drops() {
    let session = frame_3b_at_access_4();
    let retag = plan(
        &session,
        way(3, 0),
        &way_patch(serde_json::json!({"line": 0x1B0})),
    );
    assert_eq!(retag.tone, Tone::Caution);
    assert_eq!(retag.title, "Ready to apply");
    assert_eq!(
        retag.text,
        "Way 0 is dirty: giving it line 0x1B0 discards 5 at 0x0B0; memory keeps 92."
    );
    assert_eq!(
        retag.consequences,
        [
            Consequence::DiscardsDirtyData {
                set: set(3),
                way: way_index(0),
                addrs: vec![addr(0x0B0)]
            },
            Consequence::LoadsLine { line: line(0x1B0) },
        ]
    );
    assert_eq!(
        retag.preview,
        ["tag 0x02 \u{2192} 0x06", "dirty 1 \u{2192} 0"]
    );
    // Keeping the dirty bit keeps the caution: the old line's 5 is still
    // lost, though the new line is dirty.
    let kept = plan(
        &session,
        way(3, 0),
        &way_patch(serde_json::json!({"line": 0x1B0, "dirty": true})),
    );
    assert_eq!(kept.tone, Tone::Caution);
    // A clean way given another line drops nothing.
    let clean = plan(
        &session,
        way(2, 0),
        &way_patch(serde_json::json!({"line": 0x2A0})),
    );
    assert_eq!(clean.tone, Tone::Ready);
    assert_eq!(
        clean.text,
        "This is recorded as a step, so Undo reverts it."
    );
}

#[test]
fn validating_and_invalidating_leave_ranks_alone_under_random_and_in_one_way_sets() {
    for config in [
        two_way(WritePolicy::WriteBack, Replacement::Random),
        Preset::DirectMapped8.config(),
    ] {
        let session = support::loads(config, &[0x0A4, 0x1A4]);
        let invalid = way_patch(serde_json::json!({"valid": false}));
        let emptied = plan(&session, way(2, 0), &invalid);
        let empty = applied(&session, edit(way(2, 0), invalid));
        let valid = way_patch(serde_json::json!({"valid": true, "line": 0x3A0}));
        let refilled = plan(&empty, way(2, 0), &valid);
        let filled = applied(&empty, edit(way(2, 0), valid));
        for plan in [&emptied, &refilled] {
            assert!(
                plan.consequences
                    .iter()
                    .all(|consequence| !matches!(consequence, Consequence::Rerank { .. })),
                "{config:?}"
            );
            assert_eq!(plan.text, "This is recorded as a step, so Undo reverts it.");
        }
        for session in [&empty, &filled] {
            let event = session.events().last().unwrap();
            assert!(
                event
                    .changed
                    .iter()
                    .all(|cell| !matches!(cell, ChangeRef::Rank { .. })),
                "{config:?}"
            );
            // The rank column is hidden, and the machine keeps the ranks
            // equal to the way numbers, which its invariants check.
            assert!(session.state().cache.sets[2]
                .ways
                .iter()
                .all(|way| way.rank.is_none()));
            assert_eq!(session.machine().check_invariants(), Ok(()));
        }
        assert!(!empty.state().cache.sets[2].ways[0].valid);
        assert_eq!(
            filled.state().cache.sets[2].ways[0].line_text.as_deref(),
            Some("0x3A0")
        );
    }
}

#[test]
fn fifo_ranks_are_editable_as_lru_ranks_are() {
    let fifo = support::loads(
        two_way(WritePolicy::WriteBack, Replacement::Fifo),
        &[0x0A4, 0x1A4],
    );
    assert_eq!(ranks(&fifo, 2), [1, 0]);
    let rerank = plan(&fifo, way(2, 0), &way_patch(serde_json::json!({"rank": 0})));
    assert_eq!(
        rerank.preview,
        ["rank way 0: 1 \u{2192} 0", "rank way 1: 0 \u{2192} 1"]
    );
    let after = applied(
        &fifo,
        edit(way(2, 0), way_patch(serde_json::json!({"rank": 0}))),
    );
    assert_eq!(ranks(&after, 2), [0, 1]);
    // The next miss in set 2 evicts the way now filled "earliest".
    let after = applied(
        &after,
        Op::Issue {
            kind: AccessKind::Read,
            addr: addr(0x2A4),
            value: None,
        },
    );
    assert_eq!(
        after.state().cache.sets[2].ways[1].line_text.as_deref(),
        Some("0x2A0")
    );
}

#[test]
fn validating_a_way_fills_it_from_memory_clean_and_first() {
    let session = frame_3b_at_access_4();
    let validate = plan(
        &session,
        way(3, 1),
        &way_patch(serde_json::json!({"valid": true, "line": 0x1B4})),
    );
    assert_eq!(validate.tone, Tone::Ready);
    assert_eq!(
        validate.preview,
        [
            "valid 0 \u{2192} 1",
            "tag \u{2014} \u{2192} 0x06",
            "rank way 0: 0 \u{2192} 1",
            "rank way 1: 1 \u{2192} 0"
        ]
    );
    assert_eq!(
        validate.patch,
        way_patch(serde_json::json!({
            "valid": true, "line": 0x1B0, "data": [51, 64, 77, 90], "dirty": false, "rank": 0
        }))
    );
    assert_eq!(
        validate.word_marks,
        [
            WordMark::Changed,
            WordMark::Changed,
            WordMark::Changed,
            WordMark::Changed
        ]
    );
    let after = applied(&session, edit(way(3, 1), validate.patch.clone()));
    let filled = &after.state().cache.sets[3].ways[1];
    assert_eq!(filled.words, ["51", "64", "77", "90"]);
    assert!(!filled.dirty);
    assert_eq!(ranks(&after, 3), [1, 0]);
}

// ── Memory words ────────────────────────────────────────────────────────

#[test]
fn memory_edits_follow_the_line_that_holds_the_word() {
    let session = frame_3b_at_access_4();
    let misaligned = refusal(&session, memory(0x0A6), &value(1));
    assert_eq!(misaligned.reason, RefusalReason::Misaligned);
    assert_eq!(
        said(&misaligned),
        (
            "Refused",
            "0x0A6 is not aligned to a 4-byte word; use 0x0A4 or 0x0A8."
        )
    );
    let unchanged = refusal(&session, memory(0x0A4), &value(53));
    assert_eq!(
        (unchanged.reason, unchanged.tone),
        (RefusalReason::NoChange, Tone::Idle)
    );
    assert_eq!(
        said(&unchanged),
        ("No changes", "Edit the value to preview what will change.")
    );
    let only = plan(&session, memory(0x300), &value(1));
    assert_eq!(
        (
            only.tone,
            only.title.as_str(),
            only.text.as_str(),
            only.action.as_str()
        ),
        (
            Tone::Ready,
            "Memory only",
            "No cache line holds this word, so only memory changes.",
            "Apply"
        )
    );
    let clean = plan(&session, memory(0x0A8), &value(12));
    assert_eq!(
        (
            clean.tone,
            clean.title.as_str(),
            clean.text.as_str(),
            clean.action.as_str()
        ),
        (
            Tone::Move,
            "Set 2, way 0 holds this word, clean",
            "A clean line must match memory, so the cached copy is updated as well.",
            "Apply to both"
        )
    );
    assert_eq!(
        serde_json::to_value(&clean.consequences[1]).unwrap(),
        serde_json::json!({
            "consequence": "cachedCopyUpdated", "set": 2, "way": 0, "word": 2, "from": 66, "to": 12
        })
    );
    assert_eq!(
        clean.preview,
        [
            "memory 0x0A8: 66 \u{2192} 12",
            "set 2 way 0 word 2: 66 \u{2192} 12"
        ]
    );
    let matches = plan(&session, memory(0x1A4), &value(7));
    assert_eq!(
        (matches.tone, matches.title.as_str(), matches.text.as_str()),
        (
            Tone::Ready,
            "Set 2, way 1 holds this value",
            "Memory now matches the dirty line, so the word is no longer stale."
        )
    );
    assert_eq!(
        matches.consequences[1],
        Consequence::NoLongerStale { addr: addr(0x1A4) }
    );
    let after = applied(&session, edit(memory(0x0A8), value(12)));
    assert_eq!(after.state().cache.sets[2].ways[0].words[2], "12");
    assert!(!after.state().cache.sets[2].ways[0].dirty);
}

#[test]
fn a_memory_edit_under_a_dirty_line_that_agreed_with_memory_makes_the_word_stale() {
    // 0x0B4 sits in the dirty line of set 3, way 0, which holds memory's
    // value for it: only 0x0B0 is stale there.
    let session = frame_3b_at_access_4();
    assert_eq!(session.machine().stale(addr(0x0B4)), None);
    let plan = plan(&session, memory(0x0B4), &value(77));
    assert_eq!(
        (
            plan.tone,
            plan.title.as_str(),
            plan.text.as_str(),
            plan.action.as_str()
        ),
        (
            Tone::Caution,
            "Set 3, way 0 holds a newer value",
            "The dirty line there has 8 for this word. Editing memory does not change that; the \
             word becomes stale until the line is written back.",
            "Apply to memory"
        )
    );
    assert_eq!(
        plan.consequences,
        [
            Consequence::MemoryWrite {
                addr: addr(0x0B4),
                from: word(8),
                to: word(77)
            },
            Consequence::BecomesStale { addr: addr(0x0B4) },
        ]
    );
    let after = applied(&session, edit(memory(0x0B4), value(77)));
    assert_eq!(
        after.events().last().unwrap().explain,
        "Edited memory 0x0B4: 8 \u{2192} 77. Set 3, way 0 holds this line dirty, so the word \
         becomes stale."
    );
    let stale = after.machine().stale(addr(0x0B4)).unwrap();
    assert_eq!((stale.cached, stale.memory), (word(8), word(77)));
}

// ── Drafts ──────────────────────────────────────────────────────────────

#[test]
fn an_empty_way_opens_empty_and_a_bare_apply_changes_nothing() {
    let session = frame_3b_at_access_4();
    let form_opened = form(false, "", &["", "", "", ""], false, None);
    let opened = draft(&session, 0, 0, &form_opened);
    assert_eq!(opened.line, FieldInput::Empty);
    assert_eq!(opened.words, vec![FieldInput::Empty; 4]);
    assert_eq!(opened.word_texts, ["", "", "", ""]);
    assert_eq!(
        (opened.tag_text.as_ref(), opened.index_text.as_ref()),
        (None, None)
    );
    assert_eq!(opened.patch, Some(way_patch(serde_json::json!({}))));
    let EditOutcome::Refusal(refused) = &opened.outcome else {
        panic!("an empty way opened as it is changes nothing");
    };
    assert_eq!(refused.reason, RefusalReason::NoChange);
    assert!(!opened.result.valid);
    // Switched to valid, it asks for a line, then fills from memory.
    let valid = form(true, "", &["", "", "", ""], false, None);
    let EditOutcome::Refusal(missing) = draft_outcome(&session, 0, 0, &valid) else {
        panic!("no line yet");
    };
    assert_eq!(missing.reason, RefusalReason::MissingLine);
    let typed = form(true, "0xC4", &["", "", "", ""], false, None);
    let validated = draft(&session, 0, 0, &typed);
    assert_eq!(
        validated.patch,
        Some(way_patch(serde_json::json!({"valid": true, "line": 0x0C0}))),
        "a validation without a rank"
    );
    assert_eq!(validated.word_texts, ["47", "60", "73", "86"]);
    assert_eq!(validated.result.words, ["47", "60", "73", "86"]);
    assert_eq!(validated.result.rank.map(|rank| rank.value), Some(0));
    // The rank the invalid way had, sent back as it was shown, is not a
    // choice either.
    let with_rank = form(true, "0x0C0", &["", "", "", ""], false, Some(0));
    assert_eq!(draft(&session, 0, 0, &with_rank).patch, validated.patch);
    let EditOutcome::Plan(_) = validated.outcome else {
        panic!("it plans");
    };
}

#[test]
fn a_bare_invalidation_leaves_the_fields_as_they_were_out_of_its_patch() {
    let session = frame_3b_at_access_4();
    let form = form(false, "0x1A0", &["96", "7", "25", "38"], true, Some(1));
    let draft = draft(&session, 2, 1, &form);
    assert_eq!(
        draft.patch,
        Some(way_patch(serde_json::json!({"valid": false})))
    );
    let EditOutcome::Plan(plan) = &draft.outcome else {
        panic!("a bare invalidation plans");
    };
    assert_eq!(plan.tone, Tone::Caution);
    assert!(!draft.result.valid);
    assert_eq!(draft.result.rank.map(|rank| rank.value), Some(1));
    // A field the student changed is sent, and the empty way refuses it.
    let changed = WayForm {
        dirty: false,
        ..form
    };
    let draft = support::draft(&session, 2, 1, &changed);
    assert_eq!(
        draft.patch,
        Some(way_patch(
            serde_json::json!({"valid": false, "dirty": false})
        ))
    );
    let EditOutcome::Refusal(refused) = draft.outcome else {
        panic!("fields on an empty way are refused");
    };
    assert_eq!(refused.reason, RefusalReason::FieldsOnInvalidWay);
    assert_eq!(offer(&refused), Some("Just invalidate it"));
}

#[test]
fn a_field_that_does_not_read_names_itself_and_sends_no_patch() {
    let session = frame_3b_at_access_4();
    let bad_line = draft(
        &session,
        2,
        0,
        &form(true, "0xZZ", &["40", "53", "66", "79"], false, None),
    );
    let message = "Type a 12-bit hex address, 0x000 to 0xFFF.";
    assert_eq!(
        bad_line.line,
        FieldInput::Invalid {
            message: message.to_owned()
        }
    );
    assert_eq!(bad_line.patch, None);
    let EditOutcome::Refusal(refused) = &bad_line.outcome else {
        panic!("refused");
    };
    assert_eq!(refused.reason, RefusalReason::LineOutOfRange);
    assert_eq!(
        (refused.tone, said(refused)),
        (Tone::Refused, ("Not an address", message))
    );
    // The way is shown as it is.
    let shown = &session.state().cache.sets[2].ways[0];
    assert_eq!(
        (
            &bad_line.result.tag_text,
            &bad_line.result.words,
            bad_line.result.dirty
        ),
        (&shown.tag_text, &shown.words, shown.dirty)
    );
    let bad_word = draft(
        &session,
        2,
        0,
        &form(true, "0x0A0", &["40", "x", "300", "79"], false, None),
    );
    let not_a_number = "Type a whole number for a 4-byte word, \u{2212}2,147,483,648 to \
                        4,294,967,295.";
    assert_eq!(
        bad_word.words[1],
        FieldInput::Invalid {
            message: not_a_number.to_owned()
        }
    );
    assert_eq!(
        bad_word.words[2],
        FieldInput::Valid {
            text: "300".to_owned()
        }
    );
    assert_eq!(bad_word.patch, None);
    let EditOutcome::Refusal(refused) = &bad_word.outcome else {
        panic!("refused");
    };
    assert_eq!(refused.reason, RefusalReason::ValueTooWide);
    assert_eq!(said(refused), ("Check word 1", not_a_number));
    let memory_draft = session.draft_memory_edit(addr(0x0A4), "abc");
    assert_eq!(memory_draft.patch, None);
    let EditOutcome::Refusal(refused) = &memory_draft.outcome else {
        panic!("refused");
    };
    assert_eq!(refused.title, "Check the value");
    // An empty value is the word's own.
    let empty = session.draft_memory_edit(addr(0x0A4), "  ");
    assert_eq!(empty.value, FieldInput::Empty);
    assert_eq!(empty.patch, Some(value(53)));
    // A negative value is stored as the word stores it.
    let negative = session.draft_memory_edit(addr(0x0A4), "-1");
    assert_eq!(negative.patch, Some(value(u64::from(u32::MAX))));
}

#[test]
fn the_data_inputs_show_memorys_copy_of_a_line_the_way_takes() {
    let session = frame_3b_at_access_4();
    let opened = draft(
        &session,
        3,
        0,
        &form(true, "0x0B0", &["5", "8", "21", "34"], true, None),
    );
    assert_eq!(opened.word_texts, ["5", "8", "21", "34"]);
    let retagged = draft(
        &session,
        3,
        0,
        &form(true, "0x1B0", &["", "", "", ""], false, None),
    );
    assert_eq!(retagged.word_texts, ["51", "64", "77", "90"]);
    assert_eq!(
        retagged.patch,
        Some(way_patch(serde_json::json!({"line": 0x1B0})))
    );
    assert_eq!(retagged.result.words, ["51", "64", "77", "90"]);
    assert!(!retagged.result.dirty);
    // Typing one word sends the whole line, the rest as shown.
    let typed = draft(
        &session,
        3,
        0,
        &form(true, "0x1B0", &["", "9", "", ""], true, None),
    );
    assert_eq!(
        typed.patch,
        Some(way_patch(
            serde_json::json!({"line": 0x1B0, "data": [51, 9, 77, 90], "dirty": true})
        ))
    );
}

#[test]
fn an_edit_draft_serializes_with_its_tags() {
    let session = frame_3b_at_access_4();
    let draft = draft(
        &session,
        2,
        0,
        &form(true, "0x0A0", &["40", "9", "66", "79"], true, None),
    );
    let json = serde_json::to_value(&draft).unwrap();
    assert_eq!(
        json["line"],
        serde_json::json!({"status": "valid", "text": "0x0A0"})
    );
    assert_eq!(json["outcome"]["outcome"], "plan");
    assert_eq!(json["outcome"]["tone"], "ready");
    assert_eq!(json["outcome"]["wordMarks"][1], "changedAndDiffers");
    assert_eq!(json["patch"]["kind"], "way");
    assert_eq!(json["tagText"], "0x02");
    let refused = serde_json::to_value(EditOutcome::from(
        session.plan_edit(&way(3, 0), &way_patch(serde_json::json!({"line": 0x0C0}))),
    ))
    .unwrap();
    assert_eq!(refused["outcome"], "refusal");
    assert_eq!(refused["reason"], "wrongSet");
    assert_eq!(refused["tone"], "move");
    assert_eq!(
        refused["suggestion"]["target"],
        serde_json::json!({"kind": "way", "set": 0, "way": 0})
    );
    assert_eq!(
        serde_json::to_value(&Consequence::StaysStale {
            addr: addr(0x0B0),
            set: set(3),
            way: way_index(0)
        })
        .unwrap(),
        serde_json::json!({"consequence": "staysStale", "addr": 176, "set": 3, "way": 0})
    );
    let form = form(true, "0x0A0", &["1"], false, Some(1));
    let json = serde_json::to_string(&form).unwrap();
    assert_eq!(
        json,
        r#"{"valid":true,"lineText":"0x0A0","wordTexts":["1"],"dirty":false,"rank":1}"#
    );
    assert_eq!(serde_json::from_str::<WayForm>(&json).unwrap(), form);
}

// ── Applied edits ───────────────────────────────────────────────────────

#[test]
fn an_edit_is_an_entry_of_its_own_in_the_log() {
    let session = frame_3b_at_access_4();
    let after = applied(
        &session,
        edit(
            way(2, 0),
            way_patch(serde_json::json!({"data": [40, 9, 66, 79], "dirty": true, "rank": 1})),
        ),
    );
    let timeline = after.timeline();
    let entry = timeline.current.unwrap();
    assert_eq!(entry.kind, EntryKind::Edit);
    assert_eq!(entry.outcome, EntryOutcome::Edit);
    assert_eq!(entry.label, "edit set 2 way 0");
    assert_eq!(entry.outcome_text, "edit");
    assert_eq!(entry.number, Some(5));
    let state = after.state();
    assert_eq!(
        state.changed_summary,
        "set 2 way 0 data, dirty, rank \u{b7} set 2 way 1 rank"
    );
    assert_eq!(state.cpu.status_text, "Edit #5 \u{b7} step 1 of 1");
    // Edits move no counter.
    assert_eq!(state.stats.counters, session.state().stats.counters);
    let memory = applied(&after, edit(memory(0x0B0), value(12)));
    let entry = memory.timeline().current.unwrap();
    assert_eq!(entry.label, "edit memory 0x0B0");
    assert_eq!(memory.state().changed_summary, "memory 0x0B0");
    // Undo takes each edit back.
    let mut undone = memory.clone();
    assert!(undone.undo());
    assert_eq!(undone.machine(), after.machine());
    assert!(undone.undo());
    assert_eq!(undone.machine(), session.machine());
}

#[test]
fn an_edit_the_rules_refuse_is_refused_as_an_op_and_leaves_the_log_alone() {
    let mut session = frame_3b_at_access_4();
    let before = (session.op_count(), session.position());
    let refused = session
        .apply(edit(
            way(3, 0),
            way_patch(serde_json::json!({"line": 0x120})),
        ))
        .unwrap_err();
    let OpError::Edit(refusal) = &refused else {
        panic!("an edit's refusal: {refused:?}");
    };
    assert_eq!(refusal.reason, RefusalReason::WrongSetFull);
    assert_eq!(
        refused.to_string(),
        "Refused. 0x120 has index 2, so it can only live in set 2, and every way of set 2 is \
         valid. Evict a line there first, or choose an address with index 3."
    );
    assert_eq!((session.op_count(), session.position()), before);
    let refused = session.apply(edit(memory(0x0A4), value(53))).unwrap_err();
    assert_eq!(
        refused.to_string(),
        "No changes. Edit the value to preview what will change."
    );
}

#[test]
fn replay_plans_every_edit_again() {
    let session = applied(
        &applied(
            &frame_3b_at_access_4(),
            edit(way(2, 1), way_patch(serde_json::json!({"valid": false}))),
        ),
        edit(memory(0x0A8), value(12)),
    );
    let replay = Session::replay(session.initial(), session.ops()).unwrap();
    assert_eq!(&replay.machine, session.machine());
    assert_eq!(replay.events, session.events());
    // An edit that no longer applies where it stands is reported.
    let mut ops = session.ops().to_vec();
    ops.push(edit(
        way(2, 1),
        way_patch(serde_json::json!({"valid": false})),
    ));
    let error = Session::replay(session.initial(), &ops).unwrap_err();
    let ReplayError::Op { index, error } = error else {
        panic!("an op is refused");
    };
    assert_eq!(index.get(), u32::try_from(ops.len() - 1).unwrap());
    assert!(matches!(error, OpError::Edit(refusal) if refusal.reason == RefusalReason::NoChange));
}

#[test]
fn edits_leave_every_invariant_whole_through_the_editor_states() {
    // Each state's plan or offer, applied in turn on one run.
    let mut session = frame_3b_at_access_4();
    let steps = [
        edit(
            way(2, 0),
            way_patch(serde_json::json!({"data": [40, 9, 66, 79], "dirty": true, "rank": 1})),
        ),
        edit(
            way(3, 0),
            way_patch(serde_json::json!({"data": [5, 8, 40, 34], "dirty": true})),
        ),
        edit(
            way(0, 0),
            way_patch(serde_json::json!({"valid": true, "line": 0x0C0, "dirty": true})),
        ),
        edit(memory(0x0B0), value(12)),
        edit(way(3, 0), way_patch(serde_json::json!({"valid": false}))),
    ];
    for step in steps {
        session.apply(step).unwrap();
        assert_eq!(session.machine().check_invariants(), Ok(()));
    }
    // Running the rest of frame 3b's program over the edited cache.
    for _ in 0..2 {
        session.apply(Op::Step).unwrap();
        assert_eq!(session.machine().check_invariants(), Ok(()));
    }
    assert!(session
        .events()
        .iter()
        .all(|event| event.kind != EventKind::Error));
}
