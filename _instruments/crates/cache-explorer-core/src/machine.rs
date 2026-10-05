//! The machine: the state of a run at one point, which the engine folds ops
//! into, and the invariants that state keeps.
//!
//! A machine is created only at the start of a run; every later state comes
//! from replaying ops onto it. [`Machine::check_invariants`] checks the
//! invariants below and reports the first one broken rather than panicking,
//! so a bug ends the op with a message instead of poisoning the session:
//!
//! - I1: the ranks in each set are a permutation of `0..ways`.
//! - I2: under LRU and FIFO, every valid way ranks below every invalid way.
//!   This one holds between accesses rather than at every step of one. A
//!   miss takes the lowest-numbered invalid way, and until an edit moves the
//!   invalid ways out of way order that way also ranks first among them, so
//!   without edits I2 holds mid-access too. After such an edit, a fill can
//!   make a way valid while it still ranks behind an invalid one, until the
//!   rank update moves it to the front.
//! - I3: at most one valid way per set holds a given tag.
//! - I4: a valid way's line address decodes to its own set and tag.
//! - I5: a dirty way exists only under write-back.
//! - I6: a valid clean way's data equals memory's copy of its line. Like I2,
//!   this one holds between accesses: a write-through store writes the cache
//!   one step before it writes memory.
//! - I7: under random replacement, and in a one-way set, the ranks equal the
//!   way numbers.
//! - I8: every line the cache holds is among the lines memory has recorded
//!   as touched.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::cache::{Cache, Dirtiness, Set, Slot, Way};
use crate::config::{Config, ConfigError, Replacement, WritePolicy};
use crate::cpu::CpuState;
use crate::event::ChangeRef;
use crate::format;
use crate::geometry::{Geometry, Location};
use crate::limits::MAX_LINES;
use crate::memory::{word_at, Memory};
use crate::prng::Prng;
use crate::replacement::VictimChoice;
use crate::request::Request;
use crate::stats::{Counters, Recent, Shadow};
use crate::units::{Cycle, LineAddr, SetIndex, Tag, WayIndex};

/// The state of a run at one point.
///
/// It holds the run's configuration and geometry, the cache, memory, the CPU
/// with its registers, the loaded program and its place in it, the counters,
/// what the classification of misses and the sparkline remember, the access
/// in progress, the run's place, and the run's generator.
///
/// There is no way to obtain one from outside the core other than by replaying
/// a run, so every machine is one a sequence of ops can reach. Not
/// serializable: nothing crosses the boundary as a `Machine`, and the
/// generator's state never leaves the core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    /// The run's configuration, which is valid.
    pub(crate) config: Config,
    /// The geometry `config` validates to.
    pub(crate) geometry: Geometry,
    /// The cache's contents and replacement order.
    pub(crate) cache: Cache,
    /// Main memory.
    pub(crate) memory: Memory,
    /// The CPU: its registers, the loaded program or trace, if any, and the
    /// CPU's place in it.
    pub(crate) cpu: CpuState,
    /// The run's counters.
    pub(crate) counters: Counters,
    /// The fully associative LRU cache that tells capacity misses from
    /// conflict misses, which sees every access of the run.
    pub(crate) shadow: Shadow,
    /// Every line the run has filled, so a miss on any other line is
    /// compulsory. It grows with the distinct lines filled, which the
    /// session's event limit bounds.
    pub(crate) filled: BTreeSet<LineAddr>,
    /// The outcomes of the run's latest accesses, for the sparkline.
    pub(crate) recent: Recent,
    /// The generator random replacement draws its victims from, seeded from
    /// `config.seed` when the run starts.
    pub(crate) prng: Prng,
    /// The access of the current entry, from its first event on; `None`
    /// before the run's first access and while the current entry is not an
    /// access.
    pub(crate) access: Option<AccessContext>,
    /// Where the run has got to: its number and its entries so far.
    pub(crate) run: RunPlace,
}

/// Where a run has got to, which is what the emitter numbers events by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RunPlace {
    /// The run's number, from 1.
    pub(crate) run: u32,
    /// The entries the run has had so far; the current entry's number.
    pub(crate) entries: u32,
}

