//! The session: the log of ops over an initial configuration, the events
//! they produced, and a cursor on those events. It is the only owner of
//! machine state.
//!
//! The log is append-only in the sense the tool promises: an op is checked
//! against the machine at the point it is appended before anything changes,
//! so a refused op leaves the log, the undone ops after the cursor and the
//! cursor as they were; an accepted op drops the undone ops, is appended, and
//! the cursor goes to its last event. No call rewrites an op the log keeps.
//! Every configuration change is an op of its own, which starts a run. The
//! log never holds more ops than a link can, nor more JSON than a saved file
//! can, wherever the cursor is and in either mode, so every session can be
//! saved, and shared by link whenever its link is short enough to open.
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
//!
//! The session also answers what the page draws, without the page computing
//! anything: the snapshot at the cursor, the timeline of the displayed run,
//! windows of the memory table and of the log, and for what the student
//! types the issue draft, the configuration bar and the editors' drafts,
//! with the plan of an edit at the cursor. Each entry of the log is
//! recorded once, as its events are, with its label, its outcome and a
//! summary of what it changed; and the cursor keeps the values its entry
//! started from, so the highlight of the net change so far costs nothing to
//! draw.
//!
//! A session is written out as a share document, keeping each op's JSON as
//! the op is appended, and with it the saved file's size, and is built again
//! from one a slice of events at a time, each op checked as it was when it
//! was appended.
//!
//! A guided module's unanswered prediction gates the session: while the
//! gate is set, every op is refused and the cursor cannot pass the point
//! where the prediction began. Each op appended is numbered with a serial
//! that is never reused, which is how a module tells an op made since one
//! of its steps began.

mod checkpoints;
mod cursor;
mod edit;
mod gate;
mod load;
mod log;
mod timeline;

use std::collections::BTreeSet;
use std::ops::Range;

use serde::Serialize;
use thiserror::Error;

pub use cursor::{Boundary, Position, StepOutcome};
pub use gate::Gate;
pub use timeline::{EntryKind, EntryOutcome, EntryView, EntryViews, EventRow, Tense, Timeline};

pub(crate) use load::{position_in_op, Loader};
pub(crate) use timeline::EntryRecord;

use checkpoints::{due, Checkpoint, CheckpointRef, Checkpoints};
use cursor::Cursor;
use log::{op_start, Extension, LastAccess, Log};
use timeline::{Entries, EntryBase};

use crate::config::{Config, ConfigError, Timing};
use crate::engine::{self, Blocked, Emitter, EventSink, Flow, OpProgress};
use crate::event::{EventKind, MicroEvent};
use crate::explain;
use crate::geometry::{AccessError, Decoded};
use crate::isa::ParseResult;
use crate::machine::Machine;
use crate::op::{Op, OpError, ProgramFormat};
use crate::share::{self, FileRoom};
use crate::snapshot::{
    self, BusView, CacheView, ControlAction, Controls, CpuView, Labels, MemoryLineView, MemoryRows,
    MemoryView, ProgramView, RequestView, State,
};
use crate::stats::Stats;
use crate::units::{EventPos, LineAddr, OpIndex};

/// A log of ops over an initial configuration, with an event cursor. The
/// only owner of machine state.
#[derive(Clone, Debug)]
pub struct Session {
    /// The first run's configuration, which is not an op.
    initial: Config,
    /// What the page shows that no op records.
    view: ViewSettings,
    /// Set by a guided module while its prediction is unanswered: every op
    /// is refused, and the cursor stays at or before the gate's limit.
    gate: Option<Gate>,
    /// Every op, the undone ones after the cursor included, and what was
    /// recorded of running them.
    log: Log,
    /// The cursor's position, the machine there, and what its entry started
    /// from.
    cursor: Cursor,
}

/// The settings of the page that no op records and the fold never reads.
#[derive(Clone, Debug)]
struct ViewSettings {
    /// Whether the student chose Advanced rather than Basics.
    advanced: bool,
    /// The memory lines the student asked to see, the "Jump to" rows.
    watch: BTreeSet<LineAddr>,
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
            view: ViewSettings {
                advanced: initial.advanced,
                watch: BTreeSet::new(),
            },
            log: Log::new(&initial, origin.clone()),
            initial,
            gate: None,
            cursor: Cursor::new(origin),
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
    /// limit leaves no room for, an op past the most a link can hold, and
    /// an op a saved file has no room for.
    pub fn replay(initial: &Config, ops: &[Op]) -> Result<Replay, ReplayError> {
        let origin = Machine::start(initial.clone())?;
        let start = Start::before_op(
            EventPos::new_unchecked(0),
            &origin,
            OpIndex::new_unchecked(0),
        );
        let file = Some(FileRoom::new(initial));
        let mut sink = Recorder::default();
        let machine = resume_into(start, ops, &mut sink, file)?;
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
        let start = Start::before_op(
            EventPos::new_unchecked(0),
            &origin,
            OpIndex::new_unchecked(0),
        );
        let file = Some(FileRoom::new(initial));
        replay_capturing(start, ops, pos, None, file).map(|(machine, _)| machine)
    }

