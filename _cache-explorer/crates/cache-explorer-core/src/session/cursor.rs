//! The event cursor: where it is, and every way it moves.
//!
//! Undo and redo move by op, between undo stops: position 0 and the end of
//! every op, except the end of a `Program` op that another op follows, so a
//! program load undoes together with the step that first ran it. Space moves
//! one event, and at the end of the log executes one instruction, loading
//! the drawer's program first when it differs from the one loaded; the
//! cursor then rests on that instruction's first event, so each event shows
//! in turn. Shift+Space moves to the end of the op, or of the next one, and
//! at the end of the log executes until an access completes.

use serde::Serialize;

use super::{last_op, Keep, ReplayError, Session, Start};
use crate::explain;
use crate::op::{Op, OpError, ProgramSource};
use crate::units::{EventPos, OpIndex};

/// Where the cursor is: its position, and the op, run, entry and event it
/// rests on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Position {
    /// The position: 0 before the first event, `k` after the `k`-th.
    pub pos: EventPos,
    /// How many ops are complete here: the spec's `cursor()`.
    pub ops_done: u32,
    /// The op holding the position; none at position 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<OpIndex>,
    /// The run the position is in, from 1.
    pub run: u32,
    /// The entry's number within the run; none for a setup and at position
    /// 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<u32>,
    /// The event's number within its entry, from 1; 0 at position 0.
    pub seq: u32,
    /// How many events the entry has; 0 at position 0.
    pub entry_length: u32,
    /// The event's label, `5.3`; empty for a setup and at position 0.
    pub label: String,
    /// How much ends here.
    pub boundary: Boundary,
    /// Whether this is a run's origin: position 0, or a new run's `Setup`.
    pub at_run_origin: bool,
}

/// How much ends at a position; each level implies the ones before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum Boundary {
    /// Inside an entry.
    MidEntry,
    /// The end of an entry inside an op.
    EntryEnd,
    /// The end of an op, and of its last entry. Position 0 is the end of an
    /// op before the first.
    OpEnd,
    /// The end of the log.
    LogEnd,
}

/// What Space or Shift+Space did. The page reads the new state afterwards,
/// so no event is returned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StepOutcome {
    /// The cursor moved along the log.
    Moved,
    /// Ops were appended at the end of the log.
    Applied,
    /// Nothing happened, for the reason given: "End of program.", or the
    /// message of the op that was refused.
    Idle {
        /// Why.
        message: String,
    },
}

/// The cursor's state, in a module of its own. A private field is visible to
/// the whole module that declares it, so the moves of [`Session`] below, which
/// share this file's module, could write it; here only `place` and
/// `place_in_entry` can write the position.
mod placed {
    use crate::machine::Machine;
    use crate::session::EntryBase;
    use crate::units::EventPos;

