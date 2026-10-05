//! The log of a session: every op, the undone ones after the cursor
//! included, and what was recorded of running them.
//!
//! An op has a serial, its JSON as a share document writes it, and the
//! position where it ends. Every event is stored, every entry has a record,
//! and each record notes the latest access of its run, at or before it, that
//! reached its tag compare. Checkpoints keep copies of the machine along the
//! log.
//!
//! These lists change together, and only here: [`Log::truncate_after`] drops
//! everything after an op, and [`Log::append`] adds a stretch of ops with what
//! running them produced. So there is one serial, one JSON and one end for
//! every op, one note for every entry, and the events end where the last op
//! does, with no checkpoint past them.

use std::ops::Range;

use super::timeline::EntryAccess;
use super::{
    position, slot, slot_of, Checkpoint, Checkpoints, EntryKind, EntryOutcome, EntryRecord,
};
use crate::config::Config;
use crate::event::MicroEvent;
use crate::machine::Machine;
use crate::op::{AccessKind, Op};
use crate::saved::{FileRoom, SavedOps};
use crate::share;
use crate::stats::Outcome;
use crate::units::{Addr, EventPos, OpIndex};

/// The work the lookups of a last access do, counted for the thread they run
/// on, for a test to read before and after a search: it bounds the work of
/// the search, not its time.
///
/// It counts the entries the lookup reads through [`Log::entry_looked_at`]
/// and the comparisons of its search, and no others: a read of `entries` in
/// `last_access` that goes around them is not counted, and a look back along
/// a run written that way passes the test that bounds the work.
#[cfg(test)]
pub(super) mod lookup_work {
    use std::cell::Cell;

    /// What the lookups have done so far on this thread.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub(in crate::session) struct Work {
        /// The lookups made.
        pub(in crate::session) lookups: u64,
        /// The entries they looked at, an entry each time it is compared or
        /// read.
        pub(in crate::session) entries: u64,
    }

    thread_local! {
        static WORK: Cell<Work> = const {
            Cell::new(Work { lookups: 0, entries: 0 })
        };
    }

    /// What the lookups on this thread have done so far.
    pub(in crate::session) fn so_far() -> Work {
        WORK.with(Cell::get)
    }

    /// Notes a lookup begun.
    pub(super) fn lookup() {
        WORK.with(|work| {
            let so_far = work.get();
            work.set(Work {
                lookups: so_far.lookups + 1,
                ..so_far
            });
        });
    }

    /// Notes an entry looked at.
    pub(super) fn entry() {
        WORK.with(|work| {
            let so_far = work.get();
            work.set(Work {
                entries: so_far.entries + 1,
                ..so_far
            });
        });
    }
}

/// The ops of a session and everything recorded of running them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Log {
    /// Every op, the undone ones after the cursor included.
    ops: Vec<Op>,
    /// Each op's serial, in the order of `ops`: the serials rise along the
    /// log and are never reused, whatever is dropped.
    serials: Vec<u64>,
    /// The serial the last op appended was given, dropped or not; 0 before
    /// the first.
    last_serial: u64,
    /// Each op as a share document writes it, kept as ops are appended and
    /// dropped, so a share string written after a cursor move serializes
    /// only the cursor, and the saved file's size is known op by op.
    saved: SavedOps,
    /// Every event of every op, in order; at most
    /// [`SESSION_EVENT_LIMIT`](crate::limits::SESSION_EVENT_LIMIT).
    events: Vec<MicroEvent>,
    /// Where each op ends: the position after its last event.
    op_ends: Vec<EventPos>,
    /// Every entry of the log, in order, each written once as its events
    /// were recorded.
    entries: Vec<EntryRecord>,
    /// For each entry, in the order of `entries`, the index of the latest
    /// access of its run, at or before it, that reached its tag compare;
    /// none while the run has made none. The last access at a position is
    /// then one lookup, not a walk back along the run.
    last_accesses: Vec<Option<u32>>,
    /// Copies of the machine along the log, which seeks replay from.
    checkpoints: Checkpoints,
}

