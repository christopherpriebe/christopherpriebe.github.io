//! The session: the log of ops over an initial configuration, the events
//! they produced, and a cursor on those events. It is the only owner of
//! machine state.
//!
//! The log is append-only in the sense the tool promises: an op is checked
//! against the machine at the point it is appended before anything changes,
//! so a refused op leaves the log, the undone ops after the cursor and the
//! cursor as they were; an accepted op drops the undone ops, is appended, and
//! the cursor goes to its last event. No call rewrites an op the log keeps.
//! Every configuration change is an op of its own, which starts a run.
//!
//! Every state is the fold of the ops over the initial configuration,
//! stopped after the event the cursor is on: [`Session::replay`] and
//! [`Session::replay_to`] are that fold as pure functions, and the session
//! reaches the same machines from its checkpoints. The events are stored;
//! the machine is recomputed at every cursor move, by a quiet replay that
//! builds no event and words nothing.
//!
//! A position counts events: position 0 is before the first, and position
//! `k` is after the `k`-th, whose id is `k − 1`. Position 0 acts as the end
//! of an op before the first.

mod checkpoints;
mod cursor;

use serde::Serialize;
use thiserror::Error;

pub use cursor::{Boundary, Position, StepOutcome};

use checkpoints::{due, Checkpoint, Checkpoints};

use crate::config::{Config, ConfigError};
use crate::engine::{self, Emitter, EventSink, Flow, OpProgress};
use crate::event::{EventKind, MicroEvent};
use crate::explain;
use crate::machine::Machine;
use crate::op::{Op, OpError};
use crate::units::{EventPos, OpIndex};

/// A log of ops over an initial configuration, with an event cursor. The
/// only owner of machine state.
#[derive(Clone, Debug)]
pub struct Session {
    /// The first run's configuration, which is not an op.
    initial: Config,
    /// Whether the student chose Advanced rather than Basics: a presentation
    /// setting that no op records and the fold never reads.
    advanced: bool,
    /// Every op, the undone ones after the cursor included.
    ops: Vec<Op>,
    /// Every event of every op, in order; at most
    /// [`SESSION_EVENT_LIMIT`](crate::limits::SESSION_EVENT_LIMIT).
    events: Vec<MicroEvent>,
    /// Where each op ends: the position after its last event.
    op_ends: Vec<EventPos>,
    /// Copies of the machine along the log, which seeks replay from.
    checkpoints: Checkpoints,
    /// The cursor's position.
    cursor: EventPos,
    /// The machine at the cursor, recomputed on every move.
    current: Machine,
}

/// The result of the fold: the machine after every op, and every event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    /// The machine after the last op.
    pub machine: Machine,
    /// Every event of every op, in order.
    pub events: Vec<MicroEvent>,
}

/// Why a log cannot be replayed. `Display` is the message the student
/// reads.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ReplayError {
    /// The initial configuration is invalid.
    #[error(transparent)]
    InitialConfig(#[from] ConfigError),
    /// An op is refused by the machine it applies to, as the session would
    /// have refused it.
    #[error("{error}")]
    Op {
        /// The op refused.
        index: OpIndex,
        /// Why.
        error: OpError,
    },
}

/// What a change from the configuration bar, or a switch between Basics and
/// Advanced, did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConfigChange {
    /// Nothing more is needed: the configuration was the displayed run's
    /// already, the change started a new run at once, or the mode changed.
    Applied,
    /// The change starts a new run, and the student is asked first.
    NeedsReset {
        /// The configuration to apply with `Op::Config` if the student
        /// agrees.
        config: Config,
        /// The question: "Configuration is fixed for a run. Applying starts a
        /// new, empty run; this one has 14 steps and stays reachable with
        /// Undo."
        message: String,
    },
}

impl Session {
    /// A session with an empty log whose first run has the configuration
    /// `initial`, with the cursor at position 0, in the mode `initial`'s
    /// `advanced` names.
    ///
    /// # Errors
    ///
    /// The [`ConfigError`] of the first rule `initial` breaks.
    pub fn with_config(initial: Config) -> Result<Session, ConfigError> {
        let origin = Machine::start(initial.clone())?;
        Ok(Session {
            advanced: initial.advanced,
            initial,
            ops: Vec::new(),
            events: Vec::new(),
            op_ends: Vec::new(),
            checkpoints: Checkpoints::new(origin.clone()),
            cursor: EventPos::new_unchecked(0),
            current: origin,
        })
    }