    /// Where the cursor is on the log: its position, the machine there, what
    /// its entry started from, and whether it last moved by a single event.
    ///
    /// The machine is the fold of the log's ops stopped after the event at the
    /// position, and the entry base belongs to the entry that event is in.
    /// Each is written together with the position, by one of the placing
    /// methods, and nowhere else.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(in crate::session) struct Cursor {
        /// The position.
        pos: EventPos,
        /// The machine at the position, recomputed on every move.
        machine: Machine,
        /// What the cursor's entry changes, with the values before it, which
        /// the highlight compares the machine at the cursor with; none at a
        /// run's origin.
        entry_base: Option<EntryBase>,
        /// Whether the cursor last moved by a single event, as Space moves it,
        /// rather than across several.
        stepped: bool,
    }

    impl Cursor {
        /// A cursor at position 0, where `origin` is, outside every entry.
        pub(in crate::session) fn new(origin: Machine) -> Self {
            Self {
                pos: EventPos::new_unchecked(0),
                machine: origin,
                entry_base: None,
                stepped: false,
            }
        }

        /// The position.
        pub(in crate::session) fn pos(&self) -> EventPos {
            self.pos
        }

        /// The machine at the position.
        pub(in crate::session) fn machine(&self) -> &Machine {
            &self.machine
        }

        /// What the entry at the position started from; none outside every
        /// entry.
        pub(in crate::session) fn entry_base(&self) -> Option<&EntryBase> {
            self.entry_base.as_ref()
        }

        /// Whether the cursor last moved by a single event, as Space moves it.
        pub(in crate::session) fn stepped(&self) -> bool {
            self.stepped
        }

        /// Places the cursor at `pos`, where `machine` is, in the entry whose
        /// values before it are `base`, or outside every entry if there is
        /// none.
        pub(in crate::session) fn place(
            &mut self,
            pos: EventPos,
            machine: Machine,
            base: Option<EntryBase>,
        ) {
            self.entry_base = base;
            self.arrive(pos, machine);
        }

        /// Places the cursor at `pos`, where `machine` is, in the entry the
        /// cursor is already in, which keeps the base it has.
        pub(in crate::session) fn place_in_entry(&mut self, pos: EventPos, machine: Machine) {
            self.arrive(pos, machine);
        }

        /// Sets whether the cursor last moved by a single event, over what the
        /// move that placed it says.
        pub(in crate::session) fn set_stepped(&mut self, stepped: bool) {
            self.stepped = stepped;
        }

        /// Moves to `pos`, where `machine` is, and notes whether that was one
        /// event away.
        fn arrive(&mut self, pos: EventPos, machine: Machine) {
            self.stepped = pos.get().abs_diff(self.pos.get()) == 1;
            self.pos = pos;
            self.machine = machine;
        }
    }
}

pub(super) use placed::Cursor;

impl Session {
    /// How many ops are complete at the cursor: the spec's `cursor()`.
    pub fn cursor(&self) -> u32 {
        self.log.ops_done(self.cursor.pos())
    }

    /// Where the cursor is.
    pub fn position(&self) -> Position {
        let pos = self.cursor.pos();
        let log_end = pos == self.log.end();
        let Some(event) = self.log.event_at(pos) else {
            return Position {
                pos,
                ops_done: 0,
                op: None,
                run: self.cursor.machine().run,
                entry: None,
                seq: 0,
                entry_length: 0,
                label: String::new(),
                boundary: if log_end {
                    Boundary::LogEnd
                } else {
                    Boundary::OpEnd
                },
                at_run_origin: true,
            };
        };
        // The events after this one in its entry: each entry's events are
        // numbered from 1, so the next entry starts where the number drops.
        let rest = self
            .log
            .events()
            .get(super::slot(pos)..)
            .unwrap_or_default()
            .iter()
            .take_while(|next| next.seq > 1)
            .count();
        let boundary = if log_end {
            Boundary::LogEnd
        } else if self.log.is_op_end(pos) {
            Boundary::OpEnd
        } else if rest == 0 {
            Boundary::EntryEnd
        } else {
            Boundary::MidEntry
        };
        Position {
            pos,
            ops_done: self.log.ops_done(pos),
            op: Some(event.op_index),
            run: event.run,
            entry: event.entry,
            seq: event.seq,
            entry_length: event
                .seq
                .saturating_add(u32::try_from(rest).unwrap_or(u32::MAX)),
            label: event.label.clone(),
            boundary,
            at_run_origin: self.at_run_origin(pos),
        }
    }

