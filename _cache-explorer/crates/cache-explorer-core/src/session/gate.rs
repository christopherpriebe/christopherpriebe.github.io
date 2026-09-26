//! The session's side of a guided module: the gate an unanswered prediction
//! sets, the serials that tell an op made since a step began, and the
//! predicates a step waits for, evaluated at the cursor or looked for along
//! the displayed run.
//!
//! While a gate is set, every op is refused with its message and the cursor
//! stays at or before its limit, the position where the prediction began:
//! Redo, Space and Shift+Space refuse a move beyond it, and a seek or a
//! scrub stops at it. Undo, and moves at or before it, are allowed. No op
//! can be appended, so the log cannot change under the gate and its limit
//! always names the same state.

use super::{resume_into, EntryKind, EntryOutcome, Session, Start};
use crate::engine::{EventSink, Flow};
use crate::event::MicroEvent;
use crate::machine::Machine;
use crate::module::{EvalContext, Predicate};
use crate::op::{AccessKind, OpError};
use crate::stats::Outcome;
use crate::units::{Addr, EventPos, OpIndex};

/// What an unanswered prediction holds the session to: the furthest
/// position the cursor may reach, and the message every refusal gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gate {
    /// The position where the prediction began.
    seek_limit: EventPos,
    /// "Answer the prediction in the module panel first."
    message: String,
}

impl Gate {
    /// The furthest position the cursor may reach: where the prediction
    /// began.
    pub fn seek_limit(&self) -> EventPos {
        self.seek_limit
    }

    /// The message of every refusal while the gate is set: "Answer the
    /// prediction in the module panel first."
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// The most recent access the displayed run completed at or before a
/// position, and the serial of the op that made it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LastAccess {
    /// Whether it loaded or stored.
    pub(crate) kind: AccessKind,
    /// The word's address.
    pub(crate) addr: Addr,
    /// Whether it hit or missed.
    pub(crate) outcome: Outcome,
    /// The serial of the op that made it.
    pub(crate) serial: u64,
}

impl Session {
    /// The gate a guided module's unanswered prediction has set, if any.
    pub fn gate(&self) -> Option<&Gate> {
        self.gate.as_ref()
    }

    /// Sets the gate: from now on every op is refused, and the cursor stays
    /// at or before `seek_limit`.
    pub(crate) fn set_gate(&mut self, seek_limit: EventPos) {
        self.gate = Some(Gate {
            seek_limit,
            message: OpError::Gated.to_string(),
        });
    }

    /// Lifts the gate, if one is set.
    pub(crate) fn clear_gate(&mut self) {
        self.gate = None;
    }

    /// The gate's limit, while one is set.
    pub(crate) fn limit(&self) -> Option<EventPos> {
        self.gate.as_ref().map(Gate::seek_limit)
    }

    /// Whether the cursor may move to `pos`: always, unless a gate is set
    /// and `pos` lies beyond its limit.
    pub(crate) fn within_gate(&self, pos: EventPos) -> bool {
        self.limit().is_none_or(|limit| pos <= limit)
    }

    /// `pos`, or the gate's limit if a gate is set and `pos` lies beyond it.
    pub(crate) fn clamp_to_gate(&self, pos: EventPos) -> EventPos {
        self.limit().map_or(pos, |limit| pos.min(limit))
    }

    /// The serial the last op appended was given, whether the log still
    /// holds it or not; 0 before the first. Every op appended after this
    /// call gets a greater one.
    pub(crate) fn last_serial(&self) -> u64 {
        self.last_serial
    }

    /// The serial of op `op`.
    fn serial(&self, op: OpIndex) -> Option<u64> {
        self.serials.get(usize::try_from(op.get()).ok()?).copied()
    }

    /// The cursor's position.
    pub(crate) fn cursor_position(&self) -> EventPos {
        self.cursor
    }