/// A stretch of ops to append to a log, with what running them produced. The
/// lists agree: one JSON and one end for each op, and the records are those
/// of the entries the events make.
pub(super) struct Extension {
    /// The ops.
    pub(super) ops: Vec<Op>,
    /// Each op as a share document writes it.
    pub(super) ops_json: Vec<String>,
    /// Their events.
    pub(super) events: Vec<MicroEvent>,
    /// Where each ends.
    pub(super) op_ends: Vec<EventPos>,
    /// The checkpoints due along them.
    pub(super) checkpoints: Vec<(EventPos, Checkpoint)>,
    /// The records of their entries.
    pub(super) records: Vec<EntryRecord>,
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

/// The load or store `record` makes and whether it hit or missed, when it
/// is an access that reached its tag compare.
fn completed(record: &EntryRecord) -> Option<(EntryAccess, Outcome)> {
    let outcome = match record.outcome {
        EntryOutcome::Hit => Outcome::Hit,
        EntryOutcome::Miss => Outcome::Miss,
        _ => return None,
    };
    let access = record.access.filter(|_| record.kind == EntryKind::Access)?;
    Some((access, outcome))
}

/// Where op `op` starts, in a log whose ops end at `op_ends`: the end of the
/// op before it, or position 0.
pub(super) fn op_start(op_ends: &[EventPos], op: u32) -> EventPos {
    op.checked_sub(1)
        .and_then(|before| op_ends.get(usize::try_from(before).ok()?))
        .copied()
        .unwrap_or(EventPos::new_unchecked(0))
}

impl Log {
    /// The log of a session that has run nothing: no op, and `origin` as the
    /// machine at position 0, the first run's start over `initial`.
    pub(super) fn new(initial: &Config, origin: Machine) -> Self {
        Self {
            ops: Vec::new(),
            serials: Vec::new(),
            last_serial: 0,
            saved: share::saved_ops(initial),
            events: Vec::new(),
            op_ends: Vec::new(),
            entries: Vec::new(),
            last_accesses: Vec::new(),
            checkpoints: Checkpoints::new(origin),
        }
    }

    /// Every op, the undone ones after the cursor included.
    pub(super) fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// How many ops the log holds.
    pub(super) fn op_count(&self) -> u32 {
        u32::try_from(self.ops.len()).unwrap_or(u32::MAX)
    }

    /// Op `op`.
    pub(super) fn op(&self, op: OpIndex) -> Option<&Op> {
        self.ops.get(usize::try_from(op.get()).ok()?)
    }

    /// Every event of every op, in order.
    pub(super) fn events(&self) -> &[MicroEvent] {
        &self.events
    }

    /// Every entry, in order.
    pub(super) fn entries(&self) -> &[EntryRecord] {
        &self.entries
    }

    /// Each op as a share document writes it, with the saved file's size.
    pub(super) fn saved(&self) -> &SavedOps {
        &self.saved
    }

    /// The machine at each checkpoint.
    pub(super) fn checkpoints(&self) -> &Checkpoints {
        &self.checkpoints
    }

    /// The room the saved file has for an op after the first `kept`, those
    /// after them not counted, since an op appended there drops them.
    pub(super) fn room_after(&self, kept: u32) -> FileRoom {
        self.saved.room_after(slot_of(kept))
    }

    /// The position after the last event.
    pub(super) fn end(&self) -> EventPos {
        position(self.events.len())
    }

    /// The serial the last op appended was given, whether the log still
    /// holds it or not; 0 before the first.
    pub(super) fn last_serial(&self) -> u64 {
        self.last_serial
    }

    /// The serial of op `op`.
    fn serial(&self, op: OpIndex) -> Option<u64> {
        self.serials.get(usize::try_from(op.get()).ok()?).copied()
    }

    /// The event the cursor rests on at `pos`: the one whose id is `pos − 1`;
    /// none at position 0.
    pub(super) fn event_at(&self, pos: EventPos) -> Option<&MicroEvent> {
        let id = pos.get().checked_sub(1)?;
        self.events.get(usize::try_from(id).ok()?)
    }

    /// The op holding position `pos`: the op of the event there; none at
    /// position 0.
    pub(super) fn op_holding(&self, pos: EventPos) -> Option<OpIndex> {
        self.event_at(pos).map(|event| event.op_index)
    }

    /// Where op `op` ends.
    pub(super) fn op_end(&self, op: OpIndex) -> Option<EventPos> {
        self.op_ends.get(usize::try_from(op.get()).ok()?).copied()
    }