    /// Moves the cursor to the undo stop before it: the end of the nearest
    /// op before the one holding it that is a stop, or position 0. The ops
    /// after the cursor are kept, for redo. Returns false at position 0,
    /// where there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let Some(holding) = self.log.op_holding(self.cursor.pos()) else {
            return false;
        };
        let target = (0..holding.get())
            .rev()
            .map(OpIndex::new_unchecked)
            .find(|&op| self.is_undo_stop(op))
            .and_then(|op| self.log.op_end(op))
            .unwrap_or(EventPos::new_unchecked(0));
        self.move_or_stay(target)
    }

    /// Moves the cursor to the next undo stop after it: from inside an op,
    /// that op's end; from the end of an op, position 0 included, the end of
    /// the next op that is a stop. Returns false at the end of the log, and
    /// while a gate is set, when that stop lies beyond the gate's limit.
    pub fn redo(&mut self) -> bool {
        self.redo_target()
            .filter(|&target| self.within_gate(target))
            .is_some_and(|target| self.move_or_stay(target))
    }

    /// Where [`Session::redo`] moves the cursor: the end of the first op
    /// that is an undo stop, from the op holding the cursor, when the cursor
    /// is inside it, or the op after; none at the end of the log.
    pub(crate) fn redo_target(&self) -> Option<EventPos> {
        let first = self.log.ops_done(self.cursor.pos());
        (first..self.op_count())
            .map(OpIndex::new_unchecked)
            .find(|&op| self.is_undo_stop(op))
            .and_then(|op| self.log.op_end(op))
    }

    /// Moves the cursor to the end of op `op_index − 1`, where `op_index`
    /// ops are complete; 0 moves it to position 0. A count beyond the log's
    /// ops moves it to the end of the log. While a gate is set, a target
    /// beyond its limit is the limit.
    pub fn scrub_to(&mut self, op_index: u32) {
        let done = op_index.min(self.op_count());
        let target = done
            .checked_sub(1)
            .and_then(|last| self.log.op_end(OpIndex::new_unchecked(last)))
            .unwrap_or(EventPos::new_unchecked(0));
        let target = self.clamp_to_gate(target);
        self.move_or_stay(target);
    }

    /// Moves the cursor to position `pos`, or to the end of the log if `pos`
    /// is past it; while a gate is set, to its limit if `pos` is beyond it.
    pub fn seek(&mut self, pos: EventPos) {
        let target = self.clamp_to_gate(pos.min(self.log.end()));
        self.move_or_stay(target);
    }

    /// Space. Before the end of the log, moves the cursor one event on. At
    /// the end, executes one instruction and moves the cursor back to its
    /// first event, so the next Space shows the next event. `program` is the
    /// drawer's text and format: when it differs from the program loaded at
    /// the cursor, or none is loaded, it is loaded first, as an op of its
    /// own that undoes together with the step.
    ///
    /// Nothing happens, with the reason, when there is nothing to execute,
    /// when the drawer's program does not parse, or when an op is refused;
    /// and while a gate is set, with its message, when the move would pass
    /// its limit or an op would be appended.
    pub fn step_event(&mut self, program: Option<&ProgramSource>) -> StepOutcome {
        let at = self.cursor.pos();
        if at < self.log.end() {
            let next = EventPos::new_unchecked(at.get().saturating_add(1));
            if !self.within_gate(next) {
                return idle(OpError::Gated);
            }
            return moved(self.move_to(next));
        }
        let mut ops: Vec<Op> = self.load_first(program).into_iter().collect();
        ops.push(Op::Step);
        let mut plan = match self.plan(ops, Keep::LastOpStart) {
            Ok(plan) => plan,
            Err(error) => return idle(error),
        };
        let step_base = plan
            .extension
            .op_ends
            .iter()
            .rev()
            .nth(1)
            .copied()
            .unwrap_or(plan.base);
        let step = last_op(plan.kept, plan.extension.ops.len());
        let last_start = plan.last_start.take();
        self.commit(plan);
        if let Some(machine) = last_start {
            let first = EventPos::new_unchecked(step_base.get().saturating_add(1));
            let start = Start::before_op(step_base, &machine, step);
            // This resumes from the op-start machine the plan kept, which
            // `move_to` would not use, so it replays here itself. As in
            // [`Session::move_or_stay`], the cursor stays at the op's end if
            // the replay fails.
            if let Ok(machine) = super::replay_quietly(start, self.log.ops(), first) {
                self.cursor.place_in_entry(first, machine);
            }
        }
        // Space shows one event at a time, even when it loads the drawer's
        // program first; the step's entry is the plan's last, whose base the
        // plan kept.
        self.cursor.set_stepped(true);
        StepOutcome::Applied
    }

    /// Shift+Space. Inside an op, moves the cursor to the op's end; at the
    /// end of an op before the end of the log, position 0 included, to the
    /// end of the next op. At the end of the log, loads the drawer's program
    /// as [`Session::step_event`] does, then executes instructions until one
    /// memory access has completed, and the cursor rests on the last event.
    /// While a gate is set, nothing happens, with its message, when the move
    /// would pass its limit or an op would be appended.
    pub fn step_access(&mut self, program: Option<&ProgramSource>) -> StepOutcome {
        let at = self.cursor.pos();
        if at < self.log.end() {
            let op = if self.log.is_op_end(at) {
                Some(OpIndex::new_unchecked(self.log.ops_done(at)))
            } else {
                self.log.op_holding(at)
            };
            let target = op
                .and_then(|op| self.log.op_end(op))
                .unwrap_or_else(|| self.log.end());
            if !self.within_gate(target) {
                return idle(OpError::Gated);
            }
            return moved(self.move_to(target));
        }
        let mut ops: Vec<Op> = self.load_first(program).into_iter().collect();
        ops.push(Op::StepAccess);
        match self.plan(ops, Keep::End) {
            Ok(plan) => {
                self.commit(plan);
                StepOutcome::Applied
            }
            Err(error) => idle(error),
        }
    }

    /// The `Program` op that loads `program` first, when it differs from the
    /// program loaded at the cursor or none is loaded. The same source in the
    /// same format is the same program.
    fn load_first(&self, program: Option<&ProgramSource>) -> Option<Op> {
        let program = program?;
        let same = self
            .cursor
            .machine()
            .program
            .as_ref()
            .is_some_and(|loaded| {
                loaded.source() == program.source && loaded.format() == program.format
            });
        (!same).then(|| Op::Program {
            source: program.source.clone(),
            format: program.format,
        })
    }

    /// Whether the end of op `op` is an undo stop: it is, unless the op is a
    /// `Program` op that another op follows.
    fn is_undo_stop(&self, op: OpIndex) -> bool {
        let last = op.get().saturating_add(1) >= self.op_count();
        last || !matches!(self.log.op(op), Some(Op::Program { .. }))
    }
}