    /// The fold: the machine after every op of `ops` over a first run of
    /// `initial`, and every event, numbered as a session numbers them. A
    /// pure function of its arguments.
    ///
    /// # Errors
    ///
    /// [`ReplayError::InitialConfig`] if `initial` is invalid, and
    /// [`ReplayError::Op`] for the first op the machine refuses, as a
    /// session would refuse it; that includes an op the session's event
    /// limit leaves no room for.
    pub fn replay(initial: &Config, ops: &[Op]) -> Result<Replay, ReplayError> {
        let mut machine = Machine::start(initial.clone())?;
        let mut sink = Recorder::default();
        for (index, op) in (0_u32..).zip(ops) {
            let index = OpIndex::new_unchecked(index);
            let first = u64::try_from(sink.events.len()).unwrap_or(u64::MAX);
            let mut out = Emitter::new(&mut sink, index, first);
            engine::apply(&mut machine, op, &mut out)
                .map_err(|error| ReplayError::Op { index, error })?;
        }
        Ok(Replay {
            machine,
            events: sink.events,
        })
    }

    /// The machine after the event at `pos`, by the same fold as
    /// [`Session::replay`] stopped there: position 0 is the first run's
    /// start. A position past the last event gives the machine after the
    /// last op.
    ///
    /// # Errors
    ///
    /// As for [`Session::replay`], for `initial` and for the ops the fold
    /// reaches before it stops.
    pub fn replay_to(initial: &Config, ops: &[Op], pos: EventPos) -> Result<Machine, ReplayError> {
        let origin = Machine::start(initial.clone())?;
        let start = Start {
            pos: EventPos::new_unchecked(0),
            machine: &origin,
            op: OpIndex::new_unchecked(0),
            progress: OpProgress::START,
        };
        replay_quietly(start, ops, pos)
    }

    /// The first run's configuration.
    pub fn initial(&self) -> &Config {
        &self.initial
    }

    /// The displayed run's configuration: the configuration of the run the
    /// cursor is in.
    pub fn config(&self) -> &Config {
        &self.current.config
    }

    /// Whether the page shows Advanced: the mode the student chose, or
    /// Advanced whatever the choice while the displayed run's configuration
    /// needs it, as after undoing back into a write-through run.
    pub fn advanced(&self) -> bool {
        self.advanced || !self.current.config.fits_basics()
    }

    /// The machine at the cursor.
    pub fn machine(&self) -> &Machine {
        &self.current
    }

    /// Every op of the log, the undone ones after the cursor included.
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// How many ops the log holds, the undone ones after the cursor
    /// included.
    pub fn op_count(&self) -> u32 {
        u32::try_from(self.ops.len()).unwrap_or(u32::MAX)
    }

    /// Every event up to the cursor, in order.
    pub fn events(&self) -> &[MicroEvent] {
        self.events.get(..slot(self.cursor)).unwrap_or_default()
    }

    /// Appends `op` at the end of the op holding the cursor, or at position
    /// 0 if there is none, and moves the cursor to the op's last event.
    ///
    /// The op is first checked against the machine there, and against the
    /// session's event limit. Only once it is accepted is the log dropped
    /// after that point: the op holding the cursor is kept whole, and the
    /// undone ops after it go.
    ///
    /// # Errors
    ///
    /// The [`OpError`] refusing the op. A refused op changes nothing: the
    /// log, the undone ops and the cursor are as they were.
    pub fn apply(&mut self, op: Op) -> Result<Vec<MicroEvent>, OpError> {
        let plan = self.plan(vec![op], false)?;
        let first = slot(plan.base);
        self.commit(plan);
        Ok(self.events.get(first..).unwrap_or_default().to_vec())
    }

    /// A change from the configuration bar to `config`.
    ///
    /// A configuration equal to the displayed run's, apart from the
    /// presentation flag `advanced`, changes nothing. While the displayed run
    /// has no entry and the cursor is at the end of the log, the change is
    /// applied at once by appending `Op::Config`, as [`Session::apply`]
    /// would: every change is an op of its own, with its own run and its own
    /// undo stop, and none replaces another. Otherwise nothing changes, and
    /// the result asks the student first, with the question worded here.
    ///
    /// # Errors
    ///
    /// [`OpError::Config`] if `config` is invalid, and the error refusing
    /// the `Config` op when it would be applied at once.
    pub fn propose_config(&mut self, config: Config) -> Result<ConfigChange, OpError> {
        config.validate()?;
        let displayed = &self.current.config;
        let same = Config {
            advanced: displayed.advanced,
            ..config.clone()
        };
        if same == *displayed {
            return Ok(ConfigChange::Applied);
        }
        if self.cursor == self.end() && self.at_run_origin(self.cursor) {
            self.apply(Op::Config { config })?;
            return Ok(ConfigChange::Applied);
        }
        Ok(ConfigChange::NeedsReset {
            message: self.reset_question(),
            config,
        })
    }