    /// The first run's configuration.
    pub fn initial(&self) -> &Config {
        &self.initial
    }

    /// The displayed run's configuration: the configuration of the run the
    /// cursor is in.
    pub fn config(&self) -> &Config {
        &self.cursor.machine().config
    }

    /// Whether the page shows Advanced: the mode the student chose, or
    /// Advanced whatever the choice while the displayed run's configuration
    /// needs it, as after undoing back into a write-through run.
    pub fn advanced(&self) -> bool {
        self.view.advanced || !self.config().fits_basics()
    }

    /// The machine at the cursor.
    pub fn machine(&self) -> &Machine {
        self.cursor.machine()
    }

    /// Every op of the log, the undone ones after the cursor included.
    pub fn ops(&self) -> &[Op] {
        self.log.ops()
    }

    /// How many ops the log holds, the undone ones after the cursor
    /// included.
    pub fn op_count(&self) -> u32 {
        self.log.op_count()
    }

    /// Every event up to the cursor, in order.
    pub fn events(&self) -> &[MicroEvent] {
        self.log
            .events()
            .get(..slot(self.cursor.pos()))
            .unwrap_or_default()
    }

    /// The events whose ids run from `range.start` up to but not including
    /// `range.end`, as far as the log holds them, the undone ones after the
    /// cursor included: a window of the log that does not grow with it.
    pub fn events_range(&self, range: Range<u32>) -> &[MicroEvent] {
        let events = self.log.events();
        let end = usize::try_from(range.end)
            .unwrap_or(usize::MAX)
            .min(events.len());
        let start = usize::try_from(range.start).unwrap_or(usize::MAX).min(end);
        events.get(start..end).unwrap_or_default()
    }

    /// What the page draws at the cursor: the machine's views, the event
    /// there, the highlight of the cursor's entry and its summary, the live
    /// controls and the texts over the workbench.
    pub fn state(&self) -> State {
        let machine = self.cursor.machine();
        let advanced = self.advanced();
        let position = self.position();
        let event = self.log.event_at(self.cursor.pos());
        let entry = self
            .log
            .record_at(self.cursor.pos())
            .map(|(_, record)| record);
        let highlight = self.highlight();
        let rows = MemoryRows::new(machine, &self.view.watch);
        let bus = snapshot::bus_line(machine, event);
        let (origin, end) = self.log.run_bounds(machine.run);
        let steps = self
            .log
            .steps_through(end)
            .saturating_sub(self.log.steps_through(origin));
        State {
            config: machine.config.clone(),
            advanced,
            geometry: machine.geometry,
            event: event.cloned(),
            cache: CacheView::new(machine),
            memory: MemoryView::new(machine, &rows, bus),
            bus: BusView::new(machine, event),
            request: RequestView::new(machine, entry),
            cpu: CpuView::new(machine, entry, &position),
            program: ProgramView::new(machine),
            stats: Stats::new(machine, advanced, &highlight),
            changed_summary: explain::changed_summary(&machine.geometry, &highlight),
            highlight,
            controls: self.controls(),
            labels: Labels {
                status_text: explain::status_line(advanced, steps, machine.config.timing.is_some()),
                config_summary: machine.config.summary(advanced),
            },
            position,
        }
    }

    /// The Stats region at the cursor, in the mode shown, with the rows the
    /// cursor's entry has changed so far marked.
    pub fn stats(&self) -> Stats {
        Stats::new(self.cursor.machine(), self.advanced(), &self.highlight())
    }

    /// The memory table's rows from its `first`, at most `count` of them,
    /// for the rows in view: the lines the displayed run has touched and the
    /// lines watched, in ascending order.
    pub fn memory_lines(&self, first: u32, count: u32) -> Vec<MemoryLineView> {
        let machine = self.cursor.machine();
        let bus = snapshot::bus_line(machine, self.log.event_at(self.cursor.pos()));
        MemoryRows::new(machine, &self.view.watch)
            .iter()
            .skip(usize::try_from(first).unwrap_or(usize::MAX))
            .take(usize::try_from(count).unwrap_or(usize::MAX))
            .map(|(line, watched)| MemoryLineView::new(machine, line, watched, bus))
            .collect()
    }

    /// Replaces the memory lines the student asked to see, the "Jump to"
    /// rows, with `lines`. A presentation setting, as the mode is: no op
    /// records it and no run reads it. The rows show each line that fits
    /// the displayed run's addresses, from the start of its line, whether
    /// the run has touched it or not.
    pub fn set_memory_watch(&mut self, lines: &[LineAddr]) {
        // Extended rather than collected: collecting sorts the lines first,
        // with a sort the wasm would carry for this type alone.
        self.view.watch.clear();
        self.view.watch.extend(lines.iter().copied());
    }