/// The access an entry is making: the request, where its address falls, and
/// what the lookup and the allocation have found so far. It is controller
/// state, replaced at the start of each access, and what a view of the cache
/// shows while the access is in flight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccessContext {
    /// The load or store.
    pub(crate) request: Request,
    /// Where its address falls.
    pub(crate) location: Location,
    /// What the tag compare found, from the `Hit` or `Miss` event on.
    pub(crate) lookup: Option<Lookup>,
    /// The way a miss chose, from the `Victim` event until the `Fill`.
    pub(crate) victim: Option<VictimChoice>,
    /// The way the access reads or writes, once the lookup has found it or
    /// the fill has brought its line in.
    pub(crate) way: Option<WayIndex>,
    /// The miss's stall, from the `Stall` event on; only in timed mode.
    pub(crate) stall: Option<StallWindow>,
    /// False until the access's last event: while false, the access is in
    /// flight and the machine is not between entries.
    pub(crate) done: bool,
    /// The run's counters as they stood before the access's first event.
    /// Only accesses move counters, so these are the counters after the
    /// run's last completed access, which AMAT and the measured cost show
    /// while this access is in flight.
    pub(crate) counters_before: Counters,
}

/// How long a miss stalls the CPU in timed mode, and when the stall is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StallWindow {
    /// The cycles the CPU waits in all: the miss penalty, or twice it when a
    /// dirty victim goes back to memory before the fill.
    pub(crate) total: u32,
    /// The run's cycle count once the last transfer, the fill or the store's
    /// write to memory, is over.
    pub(crate) end: Cycle,
}

/// The CPU's stall countdown at one point of an access: "Stalled: 20
/// cycles".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stalled {
    /// The cycles left until memory is done.
    pub(crate) remaining: u32,
    /// The stall's length in all.
    pub(crate) total: u32,
}

/// What an access's tag compare found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Lookup {
    /// The way holding the line, on a hit.
    pub(crate) hit: Option<WayIndex>,
    /// The tag each way of the set held when it was compared, in way order;
    /// `None` for an invalid way. The comparators show the compare against
    /// these, not against the tags the access later fills in.
    pub(crate) tags: Vec<Option<Tag>>,
}

/// A machine invariant that no longer holds, and where.
///
/// `Display` names the invariant as the internal-error message quotes it:
/// "Internal error: invariant I3 (no two valid ways in set 2 hold the same
/// tag) no longer holds."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum InvariantViolation {
    /// I1: a set's ranks are not a permutation of `0..ways`.
    #[error("invariant I1 ({})", ranks_rule(*.set, *.ways))]
    RanksNotAPermutation {
        /// The set.
        set: SetIndex,
        /// The ways each set has.
        ways: u32,
    },
    /// I2: under LRU or FIFO, a valid way ranks after an invalid one.
    #[error(
        "invariant I2 (every valid way in set {} ranks below every invalid way)",
        format::data(.set.get())
    )]
    ValidRankedAfterInvalid {
        /// The set.
        set: SetIndex,
    },
    /// I3: two valid ways of a set hold the same tag.
    #[error(
        "invariant I3 (no two valid ways in set {} hold the same tag)",
        format::data(.set.get())
    )]
    DuplicateTag {
        /// The set.
        set: SetIndex,
    },
    /// I4: a valid way's line does not decode to its own set and tag.
    #[error(
        "invariant I4 (the line in way {} of set {} decodes to that set and tag)",
        format::data(.way.get()),
        format::data(.set.get())
    )]
    LineOutsideItsSet {
        /// The set.
        set: SetIndex,
        /// The way holding the line.
        way: WayIndex,
    },
    /// I5: a way is dirty under write-through.
    #[error(
        "invariant I5 (way {} of set {} is not dirty under write-through)",
        format::data(.way.get()),
        format::data(.set.get())
    )]
    DirtyUnderWriteThrough {
        /// The set.
        set: SetIndex,
        /// The dirty way.
        way: WayIndex,
    },
    /// I6: a valid clean way's data differs from memory's copy of its line.
    #[error(
        "invariant I6 (the clean line in way {} of set {} matches memory)",
        format::data(.way.get()),
        format::data(.set.get())
    )]
    CleanLineDiffersFromMemory {
        /// The set.
        set: SetIndex,
        /// The clean way.
        way: WayIndex,
    },
    /// I7: under random replacement, or in a one-way set, a set's ranks are
    /// not its way numbers.
    #[error(
        "invariant I7 (the ranks in set {} equal the way numbers)",
        format::data(.set.get())
    )]
    RanksNotWayNumbers {
        /// The set.
        set: SetIndex,
    },
    /// I8: a way holds a line that memory has not recorded as touched.
    #[error(
        "invariant I8 (the line in way {} of set {} is among the lines memory has touched)",
        format::data(.way.get()),
        format::data(.set.get())
    )]
    LineNotInMemory {
        /// The set.
        set: SetIndex,
        /// The way holding the line.
        way: WayIndex,
    },
}