    /// Where op `op` starts: the end of the op before it, or position 0.
    pub(super) fn op_start(&self, op: OpIndex) -> EventPos {
        op_start(&self.op_ends, op.get())
    }

    /// Whether `pos` is the end of an op, position 0 included, as the end of
    /// an op before the first.
    pub(super) fn is_op_end(&self, pos: EventPos) -> bool {
        self.op_holding(pos)
            .is_none_or(|op| self.op_end(op) == Some(pos))
    }

    /// How many ops are complete at `pos`.
    pub(super) fn ops_done(&self, pos: EventPos) -> u32 {
        match self.op_holding(pos) {
            None => 0,
            Some(op) if self.op_end(op) == Some(pos) => op.get().saturating_add(1),
            Some(op) => op.get(),
        }
    }

    /// The entry the cursor at `pos` rests on, with its index among all the
    /// log's entries; none at a run's origin.
    pub(super) fn record_at(&self, pos: EventPos) -> Option<(usize, &EntryRecord)> {
        let index = self.entries.partition_point(|record| record.last < pos);
        self.entries
            .get(index)
            .filter(|record| record.holds(pos))
            .map(|record| (index, record))
    }

    /// The steps of the log up to and including position `pos`.
    pub(super) fn steps_through(&self, pos: EventPos) -> u32 {
        let started = self.entries.partition_point(|record| record.first <= pos);
        let Some(record) = started
            .checked_sub(1)
            .and_then(|index| self.entries.get(index))
        else {
            return 0;
        };
        let within = if record.kind == EntryKind::Program {
            0
        } else {
            pos.min(record.last)
                .get()
                .saturating_sub(record.first.get())
                .saturating_add(1)
        };
        record.steps_before.saturating_add(within)
    }

    /// Where run `run` starts, at its origin, and the position after its
    /// last event.
    pub(super) fn run_bounds(&self, run: u32) -> (EventPos, EventPos) {
        let first = self.events.partition_point(|event| event.run < run);
        let end = self.events.partition_point(|event| event.run <= run);
        // A run after the first starts at its `Setup` event, the first of its
        // events; the first run starts at position 0.
        let origin = if run <= 1 {
            EventPos::new_unchecked(0)
        } else {
            position(first.saturating_add(1))
        };
        (origin, position(end).max(origin))
    }

    /// The indices of run `run`'s entries among all the log's entries.
    pub(super) fn run_entries(&self, run: u32) -> Range<usize> {
        let start = self.entries.partition_point(|record| record.run < run);
        let end = self.entries.partition_point(|record| record.run <= run);
        start..end.max(start)
    }

    /// The events at positions `first` to `last`, both included.
    pub(super) fn events_between(&self, first: EventPos, last: EventPos) -> &[MicroEvent] {
        let start = slot(first).saturating_sub(1);
        let end = slot(last).min(self.events.len());
        self.events.get(start..end.max(start)).unwrap_or_default()
    }

    /// The most recent access of run `run` completed at or before `pos`: the
    /// last access entry of the run whose last event is there or earlier
    /// and which reached its tag compare. An access an internal error
    /// abandoned before it is passed over. The entries note it as they are
    /// appended, so finding it costs a search of the entries, however long
    /// the run.
    pub(super) fn last_access(&self, pos: EventPos, run: u32) -> Option<LastAccess> {
        #[cfg(test)]
        lookup_work::lookup();
        let done = self.entries.partition_point(|record| {
            #[cfg(test)]
            lookup_work::entry();
            record.last <= pos
        });
        let latest = done.checked_sub(1)?;
        // The latest entry done is an earlier run's while this run has none
        // done yet.
        if self.entry_looked_at(latest)?.run != run {
            return None;
        }
        let index = (*self.last_accesses.get(latest)?)?;
        let record = self.entry_looked_at(usize::try_from(index).ok()?)?;
        let (access, outcome) = completed(record)?;
        let op = self.event_at(record.first)?.op_index;
        Some(LastAccess {
            kind: access.kind,
            addr: access.addr,
            outcome,
            serial: self.serial(op)?,
        })
    }