    /// Switches between Basics (`on` false) and Advanced (`on` true). The
    /// mode is a presentation setting, not an op: it starts no run and
    /// changes no machine.
    ///
    /// Switching to Basics while the displayed run's configuration needs
    /// Advanced changes nothing and asks first, as a change from the
    /// configuration bar does, offering the configuration's Basics form as a
    /// new run. Once that run is applied, switching again succeeds.
    pub fn set_advanced(&mut self, on: bool) -> ConfigChange {
        let displayed = &self.current.config;
        if !on && !displayed.fits_basics() {
            return ConfigChange::NeedsReset {
                config: displayed.to_basics(),
                message: self.reset_question(),
            };
        }
        self.advanced = on;
        ConfigChange::Applied
    }

    /// The question asked before a configuration change starts a new run: it
    /// counts the steps of the displayed run the log keeps, and the undone
    /// steps a new op would drop.
    fn reset_question(&self) -> String {
        let base = slot(self.base().0);
        let run = self.current.run;
        let steps = |events: &[MicroEvent]| {
            let count = events
                .iter()
                .filter(|event| event.kind != EventKind::Setup)
                .count();
            u32::try_from(count).unwrap_or(u32::MAX)
        };
        let (kept, dropped) = self.events.split_at(base.min(self.events.len()));
        let run_start = kept
            .iter()
            .rposition(|event| event.run != run)
            .map_or(0, |last_before| last_before.saturating_add(1));
        explain::needs_reset(
            steps(kept.get(run_start..).unwrap_or_default()),
            steps(dropped),
        )
    }

    /// Checks `ops` in turn, each against the machine the ones before it
    /// leave, from the end of the op holding the cursor, and executes them
    /// there, without changing the session. With `keep_start`, the plan
    /// keeps the machine at the start of the last op.
    ///
    /// # Errors
    ///
    /// The [`OpError`] refusing the first op refused.
    fn plan(&self, ops: Vec<Op>, keep_start: bool) -> Result<Plan, OpError> {
        let (base, kept) = self.base();
        let mut machine = self.machine_at(base).map_err(ReplayError::into_op)?;
        let mut builder = Builder {
            base: u64::from(base.get()),
            events: Vec::new(),
            op: OpIndex::new_unchecked(kept),
            last: self.checkpoints.last_at_or_before(base),
            checkpoints: Vec::new(),
        };
        let mut op_ends = Vec::with_capacity(ops.len());
        let mut last_start = None;
        for (index, op) in (kept..).zip(&ops) {
            let index = OpIndex::new_unchecked(index);
            if keep_start {
                last_start = Some(machine.clone());
            }
            builder.op = index;
            let first = builder.next_id();
            let mut out = Emitter::new(&mut builder, index, first);
            engine::apply(&mut machine, op, &mut out)?;
            let end = builder.pos();
            op_ends.push(end);
            let next = OpIndex::new_unchecked(index.get().saturating_add(1));
            builder.keep(end, &machine, next, OpProgress::START);
        }
        Ok(Plan {
            base,
            kept,
            ops,
            events: builder.events,
            op_ends,
            checkpoints: builder.checkpoints,
            last_start,
            end: machine,
        })
    }

    /// Makes `plan` the session's: drops the log after its base, appends its
    /// ops and events, and puts the cursor on its last event.
    fn commit(&mut self, plan: Plan) {
        let kept = usize::try_from(plan.kept).unwrap_or(usize::MAX);
        self.ops.truncate(kept);
        self.op_ends.truncate(kept);
        self.events.truncate(slot(plan.base));
        self.checkpoints.truncate_after(plan.base);
        self.ops.extend(plan.ops);
        self.op_ends.extend(plan.op_ends);
        self.events.extend(plan.events);
        for (pos, checkpoint) in plan.checkpoints {
            self.checkpoints.insert(pos, checkpoint);
        }
        self.cursor = self.end();
        self.current = plan.end;
    }

