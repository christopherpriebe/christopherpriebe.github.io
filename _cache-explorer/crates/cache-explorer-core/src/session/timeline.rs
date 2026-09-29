//! The timeline: the entries of the displayed run as the log draws them,
//! the current entry's events, one tick per step, and the sentence the live
//! region reads.
//!
//! An entry is one issued access, one instruction, one edit, one program
//! load, or one `Error` that ends an op. Its record is written once, while
//! its events are recorded: its place in the log, its number, what it was
//! and did, its label, and the summary of what it changed, so drawing the
//! log never replays anything. A new run's `Setup` is the run's origin, not
//! an entry.
//!
//! What an entry changed is its net change: the cells and counters its
//! events list whose values differ between the state before the entry and
//! the state after it, so a valid bit that goes from 1 to 0 and back is not
//! among them. A way that holds a different line afterwards counts its tag
//! and every word as changed, as the event that changed its line lists them,
//! even where a number happens to agree. The same comparison, stopped at the
//! cursor, is the highlight of the snapshot.
//!
//! The views count steps, the events other than setups, since only those
//! have a place on the scrubber and a tick.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::Serialize;

use super::Session;
use crate::event::{Cause, ChangeRef, EventKind, MicroEvent};
use crate::explain;
use crate::machine::Machine;
use crate::op::{AccessKind, ProgramFormat};
use crate::request::{Origin, Request};
use crate::units::{Addr, EventPos, Register, SetIndex, WayIndex};

/// The bit of a tick set on an entry's first step. A tick is the byte the
/// scrubber draws for one step of a run; the ticks of the displayed run and
/// those of a run still loading are built from this bit and [`MISS_TICK`].
///
/// The page decodes these bits, and the docs of [`Session::ticks`] and of
/// the ticks the page receives name this one as bit 0, so changing its value
/// changes what the page reads and leaves those docs wrong.
pub(crate) const FIRST_TICK: u8 = 1;

/// The bit of a tick set on every step of an entry that missed. Like
/// [`FIRST_TICK`], the page decodes it, and the docs name it as bit 1.
pub(crate) const MISS_TICK: u8 = 2;

/// The log of the displayed run: which run it is, where it starts and ends,
/// the step the cursor is on, the log's header, the live region's sentence,
/// and the cursor's entry with its events.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    /// The displayed run, from 1.
    pub run: u32,
    /// The runs in the log, the undone ones after the cursor included.
    pub runs: u32,
    /// The run's origin: position 0, or its `Setup` event.
    pub origin: EventPos,
    /// The position after the run's last event.
    pub end: EventPos,
    /// The cursor's position.
    pub cursor: EventPos,
    /// The steps of the run up to the cursor.
    pub step: u32,
    /// The steps of the run.
    pub step_count: u32,
    /// The log's header: "step 5 of 14", or "no steps yet".
    pub header_text: String,
    /// What the page's live region reads: the log line at the cursor, after
    /// a move across more than one event opened by the entry's outcome, as
    /// in `#5 ld r3, [0x2A4]: miss · evict · write-back. Read word 1 of way 1
    /// (68) into r3.` Empty at position 0.
    pub live_text: String,
    /// The run's entries.
    pub entry_count: u32,
    /// The cursor's entry, with its events; absent at a run's origin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<EntryView>,
}

/// Where an entry or an event is, relative to the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum Tense {
    /// Before the cursor.
    Past,
    /// At the cursor: the event it rests on, or the entry holding it.
    Current,
    /// After the cursor, undone or not yet reached; the log fades it.
    Future,
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    /// A load or store, issued by hand or executed by a program.
    Access,
    /// An instruction without a memory access: `add` or `bnez`.
    Instruction,
    /// An edit of a way or a memory word.
    Edit,
    /// A program or trace load, which the log shows as a divider row.
    Program,
    /// An error that ended an op: a fault, the end of the budget or of the
    /// session's room, a program the new run could not keep, or an internal
    /// error.
    Error,
}

