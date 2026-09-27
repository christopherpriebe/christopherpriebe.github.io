//! Loading a log a slice at a time: the ops of a share link or a saved file,
//! replayed into a session in stretches of a bounded number of events, so
//! the page can draw between them.
//!
//! A slice ends at an entry boundary: between two ops, or between two
//! instructions of a `Run` or `StepAccess` op, which the next slice resumes
//! from there, as a seek resumes one from a checkpoint. It is bounded by
//! work, counted in events: the events it replays, and the programs its ops
//! parse, which one `Setup` event does not measure, at
//! [`PARSE_BYTES_PER_EVENT`] bytes an event. Each op is checked as
//! [`Session::apply`] checks it, the session's event limit and the room of
//! its saved file included, so a log that could not have been built cannot
//! be loaded either. The session built is the one the ops would have built
//! had they been applied one by one, whatever the slices: the same events,
//! entries and checkpoints, and the same JSON of its ops.
//!
//! A share document places the cursor relative to an op: after the op's
//! `n`-th event, or with `n` = 0 just before the op, so position 0 of a log
//! with ops has a place too.

use super::{check_room, Builder, Entries, Plan, ReplayError, Session};
use crate::engine::{self, Emitter, EventSink, Flow, OpProgress};
use crate::event::MicroEvent;
use crate::limits::{MAX_EVENTS_PER_ENTRY, PARSE_BYTES_PER_EVENT};
use crate::machine::Machine;
use crate::op::Op;
use crate::share::{FileRoom, SavedOps};
use crate::units::{EventPos, OpIndex};

/// A log being replayed a slice at a time into a session on its initial
/// configuration.
#[derive(Debug)]
pub(crate) struct Loader {
    /// The session the log is loaded into, with an empty log: its initial
    /// configuration and mode.
    session: Session,
    /// The ops to replay.
    ops: Vec<Op>,
    /// Each op started so far, as a share document writes it.
    ops_json: Vec<String>,
    /// The room the saved file has after the ops started so far.
    file: FileRoom,
    /// The machine after the events replayed so far.
    machine: Machine,
    /// The events, entries and checkpoints replayed so far.
    builder: Builder,
    /// Where each op replayed so far ends.
    op_ends: Vec<EventPos>,
    /// The op being replayed, or the next one.
    next: u32,
    /// The instructions the op being replayed has executed, when a slice
    /// ended inside it.
    progress: OpProgress,
}

impl Loader {
    /// A loader of `ops` into `session`, a session whose log is empty, as
    /// [`Session::with_config`] makes one: its initial configuration and
    /// mode are the loaded session's. Nothing is replayed yet.
    pub(crate) fn new(session: Session, ops: Vec<Op>) -> Loader {
        // The machine at position 0: the first run's start.
        let (_, origin) = session.checkpoints.at_or_before(EventPos::new_unchecked(0));
        let machine = origin.machine.clone();
        let builder = Builder {
            base: 0,
            events: Vec::new(),
            op: OpIndex::new_unchecked(0),
            last: EventPos::new_unchecked(0),
            checkpoints: Vec::new(),
            entries: Entries::new(&machine, 0),
        };
        let file = session.saved.room_after(0);
        Loader {
            session,
            ops,
            ops_json: Vec::new(),
            file,
            machine,
            builder,
            op_ends: Vec::new(),
            next: 0,
            progress: OpProgress::START,
        }
    }