    /// The entry at `index`, as a lookup of a last access reads one: a test
    /// build counts each read, so a lookup that reads entries along a run is
    /// seen doing it. `last_access` reads entries only through this and the
    /// comparisons of its own search.
    fn entry_looked_at(&self, index: usize) -> Option<&EntryRecord> {
        #[cfg(test)]
        lookup_work::entry();
        self.entries.get(index)
    }

    /// Drops every op after the first `kept`, with their serials, JSON,
    /// events, entries and checkpoints. The serial last given stays, so none
    /// is given twice. A count past the last op leaves the log as it is.
    pub(super) fn truncate_after(&mut self, kept: u32) {
        let kept = slot_of(kept);
        let base = match kept.checked_sub(1) {
            None => EventPos::new_unchecked(0),
            Some(last) => self
                .op_ends
                .get(last)
                .copied()
                .unwrap_or_else(|| self.end()),
        };
        self.ops.truncate(kept);
        self.serials.truncate(kept);
        self.saved.truncate(kept);
        self.op_ends.truncate(kept);
        self.events.truncate(slot(base));
        let entries = self.entries.partition_point(|record| record.last <= base);
        self.entries.truncate(entries);
        self.last_accesses.truncate(entries);
        self.checkpoints.truncate_after(base);
    }

    /// Appends `extension` at the end of the log, each of its ops with the
    /// next serial.
    pub(super) fn append(&mut self, extension: Extension) {
        let Extension {
            ops,
            ops_json,
            events,
            op_ends,
            checkpoints,
            records,
        } = extension;
        for _ in &ops {
            self.last_serial = self.last_serial.saturating_add(1);
            self.serials.push(self.last_serial);
        }
        for json in &ops_json {
            self.saved.push(json);
        }
        self.ops.extend(ops);
        self.op_ends.extend(op_ends);
        // An empty log (a fresh session, or one undone back to its start)
        // takes the extension's buffer instead of copying it in: a share
        // load builds tens of thousands of events, and extending an empty
        // `Vec` would still allocate and copy every one of them into a
        // second, exact-size buffer while the extension's own buffer is
        // still alive.
        if self.events.is_empty() {
            self.events = events;
            self.events.shrink_to_fit();
        } else {
            self.events.extend(events);
        }
        self.extend_entries(records);
        for (pos, checkpoint) in checkpoints {
            self.checkpoints.insert(pos, checkpoint);
        }
    }

    /// Appends `records` to the entries, noting for each the latest access
    /// of its run, at or before it, that reached its tag compare.
    fn extend_entries(&mut self, records: Vec<EntryRecord>) {
        self.entries.reserve(records.len());
        self.last_accesses.reserve(records.len());
        for record in records {
            let latest = if completed(&record).is_some() {
                u32::try_from(self.entries.len()).ok()
            } else {
                self.entries
                    .last()
                    .filter(|before| before.run == record.run)
                    .and_then(|_| self.last_accesses.last().copied().flatten())
            };
            self.last_accesses.push(latest);
            self.entries.push(record);
        }
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::disallowed_macros, reason = "assertions are how tests fail")]

    use super::super::Session;
    use super::*;
    use crate::config::Preset;
    use crate::limits::MAX_RUN_STEPS;
    use crate::op::ProgramFormat;
    use crate::share;

    /// A loop without an exit or a memory access: one event per instruction.
    const ENDLESS: &str = "        add r1, r0, #1\nloop:   bnez r1, loop";

    fn pos(value: u32) -> EventPos {
        EventPos::new_unchecked(value)
    }

    fn load(addr: u32) -> Op {
        Op::Issue {
            kind: AccessKind::Read,
            addr: Addr::new_unchecked(addr),
            value: None,
        }
    }

    fn endless() -> Op {
        Op::Program {
            source: ENDLESS.to_owned(),
            format: ProgramFormat::Asm,
        }
    }

    fn session() -> Session {
        Session::with_config(Preset::DirectMapped8.config()).unwrap()
    }

    /// Ops whose events cross a checkpoint interval inside a `Run`, with a
    /// new run, entries that miss and entries that hit around it.
    fn ops() -> Vec<Op> {
        vec![
            load(0x0A4),
            endless(),
            Op::Run { n: MAX_RUN_STEPS },
            Op::Run { n: MAX_RUN_STEPS },
            Op::Run { n: MAX_RUN_STEPS },
            load(0x0A4),
            Op::Config {
                config: Preset::TwoWayLru.config(),
            },
            load(0x1A4),
            Op::Step,
        ]
    }

