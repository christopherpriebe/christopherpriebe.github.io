//! Edits: the cells each lists, the counters none moves, the lines they take
//! in and out of what the classification of misses remembers, and a refused
//! edit.

use super::*;

/// An edit of the memory word at `at` to `value`.
fn memory_edit(at: u32, value: u64) -> Op {
    Op::Edit {
        target: EditTarget::Memory { addr: addr(at) },
        patch: EditPatch::Memory {
            value: Word::new_unchecked(value),
        },
    }
}

/// `two-way-lru` after frame 3b's first four accesses: set 2 holds 0x0A0,
/// clean, and 0x1A0, dirty; set 3 holds 0x0B0, dirty with 5.
fn frame_3b_at_access_4() -> Machine {
    let mut run = Run::new(preset(Preset::TwoWayLru));
    run.store(0x0B0, 5);
    run.load(0x0A4);
    run.store(0x1A4, 7);
    run.load(0x0A8);
    run.machine
}

#[test]
fn an_edit_lists_every_cell_it_changes_and_moves_no_counter() {
    let mut machine = frame_3b_at_access_4();
    let words = |values: [u64; 4]| Some(values.map(Word::new_unchecked).to_vec());
    let edits = [
        // Memory words: under a clean line, under no line, under a dirty
        // line with another value, then with its own.
        memory_edit(0x0A8, 12),
        memory_edit(0x100, 1),
        memory_edit(0x0B0, 12),
        memory_edit(0x0B0, 5),
        // Ways: a word, the dirty bit and the rank; a dirty way made empty;
        // an empty way made valid with data and the dirty bit; a dirty way
        // given another line; a rank moved back.
        way_edit(
            2,
            0,
            WayPatch {
                data: words([40, 9, 12, 79]),
                dirty: Some(true),
                rank: Some(1),
                ..WayPatch::default()
            },
        ),
        way_edit(
            2,
            1,
            WayPatch {
                valid: Some(false),
                ..WayPatch::default()
            },
        ),
        way_edit(
            0,
            1,
            WayPatch {
                valid: Some(true),
                line: Some(addr(0x0C4)),
                data: words([1, 2, 3, 4]),
                dirty: Some(true),
                ..WayPatch::default()
            },
        ),
        way_edit(
            3,
            0,
            WayPatch {
                line: Some(addr(0x130)),
                ..WayPatch::default()
            },
        ),
        way_edit(
            0,
            1,
            WayPatch {
                rank: Some(0),
                data: words([1, 2, 3, 5]),
                ..WayPatch::default()
            },
        ),
    ];
    for op in edits {
        let before = machine.clone();
        let (result, events) = apply_op(&mut machine, &op, 0);
        assert_eq!(result, Ok(()), "{op:?}");
        let [event] = events.as_slice() else {
            panic!("one event: {events:?}");
        };
        assert_eq!(event.kind, EventKind::Edit, "{op:?}");
        assert_eq!(event.cause, None, "{op:?}");
        assert_eq!(event.changed, diff(&before, &machine), "{op:?}");
        assert_eq!(machine.check_invariants(), Ok(()), "{op:?}");
        assert_eq!(machine.counters, before.counters, "{op:?}");
        assert_eq!(machine.prng, before.prng, "{op:?}");
        assert_eq!(machine.access, None, "{op:?}");
        assert_eq!(machine.entries_in_run, before.entries_in_run + 1, "{op:?}");
        // A quiet sink reaches the same machine, and gets no event.
        let mut quiet_machine = before.clone();
        let mut quiet = Quiet::default();
        let mut out = Emitter::new(&mut quiet, OpIndex::new_unchecked(1), 0);
        assert_eq!(apply(&mut quiet_machine, &op, &mut out), Ok(()), "{op:?}");
        assert_eq!((quiet_machine, quiet.seen), (machine.clone(), 1), "{op:?}");
    }
}

#[test]
fn an_edit_that_moves_a_line_updates_what_the_classification_of_misses_remembers() {
    let mut machine = frame_3b_at_access_4();
    let a0 = LineAddr::new_unchecked(0x0A0);
    let c0 = LineAddr::new_unchecked(0x0C0);
    assert!(machine.filled.contains(&a0) && machine.shadow.lines().contains(&a0));
    let (result, _) = apply_op(
        &mut machine,
        &way_edit(
            2,
            0,
            WayPatch {
                valid: Some(false),
                ..WayPatch::default()
            },
        ),
        0,
    );
    assert_eq!(result, Ok(()));
    assert!(!machine.filled.contains(&a0) && !machine.shadow.lines().contains(&a0));
    assert!(!machine.memory.is_touched(c0));
    let (result, _) = apply_op(
        &mut machine,
        &way_edit(
            0,
            0,
            WayPatch {
                valid: Some(true),
                line: Some(addr(0x0C0)),
                ..WayPatch::default()
            },
        ),
        0,
    );
    assert_eq!(result, Ok(()));
    assert!(machine.filled.contains(&c0));
    assert_eq!(machine.shadow.lines().first(), Some(&c0));
    // The line the way takes is one memory has touched, as a fill's is.
    assert!(machine.memory.is_touched(c0));
    // An edit of data only moves nothing there.
    let shadow = machine.shadow.clone();
    let (result, _) = apply_op(
        &mut machine,
        &way_edit(
            0,
            0,
            WayPatch {
                dirty: Some(true),
                ..WayPatch::default()
            },
        ),
        0,
    );
    assert_eq!(result, Ok(()));
    assert_eq!(machine.shadow, shadow);
}

#[test]
fn a_refused_edit_changes_nothing_and_says_why() {
    let mut machine = frame_3b_at_access_4();
    let before = machine.clone();
    let (result, events) = apply_op(
        &mut machine,
        &way_edit(
            3,
            0,
            WayPatch {
                line: Some(addr(0x120)),
                ..WayPatch::default()
            },
        ),
        0,
    );
    let Err(OpError::Edit(refusal)) = result else {
        panic!("refused: {result:?}");
    };
    assert_eq!(refusal.reason, RefusalReason::WrongSetFull);
    assert!(events.is_empty());
    assert_eq!(machine, before);
}