    /// `addr` split into its tag, index and offset under the displayed run's
    /// configuration.
    ///
    /// # Errors
    ///
    /// [`AccessError::OutOfRange`] if `addr` does not fit the address width.
    pub fn decode(&self, addr: u32) -> Result<Decoded, AccessError> {
        self.cursor.machine().geometry.decode(addr)
    }

    /// `source` parsed as a program or a trace under the displayed run's
    /// configuration: the drawer's summary, or its errors.
    pub fn parse_program(&self, source: &str, format: ProgramFormat) -> ParseResult {
        self.cursor.machine().geometry.parse_program(source, format)
    }

    /// The most recent timing in the log up to the cursor: the timing of
    /// the latest timed configuration among the `Config` ops the cursor has
    /// reached, or the initial configuration's; none if neither is timed.
    pub(crate) fn recent_timing(&self) -> Option<Timing> {
        let reached = self.log.op_holding(self.cursor.pos()).map_or(0, |op| {
            usize::try_from(op.get())
                .unwrap_or(usize::MAX)
                .saturating_add(1)
        });
        let ops = self.log.ops();
        ops.get(..reached.min(ops.len()))
            .unwrap_or_default()
            .iter()
            .rev()
            .find_map(|op| match op {
                Op::Config { config } => config.timing,
                _ => None,
            })
            .or(self.initial.timing)
    }

    /// Which controls are live: Undo away from position 0, Redo while there
    /// is an undo stop ahead, Reset while the displayed run has an entry up
    /// to the cursor, and the rest always. While a gate is set, Redo only
    /// to a stop at or before its limit, and none of the controls that
    /// append an op or move the cursor on.
    fn controls(&self) -> Controls {
        let mut enabled = Vec::new();
        if self.cursor.pos().get() > 0 {
            enabled.push(ControlAction::Undo);
        }
        let limit = self.limit();
        if self
            .redo_target()
            .is_some_and(|target| limit.is_none_or(|limit| target <= limit))
        {
            enabled.push(ControlAction::Redo);
        }
        if let Some(gate) = &self.gate {
            return Controls {
                enabled,
                seek_limit: Some(gate.seek_limit()),
                guard: Some(gate.message().to_owned()),
            };
        }
        enabled.extend([
            ControlAction::Step,
            ControlAction::StepAccess,
            ControlAction::Run,
        ]);
        let run = self.log.run_entries(self.cursor.machine().run);
        let reached = self
            .log
            .entries()
            .get(run)
            .and_then(<[EntryRecord]>::first)
            .is_some_and(|record| record.first <= self.cursor.pos());
        if reached {
            enabled.push(ControlAction::Reset);
        }
        enabled.extend([
            ControlAction::Issue,
            ControlAction::Edit,
            ControlAction::Configure,
            ControlAction::Program,
            ControlAction::Load,
        ]);
        Controls {
            enabled,
            seek_limit: None,
            guard: None,
        }
    }

    /// Appends `op` at the end of the op holding the cursor, or at position
    /// 0 if there is none, and moves the cursor to the op's last event.
    /// Returns the events appended, as the log holds them.
    ///
    /// The op is first checked against the gate, if one is set, then
    /// against the session's event limit, then against the ops a link can
    /// hold, then against the bytes a saved file can hold, counting the ops
    /// kept before it, and then against the machine there. Only once it is
    /// accepted is the log dropped after that point: the op holding the
    /// cursor is kept whole, and the undone ops after it go. So Undo makes
    /// room for an op when the log is as long as a link can hold, or as
    /// large as a file can.
    ///
    /// # Errors
    ///
    /// The [`OpError`] refusing the op. A refused op changes nothing: the
    /// log, the undone ops and the cursor are as they were.
    pub fn apply(&mut self, op: Op) -> Result<&[MicroEvent], OpError> {
        self.apply_all(vec![op])
    }