    /// Where a new op goes: the end of the op holding the cursor, or
    /// position 0 if there is none; and how many ops the log keeps before
    /// it.
    fn base(&self) -> (EventPos, u32) {
        match self.op_holding(self.cursor) {
            Some(op) => (
                self.op_end(op).unwrap_or(self.cursor),
                op.get().saturating_add(1),
            ),
            None => (EventPos::new_unchecked(0), 0),
        }
    }

    /// The machine at `pos`, replayed quietly from the nearest point before
    /// it that the session holds a machine for: a checkpoint, or the cursor
    /// when it is at the end of an op.
    fn machine_at(&self, pos: EventPos) -> Result<Machine, ReplayError> {
        if pos == self.cursor {
            return Ok(self.current.clone());
        }
        let (at, checkpoint) = self.checkpoints.at_or_before(pos);
        let start = if at < self.cursor && self.cursor < pos && self.is_op_end(self.cursor) {
            Start {
                pos: self.cursor,
                machine: &self.current,
                op: OpIndex::new_unchecked(self.ops_done(self.cursor)),
                progress: OpProgress::START,
            }
        } else {
            Start {
                pos: at,
                machine: checkpoint.machine,
                op: checkpoint.op,
                progress: checkpoint.progress,
            }
        };
        replay_quietly(start, &self.ops, pos)
    }

    /// Moves the cursor to `pos`, recomputing the machine there.
    ///
    /// # Errors
    ///
    /// The [`ReplayError`] of a replay of ops the session has accepted,
    /// which only a bug could cause; the cursor then stays where it was.
    fn move_to(&mut self, pos: EventPos) -> Result<(), ReplayError> {
        if pos != self.cursor {
            self.current = self.machine_at(pos)?;
            self.cursor = pos;
        }
        Ok(())
    }

    /// The position after the last event.
    fn end(&self) -> EventPos {
        position(self.events.len())
    }

    /// The event the cursor rests on at `pos`: the one whose id is `pos − 1`;
    /// none at position 0.
    fn event_at(&self, pos: EventPos) -> Option<&MicroEvent> {
        let id = pos.get().checked_sub(1)?;
        self.events.get(usize::try_from(id).ok()?)
    }

    /// The op holding position `pos`: the op of the event there; none at
    /// position 0.
    fn op_holding(&self, pos: EventPos) -> Option<OpIndex> {
        self.event_at(pos).map(|event| event.op_index)
    }

    /// Where op `op` ends.
    fn op_end(&self, op: OpIndex) -> Option<EventPos> {
        self.op_ends.get(usize::try_from(op.get()).ok()?).copied()
    }

    /// Whether `pos` is the end of an op, position 0 included, as the end of
    /// an op before the first.
    fn is_op_end(&self, pos: EventPos) -> bool {
        self.op_holding(pos)
            .is_none_or(|op| self.op_end(op) == Some(pos))
    }

    /// How many ops are complete at `pos`.
    fn ops_done(&self, pos: EventPos) -> u32 {
        match self.op_holding(pos) {
            None => 0,
            Some(op) if self.op_end(op) == Some(pos) => op.get().saturating_add(1),
            Some(op) => op.get(),
        }
    }

    /// Whether `pos` is a run's origin: position 0, where the first run
    /// starts, or the `Setup` event of a `Config` op, which starts a new one.
    fn at_run_origin(&self, pos: EventPos) -> bool {
        match self.event_at(pos) {
            None => true,
            Some(event) => {
                event.kind == EventKind::Setup
                    && matches!(self.op(event.op_index), Some(Op::Config { .. }))
            }
        }
    }

    /// Op `op`.
    fn op(&self, op: OpIndex) -> Option<&Op> {
        self.ops.get(usize::try_from(op.get()).ok()?)
    }
}

impl ReplayError {
    /// The error refusing the op, for a replay of ops a session has already
    /// accepted, which only a bug could make fail.
    fn into_op(self) -> OpError {
        match self {
            ReplayError::InitialConfig(error) => OpError::Config(error),
            ReplayError::Op { error, .. } => error,
        }
    }
}

/// Ops checked and executed from a point of the log, ready to be appended
/// there.
struct Plan {
    /// Where the ops go.
    base: EventPos,
    /// How many ops the log keeps before them.
    kept: u32,
    /// The ops.
    ops: Vec<Op>,
    /// Their events.
    events: Vec<MicroEvent>,
    /// Where each ends.
    op_ends: Vec<EventPos>,
    /// The checkpoints due along them.
    checkpoints: Vec<(EventPos, Checkpoint)>,
    /// The machine at the start of the last op, when the plan was asked to
    /// keep it.
    last_start: Option<Machine>,
    /// The machine after the last op.
    end: Machine,
}