/// What an entry did, as its header's mark shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum EntryOutcome {
    /// An access that hit.
    Hit,
    /// An access that missed.
    Miss,
    /// An edit.
    Edit,
    /// An instruction without a memory access.
    CpuOnly,
    /// A program or trace load.
    Program,
    /// An error; also an access abandoned before its tag compare.
    Error,
}

/// One entry of the log, as its row shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    /// Its place among the run's entries, from 0.
    pub index: u32,
    /// Its number within the run, from 1; absent for a program load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    /// What it is, in a few words: the instruction, as in `ld r3, [0x2A4]`;
    /// "edit set 2 way 0"; "Loaded a program of 5 instructions" for a load;
    /// for an error, the instruction the program stopped at, or "—".
    pub label: String,
    /// What it is.
    pub kind: EntryKind,
    /// What it did.
    pub outcome: EntryOutcome,
    /// What it did, in words: "miss · evict · write-back", "hit", "edit",
    /// "CPU only", "program", "error".
    pub outcome_text: String,
    /// The position of its first event.
    pub first: EventPos,
    /// The position of its last event.
    pub last: EventPos,
    /// Where it is, relative to the cursor.
    pub tense: Tense,
    /// What it changed: over the whole entry for a past entry, up to the
    /// cursor for the current one; absent for a future entry and for a
    /// program load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_summary: Option<String>,
    /// The current entry's events, in order; empty for every other entry.
    pub events: Vec<EventRow>,
}

/// Entries of the log in order, as the boundary returns a window of them:
/// an array in JavaScript, which one serializer call builds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct EntryViews(pub Vec<EntryView>);

/// One event of the current entry, as its row in the log shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct EventRow {
    /// The position the cursor rests on at this event.
    pub pos: EventPos,
    /// Its label, `5.3`; empty for a program load.
    pub label: String,
    /// What happened.
    pub kind: EventKind,
    /// Its log line.
    pub text: String,
    /// The configuration group responsible, for the row's "why:" link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Cause>,
    /// Where it is, relative to the cursor.
    pub tense: Tense,
}

/// What the session keeps of an entry, written once while its events are
/// recorded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntryRecord {
    /// The position of its first event.
    pub(crate) first: EventPos,
    /// The position of its last event.
    pub(crate) last: EventPos,
    /// Its run.
    pub(crate) run: u32,
    /// Its number within the run; none for a program load.
    pub(crate) number: Option<u32>,
    /// What it is.
    pub(crate) kind: EntryKind,
    /// What it did.
    pub(crate) outcome: EntryOutcome,
    /// What it did, in words.
    pub(crate) outcome_text: &'static str,
    /// What it is, in a few words. An instruction's is the text the program
    /// holds for it, shared with the program.
    pub(crate) label: Arc<str>,
    /// What it changed over the whole entry; none for a program load, whose
    /// divider row has no changed line.
    pub(crate) summary: Option<String>,
    /// The steps in the log before it.
    pub(crate) steps_before: u32,
    /// The load or store it makes, for an access.
    pub(crate) access: Option<EntryAccess>,
}

/// The load or store an access entry makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EntryAccess {
    /// Whether it loads or stores.
    pub(crate) kind: AccessKind,
    /// The word's address.
    pub(crate) addr: Addr,
}

/// The cells and counters an entry's events change, each with its value
/// before the entry began: what the highlight at any point of the entry
/// compares the machine with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EntryBase {
    /// The position of the entry's first event.
    pub(crate) first: EventPos,
    /// Each cell and counter, with its value before the entry.
    pub(crate) before: BTreeMap<ChangeRef, Option<u64>>,
}