    /// A change from the configuration bar to `config`.
    ///
    /// A configuration equal to the displayed run's, apart from the
    /// presentation flag `advanced`, changes nothing. While the displayed run
    /// has no entry and the cursor is at the end of the log, the change is
    /// applied at once by appending `Op::Config`, as [`Session::apply`]
    /// would: every change is an op of its own, with its own run and its own
    /// undo stop, and none replaces another. Otherwise nothing changes, and
    /// the result asks the student first, with the question worded here,
    /// unless the log has no room for the `Config` op: then the change is
    /// refused at once, as the editors refuse an edit before planning it,
    /// rather than asked about and refused once the student agrees.
    ///
    /// # Errors
    ///
    /// [`OpError::Gated`] while a gate is set, [`OpError::Config`] if
    /// `config` is invalid, the error refusing the `Config` op when it would
    /// be applied at once, and otherwise [`OpError::SessionFull`],
    /// [`OpError::LinkFull`] or [`OpError::FileFull`] when the log has no
    /// room for it.
    pub fn propose_config(&mut self, config: Config) -> Result<ConfigChange, OpError> {
        if self.gate.is_some() {
            return Err(OpError::Gated);
        }
        config.validate()?;
        let displayed = self.config();
        let same = Config {
            advanced: displayed.advanced,
            ..config.clone()
        };
        if same == *displayed {
            return Ok(ConfigChange::Applied);
        }
        if self.cursor.pos() == self.log.end() && self.at_run_origin(self.cursor.pos()) {
            self.apply(Op::Config { config })?;
            return Ok(ConfigChange::Applied);
        }
        self.room(&Op::Config {
            config: config.clone(),
        })?;
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
    /// new run. Once that run is applied, switching again succeeds. The
    /// question is asked whether or not the new run's op could be appended:
    /// while a gate is set, or once the log has no room for it, the op is
    /// refused when the student agrees, as Reset is.
    pub fn set_advanced(&mut self, on: bool) -> ConfigChange {
        let displayed = self.config();
        if !on && !displayed.fits_basics() {
            return ConfigChange::NeedsReset {
                config: displayed.to_basics(),
                message: self.reset_question(),
            };
        }
        self.view.advanced = on;
        ConfigChange::Applied
    }

    /// The question asked before a configuration change starts a new run: it
    /// counts the steps of the displayed run the log keeps, and the undone
    /// steps a new op would drop.
    fn reset_question(&self) -> String {
        let base = slot(self.base().0);
        let run = self.cursor.machine().run;
        let steps = |events: &[MicroEvent]| {
            let count = events
                .iter()
                .filter(|event| event.kind != EventKind::Setup)
                .count();
            u32::try_from(count).unwrap_or(u32::MAX)
        };
        let events = self.log.events();
        let (kept, dropped) = events.split_at(base.min(events.len()));
        let run_start = kept
            .iter()
            .rposition(|event| event.run != run)
            .map_or(0, |last_before| last_before.saturating_add(1));
        explain::needs_reset(
            steps(kept.get(run_start..).unwrap_or_default()),
            steps(dropped),
        )
    }

    /// Appends `ops` together at the end of the op holding the cursor, as
    /// [`Session::apply`] appends one: each is checked against the machine
    /// the ones before it leave, and none is appended unless all are
    /// accepted. Each is an op of its own, with its own undo stop. The
    /// cursor goes to the last one's last event. Returns the events
    /// appended, as the log holds them.
    ///
    /// # Errors
    ///
    /// The [`OpError`] refusing the first op refused; nothing changes.
    pub(crate) fn apply_all(&mut self, ops: Vec<Op>) -> Result<&[MicroEvent], OpError> {
        let plan = self.plan(ops, Keep::End)?;
        let first = slot(plan.base);
        self.commit(plan);
        Ok(self.log.events().get(first..).unwrap_or_default())
    }

    /// Checks `ops` in turn, each against the machine the ones before it
    /// leave, from the end of the op holding the cursor, and executes them
    /// there, without changing the session. The plan keeps what `keep`
    /// says besides the machine after the last op.
    ///
    /// # Errors
    ///
    /// [`OpError::Gated`] while a gate is set, and otherwise the
    /// [`OpError`] refusing the first op refused.
    fn plan(&self, ops: Vec<Op>, keep: Keep) -> Result<Plan, OpError> {
        if self.gate.is_some() {
            return Err(OpError::Gated);
        }
        let (base, kept) = self.base();
        let mut machine = self.machine_at(base).map_err(ReplayError::into_op)?;
        let mut builder = Builder {
            base: u64::from(base.get()),
            events: Vec::new(),
            op: OpIndex::new_unchecked(kept),
            last: self.log.checkpoints().last_at_or_before(base),
            checkpoints: Vec::new(),
            entries: Entries::new(&machine, self.log.steps_through(base)),
        };
        let mut file = self.log.room_after(kept);
        let mut ops_json = Vec::with_capacity(ops.len());
        let mut op_ends = Vec::with_capacity(ops.len());
        let last = last_op(kept, ops.len());
        let mut last_start = None;
        for (index, op) in (kept..).zip(&ops) {
            let index = OpIndex::new_unchecked(index);
            if keep == Keep::LastOpStart && index == last {
                last_start = Some(machine.clone());
            }
            builder.op = index;
            let first = builder.next_id();
            ops_json.push(check_room(first, index, &mut file, op)?);
            let mut out = Emitter::new(&mut builder, index, first);
            engine::apply(&mut machine, op, &mut out)?;
            let end = builder.pos();
            op_ends.push(end);
            let next = OpIndex::new_unchecked(index.get().saturating_add(1));
            builder.keep(end, &machine, next, OpProgress::START);
        }
        let Builder {
            events,
            checkpoints,
            entries,
            ..
        } = builder;
        let (records, last_base) = entries.finish();
        Ok(Plan {
            base,
            kept,
            extension: Extension {
                ops,
                ops_json,
                events,
                op_ends,
                checkpoints,
                records,
            },
            last_base,
            last_start,
            end: machine,
        })
    }

    /// Makes `plan` the session's: drops the log after its base, appends its
    /// ops, their JSON, events and entries, each op with the next serial, and
    /// puts the cursor on its last event.
    fn commit(&mut self, plan: Plan) {
        self.log.truncate_after(plan.kept);
        self.log.append(plan.extension);
        self.cursor.place(self.log.end(), plan.end, plan.last_base);
    }

    /// Whether the log has room for `op` where a new op would go, as
    /// [`Session::apply`] checks after the gate: under the session's event
    /// limit, then under the ops a link can hold, then under the bytes a
    /// saved file can hold, counting the ops kept before it. The editors ask
    /// the same, so that they refuse as `apply` does.
    ///
    /// # Errors
    ///
    /// The [`Room`] that is lacking, the first that fails.
    fn room(&self, op: &Op) -> Result<(), Room> {
        let (base, kept) = self.base();
        let mut file = self.log.room_after(kept);
        check_room(
            u64::from(base.get()),
            OpIndex::new_unchecked(kept),
            &mut file,
            op,
        )
        .map(drop)
    }

    /// Where a new op goes: the end of the op holding the cursor, or
    /// position 0 if there is none; and how many ops the log keeps before
    /// it.
    fn base(&self) -> (EventPos, u32) {
        match self.log.op_holding(self.cursor.pos()) {
            Some(op) => (
                self.log.op_end(op).unwrap_or(self.cursor.pos()),
                op.get().saturating_add(1),
            ),
            None => (EventPos::new_unchecked(0), 0),
        }
    }

    /// Where a quiet replay to `target` starts: the cursor, when it is at
    /// the end of an op and lies after the nearest checkpoint at or before
    /// `target` without passing `target`, and otherwise that checkpoint.
    fn replay_start(&self, target: EventPos) -> Start<'_> {
        let checkpoint = Start::from(self.log.checkpoints().at_or_before(target));
        let cursor = self.cursor.pos();
        if checkpoint.pos < cursor && cursor <= target && self.log.is_op_end(cursor) {
            let done = OpIndex::new_unchecked(self.log.ops_done(cursor));
            Start::before_op(cursor, self.cursor.machine(), done)
        } else {
            checkpoint
        }
    }