/// Where a quiet replay starts: a machine at a position of the log, and the
/// op the replay goes on with, from its start or from inside it.
#[derive(Clone, Copy)]
struct Start<'m> {
    /// The position.
    pos: EventPos,
    /// The machine there.
    machine: &'m Machine,
    /// The op the replay goes on with.
    op: OpIndex,
    /// The instructions that op had executed there.
    progress: OpProgress,
}

/// Replays `ops` quietly from `start` and returns the machine after the
/// event at `target`, or after the last op if the ops end first.
fn replay_quietly(start: Start<'_>, ops: &[Op], target: EventPos) -> Result<Machine, ReplayError> {
    let mut machine = start.machine.clone();
    let remaining = target.get().saturating_sub(start.pos.get());
    if remaining == 0 {
        return Ok(machine);
    }
    let mut sink = Stopper { remaining };
    let mut next_id = u64::from(start.pos.get());
    let mut progress = start.progress;
    let first = usize::try_from(start.op.get()).unwrap_or(usize::MAX);
    for (index, op) in (0_u32..).zip(ops).skip(first) {
        let index = OpIndex::new_unchecked(index);
        let mut out = Emitter::new(&mut sink, index, next_id);
        engine::resume(&mut machine, op, progress, &mut out)
            .map_err(|error| ReplayError::Op { index, error })?;
        next_id = out.next_id();
        progress = OpProgress::START;
        if sink.remaining == 0 {
            break;
        }
    }
    Ok(machine)
}

/// The position after `count` events.
fn position(count: usize) -> EventPos {
    EventPos::new_unchecked(u32::try_from(count).unwrap_or(u32::MAX))
}

/// The number of events before `pos`, as an index into the events.
fn slot(pos: EventPos) -> usize {
    usize::try_from(pos.get()).unwrap_or(usize::MAX)
}

/// Keeps every event.
#[derive(Default)]
struct Recorder {
    /// The events so far, in order.
    events: Vec<MicroEvent>,
}

impl EventSink for Recorder {
    fn records(&self) -> bool {
        true
    }

    fn event(&mut self, event: Option<MicroEvent>, _after: &Machine) -> Flow {
        self.events.extend(event);
        Flow::Continue
    }
}

/// Builds a stretch of the log from a base: keeps every event, and a
/// checkpoint wherever one is due.
struct Builder {
    /// The events before the stretch.
    base: u64,
    /// The stretch's events so far.
    events: Vec<MicroEvent>,
    /// The op being applied.
    op: OpIndex,
    /// The position of the last checkpoint so far.
    last: EventPos,
    /// The checkpoints taken in the stretch.
    checkpoints: Vec<(EventPos, Checkpoint)>,
}

impl Builder {
    /// The id the next event gets.
    fn next_id(&self) -> u64 {
        self.base
            .saturating_add(u64::try_from(self.events.len()).unwrap_or(u64::MAX))
    }

    /// The position after the events so far.
    fn pos(&self) -> EventPos {
        EventPos::new_unchecked(u32::try_from(self.next_id()).unwrap_or(u32::MAX))
    }

    /// Keeps `machine` as the checkpoint at `pos`, going on with op `op`
    /// after its `progress`, if one is due there.
    fn keep(&mut self, pos: EventPos, machine: &Machine, op: OpIndex, progress: OpProgress) {
        if due(self.last, pos) {
            self.last = pos;
            let checkpoint = Checkpoint {
                machine: machine.clone(),
                op,
                progress,
            };
            self.checkpoints.push((pos, checkpoint));
        }
    }
}

impl EventSink for Builder {
    fn records(&self) -> bool {
        true
    }

    fn event(&mut self, event: Option<MicroEvent>, _after: &Machine) -> Flow {
        self.events.extend(event);
        Flow::Continue
    }

    fn boundary(&mut self, machine: &Machine, progress: OpProgress) {
        self.keep(self.pos(), machine, self.op, progress);
    }
}

/// Counts events without recording them, and stops the op after the last
/// one it waits for.
struct Stopper {
    /// The events still to come before it stops.
    remaining: u32,
}

impl EventSink for Stopper {
    fn records(&self) -> bool {
        false
    }