/// Writes the records of the entries whose events a stretch of the log
/// records, one event at a time.
///
/// The values an entry is compared with come from the events themselves:
/// every event lists each cell and counter whose value it changed, so a
/// value before an entry is the value after the last event that changed it,
/// or, for one no event of the stretch has changed yet, its value in the
/// machine the stretch started from, or the new run's.
#[derive(Clone, Debug)]
pub(crate) struct Entries {
    /// The machine the stretch starts from, or the latest new run's start:
    /// where a value no event has changed since is read.
    origin: Machine,
    /// The value of every cell and counter an event has changed since
    /// `origin`, after the latest such event.
    latest: BTreeMap<ChangeRef, Option<u64>>,
    /// The entry whose events are being recorded.
    open: Option<Open>,
    /// The entries recorded and closed so far.
    records: Vec<EntryRecord>,
    /// The base of the entry closed last.
    last_base: Option<EntryBase>,
    /// The steps in the log before the next entry.
    steps: u32,
    /// The run of the latest event.
    run: u32,
    /// The pc after the latest event: the instruction an `add` or `bnez`
    /// entry executes, since its one event has moved the pc on.
    pc: u32,
}

/// An entry whose events are being recorded.
#[derive(Clone, Debug)]
struct Open {
    /// The position of its first event.
    first: EventPos,
    /// The position of its latest event.
    last: EventPos,
    /// Its run.
    run: u32,
    /// Its number within the run.
    number: Option<u32>,
    /// What it is.
    kind: EntryKind,
    /// Its label.
    label: Arc<str>,
    /// The format of the program it loads, for a program load.
    format: Option<ProgramFormat>,
    /// The load or store it makes, for an access.
    access: Option<EntryAccess>,
    /// The kinds of its events so far.
    kinds: Vec<EventKind>,
    /// Each cell and counter its events have changed, with its value before
    /// the entry.
    before: BTreeMap<ChangeRef, Option<u64>>,
    /// The steps in the log before it.
    steps_before: u32,
}

impl EntryRecord {
    /// The steps the entry has: one per event, none for a program load.
    pub(crate) fn steps(&self) -> u32 {
        if self.kind == EntryKind::Program {
            0
        } else {
            self.last
                .get()
                .saturating_sub(self.first.get())
                .saturating_add(1)
        }
    }

    /// Whether the cursor at `pos` rests on one of the entry's events.
    pub(crate) fn holds(&self, pos: EventPos) -> bool {
        self.first <= pos && pos <= self.last
    }

    /// The entry's tense with the cursor at `cursor`.
    pub(crate) fn tense(&self, cursor: EventPos) -> Tense {
        if self.last < cursor {
            Tense::Past
        } else if self.first > cursor {
            Tense::Future
        } else {
            Tense::Current
        }
    }
}

impl EntryBase {
    /// The base of the entry whose first event is at `first` and whose
    /// events are `events`, from `start`, the machine before that event.
    pub(crate) fn from_start(first: EventPos, events: &[MicroEvent], start: &Machine) -> Self {
        // Extended rather than collected: collecting sorts the pairs first,
        // with a sort the wasm would carry for this type alone.
        let mut before = BTreeMap::new();
        before.extend(
            events
                .iter()
                .flat_map(|event| event.changed.iter().copied())
                .map(|cell| (cell, start.cell(cell))),
        );
        Self { first, before }
    }
}

impl Entries {
    /// Records the entries of a stretch of the log that starts from
    /// `start`, the machine at an entry boundary, after `steps` steps.
    pub(crate) fn new(start: &Machine, steps: u32) -> Self {
        Self {
            origin: start.clone(),
            latest: BTreeMap::new(),
            open: None,
            records: Vec::new(),
            last_base: None,
            steps,
            run: start.run,
            pc: start.cpu.pc,
        }
    }

    /// Takes in `event`, which has left the machine as `after`.
    pub(crate) fn observe(&mut self, event: &MicroEvent, after: &Machine) {
        let pos = position_of(event);
        if event.seq == 1 {
            self.close();
            if event.kind == EventKind::Setup && after.run != self.run {
                // A new run's origin, which is not an entry: the values its
                // entries are compared with are the new run's.
                self.origin = after.clone();
                self.latest.clear();
            } else {
                let (kind, label, format) = describe(event, after, self.pc);
                let access = (kind == EntryKind::Access)
                    .then_some(after.access.as_ref())
                    .flatten()
                    .map(|access| EntryAccess {
                        kind: access.request.kind(),
                        addr: access.request.addr(),
                    });
                self.open = Some(Open {
                    first: pos,
                    last: pos,
                    run: event.run,
                    number: event.entry,
                    kind,
                    label,
                    format,
                    access,
                    kinds: Vec::new(),
                    before: BTreeMap::new(),
                    steps_before: self.steps,
                });
            }
        }
        if let Some(open) = &mut self.open {
            open.last = pos;
            open.kinds.push(event.kind);
            for &cell in &event.changed {
                let prior = self
                    .latest
                    .get(&cell)
                    .copied()
                    .unwrap_or_else(|| self.origin.cell(cell));
                open.before.entry(cell).or_insert(prior);
            }
        }
        for &cell in &event.changed {
            self.latest.insert(cell, after.cell(cell));
        }
        self.pc = after.cpu.pc;
        self.run = after.run;
    }