    /// The machine at `pos`, replayed quietly from the nearest point before
    /// it that the session holds a machine for: a checkpoint, or the cursor
    /// when it is at the end of an op.
    fn machine_at(&self, pos: EventPos) -> Result<Machine, ReplayError> {
        if pos == self.cursor.pos() {
            return Ok(self.cursor.machine().clone());
        }
        replay_quietly(self.replay_start(pos), self.log.ops(), pos)
    }

    /// The machines at `capture` and at `pos`, `capture` not after `pos`,
    /// by one quiet replay that passes through both, from the nearest point
    /// before `capture` that the session holds a machine for.
    fn machines_at(
        &self,
        capture: EventPos,
        pos: EventPos,
    ) -> Result<(Machine, Machine), ReplayError> {
        let start = self.replay_start(capture);
        let (machine, captured) =
            replay_capturing(start, self.log.ops(), pos, Some(capture), None)?;
        let captured = captured.unwrap_or_else(|| machine.clone());
        Ok((captured, machine))
    }

    /// Moves the cursor to `pos`, recomputing the machine there and, when
    /// the cursor enters another entry, the values before that entry, which
    /// the highlight compares with: from the machine at the cursor when the
    /// entry starts there, as it does when Space steps into it, and
    /// otherwise from the same replay that reaches `pos`.
    ///
    /// # Errors
    ///
    /// The [`ReplayError`] of a replay of ops the session has accepted,
    /// which only a bug could cause; the cursor then stays where it was.
    fn move_to(&mut self, pos: EventPos) -> Result<(), ReplayError> {
        if pos == self.cursor.pos() {
            return Ok(());
        }
        let entry = self
            .log
            .record_at(pos)
            .map(|(_, record)| (record.first, record.last));
        let known = entry.is_some_and(|(first, _)| {
            self.cursor
                .entry_base()
                .is_some_and(|base| base.first == first)
        });
        match entry {
            Some((first, last)) if !known => {
                let start = EventPos::new_unchecked(first.get().saturating_sub(1));
                let (at_start, machine) = if start == self.cursor.pos() {
                    (None, self.machine_at(pos)?)
                } else {
                    let (at_start, machine) = self.machines_at(start, pos)?;
                    (Some(at_start), machine)
                };
                let events = self.log.events_between(first, last);
                let base = EntryBase::from_start(
                    first,
                    events,
                    at_start.as_ref().unwrap_or(self.cursor.machine()),
                );
                self.cursor.place(pos, machine, Some(base));
            }
            Some(_) => {
                let machine = self.machine_at(pos)?;
                self.cursor.place_in_entry(pos, machine);
            }
            None => {
                let machine = self.machine_at(pos)?;
                self.cursor.place(pos, machine, None);
            }
        }
        Ok(())
    }

