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
//!
//! The session stores at most [`SESSION_EVENT_LIMIT`] events. An entry never
//! has more than [`MAX_EVENTS_PER_ENTRY`], so the limit is checked once per
//! entry rather than per event: an op is refused unless the log before it
//! leaves room for one more entry, and an op that executes several
//! instructions stops with an `Error` event at the first entry boundary
//! without that room.

mod access;
mod edit;
mod exec;
#[cfg(test)]
mod tests;

pub(crate) use access::{Origin, Request};
pub(crate) use edit::{draft_memory, draft_way, plan_edit, Blocked};
pub use edit::{
    Consequence, EditDraft, EditOutcome, EditPlan, EditRefusal, FieldChange, FieldInput,
    MemoryEditDraft, RefusalReason, Suggestion, Tone, WayForm, WordMark,
};

use access::access;
use exec::{execute, load_program, ready, step, Until};

use crate::event::{Cause, ChangeRef, EventKind, MicroEvent};
use crate::explain;
use crate::format;
use crate::geometry::{AccessError, Geometry};
use crate::isa::{ParseFailure, Program};
use crate::limits::{MAX_EVENTS_PER_ENTRY, MAX_RUN_STEPS, SESSION_EVENT_LIMIT};
use crate::machine::{InvariantViolation, Machine};
use crate::op::{AccessKind, Op, OpError};
use crate::units::{Addr, OpIndex, Register, SetIndex, WayIndex, Word};

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

    /// An entry boundary inside a `Run` or `StepAccess` op, where the op
    /// goes on to execute another instruction: the machine there, and the
    /// instructions the op has executed so far. Replay can resume the op from
    /// that point with [`resume`], so a sink that keeps checkpoints may keep
    /// one here, and a sink that replays a log in slices may stop the op
    /// here with [`Flow::Stop`], to resume it later from this progress. By
    /// default the op goes on.
    fn boundary(&mut self, _machine: &Machine, _progress: OpProgress) -> Flow {
        Flow::Continue
    }
}

/// How far an op that executes several instructions has got: the
/// instructions it has executed so far. [`OpProgress::START`] is the start
/// of any op.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OpProgress(u32);

impl OpProgress {
    /// The start of an op, before it has executed anything.
    pub(crate) const START: OpProgress = OpProgress(0);

    /// The instructions executed so far.
    pub(crate) fn get(self) -> u32 {
        self.0
    }

    /// One instruction further.
    #[must_use]
    fn next(self) -> OpProgress {
        OpProgress(self.0.saturating_add(1))
    }
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
/// A program load's `Setup` is an entry without a number, and its event has
/// no label.
pub(crate) struct Emitter<'s> {
    /// Where the events go.
    sink: &'s mut dyn EventSink,
    /// The op producing the events.
    op: OpIndex,
    /// The id of the next event.
    next_id: u64,
    /// The events of the current entry so far.
    seq: u32,
    /// Whether the current entry is a setup, which is not numbered.
    setup: bool,
}

impl<'s> Emitter<'s> {
    /// An emitter for op `op`, whose first event gets id `first_id`.
    pub(crate) fn new(sink: &'s mut dyn EventSink, op: OpIndex, first_id: u64) -> Self {
        Self {
            sink,
            op,
            next_id: first_id,
            seq: 0,
            setup: false,
        }
    }

    /// The id the next event gets, which is also the number of events in the
    /// log before it.
    pub(crate) fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Whether the log so far leaves room for one more entry under the
    /// session's limit.
    pub(crate) fn has_room(&self) -> bool {
        has_room(self.next_id)
    }

    /// Tells the sink that the op, which has executed `progress`
    /// instructions, goes on from an entry boundary here, unless the sink
    /// stops it.
    fn boundary(&mut self, machine: &Machine, progress: OpProgress) -> Flow {
        self.sink.boundary(machine, progress)
    }

    /// Starts the run's next entry: the events that follow are numbered from
    /// 1 within it.
    pub(crate) fn begin_entry(&mut self, machine: &mut Machine) {
        machine.entries_in_run = machine.entries_in_run.saturating_add(1);
        self.seq = 0;
        self.setup = false;
    }

    /// Starts a setup: an entry the run does not count, whose events have no
    /// entry number and no label.
    pub(crate) fn begin_setup(&mut self) {
        self.seq = 0;
        self.setup = true;
    }