    /// Replays the next slice: at least one entry, then more while another
    /// entry is sure to fit within `max_events` events of the slice, since
    /// an entry has fewer than [`MAX_EVENTS_PER_ENTRY`], its parsing counted
    /// as events too. The slice ends at an entry boundary, inside an op or
    /// between two. Returns true once every op has been replayed.
    ///
    /// # Errors
    ///
    /// [`ReplayError::Op`] for an op the machine refuses, as the session
    /// would have refused it, the session's event limit and the room of its
    /// saved file included. Nothing after it is replayed.
    pub(crate) fn advance(&mut self, max_events: u32) -> Result<bool, ReplayError> {
        let start = self.builder.events.len();
        // The slice's parsing so far, in events.
        let mut parsed: u64 = 0;
        loop {
            let index = OpIndex::new_unchecked(self.next);
            let Some(op) = self
                .ops
                .get(usize::try_from(self.next).unwrap_or(usize::MAX))
            else {
                return Ok(true);
            };
            if self.progress == OpProgress::START {
                let parsing = parsed.saturating_add(parse_cost(&self.machine, op));
                let replayed = self.builder.events.len().saturating_sub(start);
                if full(replayed, parsing, max_events) {
                    return Ok(false);
                }
                parsed = parsing;
            }
            self.builder.op = index;
            let first = self.builder.next_id();
            if self.progress == OpProgress::START {
                let json = check_room(first, index, &mut self.file, op)
                    .map_err(|error| ReplayError::Op { index, error })?;
                self.ops_json.push(json);
            }
            let mut sink = Slice {
                builder: &mut self.builder,
                start,
                parsed,
                max_events,
                stopped: None,
            };
            let mut out = Emitter::new(&mut sink, index, first);
            engine::resume(&mut self.machine, op, self.progress, &mut out)
                .map_err(|error| ReplayError::Op { index, error })?;
            if let Some(progress) = sink.stopped {
                self.progress = progress;
                return Ok(false);
            }
            let end = self.builder.pos();
            self.op_ends.push(end);
            self.next = self.next.saturating_add(1);
            self.progress = OpProgress::START;
            let next = OpIndex::new_unchecked(self.next);
            self.builder
                .keep(end, &self.machine, next, OpProgress::START);
        }
    }

    /// The events replayed so far, in order.
    pub(crate) fn events(&self) -> &[MicroEvent] {
        &self.builder.events
    }

    /// Where each op replayed so far ends.
    pub(crate) fn op_ends(&self) -> &[EventPos] {
        &self.op_ends
    }

    /// The ops being loaded, every one of them.
    pub(crate) fn into_ops(self) -> Vec<Op> {
        self.ops
    }

    /// The session with the log replayed so far, which is every op once
    /// [`Loader::advance`] has returned true, and its cursor at `cursor`, or
    /// at the end of the log if `cursor` is past it.
    pub(crate) fn finish(self, cursor: EventPos) -> Session {
        let Loader {
            mut session,
            mut ops,
            mut ops_json,
            machine,
            builder,
            op_ends,
            ..
        } = self;
        let Builder {
            events,
            checkpoints,
            entries,
            ..
        } = builder;
        ops.truncate(op_ends.len());
        ops_json.truncate(op_ends.len());
        let (records, last_base) = entries.finish();
        session.commit(Plan {
            base: EventPos::new_unchecked(0),
            kept: 0,
            ops,
            ops_json,
            events,
            op_ends,
            checkpoints,
            records,
            last_base,
            last_start: None,
            end: machine,
        });
        // A replay of the ops just loaded cannot fail; if it did, the cursor
        // stays at the end of the log.
        let _ = session.move_to(cursor.min(session.end()));
        // The session opens where the document says, not after a step, so
        // the live region reads the entry whole.
        session.stepped = false;
        session
    }
}

impl Session {
    /// Where the cursor is relative to the op holding it: that op, and how
    /// many of its events are done, from 1. Position 0 is op 0 with none
    /// done. None for an empty log, which has no op to place it by.
    pub(crate) fn cursor_in_op(&self) -> Option<(OpIndex, u32)> {
        if self.ops.is_empty() {
            return None;
        }
        let Some(op) = self.op_holding(self.cursor) else {
            return Some((OpIndex::new_unchecked(0), 0));
        };
        let start = op_start(&self.op_ends, op.get());
        Some((op, self.cursor.get().saturating_sub(start.get())))
    }

    /// The mode the student chose, which a share document keeps: Basics or
    /// Advanced, whatever the displayed run needs.
    pub(crate) fn mode_chosen(&self) -> bool {
        self.advanced
    }

    /// Each op as a share document writes it, with the saved file's size.
    pub(crate) fn saved_ops(&self) -> &SavedOps {
        &self.saved
    }
}

/// The position `event` events into op `op` of a log whose ops end at
/// `op_ends`: 0 events is just before the op, and a count beyond the op's
/// events is its end. An op beyond the log is the end of the log.
pub(crate) fn position_in_op(op_ends: &[EventPos], op: u32, event: u32) -> EventPos {
    let Some(&end) = op_ends.get(usize::try_from(op).unwrap_or(usize::MAX)) else {
        return op_ends
            .last()
            .copied()
            .unwrap_or(EventPos::new_unchecked(0));
    };
    let start = op_start(op_ends, op);
    EventPos::new_unchecked(start.get().saturating_add(event).min(end.get()))
}