    /// Moves the cursor to `pos` as [`Session::move_to`] does, and says
    /// whether it moved.
    ///
    /// Every replay the session makes is of ops it accepted, each checked
    /// against the machine it applies to when it was appended, so one that
    /// fails could only be caused by a bug, which the core can neither log
    /// nor panic on. Whatever moves the cursor with [`Session::move_to`]
    /// without being able to report why it did not, as Undo, Redo, seeking
    /// and scrubbing cannot, goes through here: a replay that fails leaves
    /// the session as it was.
    fn move_or_stay(&mut self, pos: EventPos) -> bool {
        self.move_to(pos).is_ok()
    }

    /// The event the cursor rests on at `pos`: the one whose id is `pos − 1`;
    /// none at position 0.
    pub(crate) fn event_at(&self, pos: EventPos) -> Option<&MicroEvent> {
        self.log.event_at(pos)
    }

    /// Whether `pos` is a run's origin: position 0, where the first run
    /// starts, or the `Setup` event of a `Config` op, which starts a new one.
    fn at_run_origin(&self, pos: EventPos) -> bool {
        match self.log.event_at(pos) {
            None => true,
            Some(event) => {
                event.kind == EventKind::Setup
                    && matches!(self.log.op(event.op_index), Some(Op::Config { .. }))
            }
        }
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

/// What a plan keeps of the machine, besides the machine after its last op,
/// which every plan keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Keep {
    /// Nothing more.
    End,
    /// The machine at the start of the last op.
    LastOpStart,
}

/// Ops checked and executed from a point of the log, ready to be appended
/// there.
struct Plan {
    /// Where the ops go.
    base: EventPos,
    /// How many ops the log keeps before them.
    kept: u32,
    /// The ops, and what running them produced, for the log.
    extension: Extension,
    /// The base of the entry of their last event, if it belongs to one.
    last_base: Option<EntryBase>,
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

impl<'m> Start<'m> {
    /// The start of a replay that goes on with op `op` from its start, at
    /// `pos`, where `machine` is.
    fn before_op(pos: EventPos, machine: &'m Machine, op: OpIndex) -> Self {
        Start {
            pos,
            machine,
            op,
            progress: OpProgress::START,
        }
    }
}

impl<'m> From<(EventPos, CheckpointRef<'m>)> for Start<'m> {
    /// The start of a replay from a checkpoint and the position it is at.
    fn from((pos, checkpoint): (EventPos, CheckpointRef<'m>)) -> Self {
        Start {
            pos,
            machine: checkpoint.machine,
            op: checkpoint.op,
            progress: checkpoint.progress,
        }
    }
}

/// Replays `ops` quietly from `start` and returns the machine after the
/// event at `target`, or after the last op if the ops end first.
fn replay_quietly(start: Start<'_>, ops: &[Op], target: EventPos) -> Result<Machine, ReplayError> {
    replay_capturing(start, ops, target, None, None).map(|(machine, _)| machine)
}

/// Replays `ops` quietly from `start` as [`replay_quietly`] does, keeping a
/// copy of the machine at `capture` on the way, when the replay passes that
/// position. With `file`, the room a saved file has at `start`, each op the
/// replay starts is checked for room first, as the fold checks it; a
/// session's own replays leave it out, since they replay ops it accepted.
fn replay_capturing(
    start: Start<'_>,
    ops: &[Op],
    target: EventPos,
    capture: Option<EventPos>,
    file: Option<FileRoom>,
) -> Result<(Machine, Option<Machine>), ReplayError> {
    let at_start = capture
        .filter(|&capture| capture == start.pos)
        .map(|_| start.machine.clone());
    let remaining = target.get().saturating_sub(start.pos.get());
    if remaining == 0 {
        return Ok((start.machine.clone(), at_start));
    }
    let mut sink = Stopper {
        remaining,
        seen: 0,
        capture: capture
            .and_then(|capture| capture.get().checked_sub(start.pos.get()))
            .filter(|&events| events > 0),
        captured: at_start,
    };
    let machine = resume_into(start, ops, &mut sink, file)?;
    Ok((machine, sink.captured))
}

/// Replays `ops` from `start`, on a copy of its machine, from its op and
/// progress, handing each event to `sink`, until the ops end or the sink
/// is finished after an op; a sink stops the op it wants to end inside.
/// Gives the machine the replay ends on. With `file`, the room a saved file
/// has at `start`, each op started is first checked for room, as the fold
/// checks it.
fn resume_into(
    start: Start<'_>,
    ops: &[Op],
    sink: &mut dyn Waiting,
    mut file: Option<FileRoom>,
) -> Result<Machine, ReplayError> {
    let mut machine = start.machine.clone();
    let mut next_id = u64::from(start.pos.get());
    let mut progress = start.progress;
    let first = usize::try_from(start.op.get()).unwrap_or(usize::MAX);
    for (index, op) in (0_u32..).zip(ops).skip(first) {
        let index = OpIndex::new_unchecked(index);
        if let Some(file) = file.as_mut().filter(|_| progress == OpProgress::START) {
            check_room(next_id, index, file, op).map_err(|room| ReplayError::Op {
                index,
                error: room.into(),
            })?;
        }
        let mut out = Emitter::new(&mut *sink, index, next_id);
        engine::resume(&mut machine, op, progress, &mut out)
            .map_err(|error| ReplayError::Op { index, error })?;
        next_id = out.next_id();
        progress = OpProgress::START;
        if sink.finished() {
            break;
        }
    }
    Ok(machine)
}

/// A sink that [`resume_into`] hands the events of a replay to, and that says
/// after each op whether it has what it waits for. One replay serves every
/// such sink, rather than one copy of it for each.
trait Waiting: EventSink {
    /// Whether the replay has what the sink waits for, and may end.
    fn finished(&self) -> bool;
}

/// The last of `count` ops appended after the `kept` the log keeps.
fn last_op(kept: u32, count: usize) -> OpIndex {
    let appended = u32::try_from(count).unwrap_or(u32::MAX);
    OpIndex::new_unchecked(kept.saturating_add(appended).saturating_sub(1))
}

/// The position after `count` events.
fn position(count: usize) -> EventPos {
    EventPos::new_unchecked(u32::try_from(count).unwrap_or(u32::MAX))
}

/// The number of events before `pos`, as an index into the events.
fn slot(pos: EventPos) -> usize {
    usize::try_from(pos.get()).unwrap_or(usize::MAX)
}

/// A count of ops, as an index into the ops.
fn slot_of(ops: u32) -> usize {
    usize::try_from(ops).unwrap_or(usize::MAX)
}

/// Which limit of the log an op finds no room under, in the order they are
/// checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Room {
    /// The session's events leave no room for another entry.
    Events,
    /// The log already holds as many ops as a link can.
    Ops,
    /// The op's JSON does not fit in what is left of a saved file.
    File,
}