    /// Numbers `draft` as the next event of the current entry and hands it to
    /// the sink with `machine`, which it has already changed. In timed mode
    /// the event is stamped with the machine's cycle count. `explain` gives
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
            let entry = (!self.setup).then_some(machine.entries_in_run);
            let mut changed = draft.changed;
            changed.sort_unstable();
            changed.dedup();
            MicroEvent {
                id,
                op_index: self.op,
                run: machine.run,
                entry,
                seq: self.seq,
                label: entry.map_or_else(String::new, |entry| format::label(entry, self.seq)),
                // The run's cycle count once the event's effect is complete.
                cycle: machine.config.timing.map(|_| machine.counters.cycles),
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

/// Ends an entry whose events went as far as `result` says: checks the
/// machine's invariants if the entry is complete, and ends the op with an
/// internal error if one no longer holds.
///
/// Returns [`Flow::Stop`] when the sink stopped the entry, or when an
/// invariant no longer holds.
pub(crate) fn end_entry(
    machine: &mut Machine,
    result: Result<(), Halt>,
    out: &mut Emitter<'_>,
) -> Flow {
    match result.and_then(|()| machine.check_invariants().map_err(Halt::Broken)) {
        Ok(()) => Flow::Continue,
        Err(Halt::Stopped) => Flow::Stop,
        Err(Halt::Broken(violation)) => {
            out.internal_error(machine, violation);
            Flow::Stop
        }
    }
}

/// Whether a log of `events` events leaves room for one more entry under
/// [`SESSION_EVENT_LIMIT`]. An entry has fewer than [`MAX_EVENTS_PER_ENTRY`]
/// events, even with the `Error` that may follow it, so an op that starts
/// with room, or an `Error` that ends one for want of it, never takes the log
/// past the limit.
pub(crate) fn has_room(events: u64) -> bool {
    events.saturating_add(u64::from(MAX_EVENTS_PER_ENTRY)) <= u64::from(SESSION_EVENT_LIMIT)
}

/// An op checked against the machine it applies to, with what execution
/// needs from the check.
enum Checked {
    /// A new run, on this machine at the run's start.
    NewRun(Box<Machine>),
    /// A load or store issued by hand.
    Access(Request),
    /// A program that parsed.
    Load(Program),
    /// One instruction, which there is.
    Step,
    /// Instructions until the condition, the first of which there is.
    Execute(Until),
    /// An edit the rules accept.
    Edit(edit::Decision),
}

/// Applies `op` to `machine`, handing its events to `out`: checks the op
/// against the machine and against the session's limit, which changes
/// nothing, then executes it, which cannot fail. Whatever goes wrong while
/// executing becomes an `Error` event.
///
/// # Errors
///
/// The [`OpError`] refusing the op, with the machine unchanged and no event
/// emitted: no room left in the log for one more entry, an invalid
/// configuration, an issued access with a bad address or value, a program
/// that does not parse, a `Run` of no instructions or of too many, nothing
/// to execute, or an edit its plan refuses.
pub(crate) fn apply(machine: &mut Machine, op: &Op, out: &mut Emitter<'_>) -> Result<(), OpError> {
    let checked = check(machine, op, out.next_id())?;
    match checked {
        Checked::NewRun(start) => new_run(machine, *start, out),
        Checked::Access(request) => {
            access(machine, request, out);
        }
        Checked::Load(program) => {
            load_program(machine, program, out);
        }
        Checked::Step => {
            step(machine, out);
        }
        Checked::Execute(until) => execute(machine, until, OpProgress::START, out),
        Checked::Edit(decision) => {
            edit::edit(machine, &decision, out);
        }
    }
    Ok(())
}

/// Goes on with `op` from where it had got to on `machine`: from its start,
/// as [`apply`] does, or, inside a `Run` or `StepAccess` op, from the entry
/// boundary its sink was told of with `progress`, the instructions it had
/// executed there. The op was checked when it started, so it is not checked
/// again inside it.
///
/// # Errors
///
/// From the start of the op, the [`OpError`] refusing it, as for [`apply`].
pub(crate) fn resume(
    machine: &mut Machine,
    op: &Op,
    progress: OpProgress,
    out: &mut Emitter<'_>,
) -> Result<(), OpError> {
    if progress == OpProgress::START {
        return apply(machine, op, out);
    }
    match op {
        Op::StepAccess => execute(machine, Until::Access, progress, out),
        Op::Run { n } => execute(machine, Until::Count(*n), progress, out),
        // Every other op is one entry, or a setup and the error after it, so
        // no boundary inside it is ever handed to a sink.
        Op::Config { .. } | Op::Issue { .. } | Op::Program { .. } | Op::Step | Op::Edit { .. } => {}
    }
    Ok(())
}

/// Checks `op` against `machine`, in a log of `events` events so far.
fn check(machine: &Machine, op: &Op, events: u64) -> Result<Checked, OpError> {
    if !has_room(events) {
        return Err(OpError::SessionFull);
    }
    match op {
        Op::Config { config } => Machine::start(config.clone())
            .map(|start| Checked::NewRun(Box::new(start)))
            .map_err(OpError::Config),
        Op::Issue { kind, addr, value } => {
            request(&machine.geometry, *kind, *addr, *value).map(Checked::Access)
        }
        Op::Program { source, format } => Program::parse(&machine.geometry, source, *format)
            .map(Checked::Load)
            .map_err(ParseFailure::into_op_error),
        Op::Step => ready(machine).map(|()| Checked::Step),
        Op::StepAccess => ready(machine).map(|()| Checked::Execute(Until::Access)),
        Op::Run { n } => {
            if !(1..=MAX_RUN_STEPS).contains(n) {
                return Err(OpError::RunSteps);
            }
            ready(machine).map(|()| Checked::Execute(Until::Count(*n)))
        }
        Op::Edit { target, patch } => edit::check(machine, target, patch).map(Checked::Edit),
    }
}

/// Checks what of `op` holds whatever state a run has reached, against
/// `machine`, a machine of the configuration in force: a configuration
/// validates, an issued access's address and value fit, a program parses, a
/// `Run` asks for a count in range, and an edit has the form
/// [`edit::check_form`] checks. Whether there is a program to execute, and
/// the rules of an edit that read the cache and memory, depend on the state
/// the op meets, and are left to its own check. A guided module's ops are
/// checked this way when the module is read.
///
/// # Errors
///
/// The [`OpError`] of the first of these checks `op` fails.
pub(crate) fn check_form(machine: &Machine, op: &Op) -> Result<(), OpError> {
    match op {
        Op::Config { config } => config.validate().map(|_| ()).map_err(OpError::Config),
        Op::Issue { kind, addr, value } => {
            request(&machine.geometry, *kind, *addr, *value).map(|_| ())
        }
        Op::Program { source, format } => Program::parse(&machine.geometry, source, *format)
            .map(|_| ())
            .map_err(ParseFailure::into_op_error),
        Op::Step | Op::StepAccess => Ok(()),
        Op::Run { n } => {
            if (1..=MAX_RUN_STEPS).contains(n) {
                Ok(())
            } else {
                Err(OpError::RunSteps)
            }
        }
        Op::Edit { target, patch } => edit::check_form(machine, target, patch),
    }
}

/// The request of an access issued by hand, once its address and value are
/// checked against `geometry`: the address must fit the width and be a
/// multiple of the word size, and a store's value must fit the word. A load
/// writes `r1`.
fn request(
    geometry: &Geometry,
    kind: AccessKind,
    addr: Addr,
    value: Option<Word>,
) -> Result<Request, OpError> {
    let Some(addr) = geometry.addr(addr.get()) else {
        return Err(OpError::Access(AccessError::OutOfRange {
            addr: u64::from(addr.get()),
            address_bits: geometry.address_bits(),
        }));
    };
    if addr.get() % geometry.word_bytes() != 0 {
        return Err(OpError::Access(AccessError::Misaligned {
            addr,
            address_bits: geometry.address_bits(),
            word_bytes: geometry.word_bytes(),
        }));
    }
    match (kind, value) {
        (AccessKind::Read, None) => Ok(Request::Load {
            addr,
            into: Register::R1,
            origin: Origin::Hand,
        }),
        (AccessKind::Read, Some(_)) => Err(OpError::UnexpectedValue),
        (AccessKind::Write, None) => Err(OpError::MissingValue),
        (AccessKind::Write, Some(value)) if geometry.fit_word(value.get()) != value => {
            Err(OpError::ValueTooWide {
                value,
                word_bytes: geometry.word_bytes(),
            })
        }
        (AccessKind::Write, Some(value)) => Ok(Request::Store {
            addr,
            value: value.get(),
            from: None,
            origin: Origin::Hand,
        }),
    }
}

/// Starts a new run on `start`, the machine at the start of a run of the new
/// configuration, as one `Setup` event: the run's origin, an entry the run
/// does not number. The run is numbered after the one before it. The loaded
/// program is kept and parsed again under the new configuration, and
/// restarts at its first instruction; if it no longer parses, an `Error`
/// entry after the `Setup` says so, and it is unloaded.
fn new_run(machine: &mut Machine, start: Machine, out: &mut Emitter<'_>) {
    let run = machine.run.saturating_add(1);
    let program = machine.program.take();
    *machine = start;
    machine.run = run;
    let failure = match program
        .map(|program| Program::parse(&machine.geometry, program.source(), program.format()))
    {
        Some(Ok(program)) => {
            machine.program = Some(program);
            None
        }
        Some(Err(failure)) => Some(failure),
        None => None,
    };
    out.begin_setup();
    let restart = machine
        .program
        .as_ref()
        .and_then(|program| Some((program.format(), program.get(0)?.line)));
    let draft = Draft::new(EventKind::Setup, None);
    let config = &machine.config;
    let result = out.emit(machine, draft, || explain::new_run(config, restart));
    if end_entry(machine, result, out) == Flow::Stop {
        return;
    }
    if let Some(failure) = failure {
        out.begin_entry(machine);
        let draft = Draft::new(EventKind::Error, None);
        // The op ends here whatever the sink answers.
        let _ = out.emit(machine, draft, || {
            explain::program_unloaded(failure.format(), failure.first())
        });
    }
}