impl Machine {
    /// The machine at the start of a run of `config`: every way invalid and
    /// ranked by its way number, memory untouched, registers and counters at
    /// 0, no program loaded, no line filled, an empty shadow cache and no
    /// outcome yet, no access yet, and the generator seeded from
    /// `config.seed`.
    ///
    /// # Errors
    ///
    /// The [`ConfigError`] of the first rule `config` breaks.
    pub(crate) fn start(config: Config) -> Result<Self, ConfigError> {
        let geometry = config.validate()?;
        Ok(Self {
            cache: Cache::new(&geometry),
            memory: Memory::new(&geometry, config.memory_init),
            cpu: CpuState::default(),
            counters: Counters::default(),
            shadow: Shadow::new(geometry.lines()),
            filled: BTreeSet::new(),
            recent: Recent::default(),
            prng: Prng::new(config.seed),
            access: None,
            run: RunPlace { run: 1, entries: 0 },
            geometry,
            config,
        })
    }

    /// True while an access has started and not yet had its last event.
    pub(crate) fn in_flight(&self) -> bool {
        self.access.as_ref().is_some_and(|access| !access.done)
    }

    /// The stall countdown the CPU shows: from a miss's `Stall` event until
    /// the event that ends its last transfer, the cycles left and the stall's
    /// length in all. With a dirty victim and 20-cycle misses it reads 40,
    /// then 20 once the write-back is done. `None` at every other point, and
    /// always when untimed.
    pub(crate) fn stall(&self) -> Option<Stalled> {
        let window = self.access.as_ref()?.stall?;
        let left = window.end.get().saturating_sub(self.counters.cycles.get());
        (left > 0).then(|| Stalled {
            remaining: u32::try_from(left).unwrap_or(u32::MAX),
            total: window.total,
        })
    }

    /// The value of the cell or counter `cell` names, as the net change of an
    /// entry compares it: a flag as 0 or 1, and a number as itself. A way's
    /// tag, and each of its words, is `None` while the way is invalid, so a
    /// way whose line changed has a different tag even when the numbers
    /// happen to agree. `None` too for a way the cache does not have.
    pub(crate) fn cell(&self, cell: ChangeRef) -> Option<u64> {
        match cell {
            ChangeRef::Valid { set, way } => Some(u64::from(self.cache.way(set, way)?.is_valid())),
            ChangeRef::Tag { set, way } => self
                .cache
                .way(set, way)?
                .tag()
                .map(|tag| u64::from(tag.get())),
            ChangeRef::Dirty { set, way } => Some(u64::from(self.cache.way(set, way)?.is_dirty())),
            ChangeRef::Rank { set, way } => Some(u64::from(self.cache.way(set, way)?.rank)),
            ChangeRef::Word { set, way, word } => match &self.cache.way(set, way)?.slot {
                Slot::Valid { data, .. } => {
                    let word_bytes = self.geometry.word_bytes();
                    Some(word_at(
                        data,
                        word.get().saturating_mul(word_bytes),
                        word_bytes,
                    ))
                }
                Slot::Invalid => None,
            },
            ChangeRef::Memory { addr } => Some(self.memory.word(addr).get()),
            ChangeRef::Register { register } => Some(self.cpu.register(register)),
            ChangeRef::Pc => Some(u64::from(self.cpu.pc())),
            ChangeRef::Counter { counter } => Some(self.counters.get(counter)),
        }
    }