    /// The most recent access of run `run` completed at or before `pos`: the
    /// last access entry of the run whose last event is there or earlier
    /// and which reached its tag compare. An access an internal error
    /// abandoned before it is passed over.
    pub(crate) fn last_access(&self, pos: EventPos, run: u32) -> Option<LastAccess> {
        let done = self.entries.partition_point(|record| record.last <= pos);
        let (record, access, outcome) = self
            .entries
            .get(..done)?
            .iter()
            .rev()
            .take_while(|record| record.run == run)
            .find_map(|record| {
                let outcome = match record.outcome {
                    EntryOutcome::Hit => Outcome::Hit,
                    EntryOutcome::Miss => Outcome::Miss,
                    _ => return None,
                };
                let access = record.access.filter(|_| record.kind == EntryKind::Access)?;
                Some((record, access, outcome))
            })?;
        let op = self.event_at(record.first)?.op_index;
        Some(LastAccess {
            kind: access.kind,
            addr: access.addr,
            outcome,
            serial: self.serial(op)?,
        })
    }

    /// Whether `predicate` holds at the cursor, every op of the log counting
    /// as made since a step began: the spec's `evalPredicate`.
    pub fn eval_predicate(&self, predicate: &Predicate) -> bool {
        predicate.eval(&EvalContext::new(self))
    }

    /// The first position of the displayed run, from its origin to its last
    /// event, where `predicate` holds, every op of the log counting as made
    /// since a step began; none if it holds nowhere in the run.
    pub fn find_event(&self, predicate: &Predicate) -> Option<EventPos> {
        self.find_event_since(predicate, 0)
    }

    /// [`Session::find_event`], counting only the ops whose serial is above
    /// `since` as made since the step began.
    pub(crate) fn find_event_since(&self, predicate: &Predicate, since: u64) -> Option<EventPos> {
        let (origin, end) = self.run_bounds(self.current.run);
        let at_origin = self.machine_at(origin).ok()?;
        if predicate.eval(&EvalContext::at(self, &at_origin, origin, since)) {
            return Some(origin);
        }
        if end <= origin {
            return None;
        }
        // Replays from the checkpoint at or before the origin, looking at
        // each event after the origin, until the predicate holds or the run
        // ends.
        let (at, checkpoint) = self.checkpoints.at_or_before(origin);
        let start = Start {
            pos: at,
            machine: checkpoint.machine,
            op: checkpoint.op,
            progress: checkpoint.progress,
        };
        let mut machine = checkpoint.machine.clone();
        let mut finder = Finder {
            session: self,
            predicate,
            since,
            pos: at.get(),
            origin: origin.get(),
            end: end.get(),
            found: None,
            done: false,
        };
        resume_into(&mut machine, start, &self.ops, &mut finder, |finder| {
            finder.done
        })
        .ok()?;
        finder.found
    }
}

/// Looks for the first event after a run's origin at which a predicate
/// holds, through the run's last event, and stops the replay there.
struct Finder<'s> {
    /// The session whose log is replayed.
    session: &'s Session,
    /// The predicate looked for.
    predicate: &'s Predicate,
    /// The serial of the last op made before the step began.
    since: u64,
    /// The position of the last event seen.
    pos: u32,
    /// The run's origin, after which positions are looked at.
    origin: u32,
    /// The position after the run's last event.
    end: u32,
    /// Where the predicate holds, once found.
    found: Option<EventPos>,
    /// Whether the search is over.
    done: bool,
}

