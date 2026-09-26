//! Checkpoints: copies of the machine at points of the log, so that moving
//! the cursor replays a bounded stretch of the log rather than all of it.
//!
//! The session keeps one at position 0 and one at the first entry boundary
//! at or after every [`CHECKPOINT_INTERVAL`] events. A boundary inside a
//! `Run` or `StepAccess` op counts, since one such op can execute thousands
//! of instructions; each checkpoint records where the replay goes on from:
//! the op, and the instructions that op had executed there. A checkpoint at
//! the end of an op goes on with the next op from its start. The other ops
//! are a single entry, or a new run's setup and the error that may follow
//! it, so their ends are the only boundaries the session needs from them.
//!
//! A seek then replays quietly from the nearest checkpoint at or before its
//! target: at most about [`CHECKPOINT_INTERVAL`] events plus the rest of an
//! entry. Checkpoints change no result, only how fast it is reached.

use std::collections::BTreeMap;

use crate::engine::OpProgress;
use crate::limits::CHECKPOINT_INTERVAL;
use crate::machine::Machine;
use crate::units::{EventPos, OpIndex};

/// The machine at a point of the log, and where a replay from there goes on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Checkpoint {
    /// The machine there.
    pub(crate) machine: Machine,
    /// The op the replay goes on with.
    pub(crate) op: OpIndex,
    /// The instructions that op had executed there; none at its start.
    pub(crate) progress: OpProgress,
}

/// A log's checkpoints: the machine at position 0, which is the start of op
/// 0, and those taken since, by position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Checkpoints {
    /// The machine at position 0, before the first op.
    origin: Machine,
    /// The later checkpoints, by position.
    later: BTreeMap<EventPos, Checkpoint>,
}

impl Checkpoints {
    /// The checkpoints of an empty log, whose machine at position 0 is
    /// `origin`.
    pub(crate) fn new(origin: Machine) -> Self {
        Self {
            origin,
            later: BTreeMap::new(),
        }
    }

    /// The checkpoint nearest before `pos`, or at it: its position, and the
    /// checkpoint. Position 0 always has one.
    pub(crate) fn at_or_before(&self, pos: EventPos) -> (EventPos, CheckpointRef<'_>) {
        match self.later.range(..=pos).next_back() {
            Some((&at, checkpoint)) => (
                at,
                CheckpointRef {
                    machine: &checkpoint.machine,
                    op: checkpoint.op,
                    progress: checkpoint.progress,
                },
            ),
            None => (
                EventPos::new_unchecked(0),
                CheckpointRef {
                    machine: &self.origin,
                    op: OpIndex::new_unchecked(0),
                    progress: OpProgress::START,
                },
            ),
        }
    }

    /// The position of the last checkpoint at or before `pos`.
    pub(crate) fn last_at_or_before(&self, pos: EventPos) -> EventPos {
        self.at_or_before(pos).0
    }

    /// Drops the checkpoints after `pos`, as a truncation of the log there
    /// does.
    pub(crate) fn truncate_after(&mut self, pos: EventPos) {
        let after = EventPos::new_unchecked(pos.get().saturating_add(1));
        self.later.split_off(&after);
    }

    /// Adds `checkpoint` at `pos`.
    pub(crate) fn insert(&mut self, pos: EventPos, checkpoint: Checkpoint) {
        self.later.insert(pos, checkpoint);
    }

    /// How many checkpoints there are after position 0.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.later.len()
    }
}

/// A checkpoint as the session reads it: the machine, the op a replay goes
/// on with, and that op's progress.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CheckpointRef<'c> {
    /// The machine there.
    pub(crate) machine: &'c Machine,
    /// The op the replay goes on with.
    pub(crate) op: OpIndex,
    /// The instructions that op had executed there.
    pub(crate) progress: OpProgress,
}

/// Whether an entry boundary at `pos` is due a checkpoint when the last one
/// is at `last`: when an interval's worth of events has started since it,
/// counted in whole intervals from position 0, so a log of `n` events has
/// at most `n / CHECKPOINT_INTERVAL` checkpoints after position 0.
pub(crate) fn due(last: EventPos, pos: EventPos) -> bool {
    pos.get() / CHECKPOINT_INTERVAL > last.get() / CHECKPOINT_INTERVAL
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::Preset;

    fn pos(value: u32) -> EventPos {
        EventPos::new_unchecked(value)
    }

    #[test]
    fn a_checkpoint_is_due_once_per_interval() {
        assert!(!due(pos(0), pos(0)));
        assert!(!due(pos(0), pos(CHECKPOINT_INTERVAL - 1)));
        assert!(due(pos(0), pos(CHECKPOINT_INTERVAL)));
        assert!(due(pos(0), pos(CHECKPOINT_INTERVAL + 5)));
        // A checkpoint taken late in an interval does not delay the next.
        let late = pos(CHECKPOINT_INTERVAL + 7);
        assert!(!due(late, pos(2 * CHECKPOINT_INTERVAL - 1)));
        assert!(due(late, pos(2 * CHECKPOINT_INTERVAL)));
    }

    #[test]
    fn the_nearest_checkpoint_is_found_and_truncation_drops_later_ones() {
        let machine = Machine::start(Preset::DirectMapped8.config()).unwrap();
        let mut checkpoints = Checkpoints::new(machine.clone());
        let at = |checkpoints: &Checkpoints, value| {
            let (at, checkpoint) = checkpoints.at_or_before(pos(value));
            (at.get(), checkpoint.op.get(), checkpoint.progress.get())
        };
        assert_eq!(at(&checkpoints, 50), (0, 0, 0));
        for (value, op) in [(10, 2), (20, 5)] {
            checkpoints.insert(
                pos(value),
                Checkpoint {
                    machine: machine.clone(),
                    op: OpIndex::new_unchecked(op),
                    progress: OpProgress::START,
                },
            );
        }
        assert_eq!(at(&checkpoints, 9), (0, 0, 0));
        assert_eq!(at(&checkpoints, 10), (10, 2, 0));
        assert_eq!(at(&checkpoints, 19), (10, 2, 0));
        assert_eq!(at(&checkpoints, 25), (20, 5, 0));
        checkpoints.truncate_after(pos(20));
        assert_eq!(checkpoints.len(), 2);
        checkpoints.truncate_after(pos(19));
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints.last_at_or_before(pos(25)), pos(10));
        checkpoints.truncate_after(pos(0));
        assert_eq!(checkpoints.len(), 0);
        assert_eq!(at(&checkpoints, 25), (0, 0, 0));
    }
}