    /// The records of the stretch's entries, and the base of its last entry
    /// when its last event belongs to one rather than to a new run's origin.
    pub(crate) fn finish(mut self) -> (Vec<EntryRecord>, Option<EntryBase>) {
        let in_entry = self.open.is_some();
        self.close();
        let base = if in_entry { self.last_base } else { None };
        (self.records, base)
    }

    /// Closes the entry being recorded: its outcome and its summary, from
    /// the values before it and the latest ones, which are its last event's.
    fn close(&mut self) {
        let Some(open) = self.open.take() else {
            return;
        };
        let has = |kind| open.kinds.contains(&kind);
        let outcome = match open.kind {
            EntryKind::Access if has(EventKind::Hit) => EntryOutcome::Hit,
            EntryKind::Access if has(EventKind::Miss) => EntryOutcome::Miss,
            EntryKind::Access | EntryKind::Error => EntryOutcome::Error,
            EntryKind::Instruction => EntryOutcome::CpuOnly,
            EntryKind::Edit => EntryOutcome::Edit,
            EntryKind::Program => EntryOutcome::Program,
        };
        let summary = (open.kind != EntryKind::Program).then(|| {
            let changed = net_change(open.before.iter().map(|(&cell, &before)| {
                let after = self.latest.get(&cell).copied().flatten();
                (cell, before, after)
            }));
            explain::changed_summary(&self.origin.geometry, &changed)
        });
        let record = EntryRecord {
            first: open.first,
            last: open.last,
            run: open.run,
            number: open.number,
            kind: open.kind,
            outcome,
            outcome_text: explain::entry_outcome(outcome, &open.kinds, open.format),
            label: open.label,
            summary,
            steps_before: open.steps_before,
            access: open.access,
        };
        self.steps = self.steps.saturating_add(record.steps());
        self.records.push(record);
        self.last_base = Some(EntryBase {
            first: open.first,
            before: open.before,
        });
    }
}

/// What the entry that `event` opens is, its label, and the format of the
/// program it loads, from the machine `after` it and `pc`, the pc before
/// it. The label of an instruction is the program's own text for it, shared;
/// any other is built here.
fn describe(
    event: &MicroEvent,
    after: &Machine,
    pc: u32,
) -> (EntryKind, Arc<str>, Option<ProgramFormat>) {
    let geometry = &after.geometry;
    let no_instruction = || Arc::<str>::from(explain::no_instruction());
    let instruction = |at: u32| {
        after
            .program
            .as_ref()
            .and_then(|program| program.get(at))
            .map_or_else(no_instruction, |placed| Arc::clone(&placed.text))
    };
    match event.kind {
        EventKind::Setup => {
            let (format, count) = after
                .program
                .as_ref()
                .map_or((ProgramFormat::Asm, 0), |program| {
                    (program.format(), program.len())
                });
            (
                EntryKind::Program,
                explain::program_divider(format, count).into(),
                Some(format),
            )
        }
        EventKind::Edit => {
            let label = match (event.set, event.way, event.addr) {
                (Some(set), Some(way), _) => explain::edit_way(set, way).into(),
                (_, _, Some(addr)) => explain::edit_memory(geometry, addr).into(),
                _ => no_instruction(),
            };
            (EntryKind::Edit, label, None)
        }
        EventKind::Error => {
            // A program stopped at an instruction names it; any other error
            // stands for no instruction.
            let label = if after.cpu.halted.is_some() {
                instruction(after.cpu.pc)
            } else {
                no_instruction()
            };
            (EntryKind::Error, label, None)
        }
        EventKind::Decode => {
            let label = match after.access.as_ref().map(|access| access.request) {
                Some(request) if request.origin() == Origin::Hand => {
                    let (into, value) = match request {
                        Request::Load { into, .. } => (into, None),
                        Request::Store { value, .. } => (Register::R1, Some(value)),
                    };
                    explain::issued(geometry, request.addr(), into, value).into()
                }
                // The instruction is still at the pc: it retires at the
                // access's last event.
                _ => instruction(after.cpu.pc),
            };
            (EntryKind::Access, label, None)
        }
        // An instruction's one event, which has moved the pc on.
        _ => (EntryKind::Instruction, instruction(pc), None),
    }
}