    /// The session applying `ops` one by one builds.
    fn applied(ops: &[Op]) -> Session {
        let mut session = session();
        for op in ops {
            session.apply(op.clone()).unwrap();
        }
        session
    }

    /// Where the first `kept` ops of `session` end.
    fn end_of(session: &Session, kept: u32) -> EventPos {
        kept.checked_sub(1)
            .and_then(|last| session.log.op_end(OpIndex::new_unchecked(last)))
            .unwrap_or(pos(0))
    }

    /// Asserts the lists of `log` agree: one serial, one JSON and one end
    /// for every op, one note for every entry, the events ending where the
    /// last op does, and no entry or checkpoint past them.
    fn assert_lists_agree(log: &Log, when: &str) {
        let ops = log.ops.len();
        assert_eq!(log.serials.len(), ops, "{when}: serials");
        assert_eq!(log.op_ends.len(), ops, "{when}: op ends");
        let json: Vec<String> = log.ops.iter().map(share::op_json).collect();
        assert_eq!(log.saved.text(), json.join(","), "{when}: saved JSON");
        assert_eq!(
            log.last_accesses.len(),
            log.entries.len(),
            "{when}: last accesses"
        );
        let end = log.end();
        assert_eq!(
            log.op_ends.last().copied().unwrap_or(pos(0)),
            end,
            "{when}: the last op ends the events"
        );
        assert!(
            log.entries.last().is_none_or(|record| record.last <= end),
            "{when}: an entry past the events"
        );
        assert!(
            log.checkpoints.last_at_or_before(pos(u32::MAX)) <= end,
            "{when}: a checkpoint past the events"
        );
        assert!(
            log.serials.windows(2).all(|pair| pair[0] < pair[1]),
            "{when}: serials do not rise"
        );
        assert!(
            log.serials
                .last()
                .is_none_or(|&last| last <= log.last_serial),
            "{when}: a serial past the last given"
        );
    }

    /// Asserts `log` is `expected` but for the serials, which a log that was
    /// truncated and appended to numbers differently from one built at once.
    fn assert_same_but_serials(log: &Log, expected: &Log, when: &str) {
        let mut expected = expected.clone();
        expected.serials.clone_from(&log.serials);
        expected.last_serial = log.last_serial;
        assert_eq!(*log, expected, "{when}");
    }

    #[test]
    fn a_new_log_is_empty_at_position_0() {
        let log = session().log;
        assert_lists_agree(&log, "new");
        assert_eq!(log.op_count(), 0);
        assert_eq!(log.end(), pos(0));
        assert_eq!(log.last_serial(), 0);
    }

    #[test]
    fn the_lists_agree_after_appends() {
        let mut session = session();
        for (index, op) in ops().into_iter().enumerate() {
            session.apply(op).unwrap();
            assert_lists_agree(&session.log, &format!("after op {index}"));
        }
        assert!(session.log.end().get() > crate::limits::CHECKPOINT_INTERVAL);
        assert!(session.log.checkpoints.len() > 0);
    }

    #[test]
    fn truncating_leaves_the_log_the_kept_ops_build() {
        let ops = ops();
        let whole = applied(&ops);
        let count = u32::try_from(ops.len()).unwrap();
        // Past the last op too, which leaves the log as it is.
        for kept in 0..=count + 2 {
            let mut log = whole.log.clone();
            log.truncate_after(kept);
            let when = format!("truncated to {kept}");
            assert_lists_agree(&log, &when);
            assert_eq!(log.op_count(), kept.min(count), "{when}");
            let expected = applied(ops.get(..usize::try_from(kept).unwrap()).unwrap_or(&ops));
            assert_same_but_serials(&log, &expected.log, &when);
            // The serial last given is kept, so none is given twice.
            assert_eq!(log.last_serial(), whole.log.last_serial(), "{when}");
        }
    }

