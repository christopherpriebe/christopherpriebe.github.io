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
use crate::cpu::{Cpu, Registers};
use crate::engine::Request;
use crate::event::ChangeRef;
use crate::format;
use crate::geometry::{Geometry, Location};
use crate::isa::{Placed, Program};
use crate::memory::{word_at, Memory};
use crate::prng::Prng;
use crate::replacement::VictimChoice;
use crate::stats::{Counters, Recent, Shadow};
use crate::units::{Cycle, LineAddr, SetIndex, Tag, WayIndex};

/// The state of a run at one point: its configuration and geometry, the
/// cache, memory, registers and counters, the loaded program and the CPU's
/// place in it, what the classification of misses and the sparkline
/// remember, the access in progress, and the run's generator.
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
    /// The CPU's registers.
    pub(crate) registers: Registers,
    /// The loaded program or trace, if any.
    pub(crate) program: Option<Program>,
    /// The CPU's place in the program: the pc, the instructions executed
    /// against the budget, and whether the program stopped early.
    pub(crate) cpu: Cpu,
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
    /// The run's number, from 1.
    pub(crate) run: u32,
    /// The entries the run has had so far; the current entry's number.
    pub(crate) entries_in_run: u32,
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

/// A machine invariant that no longer holds, and where. `Display` names the
/// invariant as the internal-error message quotes it: "Internal error:
/// invariant I3 (no two valid ways in set 2 hold the same tag) no longer
/// holds."
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
            registers: Registers::default(),
            program: None,
            cpu: Cpu::default(),
            counters: Counters::default(),
            shadow: Shadow::new(geometry.lines()),
            filled: BTreeSet::new(),
            recent: Recent::default(),
            prng: Prng::new(config.seed),
            access: None,
            run: 1,
            entries_in_run: 0,
            geometry,
            config,
        })
    }

    /// True while an access has started and not yet had its last event.
    pub(crate) fn in_flight(&self) -> bool {
        self.access.as_ref().is_some_and(|access| !access.done)
    }

    /// The instruction the program executes next: the one at the pc, while
    /// a program is loaded, has not stopped and has not finished.
    pub(crate) fn next_instruction(&self) -> Option<&Placed> {
        if self.cpu.halted.is_some() {
            return None;
        }
        self.program.as_ref()?.get(self.cpu.pc)
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
            ChangeRef::Register { register } => Some(self.registers.get(register)),
            ChangeRef::Pc => Some(u64::from(self.cpu.pc)),
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

        let mut ranks: Vec<u32> = ways.iter().map(|way| way.rank).collect();
        ranks.sort_unstable();
        if !ranks.into_iter().eq(0..count) {
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

        let duplicate = ways.iter().enumerate().any(|(index, way)| {
            way.tag().is_some_and(|tag| {
                ways.iter()
                    .take(index)
                    .any(|other| other.tag() == Some(tag))
            })
        });
        if duplicate {
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

        if between_accesses {
            let differs = (0_u32..)
                .zip(ways)
                .find_map(|(number, way)| match &way.slot {
                    Slot::Valid {
                        tag,
                        dirty: Dirtiness::Clean,
                        data,
                    } => {
                        let line = self.geometry.line_addr(*tag, set);
                        (!self.memory.holds(line, data)).then_some(number)
                    }
                    _ => None,
                });
            if let Some(number) = differs {
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

        let untouched = (0_u32..).zip(ways).find_map(|(number, way)| {
            way.tag()
                .filter(|tag| !self.memory.is_touched(self.geometry.line_addr(*tag, set)))
                .map(|_| number)
        });
        if let Some(number) = untouched {
            return Err(InvariantViolation::LineNotInMemory {
                set,
                way: WayIndex::new_unchecked(number),
            });
        }
        Ok(())
    }
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
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::{AllocatePolicy, CacheConfig, MemoryInit, Preset, WordBytes};
    use crate::limits::{MAX_ADDRESS_BITS, MAX_LINES};
    use crate::units::Tag;

    /// A configuration of `size` bytes in 16 B lines, `ways`-way, under
    /// `replacement` and `write_policy`, with `Config::default()`'s address
    /// width and word size.
    fn config(size: u32, ways: u32, replacement: Replacement, write_policy: WritePolicy) -> Config {
        Config {
            cache: CacheConfig {
                size_bytes: size,
                line_bytes: 16,
                associativity: ways,
                replacement,
                write_policy,
                allocate_policy: AllocatePolicy::WriteAllocate,
            },
            ..Config::default()
        }
    }

    fn start(config: Config) -> Machine {
        Machine::start(config).unwrap()
    }

    fn set(index: u32) -> SetIndex {
        SetIndex::new_unchecked(index)
    }

    fn way(index: u32) -> WayIndex {
        WayIndex::new_unchecked(index)
    }

    fn line(machine: &Machine, tag: u32, dirty: Dirtiness) -> Slot {
        Slot::Valid {
            tag: Tag::new_unchecked(tag),
            dirty,
            data: vec![0; machine.geometry.line_bytes() as usize],
        }
    }

    /// Puts `slot` at rank `rank` into way `index` of set `number`, as a
    /// fill would: its line is recorded as touched in memory, and a clean
    /// line takes memory's bytes.
    fn put(machine: &mut Machine, number: u32, index: u32, slot: Slot, rank: u32) {
        let slot = match slot {
            Slot::Valid { tag, dirty, data } => {
                let line = machine.geometry.line_addr(tag, set(number));
                machine.memory.touch(line);
                let data = match dirty {
                    Dirtiness::Clean => machine.memory.line(line),
                    Dirtiness::Dirty => data,
                };
                Slot::Valid { tag, dirty, data }
            }
            Slot::Invalid => Slot::Invalid,
        };
        let target = machine.cache.way_mut(set(number), way(index)).unwrap();
        target.slot = slot;
        target.rank = rank;
    }

    /// A miss on `addr` up to its rank update: the victim filled, clean, and
    /// not yet ranked. Returns the set and the way filled.
    fn fill_unranked(machine: &mut Machine, addr: u32) -> (SetIndex, WayIndex) {
        let decoded = machine.geometry.decode(addr).unwrap();
        let policy = machine.config.cache.replacement.policy();
        machine.memory.touch(decoded.line);
        let data = machine.memory.line(decoded.line);
        let set = machine.cache.set_mut(decoded.index).unwrap();
        let choice = policy.victim(set.ways(), &mut machine.prng);
        set.way_mut(choice.way).unwrap().slot = Slot::Valid {
            tag: decoded.tag,
            dirty: Dirtiness::Clean,
            data,
        };
        (decoded.index, choice.way)
    }

    /// One load or store of `addr` as the engine will make it: a hit is
    /// ranked, and a miss fills its victim, clean, and ranks the fill. A
    /// store under write-back leaves its line dirty.
    fn access(machine: &mut Machine, addr: u32, store: bool) {
        let decoded = machine.geometry.decode(addr).unwrap();
        let policy = machine.config.cache.replacement.policy();
        let dirty = if store && machine.config.cache.write_policy == WritePolicy::WriteBack {
            Dirtiness::Dirty
        } else {
            Dirtiness::Clean
        };
        let hit = (0_u32..)
            .zip(machine.cache.set(decoded.index).unwrap().ways())
            .find_map(|(index, way)| (way.tag() == Some(decoded.tag)).then_some(index));
        let target = match hit {
            Some(index) => way(index),
            None => fill_unranked(machine, addr).1,
        };
        let set = machine.cache.set_mut(decoded.index).unwrap();
        if hit.is_some() {
            policy.on_access(set.ways_mut(), target);
        } else {
            policy.on_fill(set.ways_mut(), target);
        }
        if let Slot::Valid { dirty: bit, .. } = &mut set.way_mut(target).unwrap().slot {
            if dirty == Dirtiness::Dirty {
                *bit = Dirtiness::Dirty;
            }
        }
    }

    #[test]
    fn a_run_starts_with_an_empty_cache_and_a_seeded_generator() {
        let config = Preset::TwoWayLru.config();
        let machine = start(config.clone());
        assert_eq!(machine.geometry, config.validate().unwrap());
        assert_eq!(machine.cache, Cache::new(&machine.geometry));
        assert_eq!(
            machine.memory,
            Memory::new(&machine.geometry, MemoryInit::Pattern)
        );
        assert_eq!(machine.registers, Registers::default());
        assert_eq!(machine.counters, Counters::default());
        assert_eq!(machine.shadow, Shadow::new(machine.geometry.lines()));
        assert!(machine.shadow.lines().is_empty());
        assert!(machine.filled.is_empty());
        assert_eq!(machine.recent, Recent::default());
        assert_eq!(machine.prng, Prng::new(config.seed));
        assert_eq!(machine.access, None);
        assert_eq!((machine.run, machine.entries_in_run), (1, 0));
        assert!(!machine.in_flight());
        assert_eq!(machine.config, config);
        assert_eq!(machine.check_invariants(), Ok(()));
    }

    #[test]
    fn a_run_cannot_start_from_an_invalid_configuration() {
        let mut config = Config::default();
        config.cache.associativity = 3;
        assert_eq!(
            Machine::start(config.clone()),
            Err(config.validate().unwrap_err())
        );
    }

    #[test]
    fn every_invariant_holds_through_a_run_of_accesses() {
        // Every placement and policy up to a fully associative cache of
        // `MAX_LINES` lines, under both write policies, over a pseudo-random
        // stream of loads and stores to three times as many lines as the
        // cache holds. The widest addresses hold those lines and a tag bit
        // whatever `MAX_LINES` is.
        let mut ways = 1;
        while ways <= MAX_LINES {
            for replacement in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
                for write_policy in [WritePolicy::WriteBack, WritePolicy::WriteThrough] {
                    let size = MAX_LINES * 16;
                    let mut machine = start(Config {
                        address_bits: MAX_ADDRESS_BITS,
                        ..config(size, ways, replacement, write_policy)
                    });
                    let mut stream = Prng::new(u64::from(ways));
                    for _ in 0..300 {
                        let line = stream.draw_way(MAX_LINES * 4) % (MAX_LINES * 3);
                        let store = stream.draw_way(2) == 1;
                        access(&mut machine, line * 16, store);
                        assert_eq!(machine.check_invariants(), Ok(()));
                    }
                }
            }
            ways *= 2;
        }
    }

    #[test]
    fn random_replacement_draws_from_the_run_generator() {
        // Two machines with the same seed and the same accesses reach the same
        // state; a different seed evicts differently. The accesses cycle
        // through eight lines of set 0, twice what it holds, all within the
        // narrowest addresses.
        let config = config(128, 4, Replacement::Random, WritePolicy::WriteBack);
        let addrs: Vec<u32> = (0..40).map(|i| (i * 5 % 8) * 0x20).collect();
        let run = |config: Config| {
            let mut machine = start(config);
            for &addr in &addrs {
                access(&mut machine, addr, false);
            }
            machine
        };
        let first = run(config.clone());
        assert_eq!(first, run(config.clone()));
        assert_ne!(first.prng, Prng::new(config.seed));
        let other = run(Config {
            seed: 1,
            ..config.clone()
        });
        assert_ne!(first.cache, other.cache);
    }

    #[test]
    fn i1_reports_ranks_that_are_not_a_permutation() {
        let mut machine = start(Preset::FourWayWriteBack.config());
        machine.cache.way_mut(set(2), way(3)).unwrap().rank = 1;
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::RanksNotAPermutation {
                set: set(2),
                ways: 4
            })
        );
        machine.cache.way_mut(set(2), way(3)).unwrap().rank = 4;
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::RanksNotAPermutation {
                set: set(2),
                ways: 4
            })
        );
        let mut machine = start(Config::default());
        machine.cache.way_mut(set(5), way(0)).unwrap().rank = 1;
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::RanksNotAPermutation {
                set: set(5),
                ways: 1
            })
        );
    }

    #[test]
    fn i2_reports_a_valid_way_ranked_after_an_invalid_one() {
        for replacement in [Replacement::Lru, Replacement::Fifo] {
            let mut machine = start(config(256, 4, replacement, WritePolicy::WriteBack));
            let slot = line(&machine, 1, Dirtiness::Clean);
            // Way 1 is valid and ranked 1; way 0 is invalid and ranked 0.
            put(&mut machine, 3, 1, slot, 1);
            assert_eq!(
                machine.check_invariants(),
                Err(InvariantViolation::ValidRankedAfterInvalid { set: set(3) })
            );
        }
        let mut machine = start(config(256, 4, Replacement::Random, WritePolicy::WriteBack));
        let slot = line(&machine, 1, Dirtiness::Clean);
        put(&mut machine, 3, 1, slot, 1);
        assert_eq!(machine.check_invariants(), Ok(()));
    }

    #[test]
    fn i2_breaks_mid_fill_only_after_invalid_ways_leave_way_order() {
        for replacement in [Replacement::Lru, Replacement::Fifo] {
            let policy = replacement.policy();
            let mut machine = start(config(64, 4, replacement, WritePolicy::WriteBack));
            // Without edits, the invalid way a miss takes also ranks first
            // among the invalid ways, so I2 holds between a fill and its
            // rank update.
            for addr in [0x000, 0x010, 0x020, 0x030] {
                let (index, filled) = fill_unranked(&mut machine, addr);
                assert_eq!(machine.check_invariants(), Ok(()));
                policy.on_fill(machine.cache.set_mut(index).unwrap().ways_mut(), filled);
                assert_eq!(machine.check_invariants(), Ok(()));
            }
            let ranks = |machine: &Machine| -> Vec<u32> {
                let ways = machine.cache.set(set(0)).unwrap().ways();
                ways.iter().map(|way| way.rank).collect()
            };
            assert_eq!(ranks(&machine), [3, 2, 1, 0]);
            // An edit that invalidates a way sends it to rank last.
            // Invalidating way 3 and then way 0 leaves way 0 ranked behind
            // way 3.
            for (index, rank) in (0..).zip([3, 1, 0, 2]) {
                machine.cache.way_mut(set(0), way(index)).unwrap().rank = rank;
            }
            for index in [0, 3] {
                machine.cache.way_mut(set(0), way(index)).unwrap().slot = Slot::Invalid;
            }
            assert_eq!(machine.check_invariants(), Ok(()));
            // The next miss fills way 0, which ranks behind invalid way 3
            // until its rank update.
            let (index, filled) = fill_unranked(&mut machine, 0x040);
            assert_eq!(filled, way(0));
            assert_eq!(
                machine.check_invariants(),
                Err(InvariantViolation::ValidRankedAfterInvalid { set: set(0) })
            );
            policy.on_fill(machine.cache.set_mut(index).unwrap().ways_mut(), filled);
            assert_eq!(ranks(&machine), [0, 2, 1, 3]);
            assert_eq!(machine.check_invariants(), Ok(()));
        }
    }

    #[test]
    fn i3_reports_a_tag_held_twice_in_a_set() {
        let mut machine = start(Preset::TwoWayLru.config());
        let slot = line(&machine, 1, Dirtiness::Clean);
        put(&mut machine, 1, 0, slot.clone(), 1);
        put(&mut machine, 1, 1, slot.clone(), 0);
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::DuplicateTag { set: set(1) })
        );
        // The same tag in two sets is two different lines.
        let mut machine = start(Preset::TwoWayLru.config());
        put(&mut machine, 1, 0, slot.clone(), 0);
        put(&mut machine, 2, 0, slot, 0);
        assert_eq!(machine.check_invariants(), Ok(()));
    }

    #[test]
    fn i4_reports_a_tag_too_wide_for_its_line_to_decode_back() {
        // The widest tag the tag field holds is the last that fits.
        let mut machine = start(Preset::TwoWayLru.config());
        let limit = 1_u32 << machine.geometry.tag_bits();
        let slot = line(&machine, limit - 1, Dirtiness::Clean);
        put(&mut machine, 2, 0, slot, 0);
        assert_eq!(machine.check_invariants(), Ok(()));
        let slot = line(&machine, limit, Dirtiness::Clean);
        put(&mut machine, 2, 0, slot, 0);
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::LineOutsideItsSet {
                set: set(2),
                way: way(0)
            })
        );
    }

    #[test]
    fn i5_reports_a_dirty_way_under_write_through() {
        let mut machine = start(config(128, 2, Replacement::Lru, WritePolicy::WriteThrough));
        let slot = line(&machine, 1, Dirtiness::Dirty);
        put(&mut machine, 0, 1, slot.clone(), 0);
        machine.cache.way_mut(set(0), way(0)).unwrap().rank = 1;
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::DirtyUnderWriteThrough {
                set: set(0),
                way: way(1)
            })
        );
        let mut machine = start(config(128, 2, Replacement::Lru, WritePolicy::WriteBack));
        put(&mut machine, 0, 1, slot, 0);
        machine.cache.way_mut(set(0), way(0)).unwrap().rank = 1;
        assert_eq!(machine.check_invariants(), Ok(()));
    }

    #[test]
    fn i6_reports_a_clean_line_that_differs_from_memory() {
        let mut machine = start(Preset::TwoWayLru.config());
        let slot = line(&machine, 1, Dirtiness::Clean);
        put(&mut machine, 2, 1, slot, 0);
        machine.cache.way_mut(set(2), way(0)).unwrap().rank = 1;
        assert_eq!(machine.check_invariants(), Ok(()));
        let Slot::Valid { data, dirty, .. } =
            &mut machine.cache.way_mut(set(2), way(1)).unwrap().slot
        else {
            panic!("way 1 holds a line");
        };
        data[4] ^= 0xFF;
        assert_eq!(*dirty, Dirtiness::Clean);
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::CleanLineDiffersFromMemory {
                set: set(2),
                way: way(1)
            })
        );
        // A dirty line may differ: its words are newer than memory's.
        if let Slot::Valid { dirty, .. } = &mut machine.cache.way_mut(set(2), way(1)).unwrap().slot
        {
            *dirty = Dirtiness::Dirty;
        }
        assert_eq!(machine.check_invariants(), Ok(()));
    }

    #[test]
    fn i2_and_i6_are_not_checked_while_an_access_is_in_flight() {
        let mut machine = start(Preset::TwoWayLru.config());
        // Way 1 of set 2 valid and ranked behind invalid way 0 (I2), with a
        // clean line that differs from memory (I6).
        let slot = line(&machine, 1, Dirtiness::Clean);
        put(&mut machine, 2, 1, slot, 1);
        if let Slot::Valid { data, .. } = &mut machine.cache.way_mut(set(2), way(1)).unwrap().slot {
            data[0] ^= 0xFF;
        }
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::ValidRankedAfterInvalid { set: set(2) })
        );
        let geometry = machine.geometry;
        let addr = geometry.addr(0x0A4).unwrap();
        machine.access = Some(AccessContext {
            request: Request::Load {
                addr,
                into: crate::units::Register::R1,
                origin: crate::engine::Origin::Hand,
            },
            location: geometry.locate(addr),
            lookup: None,
            victim: None,
            way: None,
            stall: None,
            done: false,
        });
        assert!(machine.in_flight());
        assert_eq!(machine.check_invariants(), Ok(()));
        // Once the access is done, both are checked again.
        machine.access.as_mut().unwrap().done = true;
        assert!(!machine.in_flight());
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::ValidRankedAfterInvalid { set: set(2) })
        );
        // Ranked ahead of the invalid way, the line still differs.
        machine.cache.way_mut(set(2), way(1)).unwrap().rank = 0;
        machine.cache.way_mut(set(2), way(0)).unwrap().rank = 1;
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::CleanLineDiffersFromMemory {
                set: set(2),
                way: way(1)
            })
        );
    }

    #[test]
    fn i8_reports_a_line_memory_has_not_touched() {
        let mut machine = start(Config::default());
        // Zero memory, so the line's zero bytes match it (I6 holds), but the
        // line was never recorded as touched.
        machine.cache.way_mut(set(3), way(0)).unwrap().slot = line(&machine, 1, Dirtiness::Clean);
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::LineNotInMemory {
                set: set(3),
                way: way(0)
            })
        );
        let line = machine.geometry.line_addr(Tag::new_unchecked(1), set(3));
        machine.memory.touch(line);
        assert_eq!(machine.check_invariants(), Ok(()));
    }

    #[test]
    fn i7_reports_random_ranks_that_moved() {
        let mut machine = start(config(128, 2, Replacement::Random, WritePolicy::WriteBack));
        machine.cache.way_mut(set(3), way(0)).unwrap().rank = 1;
        machine.cache.way_mut(set(3), way(1)).unwrap().rank = 0;
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::RanksNotWayNumbers { set: set(3) })
        );
        // The same ranks are a valid LRU order.
        let mut machine = start(config(128, 2, Replacement::Lru, WritePolicy::WriteBack));
        machine.cache.way_mut(set(3), way(0)).unwrap().rank = 1;
        machine.cache.way_mut(set(3), way(1)).unwrap().rank = 0;
        assert_eq!(machine.check_invariants(), Ok(()));
    }

    #[test]
    fn the_lowest_set_and_invariant_is_reported_first() {
        let mut machine = start(config(128, 2, Replacement::Lru, WritePolicy::WriteThrough));
        let dirty = line(&machine, 1, Dirtiness::Dirty);
        put(&mut machine, 1, 0, dirty.clone(), 0);
        put(&mut machine, 1, 1, dirty, 1);
        machine.cache.way_mut(set(2), way(0)).unwrap().rank = 1;
        // Set 1 breaks I3 and I5, at ways 1 and 0; set 2 breaks I1.
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::DuplicateTag { set: set(1) })
        );
    }

    #[test]
    fn the_lowest_way_is_reported_first() {
        // Ways 1 and 3 of a full set break I4, and in a second cache I5;
        // both times way 1 is named.
        let mut machine = start(config(256, 4, Replacement::Lru, WritePolicy::WriteBack));
        let limit = 1_u32 << machine.geometry.tag_bits();
        for (index, tag) in (0..).zip([0, limit, 1, limit + 1]) {
            let slot = line(&machine, tag, Dirtiness::Clean);
            put(&mut machine, 2, index, slot, index);
        }
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::LineOutsideItsSet {
                set: set(2),
                way: way(1)
            })
        );
        let mut machine = start(config(256, 4, Replacement::Lru, WritePolicy::WriteThrough));
        let clean = Dirtiness::Clean;
        let dirty = Dirtiness::Dirty;
        for (index, dirtiness) in (0..).zip([clean, dirty, clean, dirty]) {
            let slot = line(&machine, index, dirtiness);
            put(&mut machine, 2, index, slot, index);
        }
        assert_eq!(
            machine.check_invariants(),
            Err(InvariantViolation::DirtyUnderWriteThrough {
                set: set(2),
                way: way(1)
            })
        );
    }

    #[test]
    fn messages_name_the_invariant_and_where_it_broke() {
        let cases = [
            (
                InvariantViolation::RanksNotAPermutation {
                    set: set(2),
                    ways: 4,
                },
                "invariant I1 (the ranks in set 2 are a permutation of 0 to 3)",
            ),
            (
                InvariantViolation::RanksNotAPermutation {
                    set: set(5),
                    ways: 1,
                },
                "invariant I1 (the only way in set 5 has rank 0)",
            ),
            (
                InvariantViolation::ValidRankedAfterInvalid { set: set(3) },
                "invariant I2 (every valid way in set 3 ranks below every invalid way)",
            ),
            (
                InvariantViolation::DuplicateTag { set: set(1) },
                "invariant I3 (no two valid ways in set 1 hold the same tag)",
            ),
            (
                InvariantViolation::LineOutsideItsSet {
                    set: set(2),
                    way: way(0),
                },
                "invariant I4 (the line in way 0 of set 2 decodes to that set and tag)",
            ),
            (
                InvariantViolation::DirtyUnderWriteThrough {
                    set: set(0),
                    way: way(1),
                },
                "invariant I5 (way 1 of set 0 is not dirty under write-through)",
            ),
            (
                InvariantViolation::CleanLineDiffersFromMemory {
                    set: set(2),
                    way: way(1),
                },
                "invariant I6 (the clean line in way 1 of set 2 matches memory)",
            ),
            (
                InvariantViolation::RanksNotWayNumbers { set: set(3) },
                "invariant I7 (the ranks in set 3 equal the way numbers)",
            ),
            (
                InvariantViolation::LineNotInMemory {
                    set: set(0),
                    way: way(3),
                },
                "invariant I8 (the line in way 3 of set 0 is among the lines memory has touched)",
            ),
        ];
        for (violation, message) in cases {
            assert_eq!(violation.to_string(), message);
        }
    }

    #[test]
    fn a_one_byte_word_cache_keeps_its_invariants() {
        // The narrowest words and a fully associative cache: every line is
        // one set, and the tag takes every bit above the offset.
        let mut machine = start(Config {
            address_bits: 8,
            word_bytes: WordBytes::One,
            ..config(64, 4, Replacement::Fifo, WritePolicy::WriteBack)
        });
        for addr in [0x00, 0x10, 0x20, 0x30, 0x40, 0x00, 0x50, 0xF0] {
            access(&mut machine, addr, true);
            assert_eq!(machine.check_invariants(), Ok(()));
        }
    }
}