/// The net change of `cells`, each given with its value before and after:
/// those whose values differ, and every word of a way whose tag differs,
/// since the way holds another line. In the order given.
pub(crate) fn net_change(
    cells: impl Iterator<Item = (ChangeRef, Option<u64>, Option<u64>)>,
) -> Vec<ChangeRef> {
    let cells: Vec<(ChangeRef, Option<u64>, Option<u64>)> = cells.collect();
    // A few ways at most, so a list, which needs no sort.
    let moved: Vec<(SetIndex, WayIndex)> = cells
        .iter()
        .filter_map(|&(cell, before, after)| match cell {
            ChangeRef::Tag { set, way } if before != after => Some((set, way)),
            _ => None,
        })
        .collect();
    cells
        .into_iter()
        .filter(|&(cell, before, after)| {
            before != after
                || matches!(cell, ChangeRef::Word { set, way, .. } if moved.contains(&(set, way)))
        })
        .map(|(cell, _, _)| cell)
        .collect()
}

/// The position the cursor rests on at `event`: its id plus one.
pub(crate) fn position_of(event: &MicroEvent) -> EventPos {
    EventPos::new_unchecked(u32::try_from(event.id.saturating_add(1)).unwrap_or(u32::MAX))
}

impl Session {
    /// The log of the displayed run, as the page draws it around the rows in
    /// view: the run, its bounds and steps, the header, the live region's
    /// sentence, and the cursor's entry with its events.
    pub fn timeline(&self) -> Timeline {
        let run = self.cursor.machine().run;
        let cursor = self.cursor.pos();
        let (origin, end) = self.log.run_bounds(run);
        let at_origin = self.log.steps_through(origin);
        let entries = self.log.run_entries(run);
        let current = self.log.record_at(cursor).map(|(index, record)| {
            let index = index.saturating_sub(entries.start);
            self.entry_view(index, record)
        });
        let step = self.log.steps_through(cursor).saturating_sub(at_origin);
        let step_count = self.log.steps_through(end).saturating_sub(at_origin);
        Timeline {
            run,
            runs: self.log.events().last().map_or(1, |event| event.run),
            origin,
            end,
            cursor,
            step,
            step_count,
            header_text: explain::log_header(step, step_count),
            live_text: self.live_text(),
            entry_count: count(entries.len()),
            current,
        }
    }

    /// The displayed run's entries from its `first`, at most `count` of
    /// them, for the rows of the log in view. The current entry comes with
    /// its events; the others without.
    pub fn entries(&self, first: u32, count: u32) -> Vec<EntryView> {
        let entries = self.log.run_entries(self.cursor.machine().run);
        let start = entries
            .start
            .saturating_add(usize::try_from(first).unwrap_or(usize::MAX))
            .min(entries.end);
        let end = start
            .saturating_add(usize::try_from(count).unwrap_or(usize::MAX))
            .min(entries.end);
        self.log
            .entries()
            .get(start..end)
            .unwrap_or_default()
            .iter()
            .zip(start..)
            .map(|(record, index)| self.entry_view(index.saturating_sub(entries.start), record))
            .collect()
    }