impl From<Room> for OpError {
    fn from(room: Room) -> OpError {
        match room {
            Room::Events => OpError::SessionFull,
            Room::Ops => OpError::LinkFull,
            Room::File => OpError::FileFull,
        }
    }
}

impl From<Room> for Blocked {
    fn from(room: Room) -> Blocked {
        match room {
            Room::Events => Blocked::Full,
            Room::Ops => Blocked::LinkFull,
            Room::File => Blocked::FileFull,
        }
    }
}

/// Checks that a log of `events` events whose ops before it number `index`
/// has room for `op`, as every op is checked before the machine's rules:
/// room for one more entry, then for one more op a link can hold, then for
/// the op's JSON in a saved file, whose room `file` it takes. Gives the op's
/// JSON, as a share document writes it.
///
/// # Errors
///
/// The [`Room`] that is lacking, the first that fails; nothing is taken.
fn check_room(events: u64, index: OpIndex, file: &mut FileRoom, op: &Op) -> Result<String, Room> {
    if !engine::has_room(events) {
        return Err(Room::Events);
    }
    if !engine::has_op_room(index.get()) {
        return Err(Room::Ops);
    }
    let json = share::op_json(op);
    file.take(&json).map_err(|_| Room::File)?;
    Ok(json)
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

impl Waiting for Recorder {
    fn finished(&self) -> bool {
        false
    }
}

/// Builds a stretch of the log from a base: keeps every event, and a
/// checkpoint wherever one is due.
#[derive(Debug)]
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
    /// The records of the stretch's entries, written as their events come.
    entries: Entries,
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

    fn event(&mut self, event: Option<MicroEvent>, after: &Machine) -> Flow {
        if let Some(event) = &event {
            self.entries.observe(event, after);
        }
        self.events.extend(event);
        Flow::Continue
    }

    fn boundary(&mut self, machine: &Machine, progress: OpProgress) -> Flow {
        self.keep(self.pos(), machine, self.op, progress);
        Flow::Continue
    }
}

/// Counts events without recording them, and stops the op after the last
/// one it waits for, keeping a copy of the machine after one of them if
/// asked.
struct Stopper {
    /// The events still to come before it stops.
    remaining: u32,
    /// The events seen so far.
    seen: u32,
    /// After how many events to keep a copy of the machine.
    capture: Option<u32>,
    /// The copy kept.
    captured: Option<Machine>,
}

impl EventSink for Stopper {
    fn records(&self) -> bool {
        false
    }

    fn event(&mut self, _event: Option<MicroEvent>, after: &Machine) -> Flow {
        self.seen = self.seen.saturating_add(1);
        if self.capture == Some(self.seen) {
            self.captured = Some(after.clone());
        }
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining == 0 {
            Flow::Stop
        } else {
            Flow::Continue
        }
    }
}

impl Waiting for Stopper {
    fn finished(&self) -> bool {
        self.remaining == 0
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::{CacheConfig, Preset, WritePolicy};
    use crate::limits::{CHECKPOINT_INTERVAL, INSTRUCTION_BUDGET, MAX_RUN_STEPS};
    use crate::op::{AccessKind, ProgramFormat};
    use crate::units::{Addr, Word};

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
        let session = Session::with_config(Preset::DirectMapped8.config()).unwrap();
        run_across_an_interval(session)
    }