impl EventSink for Finder<'_> {
    fn records(&self) -> bool {
        false
    }

    fn event(&mut self, _event: Option<MicroEvent>, after: &Machine) -> Flow {
        self.pos = self.pos.saturating_add(1);
        if self.pos <= self.origin {
            return Flow::Continue;
        }
        let pos = EventPos::new_unchecked(self.pos);
        if self
            .predicate
            .eval(&EvalContext::at(self.session, after, pos, self.since))
        {
            self.found = Some(pos);
            self.done = true;
        } else if self.pos >= self.end {
            self.done = true;
        }
        if self.done {
            Flow::Stop
        } else {
            Flow::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::Preset;
    use crate::op::{Op, ProgramFormat};
    use crate::units::Addr;

    fn load(addr: u32) -> Op {
        Op::Issue {
            kind: AccessKind::Read,
            addr: Addr::new_unchecked(addr),
            value: None,
        }
    }

    fn session() -> Session {
        Session::with_config(Preset::DirectMapped8.config()).unwrap()
    }

    #[test]
    fn serials_rise_and_are_never_reused() {
        let mut session = session();
        assert_eq!(session.last_serial(), 0);
        for addr in [0x0A4, 0x0A8, 0x1A4] {
            session.apply(load(addr)).unwrap();
        }
        assert_eq!(session.serials, [1, 2, 3]);
        // Undone and replaced: the dropped serial is not given again.
        assert!(session.undo());
        session.apply(load(0x2A4)).unwrap();
        assert_eq!(session.serials, [1, 2, 4]);
        assert_eq!(session.last_serial(), 4);
        // A refused op takes none.
        assert!(session.apply(load(0x0A6)).is_err());
        assert_eq!(session.last_serial(), 4);
        // Several ops appended together take one each.
        session.apply_all(vec![load(0x0B0), load(0x0B4)]).unwrap();
        assert_eq!(session.serials, [1, 2, 4, 5, 6]);
    }

    #[test]
    fn a_loaded_session_numbers_its_ops_from_1() {
        let mut session = session();
        for addr in [0x0A4, 0x0A8, 0x1A4] {
            session.apply(load(addr)).unwrap();
        }
        session.undo();
        session.apply(load(0x2A4)).unwrap();
        let loaded = Session::from_json(&session.to_json()).unwrap();
        assert_eq!(loaded.serials, [1, 2, 3]);
        assert_eq!(loaded.last_serial(), 3);
        assert!(loaded.gate().is_none());
    }

    #[test]
    fn the_last_access_is_the_run_s_latest_complete_one() {
        let mut session = session();
        assert_eq!(session.last_access(session.cursor, 1), None);
        session.apply(load(0x0A4)).unwrap();
        session
            .apply(Op::Program {
                source: "add r1, r0, #1".to_owned(),
                format: ProgramFormat::Asm,
            })
            .unwrap();
        session.apply(Op::Step).unwrap();
        // An instruction without an access and a program load are passed
        // over.
        let last = session.last_access(session.cursor, 1).unwrap();
        assert_eq!(last.addr.get(), 0x0A4);
        assert_eq!(last.kind, AccessKind::Read);
        assert_eq!(last.outcome, Outcome::Miss);
        assert_eq!(last.serial, 1);
        // Another run has none of this run's.
        assert_eq!(session.last_access(session.cursor, 2), None);
    }

    #[test]
    fn a_found_position_holds_the_machine_the_fold_gives() {
        let mut session = session();
        session
            .apply(Op::Program {
                source: "R 0x000\nR 0x080\nR 0x000\nW 0x080 7".to_owned(),
                format: ProgramFormat::Trace,
            })
            .unwrap();
        session.apply(Op::Run { n: 4 }).unwrap();
        let predicates = [
            r#"{"counter": "misses", "op": "==", "value": 2}"#,
            r#"{"line": {"addr": 128}, "dirty": true}"#,
            r#"{"event": {"kind": "fill", "entry": 3}}"#,
            r#"{"lastAccess": {"addr": 0}}"#,
            r#"{"register": "r1", "equals": 5}"#,
        ];
        for json in predicates {
            let predicate: Predicate = serde_json::from_str(json).unwrap();
            let found = session.find_event(&predicate).unwrap();
            let machine = Session::replay_to(&session.initial, &session.ops, found).unwrap();
            assert!(
                predicate.eval(&EvalContext::at(&session, &machine, found, 0)),
                "{json}"
            );
            // No earlier position of the run holds it.
            for pos in 0..found.get() {
                let pos = EventPos::new_unchecked(pos);
                let machine = Session::replay_to(&session.initial, &session.ops, pos).unwrap();
                assert!(
                    !predicate.eval(&EvalContext::at(&session, &machine, pos, 0)),
                    "{json} at {pos:?}"
                );
            }
        }
    }
}