    /// One byte per step of the displayed run, in order: bit 0 set on an
    /// entry's first step, bit 1 on every step of an entry that missed.
    pub fn ticks(&self) -> Vec<u8> {
        let entries = self.log.run_entries(self.cursor.machine().run);
        self.log
            .entries()
            .get(entries)
            .unwrap_or_default()
            .iter()
            .flat_map(|record| {
                let miss = if record.outcome == EntryOutcome::Miss {
                    MISS_TICK
                } else {
                    0
                };
                (0..record.steps()).map(move |step| {
                    let first = if step == 0 { FIRST_TICK } else { 0 };
                    first | miss
                })
            })
            .collect()
    }

    /// The view of `record`, the run's entry `index`, as the cursor now
    /// finds it.
    fn entry_view(&self, index: usize, record: &EntryRecord) -> EntryView {
        let cursor = self.cursor.pos();
        let tense = record.tense(cursor);
        let changed_summary = match (tense, record.kind) {
            (_, EntryKind::Program) | (Tense::Future, _) => None,
            (Tense::Past, _) => record.summary.clone(),
            (Tense::Current, _) => Some(explain::changed_summary(
                &self.cursor.machine().geometry,
                &self.highlight(),
            )),
        };
        let events = if tense == Tense::Current {
            self.log
                .events_between(record.first, record.last)
                .iter()
                .map(|event| {
                    let pos = position_of(event);
                    EventRow {
                        pos,
                        label: event.label.clone(),
                        kind: event.kind,
                        text: event.explain.clone(),
                        cause: event.cause,
                        tense: tense_of(pos, cursor),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        EntryView {
            index: count(index),
            number: record.number,
            label: String::from(&*record.label),
            kind: record.kind,
            outcome: record.outcome,
            outcome_text: record.outcome_text.to_owned(),
            first: record.first,
            last: record.last,
            tense,
            changed_summary,
            events,
        }
    }

    /// The live region's sentence: the log line at the cursor, opened by
    /// its entry's number, label and outcome unless the cursor last moved
    /// by a single step, as Space moves it. Empty at position 0.
    fn live_text(&self) -> String {
        let cursor = self.cursor.pos();
        let Some(event) = self.log.event_at(cursor) else {
            return String::new();
        };
        let entry = if self.cursor.stepped() {
            None
        } else {
            self.log.record_at(cursor).and_then(|(_, record)| {
                record
                    .number
                    .map(|number| (number, &*record.label, record.outcome_text))
            })
        };
        explain::live(entry, &event.explain)
    }

    /// The net change of the cursor's entry so far: the cells and counters
    /// its events up to the cursor list whose values differ from those
    /// before the entry, with every word of a way that now holds another
    /// line. Empty at a run's origin and on a program load.
    pub(crate) fn highlight(&self) -> Vec<ChangeRef> {
        let cursor = self.cursor.pos();
        let Some((_, record)) = self.log.record_at(cursor) else {
            return Vec::new();
        };
        let Some(base) = self
            .cursor
            .entry_base()
            .filter(|base| base.first == record.first && record.kind != EntryKind::Program)
        else {
            return Vec::new();
        };
        let machine = self.cursor.machine();
        let mut cells: Vec<ChangeRef> = self
            .log
            .events_between(record.first, cursor)
            .iter()
            .flat_map(|event| event.changed.iter().copied())
            .collect();
        // In order and each once, as a set would give them, with the sort
        // the engine already has for these.
        cells.sort_unstable();
        cells.dedup();
        net_change(cells.into_iter().map(|cell| {
            let before = base
                .before
                .get(&cell)
                .copied()
                .unwrap_or_else(|| machine.cell(cell));
            (cell, before, machine.cell(cell))
        }))
    }
}

/// An event's tense with the cursor at `cursor`.
fn tense_of(pos: EventPos, cursor: EventPos) -> Tense {
    match pos.cmp(&cursor) {
        std::cmp::Ordering::Less => Tense::Past,
        std::cmp::Ordering::Equal => Tense::Current,
        std::cmp::Ordering::Greater => Tense::Future,
    }
}

/// A count of entries as the views give it.
fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use std::collections::BTreeSet;

    use super::*;
    use crate::config::{AllocatePolicy, Config, Preset, Timing, WritePolicy};
    use crate::op::{AccessKind, Op};
    use crate::units::{Addr, Word, WordIndex};

    fn pos(value: u32) -> EventPos {
        EventPos::new_unchecked(value)
    }

    fn load(at: u32) -> Op {
        Op::Issue {
            kind: AccessKind::Read,
            addr: Addr::new_unchecked(at),
            value: None,
        }
    }

    fn store(at: u32, value: u64) -> Op {
        Op::Issue {
            kind: AccessKind::Write,
            addr: Addr::new_unchecked(at),
            value: Some(Word::new_unchecked(value)),
        }
    }

    /// A log with every kind of entry and two runs: a program stepped one
    /// instruction and one access at a time and run in bulk, hand-issued
    /// accesses that evict dirty lines, then a timed write-through,
    /// no-allocate run whose kept program no longer fits and is unloaded,
    /// and a trace.
    fn varied() -> Session {
        let mut session = Session::with_config(Preset::DirectMapped8.config()).unwrap();
        let program = "        add r2, r0, #0x0A0\n        add r3, r0, #3\nloop:   ld r1, [r2]\n        st [r2], r3\n        add r2, r2, #0x80\n        add r3, r3, #-1\n        bnez r3, loop\n        st [0x1A4], r1";
        let ops = [
            Op::Program {
                source: program.to_owned(),
                format: ProgramFormat::Asm,
            },
            Op::Step,
            Op::StepAccess,
            Op::Run { n: 6 },
            store(0x0A4, 9),
            load(0x1A4),
            load(0x0A4),
            Op::StepAccess,
            Op::Config {
                config: Config {
                    address_bits: 8,
                    cache: crate::config::CacheConfig {
                        associativity: 2,
                        write_policy: WritePolicy::WriteThrough,
                        allocate_policy: AllocatePolicy::WriteNoAllocate,
                        ..Preset::DirectMapped8.config().cache
                    },
                    timing: Some(Timing {
                        hit_cycles: 1,
                        miss_cycles: 3,
                    }),
                    ..Preset::DirectMapped8.config()
                },
            },
            store(0x24, 5),
            load(0x24),
            store(0x24, 6),
            Op::Program {
                source: "R 0x44\nW 0x44 3\nR 0xC4".to_owned(),
                format: ProgramFormat::Trace,
            },
            Op::Run { n: 3 },
        ];
        for op in ops {
            session.apply(op).unwrap();
        }
        session
    }

    /// The highlight at `pos` computed from scratch: the machines before the
    /// entry and at `pos`, replayed from the start, compared over the cells
    /// its events up to `pos` list.
    fn replayed_highlight(session: &Session, at: EventPos) -> Vec<ChangeRef> {
        let Some((_, record)) = session.log.record_at(at) else {
            return Vec::new();
        };
        if record.kind == EntryKind::Program {
            return Vec::new();
        }
        let start = pos(record.first.get() - 1);
        let before = Session::replay_to(&session.initial, session.ops(), start).unwrap();
        let now = Session::replay_to(&session.initial, session.ops(), at).unwrap();
        let cells: BTreeSet<ChangeRef> = session
            .log
            .events_between(record.first, at)
            .iter()
            .flat_map(|event| event.changed.iter().copied())
            .collect();
        net_change(
            cells
                .into_iter()
                .map(|cell| (cell, before.cell(cell), now.cell(cell))),
        )
    }

    #[test]
    fn the_highlight_is_the_net_change_since_the_entry_began_wherever_the_cursor_comes_from() {
        let mut session = varied();
        let end = session.log.end().get();
        assert_eq!(session.machine().run, 2);
        // Backwards, so every move but the first lands in another entry
        // from a position that is not its start; then forwards one event at
        // a time, as Space moves.
        let order: Vec<u32> = (0..=end).rev().chain(0..=end).collect();
        for at in order {
            session.seek(pos(at));
            assert_eq!(
                session.highlight(),
                replayed_highlight(&session, pos(at)),
                "at {at}"
            );
        }
        // Undo and redo land on op ends from far away.
        while session.undo() {
            let at = session.cursor.pos();
            assert_eq!(session.highlight(), replayed_highlight(&session, at));
        }
        while session.redo() {
            let at = session.cursor.pos();
            assert_eq!(session.highlight(), replayed_highlight(&session, at));
        }
    }

    #[test]
    fn a_summary_is_the_net_change_of_its_whole_entry() {
        let session = varied();
        assert!(session.log.entries().len() > 20);
        // The new run could not keep the program, whose last line does not
        // fit 8-bit addresses: its error entry names no instruction.
        assert!(session
            .log
            .entries()
            .iter()
            .any(|record| record.kind == EntryKind::Error
                && *record.label == explain::no_instruction()
                && record.number == Some(1)));
        for record in session.log.entries() {
            let expected = (record.kind != EntryKind::Program).then(|| {
                let geometry = Session::replay_to(&session.initial, session.ops(), record.last)
                    .unwrap()
                    .geometry;
                explain::changed_summary(&geometry, &replayed_highlight(&session, record.last))
            });
            assert_eq!(record.summary, expected, "{record:?}");
        }
    }

    #[test]
    fn an_entry_of_a_program_instruction_shares_the_programs_text_for_it() {
        let mut session = Session::with_config(Preset::DirectMapped8.config()).unwrap();
        session
            .apply(Op::Program {
                source: "add r1, r0, #1\nld r2, [0x0B0]".to_owned(),
                format: ProgramFormat::Asm,
            })
            .unwrap();
        session.apply(Op::Run { n: 2 }).unwrap();
        let program = session.machine().program.clone().unwrap();
        let entries: Vec<&EntryRecord> = session
            .log
            .entries()
            .iter()
            .filter(|record| record.kind != EntryKind::Program)
            .collect();
        assert_eq!(entries.len(), 2);
        for (index, record) in (0..).zip(entries) {
            let placed = program.get(index).unwrap();
            assert!(Arc::ptr_eq(&record.label, &placed.text), "{record:?}");
        }
    }

    #[test]
    fn records_cover_every_event_but_a_new_runs_origin() {
        let session = varied();
        let mut covered = 0_u32;
        let mut next = 1;
        for record in session.log.entries() {
            // Only a new run's setup lies between two entries.
            while next < record.first.get() {
                let event = session.event_at(pos(next)).unwrap();
                assert_eq!((event.kind, event.entry), (EventKind::Setup, None));
                next += 1;
            }
            covered += record.last.get() - record.first.get() + 1;
            next = record.last.get() + 1;
        }
        let setups = u32::try_from(
            session
                .log
                .events()
                .iter()
                .filter(|event| event.kind == EventKind::Setup && event.entry.is_none())
                .count(),
        )
        .unwrap();
        let programs = u32::try_from(
            session
                .log
                .entries()
                .iter()
                .filter(|record| record.kind == EntryKind::Program)
                .count(),
        )
        .unwrap();
        assert_eq!(covered + (setups - programs), session.log.end().get());
    }

    #[test]
    fn a_way_that_holds_another_line_counts_every_word() {
        let set = SetIndex::new_unchecked(2);
        let way = WayIndex::new_unchecked(0);
        let word = |index| ChangeRef::Word {
            set,
            way,
            word: WordIndex::new_unchecked(index),
        };
        // The tag changed; word 0 happens to hold the same number.
        let cells = [
            (ChangeRef::Valid { set, way }, Some(1), Some(1)),
            (ChangeRef::Tag { set, way }, Some(2), Some(10)),
            (word(0), Some(7), Some(7)),
            (word(1), Some(7), Some(8)),
        ];
        assert_eq!(
            net_change(cells.into_iter()),
            [ChangeRef::Tag { set, way }, word(0), word(1)]
        );
        // Same line: only the values that differ.
        let cells = [
            (ChangeRef::Tag { set, way }, Some(2), Some(2)),
            (word(0), Some(7), Some(7)),
            (word(1), Some(7), Some(8)),
            (ChangeRef::Dirty { set, way }, Some(0), Some(0)),
        ];
        assert_eq!(net_change(cells.into_iter()), [word(1)]);
    }
}