    /// Checks the machine's invariants, set by set. I2 and I6 hold only
    /// between accesses, so they are checked only when no access is in
    /// flight; the others are checked always.
    ///
    /// # Errors
    ///
    /// The first invariant found broken: in the lowest-numbered set that
    /// breaks one, the lowest-numbered invariant it breaks, at the
    /// lowest-numbered way that breaks it.
    pub fn check_invariants(&self) -> Result<(), InvariantViolation> {
        for set in (0..self.geometry.sets()).map(SetIndex::new_unchecked) {
            // A missing set has no ranks, which I1 reports.
            let ways = self.cache.set(set).map_or(&[][..], Set::ways);
            self.check_set(set, ways)?;
        }
        Ok(())
    }

    /// Checks the invariants of one set, whose ways are `ways`.
    fn check_set(&self, set: SetIndex, ways: &[Way]) -> Result<(), InvariantViolation> {
        let count = self.geometry.ways();
        let replacement = self.config.cache.replacement;

        if !ranks_are_permutation(ways, count) {
            return Err(InvariantViolation::RanksNotAPermutation { set, ways: count });
        }

        let between_accesses = !self.in_flight();
        if between_accesses && matches!(replacement, Replacement::Lru | Replacement::Fifo) {
            let last_valid = ways
                .iter()
                .filter(|way| way.is_valid())
                .map(|way| way.rank)
                .max();
            let first_invalid = ways
                .iter()
                .filter(|way| !way.is_valid())
                .map(|way| way.rank)
                .min();
            if let (Some(last_valid), Some(first_invalid)) = (last_valid, first_invalid) {
                if last_valid > first_invalid {
                    return Err(InvariantViolation::ValidRankedAfterInvalid { set });
                }
            }
        }

        if has_duplicate_tag(ways) {
            return Err(InvariantViolation::DuplicateTag { set });
        }

        // A way's set is its place in the cache, so the line rebuilt from its
        // tag and set always has that set's index. It decodes back to the same
        // tag, and so to the same line, exactly when the tag fits the tag
        // field: the excess of a wider tag would fall outside the address.
        let too_wide = (0_u32..).zip(ways).find_map(|(number, way)| {
            way.tag()
                .filter(|tag| self.geometry.tag(tag.get()).is_none())
                .map(|_| number)
        });
        if let Some(number) = too_wide {
            return Err(InvariantViolation::LineOutsideItsSet {
                set,
                way: WayIndex::new_unchecked(number),
            });
        }

        if self.config.cache.write_policy == WritePolicy::WriteThrough {
            let dirty = (0_u32..)
                .zip(ways)
                .find_map(|(number, way)| way.is_dirty().then_some(number));
            if let Some(number) = dirty {
                return Err(InvariantViolation::DirtyUnderWriteThrough {
                    set,
                    way: WayIndex::new_unchecked(number),
                });
            }
        }

        let (clean_differs, untouched) = self.memory_state(set, ways, between_accesses);
        if between_accesses {
            if let Some(number) = clean_differs {
                return Err(InvariantViolation::CleanLineDiffersFromMemory {
                    set,
                    way: WayIndex::new_unchecked(number),
                });
            }
        }

        if (replacement == Replacement::Random || count == 1)
            && !(0_u32..).zip(ways).all(|(number, way)| way.rank == number)
        {
            return Err(InvariantViolation::RanksNotWayNumbers { set });
        }

        if let Some(number) = untouched {
            return Err(InvariantViolation::LineNotInMemory {
                set,
                way: WayIndex::new_unchecked(number),
            });
        }
        Ok(())
    }