    fn event(&mut self, _event: Option<MicroEvent>, _after: &Machine) -> Flow {
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining == 0 {
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
    use crate::limits::{CHECKPOINT_INTERVAL, INSTRUCTION_BUDGET, MAX_RUN_STEPS};
    use crate::op::ProgramFormat;

    /// A loop without an exit or a memory access: one event per instruction.
    const ENDLESS: &str = "        add r1, r0, #1\nloop:   bnez r1, loop";

    fn endless() -> Op {
        Op::Program {
            source: ENDLESS.to_owned(),
            format: ProgramFormat::Asm,
        }
    }

    fn pos(value: u32) -> EventPos {
        EventPos::new_unchecked(value)
    }

    /// A session whose log ends one event short of the first checkpoint
    /// interval, with the loop loaded and budget left, then a `StepAccess`
    /// that executes the loop across the interval.
    fn across_an_interval() -> Session {
        let mut session = Session::with_config(Preset::DirectMapped8.config()).unwrap();
        let target = CHECKPOINT_INTERVAL - 1;
        session.apply(endless()).unwrap();
        loop {
            let at = session.position().pos.get();
            if at >= target {
                break;
            }
            let left = INSTRUCTION_BUDGET - session.current.cpu.executed;
            if left <= 1 {
                session.apply(endless()).unwrap();
                continue;
            }
            let n = MAX_RUN_STEPS.min(left - 1).min(target - at);
            session.apply(Op::Run { n }).unwrap();
        }
        session.apply(Op::StepAccess).unwrap();
        session
    }

    #[test]
    fn a_checkpoint_is_taken_inside_an_op_that_crosses_an_interval() {
        let mut session = across_an_interval();
        let last = OpIndex::new_unchecked(session.op_count() - 1);
        let (at, checkpoint) = session.checkpoints.at_or_before(pos(CHECKPOINT_INTERVAL));
        assert_eq!(at, pos(CHECKPOINT_INTERVAL));
        assert_eq!(checkpoint.op, last);
        assert_eq!(checkpoint.progress.get(), 1);
        let end = session.end().get();
        assert!(session.checkpoints.len() <= (end / CHECKPOINT_INTERVAL) as usize);
        // Seeks on either side of it, and from it, reach the fold's machines.
        let initial = session.initial.clone();
        let ops = session.ops.clone();
        for at in (CHECKPOINT_INTERVAL - 2..=CHECKPOINT_INTERVAL + 3).chain([end, 0, end - 1]) {
            session.seek(pos(at));
            assert_eq!(
                Ok(&session.current),
                Session::replay_to(&initial, &ops, pos(at)).as_ref(),
                "at {at}"
            );
        }
    }

    #[test]
    fn checkpoints_after_the_point_an_op_is_appended_are_dropped() {
        let mut session = across_an_interval();
        assert!(session.checkpoints.len() >= 1);
        session.seek(pos(CHECKPOINT_INTERVAL / 2));
        let Some(op) = session.op_holding(session.cursor) else {
            panic!("the cursor is inside the log");
        };
        let base = session.op_end(op).unwrap();
        session
            .apply(Op::Issue {
                kind: crate::op::AccessKind::Read,
                addr: crate::units::Addr::new_unchecked(0x0A4),
                value: None,
            })
            .unwrap();
        assert_eq!(session.checkpoints.last_at_or_before(session.end()), pos(0));
        assert!(base < pos(CHECKPOINT_INTERVAL));
        assert_eq!(session.checkpoints.len(), 0);
    }

    #[test]
    fn the_fold_numbers_a_new_runs_events_after_the_old_ones() {
        let config = Preset::DirectMapped8.config();
        let ops = [
            endless(),
            Op::Step,
            Op::Config {
                config: Preset::TwoWayLru.config(),
            },
            Op::Step,
        ];
        let replay = Session::replay(&config, &ops).unwrap();
        let numbers: Vec<(u64, u32, u32, Option<u32>, &str)> = replay
            .events
            .iter()
            .map(|event| {
                (
                    event.id,
                    event.op_index.get(),
                    event.run,
                    event.entry,
                    event.label.as_str(),
                )
            })
            .collect();
        assert_eq!(
            numbers,
            [
                (0, 0, 1, None, ""),
                (1, 1, 1, Some(1), "1.1"),
                (2, 2, 2, None, ""),
                (3, 3, 2, Some(1), "1.1"),
            ]
        );
        assert_eq!(replay.machine.run, 2);
        assert_eq!(replay.machine.entries_in_run, 1);
        assert!(replay.machine.program.is_some());
    }
}