/// The outcome of a move along the log.
fn moved(result: Result<(), ReplayError>) -> StepOutcome {
    match result {
        Ok(()) => StepOutcome::Moved,
        Err(error) => StepOutcome::Idle {
            message: error.to_string(),
        },
    }
}

/// The outcome of a Space or Shift+Space whose op was refused: nothing
/// happens, and the message says why. A program that has finished reads
/// "End of program."
fn idle(error: OpError) -> StepOutcome {
    let message = match error {
        OpError::Finished => explain::end_of_program(),
        error => error.to_string(),
    };
    StepOutcome::Idle { message }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::Preset;
    use crate::machine::Machine;
    use crate::session::EntryBase;

    fn pos(value: u32) -> EventPos {
        EventPos::new_unchecked(value)
    }

    fn origin() -> Machine {
        Machine::start(Preset::DirectMapped8.config()).unwrap()
    }

    #[test]
    fn a_cursor_notes_whether_it_moved_by_one_event() {
        let mut cursor = Cursor::new(origin());
        assert_eq!(cursor.pos(), pos(0));
        assert!(!cursor.stepped());
        cursor.place(pos(1), origin(), None);
        assert!(cursor.stepped());
        cursor.place(pos(4), origin(), None);
        assert!(!cursor.stepped());
        // Back by one is a step as well; not moving is not.
        cursor.place_in_entry(pos(3), origin());
        assert!(cursor.stepped());
        cursor.place_in_entry(pos(3), origin());
        assert!(!cursor.stepped());
        cursor.set_stepped(true);
        assert!(cursor.stepped());
        cursor.set_stepped(false);
        assert!(!cursor.stepped());
    }

    #[test]
    fn a_cursor_keeps_its_entry_base_until_placed_with_another() {
        let base = EntryBase::from_start(pos(3), &[], &origin());
        let mut cursor = Cursor::new(origin());
        assert_eq!(cursor.entry_base(), None);
        cursor.place(pos(3), origin(), Some(base.clone()));
        assert_eq!(cursor.entry_base(), Some(&base));
        cursor.place_in_entry(pos(4), origin());
        assert_eq!(cursor.pos(), pos(4));
        assert_eq!(cursor.entry_base(), Some(&base));
        cursor.place(pos(9), origin(), None);
        assert_eq!(cursor.entry_base(), None);
    }

    #[test]
    fn boundaries_are_ordered_by_how_much_ends() {
        assert!(Boundary::MidEntry < Boundary::EntryEnd);
        assert!(Boundary::EntryEnd < Boundary::OpEnd);
        assert!(Boundary::OpEnd < Boundary::LogEnd);
        assert_eq!(
            serde_json::to_string(&Boundary::MidEntry).unwrap(),
            "\"midEntry\""
        );
        assert_eq!(
            serde_json::to_string(&Boundary::LogEnd).unwrap(),
            "\"logEnd\""
        );
    }

    #[test]
    fn step_outcomes_serialize_with_their_status() {
        assert_eq!(
            serde_json::to_string(&StepOutcome::Moved).unwrap(),
            "{\"status\":\"moved\"}"
        );
        assert_eq!(
            serde_json::to_string(&StepOutcome::Applied).unwrap(),
            "{\"status\":\"applied\"}"
        );
        assert_eq!(
            serde_json::to_string(&StepOutcome::Idle {
                message: "End of program.".to_owned()
            })
            .unwrap(),
            "{\"status\":\"idle\",\"message\":\"End of program.\"}"
        );
    }

    #[test]
    fn a_finished_program_reads_end_of_program() {
        assert_eq!(
            idle(OpError::Finished),
            StepOutcome::Idle {
                message: "End of program.".to_owned()
            }
        );
        assert_eq!(
            idle(OpError::NoProgram),
            StepOutcome::Idle {
                message: OpError::NoProgram.to_string()
            }
        );
    }

    #[test]
    fn a_position_serializes_in_camel_case_without_absent_fields() {
        let origin = Position {
            pos: EventPos::new_unchecked(0),
            ops_done: 0,
            op: None,
            run: 1,
            entry: None,
            seq: 0,
            entry_length: 0,
            label: String::new(),
            boundary: Boundary::LogEnd,
            at_run_origin: true,
        };
        assert_eq!(
            serde_json::to_string(&origin).unwrap(),
            "{\"pos\":0,\"opsDone\":0,\"run\":1,\"seq\":0,\"entryLength\":0,\"label\":\"\",\
             \"boundary\":\"logEnd\",\"atRunOrigin\":true}"
        );
        let inside = Position {
            pos: EventPos::new_unchecked(7),
            op: Some(OpIndex::new_unchecked(1)),
            entry: Some(2),
            seq: 3,
            entry_length: 5,
            label: "2.3".to_owned(),
            boundary: Boundary::MidEntry,
            at_run_origin: false,
            ..origin
        };
        assert_eq!(
            serde_json::to_string(&inside).unwrap(),
            "{\"pos\":7,\"opsDone\":0,\"op\":1,\"run\":1,\"entry\":2,\"seq\":3,\
             \"entryLength\":5,\"label\":\"2.3\",\"boundary\":\"midEntry\",\"atRunOrigin\":false}"
        );
    }
}
