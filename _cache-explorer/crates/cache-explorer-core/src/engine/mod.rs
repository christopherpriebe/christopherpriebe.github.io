//! The engine: the only code that changes a machine.
//!
//! An op runs in two phases. Validation may refuse it and changes nothing;
//! execution cannot fail, and whatever goes wrong while executing becomes an
//! `Error` event. As it executes, the engine hands each micro-event to an
//! [`EventSink`] with the machine as it stands after that event, and the sink
//! may stop it there. A state in the middle of an op is obtained that way:
//! by the same fold as every other state, stopped early.
//!
//! The engine checks the machine's invariants at the end of every entry. A
//! broken one ends the op with an `Error` event that names it, rather than a
//! panic, so the run stops with a message and a share link reproduces it.

mod access;
#[cfg(test)]
mod tests;

pub(crate) use access::Request;

use crate::event::{Cause, ChangeRef, EventKind, MicroEvent};
use crate::explain;
use crate::format;
use crate::machine::{InvariantViolation, Machine};
use crate::units::{Addr, OpIndex, SetIndex, WayIndex};

/// Whether the engine goes on after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flow {
    /// Go on with the op.
    Continue,
    /// Stop here, leaving the machine as it stands after the last event.
    Stop,
}

/// Where the engine sends an op's events.
pub(crate) trait EventSink {
    /// False for a quiet sink, which needs only the machine: the engine then
    /// builds no [`MicroEvent`] and formats no wording, and hands the sink
    /// `None` for each event. The machine changes the same either way.
    fn records(&self) -> bool;

    /// One event, `None` unless the sink records, and the machine after it.
    /// Returning [`Flow::Stop`] ends the op there.
    fn event(&mut self, event: Option<MicroEvent>, after: &Machine) -> Flow;
}

/// Why the engine left an op before its end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Halt {
    /// The sink asked it to stop.
    Stopped,
    /// An invariant no longer holds, which ends the op with an `Error` event.
    Broken(InvariantViolation),
}

/// An event before it is numbered: what happened, where, what it changed and
/// why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Draft {
    /// What happened.
    pub(crate) kind: EventKind,
    /// The address concerned.
    pub(crate) addr: Option<Addr>,
    /// The set concerned.
    pub(crate) set: Option<SetIndex>,
    /// The way concerned.
    pub(crate) way: Option<WayIndex>,
    /// The cells and counters whose values changed, in any order.
    pub(crate) changed: Vec<ChangeRef>,
    /// The configuration group responsible.
    pub(crate) cause: Option<Cause>,
}

impl Draft {
    /// An event of `kind`, caused by `cause`, that concerns no place and
    /// changes nothing yet.
    pub(crate) fn new(kind: EventKind, cause: Option<Cause>) -> Self {
        Self {
            kind,
            addr: None,
            set: None,
            way: None,
            changed: Vec::new(),
            cause,
        }
    }
}

/// Numbers an op's events and hands them to its sink.
///
/// An event's id is its place in the whole log, so the emitter starts from
/// the id the op's first event gets. Entries are numbered by the machine,
/// which counts them within the run; events are numbered within their entry.
pub(crate) struct Emitter<'s> {
    /// Where the events go.
    sink: &'s mut dyn EventSink,
    /// The op producing the events.
    op: OpIndex,
    /// The id of the next event.
    next_id: u64,
    /// The events of the current entry so far.
    seq: u32,
}

impl<'s> Emitter<'s> {
    /// An emitter for op `op`, whose first event gets id `first_id`.
    pub(crate) fn new(sink: &'s mut dyn EventSink, op: OpIndex, first_id: u64) -> Self {
        Self {
            sink,
            op,
            next_id: first_id,
            seq: 0,
        }
    }

    /// Starts the run's next entry: the events that follow are numbered from
    /// 1 within it.
    pub(crate) fn begin_entry(&mut self, machine: &mut Machine) {
        machine.entries_in_run = machine.entries_in_run.saturating_add(1);
        self.seq = 0;
    }

    /// Numbers `draft` as the next event of the current entry and hands it to
    /// the sink with `machine`, which it has already changed. `explain` gives
    /// the event's log line, and is called only when the sink records.
    ///
    /// # Errors
    ///
    /// [`Halt::Stopped`] when the sink stops the op here.
    pub(crate) fn emit(
        &mut self,
        machine: &Machine,
        draft: Draft,
        explain: impl FnOnce() -> String,
    ) -> Result<(), Halt> {
        self.seq = self.seq.saturating_add(1);
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let event = self.sink.records().then(|| {
            let entry = machine.entries_in_run;
            let mut changed = draft.changed;
            changed.sort_unstable();
            changed.dedup();
            MicroEvent {
                id,
                op_index: self.op,
                run: machine.run,
                entry: Some(entry),
                seq: self.seq,
                label: format::label(entry, self.seq),
                cycle: None,
                kind: draft.kind,
                addr: draft.addr,
                set: draft.set,
                way: draft.way,
                changed,
                explain: explain(),
                cause: draft.cause,
            }
        });
        match self.sink.event(event, machine) {
            Flow::Continue => Ok(()),
            Flow::Stop => Err(Halt::Stopped),
        }
    }

    /// Ends the op with an `Error` event, an entry of its own, saying that
    /// `violation` no longer holds. The access in progress, if any, is
    /// abandoned.
    pub(crate) fn internal_error(&mut self, machine: &mut Machine, violation: InvariantViolation) {
        self.begin_entry(machine);
        machine.access = None;
        let draft = Draft::new(EventKind::Error, None);
        // The op ends here whatever the sink answers.
        let _ = self.emit(machine, draft, || explain::internal_error(&violation));
    }
}