    /// I6 and I8 together, in one pass over `ways`: the lowest-numbered way
    /// whose clean line differs from memory's copy of it (I6, meaningful
    /// only `between_accesses`), and the lowest-numbered way holding a line
    /// memory has not recorded as touched (I8). Every way with a tag looks
    /// its line up in memory once, serving both checks.
    fn memory_state(
        &self,
        set: SetIndex,
        ways: &[Way],
        between_accesses: bool,
    ) -> (Option<u32>, Option<u32>) {
        let mut clean_differs = None;
        let mut untouched = None;
        for (number, way) in (0_u32..).zip(ways) {
            let Some(tag) = way.tag() else {
                continue;
            };
            let line = self.geometry.line_addr(tag, set);
            let touched = self.memory.touched_bytes(line);
            if between_accesses && clean_differs.is_none() {
                if let Slot::Valid {
                    dirty: Dirtiness::Clean,
                    data,
                    ..
                } = &way.slot
                {
                    // An untouched line reads as its configuration's initial
                    // contents; falling back to that keeps a clean way whose
                    // data differs from it reporting I6, not I8.
                    let differs = match touched {
                        Some(bytes) => bytes != data.as_slice(),
                        None => !self.memory.holds(line, data),
                    };
                    if differs {
                        clean_differs = Some(number);
                    }
                }
            }
            if untouched.is_none() && touched.is_none() {
                untouched = Some(number);
            }
        }
        (clean_differs, untouched)
    }
}

/// I1: whether `ways`' ranks are a permutation of `0..count`, without
/// allocating. There are exactly `count` of them, and folding each rank
/// into a bit of a `u64` finds any that repeats or falls outside `0..count`;
/// since both conditions are ruled out and there are exactly `count` ways,
/// the ranks found can only be `0..count` itself. A set has at most
/// [`MAX_LINES`] ways, so every rank that belongs fits the bitmask; a
/// `checked_shl` keeps one that does not from ever shifting out of range.
fn ranks_are_permutation(ways: &[Way], count: u32) -> bool {
    if u32::try_from(ways.len()) != Ok(count) {
        return false;
    }
    let mut seen: u64 = 0;
    for way in ways {
        if way.rank >= count {
            return false;
        }
        let Some(bit) = 1_u64.checked_shl(way.rank) else {
            return false;
        };
        if seen & bit != 0 {
            return false;
        }
        seen |= bit;
    }
    true
}

/// I3: whether two valid ways of `ways` hold the same tag. A pairwise scan
/// for the small sets most caches have; above 8 ways, the valid tags are
/// copied into a fixed-size array and sorted instead, since a sort finds a
/// repeat in fewer comparisons than the pairwise scan does once there are
/// enough of them. The array holds at most [`MAX_LINES`] tags, the most a
/// set can have; I1 has already confirmed `ways` holds no more than that.
fn has_duplicate_tag(ways: &[Way]) -> bool {
    if ways.len() <= 8 {
        return ways.iter().enumerate().any(|(index, way)| {
            way.tag().is_some_and(|tag| {
                ways.iter()
                    .take(index)
                    .any(|other| other.tag() == Some(tag))
            })
        });
    }
    let mut tags = [0_u32; MAX_LINES as usize];
    let mut valid = 0;
    for (slot, tag) in tags.iter_mut().zip(ways.iter().filter_map(Way::tag)) {
        *slot = tag.get();
        valid += 1;
    }
    let tags = tags.get_mut(..valid).unwrap_or_default();
    tags.sort_unstable();
    tags.windows(2).any(|pair| matches!(pair, [a, b] if a == b))
}

/// I1 as its message states it for a set of `ways` ways: "the ranks in set 2
/// are a permutation of 0 to 3", or "the only way in set 2 has rank 0".
fn ranks_rule(set: SetIndex, ways: u32) -> String {
    let set = format::data(set.get());
    if ways <= 1 {
        format!("the only way in set {set} has rank 0")
    } else {
        format!(
            "the ranks in set {set} are a permutation of 0 to {}",
            format::data(ways - 1)
        )
    }
}

#[cfg(test)]
mod tests;
