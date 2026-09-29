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

use super::{resume_into, LastAccess, ReplayError, Session, Start, Waiting};
use crate::engine::{EventSink, Flow};
use crate::event::MicroEvent;
use crate::machine::Machine;
use crate::module::{EvalContext, Predicate};
use crate::op::OpError;
use crate::units::EventPos;

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
        self.log.last_serial()
    }

    /// The cursor's position.
    pub(crate) fn cursor_position(&self) -> EventPos {
        self.cursor.pos()
    }

    /// The most recent access of run `run` completed at or before `pos`: the
    /// last access entry of the run whose last event is there or earlier
    /// and which reached its tag compare. An access an internal error
    /// abandoned before it is passed over. The entries note it as they are
    /// appended, so finding it costs a search of the entries, however long
    /// the run.
    pub(crate) fn last_access(&self, pos: EventPos, run: u32) -> Option<LastAccess> {
        self.log.last_access(pos, run)
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
    ///
    /// The replay is of ops the session accepted, so it does not fail; if
    /// it did, the run is taken to hold no such position, where
    /// [`Session::move_or_stay`] leaves the cursor where it is.
    pub(crate) fn find_event_since(&self, predicate: &Predicate, since: u64) -> Option<EventPos> {
        self.search_run(predicate, since).ok().flatten()
    }

    /// [`Session::find_event_since`], with the error of the replay.
    ///
    /// # Errors
    ///
    /// The [`ReplayError`] of a replay of ops the session has accepted,
    /// which only a bug could cause.
    fn search_run(
        &self,
        predicate: &Predicate,
        since: u64,
    ) -> Result<Option<EventPos>, ReplayError> {
        let (origin, end) = self.log.run_bounds(self.cursor.machine().run);
        let at_origin = self.machine_at(origin)?;
        if predicate.eval(&EvalContext::at(self, &at_origin, origin, since)) {
            return Ok(Some(origin));
        }
        if end <= origin {
            return Ok(None);
        }
        // Replays from the checkpoint at or before the origin, looking at
        // each event after the origin, until the predicate holds or the run
        // ends.
        let start = Start::from(self.log.checkpoints().at_or_before(origin));
        let mut finder = Finder {
            session: self,
            predicate,
            since,
            pos: start.pos.get(),
            origin: origin.get(),
            end: end.get(),
            found: None,
            done: false,
        };
        resume_into(start, self.log.ops(), &mut finder, None)?;
        Ok(finder.found)
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

impl Waiting for Finder<'_> {
    fn finished(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::super::log::lookup_work;
    use super::*;
    use crate::config::Preset;
    use crate::limits::{MAX_EVENTS_PER_ENTRY, SESSION_EVENT_LIMIT};
    use crate::op::{AccessKind, Op, ProgramFormat};
    use crate::stats::Outcome;
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
    fn the_last_access_is_the_run_s_latest_complete_one() {
        let mut session = session();
        assert_eq!(session.last_access(session.cursor.pos(), 1), None);
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
        let last = session.last_access(session.cursor.pos(), 1).unwrap();
        assert_eq!(last.addr.get(), 0x0A4);
        assert_eq!(last.kind, AccessKind::Read);
        assert_eq!(last.outcome, Outcome::Miss);
        assert_eq!(last.serial, 1);
        // Another run has none of this run's.
        assert_eq!(session.last_access(session.cursor.pos(), 2), None);
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
            let machine = Session::replay_to(&session.initial, session.ops(), found).unwrap();
            assert!(
                predicate.eval(&EvalContext::at(&session, &machine, found, 0)),
                "{json}"
            );
            // No earlier position of the run holds it.
            for pos in 0..found.get() {
                let pos = EventPos::new_unchecked(pos);
                let machine = Session::replay_to(&session.initial, session.ops(), pos).unwrap();
                assert!(
                    !predicate.eval(&EvalContext::at(&session, &machine, pos, 0)),
                    "{json} at {pos:?}"
                );
            }
        }
    }

    /// A loop without an exit and without a memory access: one event per
    /// instruction until the budget or the session's limit stops it.
    const ENDLESS: &str = "        add r1, r0, #1\nloop:   bnez r1, loop";

    #[test]
    fn a_search_along_a_full_run_looks_up_each_position_once_by_binary_search() {
        // One load, then a run as long as the session holds of instructions
        // that access nothing: `lastAccess` at every position of the run has
        // the load to find, and the run's every entry to pass on the way if
        // it were to look back along it.
        let mut session = session();
        session.apply(load(0x0A4)).unwrap();
        let loaded = session.cursor.pos();
        let endless = Op::Program {
            source: ENDLESS.to_owned(),
            format: ProgramFormat::Asm,
        };
        while session.apply(endless.clone()).is_ok() && session.apply(Op::StepAccess).is_ok() {}
        let events = session.cursor.pos().get();
        assert!(
            events + MAX_EVENTS_PER_ENTRY > SESSION_EVENT_LIMIT,
            "{events}"
        );
        let entries = session.log.run_entries(1).len();
        let never: Predicate =
            serde_json::from_str(r#"{"lastAccess": {"addr": "0x0A8"}}"#).unwrap();
        let first: Predicate =
            serde_json::from_str(r#"{"lastAccess": {"addr": "0x0A4"}}"#).unwrap();

        let before = lookup_work::so_far();
        let found = session.find_event(&never);
        let work = lookup_work::so_far();

        assert_eq!(found, None);
        // One lookup for the run's origin and one for each event after it.
        let lookups = work.lookups - before.lookups;
        assert_eq!(lookups, u64::from(events) + 1);
        // Each lookup is a binary search of the entries: twice its depth,
        // and a few reads, is room enough, where a walk back along the run
        // looks at all of them.
        let depth = u64::from(entries.ilog2()) + 1;
        let looked_at = work.entries - before.entries;
        assert!(
            looked_at <= lookups * (2 * depth + 4),
            "{looked_at} entries looked at for {lookups} lookups over {entries} entries"
        );
        assert_eq!(session.find_event(&first), Some(loaded));
    }
}