    /// `session`, its log taken one event short of the first checkpoint
    /// interval by the loop and then across it by a `StepAccess`, as
    /// [`across_an_interval`] does from an empty log.
    fn run_across_an_interval(mut session: Session) -> Session {
        let target = CHECKPOINT_INTERVAL - 1;
        session.apply(endless()).unwrap();
        loop {
            let at = session.position().pos.get();
            if at >= target {
                break;
            }
            let left = INSTRUCTION_BUDGET - session.machine().cpu.executed;
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
        let (at, checkpoint) = session
            .log
            .checkpoints()
            .at_or_before(pos(CHECKPOINT_INTERVAL));
        assert_eq!(at, pos(CHECKPOINT_INTERVAL));
        assert_eq!(checkpoint.op, last);
        assert_eq!(checkpoint.progress.get(), 1);
        let end = session.log.end().get();
        assert!(session.log.checkpoints().len() <= (end / CHECKPOINT_INTERVAL) as usize);
        // Seeks on either side of it, and from it, reach the fold's machines.
        let initial = session.initial.clone();
        let ops = session.ops().to_vec();
        for at in (CHECKPOINT_INTERVAL - 2..=CHECKPOINT_INTERVAL + 3).chain([end, 0, end - 1]) {
            session.seek(pos(at));
            assert_eq!(
                Ok(session.machine()),
                Session::replay_to(&initial, &ops, pos(at)).as_ref(),
                "at {at}"
            );
        }
    }

    #[test]
    fn checkpoints_after_the_point_an_op_is_appended_are_dropped() {
        let mut session = across_an_interval();
        assert!(session.log.checkpoints().len() >= 1);
        session.seek(pos(CHECKPOINT_INTERVAL / 2));
        let Some(op) = session.log.op_holding(session.cursor.pos()) else {
            panic!("the cursor is inside the log");
        };
        let base = session.log.op_end(op).unwrap();
        session
            .apply(Op::Issue {
                kind: AccessKind::Read,
                addr: Addr::new_unchecked(0x0A4),
                value: None,
            })
            .unwrap();
        assert_eq!(
            session
                .log
                .checkpoints()
                .last_at_or_before(session.log.end()),
            pos(0)
        );
        assert!(base < pos(CHECKPOINT_INTERVAL));
        assert_eq!(session.log.checkpoints().len(), 0);
    }

    #[test]
    fn a_checkpoint_shares_memory_lines_until_the_run_writes_them() {
        // Write-through, so a store writes memory's copy of its line.
        let preset = Preset::DirectMapped8.config();
        let config = Config {
            cache: CacheConfig {
                write_policy: WritePolicy::WriteThrough,
                ..preset.cache
            },
            ..preset
        };
        let issue = |kind, at, value: Option<Word>| Op::Issue {
            kind,
            addr: Addr::new_unchecked(at),
            value,
        };
        let mut session = Session::with_config(config).unwrap();
        for at in [0x0A4, 0x1A4, 0x2A4] {
            session.apply(issue(AccessKind::Read, at, None)).unwrap();
        }
        let mut session = run_across_an_interval(session);
        let (at, checkpoint) = session.log.checkpoints().at_or_before(session.log.end());
        assert!(at >= pos(CHECKPOINT_INTERVAL));
        let touched = checkpoint.machine.memory.touched_count() as usize;
        assert_eq!(touched, 3);
        // The checkpoint holds no bytes of its own: every line it has is
        // shared with the machine at the cursor.
        let current = &session.machine().memory;
        assert_eq!(
            checkpoint.machine.memory.lines_shared_with(current),
            touched
        );
        // A store copies the line it writes, and only that line; the
        // checkpoint keeps the value it had.
        let written = Addr::new_unchecked(0x1A4);
        let before = checkpoint.machine.memory.word(written);
        let after = Word::new_unchecked(before.get() + 1);
        session
            .apply(issue(AccessKind::Write, written.get(), Some(after)))
            .unwrap();
        let (_, checkpoint) = session.log.checkpoints().at_or_before(at);
        let memory = &checkpoint.machine.memory;
        assert_eq!(
            memory.lines_shared_with(&session.machine().memory),
            touched - 1
        );
        assert_eq!(memory.word(written), before);
        assert_eq!(session.machine().memory.word(written), after);
        // Seeks from the checkpoint still reach the fold's machines.
        let initial = session.initial.clone();
        let ops = session.ops().to_vec();
        let end = session.log.end().get();
        for at in [at.get(), at.get() + 1, end - 1, end] {
            session.seek(pos(at));
            assert_eq!(
                Ok(session.machine()),
                Session::replay_to(&initial, &ops, pos(at)).as_ref(),
                "at {at}"
            );
        }
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