/// Where op `op` starts: the end of the op before it, or position 0.
fn op_start(op_ends: &[EventPos], op: u32) -> EventPos {
    op.checked_sub(1)
        .and_then(|before| op_ends.get(usize::try_from(before).ok()?))
        .copied()
        .unwrap_or(EventPos::new_unchecked(0))
}

/// Whether a slice that has replayed `replayed` events, and parsed programs
/// worth `parsed` events, the next op's included, is full: it has replayed
/// something, and another entry might not fit within `max_events`.
fn full(replayed: usize, parsed: u64, max_events: u32) -> bool {
    let replayed = u64::try_from(replayed).unwrap_or(u64::MAX);
    replayed > 0
        && replayed
            .saturating_add(parsed)
            .saturating_add(u64::from(MAX_EVENTS_PER_ENTRY))
            > u64::from(max_events)
}

/// The parsing `op` does when it starts on `machine`, in events: a
/// `Program` op parses its source, and a `Config` op parses the loaded
/// program again under the new configuration.
fn parse_cost(machine: &Machine, op: &Op) -> u64 {
    let bytes = match op {
        Op::Program { source, .. } => source.len(),
        Op::Config { .. } => machine
            .program
            .as_ref()
            .map_or(0, |program| program.source().len()),
        Op::Issue { .. } | Op::Step | Op::StepAccess | Op::Run { .. } | Op::Edit { .. } => 0,
    };
    u64::try_from(bytes).unwrap_or(u64::MAX) / u64::from(PARSE_BYTES_PER_EVENT)
}

/// The sink of a slice: builds the log as a session's builder does, and
/// stops the op at an entry boundary once the slice is full.
struct Slice<'b> {
    /// The log being built.
    builder: &'b mut Builder,
    /// The events before the slice.
    start: usize,
    /// The slice's parsing, in events.
    parsed: u64,
    /// The events the slice may replay.
    max_events: u32,
    /// The progress of the op where the slice stopped it, if it did.
    stopped: Option<OpProgress>,
}

