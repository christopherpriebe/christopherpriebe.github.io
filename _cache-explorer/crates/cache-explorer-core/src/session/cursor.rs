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

use super::{ReplayError, Session, Start};
use crate::engine::OpProgress;
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

impl Session {
    /// How many ops are complete at the cursor: the spec's `cursor()`.
    pub fn cursor(&self) -> u32 {
        self.ops_done(self.cursor)
    }

    /// Where the cursor is.
    pub fn position(&self) -> Position {
        let pos = self.cursor;
        let log_end = pos == self.end();
        let Some(event) = self.event_at(pos) else {
            return Position {
                pos,
                ops_done: 0,
                op: None,
                run: self.current.run,
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
            .events
            .get(super::slot(pos)..)
            .unwrap_or_default()
            .iter()
            .take_while(|next| next.seq > 1)
            .count();
        let boundary = if log_end {
            Boundary::LogEnd
        } else if self.is_op_end(pos) {
            Boundary::OpEnd
        } else if rest == 0 {
            Boundary::EntryEnd
        } else {
            Boundary::MidEntry
        };
        Position {
            pos,
            ops_done: self.ops_done(pos),
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
        let Some(holding) = self.op_holding(self.cursor) else {
            return false;
        };
        let target = (0..holding.get())
            .rev()
            .map(OpIndex::new_unchecked)
            .find(|&op| self.is_undo_stop(op))
            .and_then(|op| self.op_end(op))
            .unwrap_or(EventPos::new_unchecked(0));
        self.move_to(target).is_ok()
    }

    /// Moves the cursor to the next undo stop after it: from inside an op,
    /// that op's end; from the end of an op, position 0 included, the end of
    /// the next op that is a stop. Returns false at the end of the log.
    pub fn redo(&mut self) -> bool {
        let first = self.ops_done(self.cursor);
        let target = (first..self.op_count())
            .map(OpIndex::new_unchecked)
            .find(|&op| self.is_undo_stop(op))
            .and_then(|op| self.op_end(op));
        target.is_some_and(|target| self.move_to(target).is_ok())
    }

    /// Moves the cursor to the end of op `op_index − 1`, where `op_index`
    /// ops are complete; 0 moves it to position 0. A count beyond the log's
    /// ops moves it to the end of the log.
    pub fn scrub_to(&mut self, op_index: u32) {
        let done = op_index.min(self.op_count());
        let target = done
            .checked_sub(1)
            .and_then(|last| self.op_end(OpIndex::new_unchecked(last)))
            .unwrap_or(EventPos::new_unchecked(0));
        // A replay of accepted ops cannot fail; if it did, the cursor stays.
        let _ = self.move_to(target);
    }

    /// Moves the cursor to position `pos`, or to the end of the log if `pos`
    /// is past it.
    pub fn seek(&mut self, pos: EventPos) {
        let target = pos.min(self.end());
        // A replay of accepted ops cannot fail; if it did, the cursor stays.
        let _ = self.move_to(target);
    }

    /// Space. Before the end of the log, moves the cursor one event on. At
    /// the end, executes one instruction and moves the cursor back to its
    /// first event, so the next Space shows the next event. `program` is the
    /// drawer's text and format: when it differs from the program loaded at
    /// the cursor, or none is loaded, it is loaded first, as an op of its
    /// own that undoes together with the step.
    ///
    /// Nothing happens, with the reason, when there is nothing to execute,
    /// when the drawer's program does not parse, or when an op is refused.
    pub fn step_event(&mut self, program: Option<&ProgramSource>) -> StepOutcome {
        if self.cursor < self.end() {
            let next = EventPos::new_unchecked(self.cursor.get().saturating_add(1));
            return moved(self.move_to(next));
        }
        let mut ops: Vec<Op> = self.load_first(program).into_iter().collect();
        ops.push(Op::Step);
        let mut plan = match self.plan(ops, true) {
            Ok(plan) => plan,
            Err(error) => return idle(error),
        };
        let step_base = plan
            .op_ends
            .iter()
            .rev()
            .nth(1)
            .copied()
            .unwrap_or(plan.base);
        let step = OpIndex::new_unchecked(
            plan.kept
                .saturating_add(u32::try_from(plan.ops.len()).unwrap_or(u32::MAX))
                .saturating_sub(1),
        );
        let last_start = plan.last_start.take();
        self.commit(plan);
        if let Some(machine) = last_start {
            let first = EventPos::new_unchecked(step_base.get().saturating_add(1));
            let start = Start {
                pos: step_base,
                machine: &machine,
                op: step,
                progress: OpProgress::START,
            };
            // A replay of the op just accepted cannot fail; if it did, the
            // cursor stays at the op's end.
            if let Ok(machine) = super::replay_quietly(start, &self.ops, first) {
                self.current = machine;
                self.cursor = first;
            }
        }
        StepOutcome::Applied
    }

    /// Shift+Space. Inside an op, moves the cursor to the op's end; at the
    /// end of an op before the end of the log, position 0 included, to the
    /// end of the next op. At the end of the log, loads the drawer's program
    /// as [`Session::step_event`] does, then executes instructions until one
    /// memory access has completed, and the cursor rests on the last event.
    pub fn step_access(&mut self, program: Option<&ProgramSource>) -> StepOutcome {
        if self.cursor < self.end() {
            let op = if self.is_op_end(self.cursor) {
                Some(OpIndex::new_unchecked(self.ops_done(self.cursor)))
            } else {
                self.op_holding(self.cursor)
            };
            let target = op
                .and_then(|op| self.op_end(op))
                .unwrap_or_else(|| self.end());
            return moved(self.move_to(target));
        }
        let mut ops: Vec<Op> = self.load_first(program).into_iter().collect();
        ops.push(Op::StepAccess);
        match self.plan(ops, false) {
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
        let same = self.current.program.as_ref().is_some_and(|loaded| {
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
        last || !matches!(self.op(op), Some(Op::Program { .. }))
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