    #[test]
    fn appending_after_a_truncation_leaves_the_log_the_ops_build() {
        // Each cut is followed by an op, and a cut may follow an append, as an
        // undo does; the last leaves one op after the ops it keeps.
        let cuts = [
            (7, Op::Step),
            (2, load(0x0B0)),
            (0, endless()),
            (1, load(0x1A4)),
        ];
        let mut session = applied(&ops());
        for (kept, op) in cuts {
            let when = format!("cut to {kept}, then {op:?}");
            assert!(kept <= session.op_count(), "{when}: cuts a log too short");
            session.seek(end_of(&session, kept));
            let serial = session.log.last_serial();
            session.apply(op).unwrap();
            assert_lists_agree(&session.log, &when);
            assert_eq!(session.log.op_count(), kept + 1, "{when}");
            assert_eq!(session.log.last_serial(), serial + 1, "{when}");
            let expected = applied(session.ops());
            assert_same_but_serials(&session.log, &expected.log, &when);
        }
    }

    #[test]
    fn a_share_loads_events_end_with_no_spare_capacity() {
        let mut session = Session::with_config(Preset::TwoWayLru.config()).unwrap();
        session.apply(endless()).unwrap();
        session.apply(Op::Run { n: 200 }).unwrap();
        let shared = session.to_share_string().unwrap();
        let loaded = Session::from_share_string(&shared).unwrap();
        assert!(!loaded.log.events.is_empty());
        assert_eq!(loaded.log.events.capacity(), loaded.log.events.len());
    }

    #[test]
    fn serials_rise_and_are_never_reused() {
        let mut session = session();
        assert_eq!(session.last_serial(), 0);
        for addr in [0x0A4, 0x0A8, 0x1A4] {
            session.apply(load(addr)).unwrap();
        }
        assert_eq!(session.log.serials, [1, 2, 3]);
        // Undone and replaced: the dropped serial is not given again.
        assert!(session.undo());
        session.apply(load(0x2A4)).unwrap();
        assert_eq!(session.log.serials, [1, 2, 4]);
        assert_eq!(session.last_serial(), 4);
        // A refused op takes none.
        assert!(session.apply(load(0x0A6)).is_err());
        assert_eq!(session.last_serial(), 4);
        // Several ops appended together take one each.
        session.apply_all(vec![load(0x0B0), load(0x0B4)]).unwrap();
        assert_eq!(session.log.serials, [1, 2, 4, 5, 6]);
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
        assert_eq!(loaded.log.serials, [1, 2, 3]);
        assert_eq!(loaded.last_serial(), 3);
        assert!(loaded.gate().is_none());
    }

    /// The latest access of each entry's run at or before it that reached
    /// its tag compare, found by walking back along the run.
    fn walked(session: &Session) -> Vec<Option<u32>> {
        let entries = &session.log.entries;
        (0..entries.len())
            .map(|entry| {
                let run = entries[entry].run;
                (0..=entry)
                    .rev()
                    .take_while(|&index| entries[index].run == run)
                    .find(|&index| completed(&entries[index]).is_some())
                    .map(|index| u32::try_from(index).unwrap())
            })
            .collect()
    }

    #[test]
    fn each_entry_notes_its_run_s_latest_access() {
        let mut session = session();
        let ops = [
            load(0x0A4),
            Op::Program {
                source: "add r1, r0, #1\nld r2, [0x0B0]\nadd r3, r0, #2\nst [0x1A4], r3".to_owned(),
                format: ProgramFormat::Asm,
            },
            Op::Run { n: 3 },
            Op::Edit {
                target: serde_json::from_str(r#"{"kind": "memory", "addr": 164}"#).unwrap(),
                patch: serde_json::from_str(r#"{"kind": "memory", "value": 9}"#).unwrap(),
            },
            // A new run, which keeps the program: an instruction without an
            // access, then a load.
            Op::Config {
                config: Preset::TwoWayLru.config(),
            },
            Op::Step,
            Op::Step,
            Op::StepAccess,
        ];
        for op in ops {
            session.apply(op).unwrap();
            assert_eq!(session.log.last_accesses, walked(&session));
        }
        assert!(session.log.last_accesses.contains(&None));
        // Undone, then truncated by a new op.
        assert!(session.undo());
        assert!(session.undo());
        session.apply(Op::Step).unwrap();
        assert_eq!(session.log.last_accesses, walked(&session));
        assert_eq!(session.log.last_accesses.len(), session.log.entries.len());
        // A loaded session notes the same.
        let loaded = Session::from_json(&session.to_json()).unwrap();
        assert_eq!(loaded.log.last_accesses, session.log.last_accesses);
    }
}