impl EventSink for Slice<'_> {
    fn records(&self) -> bool {
        true
    }

    fn event(&mut self, event: Option<MicroEvent>, after: &Machine) -> Flow {
        self.builder.event(event, after)
    }

    fn boundary(&mut self, machine: &Machine, progress: OpProgress) -> Flow {
        // The checkpoint due here is kept whether or not the slice ends here;
        // when the op resumes from this boundary, it is not due again.
        self.builder.boundary(machine, progress);
        let replayed = self.builder.events.len().saturating_sub(self.start);
        if full(replayed, self.parsed, self.max_events) {
            self.stopped = Some(progress);
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
    use crate::limits::{
        CHECKPOINT_INTERVAL, LOAD_SLICE_EVENTS, MAX_PROGRAM_BYTES, MAX_PROGRAM_LINES,
        MAX_RUN_STEPS, PARSE_BYTES_PER_EVENT,
    };
    use crate::op::{AccessKind, ProgramFormat};
    use crate::units::Addr;

    /// A loop that loads and stores a line at a time, then finishes.
    const WALK: &str = concat!(
        "        add r3, r0, #40\n",
        "        add r2, r0, #0x0A0\n",
        "loop:   ld r1, [r2]\n",
        "        st [r2], r3\n",
        "        add r2, r2, #16\n",
        "        add r3, r3, #-1\n",
        "        bnez r3, loop",
    );

    /// A loop without an exit or an access: one event per instruction.
    const ENDLESS: &str = "        add r1, r0, #1\nloop:   bnez r1, loop";

    fn program(source: &str) -> Op {
        Op::Program {
            source: source.to_owned(),
            format: ProgramFormat::Asm,
        }
    }

    fn load(addr: u32) -> Op {
        Op::Issue {
            kind: AccessKind::Read,
            addr: Addr::new_unchecked(addr),
            value: None,
        }
    }

    /// Ops whose events cross a checkpoint interval inside a `StepAccess`
    /// that runs to the end of its budget, with runs of several entries and
    /// single entries around them.
    fn ops() -> Vec<Op> {
        vec![
            load(0x0A4),
            program(WALK),
            Op::Run { n: 25 },
            Op::StepAccess,
            Op::Config {
                config: Preset::TwoWayLru.config(),
            },
            program(ENDLESS),
            Op::StepAccess,
            load(0x1A4),
            program(WALK),
            Op::Run { n: 7 },
        ]
    }

    /// A session with an empty log on `direct-mapped-8`.
    fn empty() -> Session {
        Session::with_config(Preset::DirectMapped8.config()).unwrap()
    }

    /// The session applying `ops` one by one builds.
    fn applied(ops: &[Op]) -> Session {
        let mut session = empty();
        for op in ops {
            session.apply(op.clone()).unwrap();
        }
        session
    }

    /// The session `ops` load into in slices of `max_events`, with the
    /// events each slice replayed; `whole` is the session the ops build one
    /// by one.
    fn loaded(ops: &[Op], max_events: u32, whole: &Session) -> (Session, Vec<usize>) {
        let mut loader = Loader::new(empty(), ops.to_vec());
        let mut slices = Vec::new();
        loop {
            let before = loader.events().len();
            let done = loader.advance(max_events).unwrap();
            slices.push(loader.events().len() - before);
            if done {
                break;
            }
            // Every slice ends at an entry boundary.
            let last = loader.events().last().unwrap();
            let next = whole.events.get(loader.events().len()).unwrap();
            assert_eq!(next.seq, 1, "a slice of {max_events} ended after {last:?}");
        }
        let end = loader.op_ends().last().copied().unwrap();
        (loader.finish(end), slices)
    }

    #[test]
    fn slices_rebuild_the_session_the_ops_build_one_by_one() {
        let ops = ops();
        let whole = applied(&ops);
        assert!(
            whole.end().get() > CHECKPOINT_INTERVAL,
            "the loop run to the end of its budget crosses a checkpoint interval"
        );
        for max_events in [1, MAX_EVENTS_PER_ENTRY, 50, LOAD_SLICE_EVENTS, u32::MAX] {
            let (session, slices) = loaded(&ops, max_events, &whole);
            assert_eq!(session.ops, whole.ops, "{max_events}");
            assert_eq!(session.saved, whole.saved, "{max_events}");
            assert_eq!(session.events, whole.events, "{max_events}");
            assert_eq!(session.op_ends, whole.op_ends, "{max_events}");
            assert_eq!(session.entries, whole.entries, "{max_events}");
            assert_eq!(session.checkpoints, whole.checkpoints, "{max_events}");
            assert_eq!(session.current, whole.current, "{max_events}");
            assert_eq!(session.entry_base, whole.entry_base, "{max_events}");
            assert_eq!(session.cursor, whole.cursor, "{max_events}");
            // A slice replays at most its events once they hold an entry.
            if max_events >= MAX_EVENTS_PER_ENTRY {
                let most = usize::try_from(max_events).unwrap_or(usize::MAX);
                assert!(slices.iter().all(|&slice| slice <= most), "{slices:?}");
            }
            // Every slice but the last replays something.
            let (_, before_last) = slices.split_last().unwrap();
            assert!(before_last.iter().all(|&slice| slice > 0), "{slices:?}");
        }
        // Small slices end inside the ops that execute several instructions.
        let (_, slices) = loaded(&ops, 1, &whole);
        assert!(slices.len() > ops.len() * 2);
    }

    /// An assembly program as long as a program may be: `ENDLESS`, then
    /// comment lines up to `MAX_PROGRAM_BYTES`, each long enough that there
    /// are fewer than `MAX_PROGRAM_LINES`.
    fn long_program() -> String {
        let width = usize::try_from(MAX_PROGRAM_BYTES / MAX_PROGRAM_LINES + 1).unwrap();
        let line = format!("; {}\n", "x".repeat(width));
        let mut source = format!("{ENDLESS}\n");
        while source.len() + line.len() <= usize::try_from(MAX_PROGRAM_BYTES).unwrap() {
            source.push_str(&line);
        }
        source
    }

    #[test]
    fn a_slice_counts_the_programs_its_ops_parse() {
        // A long program, then new runs that each parse it again, and runs
        // of it, which slices end inside: each parse is one event, and far
        // more work than one.
        let source = long_program();
        let mut ops = vec![program(&source)];
        for change in 0..24 {
            let preset = if change % 2 == 0 {
                Preset::TwoWayLru
            } else {
                Preset::FourWayWriteBack
            };
            ops.push(Op::Config {
                config: preset.config(),
            });
        }
        let run = Op::Run { n: MAX_RUN_STEPS };
        ops.extend([run.clone(), program(&source), run]);
        let whole = applied(&ops);
        let parse = u64::try_from(source.len()).unwrap() / u64::from(PARSE_BYTES_PER_EVENT);
        // Every op here that parses parses the long program.
        let parsing = |op: &Op| matches!(op, Op::Program { .. } | Op::Config { .. });
        for max_events in [1, MAX_EVENTS_PER_ENTRY, 700, LOAD_SLICE_EVENTS, u32::MAX] {
            let mut loader = Loader::new(empty(), ops.clone());
            loop {
                let events = loader.events().len();
                let replayed = loader.op_ends().len();
                let resumed = loader.progress != OpProgress::START;
                let done = loader.advance(max_events).unwrap();
                let slice = loader.events().get(events..).unwrap();
                // The ops the slice started: those it finished, but one it
                // resumed, and one it stopped inside.
                let started = replayed + usize::from(resumed)
                    ..loader.op_ends().len() + usize::from(loader.progress != OpProgress::START);
                let parsed = ops
                    .get(started)
                    .unwrap()
                    .iter()
                    .filter(|op| parsing(op))
                    .count();
                let work =
                    u64::try_from(slice.len()).unwrap() + u64::try_from(parsed).unwrap() * parse;
                // A slice holds at least an entry, then no more work than
                // its events.
                let entries = slice.iter().filter(|event| event.seq == 1).count();
                assert!(
                    entries <= 1 || work <= u64::from(max_events),
                    "{max_events}: {entries} entries, {work} events of work"
                );
                if done {
                    break;
                }
            }
            let end = loader.op_ends().last().copied().unwrap();
            assert_eq!(loader.finish(end).events, whole.events, "{max_events}");
        }
    }

    #[test]
    fn a_refused_op_stops_the_load_where_it_is() {
        let mut ops = ops();
        ops.insert(3, load(0x0A6));
        let mut loader = Loader::new(empty(), ops);
        let error = loop {
            match loader.advance(LOAD_SLICE_EVENTS) {
                Ok(true) => panic!("the misaligned load is refused"),
                Ok(false) => {}
                Err(error) => break error,
            }
        };
        let ReplayError::Op { index, .. } = error else {
            panic!("{error:?}");
        };
        assert_eq!(index.get(), 3);
        assert_eq!(loader.op_ends().len(), 3);
    }

    #[test]
    fn a_place_in_an_op_is_a_position_and_back() {
        let ops = ops();
        let mut session = applied(&ops);
        let op_ends = session.op_ends.clone();
        let last = u32::try_from(op_ends.len()).unwrap() - 1;
        assert_eq!(position_in_op(&op_ends, 0, 0).get(), 0);
        assert_eq!(position_in_op(&op_ends, 1, 0), op_ends[0]);
        assert_eq!(position_in_op(&op_ends, 1, 1).get(), op_ends[0].get() + 1);
        // Beyond the op's events is its end; beyond the log, the log's end.
        assert_eq!(position_in_op(&op_ends, 1, u32::MAX), op_ends[1]);
        assert_eq!(position_in_op(&op_ends, last + 1, 0), session.end());
        assert_eq!(position_in_op(&[], 3, 2).get(), 0);
        for pos in (0..=session.end().get())
            .step_by(97)
            .chain([session.end().get()])
        {
            session.seek(EventPos::new_unchecked(pos));
            let (op, event) = session.cursor_in_op().unwrap();
            assert_eq!(
                position_in_op(&op_ends, op.get(), event).get(),
                pos,
                "{op:?} {event}"
            );
            assert!(pos == 0 || event >= 1);
        }
        assert_eq!(empty().cursor_in_op(), None);
    }

    #[test]
    fn a_loaded_session_opens_where_it_is_asked_to() {
        let ops = ops();
        let mut whole = applied(&ops);
        let target = EventPos::new_unchecked(whole.end().get() / 2);
        whole.seek(target);
        let mut loader = Loader::new(empty(), ops);
        assert!(loader.advance(u32::MAX).unwrap());
        let session = loader.finish(target);
        assert_eq!(session.cursor, target);
        assert_eq!(session.current, whole.current);
        assert_eq!(session.entry_base, whole.entry_base);
        assert!(!session.stepped);
    }
}
