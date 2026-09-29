//! The run's counters, the classes of its misses, and the Stats region
//! derived from them.
//!
//! Counters live in the machine, move at the events that name them in their
//! `changed` lists, and reset with each run. So does what the miss
//! classification remembers: every line the run has filled, and a shadow
//! cache. A miss on a line never filled is compulsory. Any other miss is a
//! capacity miss if a fully associative LRU cache with as many lines, seeing
//! the same accesses, would miss too, and a conflict miss if that cache
//! would hit: the line was lost to where lines may go, or to which line the
//! policy gave up, not to the cache's size. The shadow is that cache. It
//! sees every access, hit or miss, and on a miss of its own it allocates as
//! the run's cache would: always for a load, and for a store only under
//! write-allocate. So under write-no-allocate a store miss fills neither,
//! and when its line was never filled, the next load of that line is
//! compulsory too.
//!
//! [`Stats`] is what the Stats region shows at one point of a run: the
//! counters, the memory traffic they add up to, the rows of the mode on
//! screen with their texts, the hit rate, in timed mode AMAT and the
//! measured cost, and the outcomes of the latest accesses for the
//! sparkline. AMAT predicts the cost of an access from the miss rate; the
//! measured cost is what the run actually spent per access. They differ by
//! the write-backs AMAT leaves out, and that difference is itself a lesson.
//! Both are shown as of the run's last completed access. An access's miss
//! is counted at its tag compare, but its miss penalty only at the transfers
//! that follow, so figures taken mid-access would weigh a counted miss
//! against cycles not yet spent. The counters themselves move at every
//! event.
//!
//! Each figure's text is computed in whole numbers of tenths and hundredths,
//! so no float is ever formatted; its value is an `f64` for the page's own
//! use.

use std::collections::VecDeque;

use serde::Serialize;

use crate::config::{Mode, Timing};
use crate::de::deserialize_unit_enum;
use crate::event::{ChangeRef, Counter};
use crate::explain;
use crate::format;
use crate::geometry::Geometry;
use crate::limits::RECENT_OUTCOMES;
use crate::machine::Machine;
use crate::units::{Cycle, LineAddr};

/// What the run has done so far. Every counter starts at 0 with the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Counters {
    /// Loads and stores looked up in the cache.
    pub accesses: u32,
    /// Loads looked up.
    pub loads: u32,
    /// Stores looked up.
    pub stores: u32,
    /// Accesses that found their line. With `misses`, it makes up
    /// `accesses` at every event.
    pub hits: u32,
    /// Accesses that did not find their line.
    pub misses: u32,
    /// Lines that left the cache to make room for another.
    pub evictions: u32,
    /// Dirty lines written back to memory.
    pub writebacks: u32,
    /// Stores written through to memory under write-through.
    pub writethroughs: u32,
    /// Stores that missed under write-no-allocate and went straight to
    /// memory: write-arounds, under either write policy.
    pub writearounds: u32,
    /// Lines brought in from memory.
    pub fills: u32,
    /// Cycles spent, in timed mode; 0 otherwise.
    pub cycles: Cycle,
    /// Misses on a line never filled in this run. With `capacity` and
    /// `conflict`, it makes up `misses` at every event.
    pub compulsory: u32,
    /// The other misses that a fully associative LRU cache with as many
    /// lines would also have: the run uses more lines than the cache holds.
    pub capacity: u32,
    /// The other misses, which that cache would not have: caused by where
    /// lines may go, and by which line the policy gives up.
    pub conflict: u32,
}

/// A figure the Stats region shows: its exact value, and the text the
/// student reads, which is rounded.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Figure {
    /// The exact value, unrounded.
    pub value: f64,
    /// The text, as in "AMAT = 1 + 0.40 × 20 = 9.0 cycles".
    pub text: String,
}

/// The traffic between the cache and memory that the counters add up to:
/// the lines and words moved, and their bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Traffic {
    /// Lines read from memory: one per fill.
    pub line_reads: u32,
    /// Lines written to memory: one per write-back.
    pub line_writes: u32,
    /// Single words written to memory: one per write-through and one per
    /// write-around.
    pub word_writes: u32,
    /// Bytes read from memory: a line's bytes per line read.
    pub bytes_read: u64,
    /// Bytes written to memory: a line's bytes per line written, and a
    /// word's per word written.
    pub bytes_written: u64,
}

/// Which row of the Stats region a [`StatRow`] is. The page labels the
/// rows; which rows there are, their order and their texts are the core's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum StatKey {
    /// Loads and stores looked up.
    Accesses,
    /// Accesses that found their line.
    Hits,
    /// Accesses that did not.
    Misses,
    /// The share of accesses that hit.
    HitRate,
    /// Lines that left the cache to make room.
    Evictions,
    /// Basics only: every write to memory, a line written back, a word
    /// written through or a word written around the cache. Basics runs
    /// write-back and write-allocate, so only the first can happen there.
    MemoryWrites,
    /// Advanced only: dirty lines written back.
    Writebacks,
    /// Advanced only: stores written through.
    Writethroughs,
    /// Advanced only: store misses written around the cache.
    Writearounds,
    /// Advanced and timed only: the cycles spent.
    Cycles,
    /// Advanced only: misses on a line never filled in this run.
    Compulsory,
    /// Advanced only: misses a fully associative cache of as many lines
    /// would also have.
    Capacity,
    /// Advanced only: the other misses.
    Conflict,
}

/// One row of the Stats region: which it is, its value as text, and whether
/// to show it as changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct StatRow {
    /// Which row it is.
    pub key: StatKey,
    /// The value: a count such as "12" or "10,000", or the hit rate such as
    /// "67%", which is an em dash before the run's first access.
    pub text: String,
    /// True when a counter the row shows has moved since the cursor's entry
    /// began, so the row takes the changed ground as the cache's cells do.
    pub changed: bool,
}

/// Whether an access found its line: one mark of the hit and miss
/// sparkline. It also deserializes, because a module's `lastAccess`
/// predicate names one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// The access hit.
    Hit,
    /// The access missed.
    Miss,
}

deserialize_unit_enum!(Outcome {
    "hit" => Hit,
    "miss" => Miss,
});

/// What the Stats region shows at one point of a run.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// Every counter of the run.
    pub counters: Counters,
    /// The traffic to and from memory that the counters add up to.
    pub traffic: Traffic,
    /// The rows of the mode on screen, in order: accesses, hits, misses, the
    /// hit rate and evictions; then in Basics the writes to memory, and in
    /// Advanced the write-backs, write-throughs and write-arounds, the cycles
    /// in timed mode, and the compulsory, capacity and conflict misses.
    pub rows: Vec<StatRow>,
    /// The share of accesses that hit: its value from 0 to 1, and its text a
    /// whole percent, "67%". Absent before the run's first access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit_rate: Option<Figure>,
    /// In timed mode, the average memory access time the miss rate
    /// predicts: "AMAT = 1 + 0.40 × 20 = 9.0 cycles". As of the run's last
    /// completed access: while an access is in flight, it is the figure
    /// from before that access began. Absent untimed, and until the run's
    /// first access has completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amat: Option<Figure>,
    /// In timed mode, what the run actually spent per access: "Measured:
    /// 11.0 cycles per access (AMAT plus 2.0 for 1 write-back)." As of the
    /// run's last completed access, like `amat`. Absent untimed, and until
    /// the run's first access has completed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured: Option<Figure>,
    /// The sparkline: the outcomes of the run's latest accesses, oldest
    /// first, at most `RECENT_OUTCOMES` of them. An access counts from its
    /// tag compare on, as it does in `accesses`.
    pub recent: Vec<Outcome>,
    /// How many of those hit and missed: "1 hit, 2 misses", or, once the run
    /// has had more accesses than the sparkline shows, "15 hits, 9 misses
    /// (last 24)". Absent before the run's first access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recent_text: Option<String>,
}

impl Default for Counters {
    /// Every counter at 0, as a run starts.
    fn default() -> Self {
        Self {
            accesses: 0,
            loads: 0,
            stores: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
            writebacks: 0,
            writethroughs: 0,
            writearounds: 0,
            fills: 0,
            cycles: Cycle::new_unchecked(0),
            compulsory: 0,
            capacity: 0,
            conflict: 0,
        }
    }
}

impl Counters {
    /// Adds `amount` to `counter`. The counts saturate rather than wrap; the
    /// session's event limit keeps them far below that.
    pub(crate) fn add(&mut self, counter: Counter, amount: u32) {
        let count = match counter {
            Counter::Accesses => &mut self.accesses,
            Counter::Loads => &mut self.loads,
            Counter::Stores => &mut self.stores,
            Counter::Hits => &mut self.hits,
            Counter::Misses => &mut self.misses,
            Counter::Evictions => &mut self.evictions,
            Counter::Writebacks => &mut self.writebacks,
            Counter::Writethroughs => &mut self.writethroughs,
            Counter::Writearounds => &mut self.writearounds,
            Counter::Fills => &mut self.fills,
            Counter::Compulsory => &mut self.compulsory,
            Counter::Capacity => &mut self.capacity,
            Counter::Conflict => &mut self.conflict,
            Counter::Cycles => {
                let cycles = self.cycles.get().saturating_add(u64::from(amount));
                self.cycles = Cycle::new_unchecked(cycles);
                return;
            }
        };
        *count = count.saturating_add(amount);
    }

    /// The value of `counter`.
    pub(crate) fn get(&self, counter: Counter) -> u64 {
        let count = match counter {
            Counter::Accesses => self.accesses,
            Counter::Loads => self.loads,
            Counter::Stores => self.stores,
            Counter::Hits => self.hits,
            Counter::Misses => self.misses,
            Counter::Evictions => self.evictions,
            Counter::Writebacks => self.writebacks,
            Counter::Writethroughs => self.writethroughs,
            Counter::Writearounds => self.writearounds,
            Counter::Fills => self.fills,
            Counter::Compulsory => self.compulsory,
            Counter::Capacity => self.capacity,
            Counter::Conflict => self.conflict,
            Counter::Cycles => return self.cycles.get(),
        };
        u64::from(count)
    }

    /// The share of accesses that hit, `hits / accesses`: its value from 0
    /// to 1, and its text a whole percent rounded half up, so 2 hits in 3
    /// accesses read "67%". `None` before the run's first access.
    pub(crate) fn hit_rate(&self) -> Option<Figure> {
        let text = format::percent(u64::from(self.hits), u64::from(self.accesses))?;
        Some(Figure {
            value: f64::from(self.hits) / f64::from(self.accesses),
            text,
        })
    }

    /// The average memory access time `timing` predicts from the miss rate:
    /// `hit + (misses / accesses) × miss`. Its text is the design's, "AMAT =
    /// 1 + 0.40 × 20 = 9.0 cycles", and is true as printed: the miss rate is
    /// rounded half up to two decimals, and the result is computed from the
    /// printed rate and rounded half up to one, so 1 miss in 3 accesses reads
    /// "AMAT = 1 + 0.33 × 20 = 7.6 cycles" although the value is 7.67. `None`
    /// before the run's first access.
    pub(crate) fn amat(&self, timing: Timing) -> Option<Figure> {
        let (hit, miss) = (timing.hit_cycles, timing.miss_cycles);
        let rate = format::half_up(u64::from(self.misses), u64::from(self.accesses), 2)?;
        let hundredths = u64::from(hit)
            .checked_mul(100)?
            .checked_add(rate.checked_mul(u64::from(miss))?)?;
        let amat = format::half_up(hundredths, 100, 1)?;
        let value =
            f64::from(hit) + f64::from(self.misses) * f64::from(miss) / f64::from(self.accesses);
        Some(Figure {
            value,
            text: explain::amat(hit, rate, miss, amat),
        })
    }

    /// What the run actually spent per access, `cycles / accesses`, and how
    /// much of it the write-backs, which AMAT leaves out, account for:
    /// `writebacks × miss / accesses`. "Measured: 11.0 cycles per access
    /// (AMAT plus 2.0 for 1 write-back)." Each number is rounded half up to
    /// one decimal on its own. Between accesses the cycles are exactly the
    /// hit time per access and the miss penalty per miss and per write-back,
    /// so the measured cost is the exact AMAT plus the write-backs' part.
    /// The printed AMAT is not the exact one: it is computed from the miss
    /// rate rounded to two places, which moves it by up to half a hundredth
    /// of the miss penalty (5 cycles at 1,000 cycles), either way, so it can
    /// print above the measured cost, and a rate below 0.005 prints as 0.00
    /// though there are misses. In the middle of an access the miss is
    /// counted before its penalty is charged, so until the fill the measured
    /// cost can read far below AMAT; the Stats region therefore takes both
    /// from the counters of the last completed access. `None` before the
    /// run's first access.
    #[expect(
        clippy::cast_precision_loss,
        reason = "a run's cycle count stays far below 2^52, where an f64 is still exact"
    )]
    pub(crate) fn measured(&self, timing: Timing) -> Option<Figure> {
        let accesses = u64::from(self.accesses);
        let cycles = self.cycles.get();
        let measured = format::half_up(cycles, accesses, 1)?;
        let write_backs = u64::from(self.writebacks).checked_mul(u64::from(timing.miss_cycles))?;
        let plus = format::half_up(write_backs, accesses, 1)?;
        Some(Figure {
            value: cycles as f64 / f64::from(self.accesses),
            text: explain::measured(measured, plus, self.writebacks),
        })
    }
}

impl Traffic {
    /// The traffic `counters` add up to in a cache of `geometry`'s line and
    /// word sizes. The byte counts saturate rather than wrap; the session's
    /// event limit keeps them far below that.
    pub(crate) fn new(counters: &Counters, geometry: &Geometry) -> Self {
        let line_bytes = u64::from(geometry.line_bytes());
        let word_bytes = u64::from(geometry.word_bytes());
        let word_writes = counters.writethroughs.saturating_add(counters.writearounds);
        Self {
            line_reads: counters.fills,
            line_writes: counters.writebacks,
            word_writes,
            bytes_read: u64::from(counters.fills).saturating_mul(line_bytes),
            bytes_written: u64::from(counters.writebacks)
                .saturating_mul(line_bytes)
                .saturating_add(u64::from(word_writes).saturating_mul(word_bytes)),
        }
    }
}

impl StatKey {
    /// The counters the row adds up: one for most rows, the three kinds of
    /// write to memory for `MemoryWrites`, and none for the hit rate, which
    /// is a ratio that no event names.
    fn counters(self) -> &'static [Counter] {
        match self {
            StatKey::Accesses => &[Counter::Accesses],
            StatKey::Hits => &[Counter::Hits],
            StatKey::Misses => &[Counter::Misses],
            StatKey::HitRate => &[],
            StatKey::Evictions => &[Counter::Evictions],
            StatKey::MemoryWrites => &[
                Counter::Writebacks,
                Counter::Writethroughs,
                Counter::Writearounds,
            ],
            StatKey::Writebacks => &[Counter::Writebacks],
            StatKey::Writethroughs => &[Counter::Writethroughs],
            StatKey::Writearounds => &[Counter::Writearounds],
            StatKey::Cycles => &[Counter::Cycles],
            StatKey::Compulsory => &[Counter::Compulsory],
            StatKey::Capacity => &[Counter::Capacity],
            StatKey::Conflict => &[Counter::Conflict],
        }
    }

    /// This row as `counters` give it, changed when one of its counters is
    /// among the cells in `highlight`.
    fn row(self, counters: &Counters, highlight: &[ChangeRef]) -> StatRow {
        let text = match self {
            StatKey::HitRate => counters
                .hit_rate()
                .map_or_else(|| format::NO_FIGURE.to_owned(), |rate| rate.text),
            _ => format::data(self.counters().iter().fold(0_u64, |sum, &counter| {
                sum.saturating_add(counters.get(counter))
            })),
        };
        let changed = self
            .counters()
            .iter()
            .any(|&counter| highlight.contains(&ChangeRef::Counter { counter }));
        StatRow {
            key: self,
            text,
            changed,
        }
    }
}

impl Stats {
    /// The Stats region of `machine`'s run where the machine stands, in
    /// `mode`. `highlight` is what the cursor's entry has changed so far; a
    /// row is changed when one of its counters is among it. The hit rate,
    /// which no event names, never is. The counters, the rows, the traffic,
    /// the hit rate and the sparkline are those where the machine stands;
    /// AMAT and the measured cost are those of the run's last completed
    /// access, so while an access is in flight they are the figures from
    /// before it began.
    pub(crate) fn new(machine: &Machine, mode: Mode, highlight: &[ChangeRef]) -> Self {
        let counters = machine.counters;
        let completed = match &machine.access {
            Some(access) if !access.done => access.counters_before,
            _ => counters,
        };
        let timing = machine.config.timing;
        let recent: Vec<Outcome> = machine.recent.outcomes().collect();
        let recent_text = (counters.accesses > 0).then(|| {
            let shown = (counters.accesses > RECENT_OUTCOMES).then_some(RECENT_OUTCOMES);
            explain::recent(
                count(&recent, Outcome::Hit),
                count(&recent, Outcome::Miss),
                shown,
            )
        });
        Self {
            counters,
            traffic: Traffic::new(&counters, &machine.geometry),
            rows: rows(&counters, mode, timing.is_some(), highlight),
            hit_rate: counters.hit_rate(),
            amat: timing.and_then(|timing| completed.amat(timing)),
            measured: timing.and_then(|timing| completed.measured(timing)),
            recent,
            recent_text,
        }
    }
}

/// The rows of the Stats region in `mode`, with the cycles row in Advanced
/// when `timed`.
fn rows(counters: &Counters, mode: Mode, timed: bool, highlight: &[ChangeRef]) -> Vec<StatRow> {
    let mut keys = vec![
        StatKey::Accesses,
        StatKey::Hits,
        StatKey::Misses,
        StatKey::HitRate,
        StatKey::Evictions,
    ];
    match mode {
        Mode::Advanced => {
            keys.extend([
                StatKey::Writebacks,
                StatKey::Writethroughs,
                StatKey::Writearounds,
            ]);
            if timed {
                keys.push(StatKey::Cycles);
            }
            keys.extend([StatKey::Compulsory, StatKey::Capacity, StatKey::Conflict]);
        }
        Mode::Basics => keys.push(StatKey::MemoryWrites),
    }
    keys.into_iter()
        .map(|key| key.row(counters, highlight))
        .collect()
}

/// How many of `outcomes` are `outcome`.
fn count(outcomes: &[Outcome], outcome: Outcome) -> u32 {
    outcomes.iter().fold(0, |count, &each| {
        count.saturating_add(u32::from(each == outcome))
    })
}

/// The outcomes of the run's latest accesses, oldest first: at most
/// `RECENT_OUTCOMES` of them. The engine adds each access's outcome at its
/// tag compare.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Recent {
    /// The outcomes, oldest first.
    outcomes: VecDeque<Outcome>,
}

impl Recent {
    /// Adds the outcome of the run's latest access, and drops the oldest
    /// once there are more than `RECENT_OUTCOMES`.
    pub(crate) fn push(&mut self, outcome: Outcome) {
        self.outcomes.push_back(outcome);
        let most = usize::try_from(RECENT_OUTCOMES).unwrap_or(usize::MAX);
        while self.outcomes.len() > most {
            self.outcomes.pop_front();
        }
    }

    /// The outcomes, oldest first.
    pub(crate) fn outcomes(&self) -> impl Iterator<Item = Outcome> + '_ {
        self.outcomes.iter().copied()
    }
}

/// The shadow cache that tells capacity misses from conflict misses: a fully
/// associative LRU cache with as many lines as the run's cache, which sees
/// every access the run makes. It has at most `MAX_LINES` lines, so a list
/// in recency order is all it needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shadow {
    /// The lines it holds, most recently used first.
    lines: Vec<LineAddr>,
    /// The most lines it holds: the run's cache's line count.
    capacity: u32,
}

impl Shadow {
    /// An empty shadow of `capacity` lines, as a run starts.
    pub(crate) fn new(capacity: u32) -> Self {
        Self {
            lines: Vec::new(),
            capacity,
        }
    }

    /// An access to `line`. Returns whether the shadow held it, which makes
    /// it the most recently used line. A line it did not hold comes in if
    /// `allocate`, as a fill would bring it.
    pub(crate) fn access(&mut self, line: LineAddr, allocate: bool) -> bool {
        let held = self.lines.contains(&line);
        if held || allocate {
            self.fill(line);
        }
        held
    }

    /// Makes `line` the most recently used, bringing it in if the shadow
    /// does not hold it; the least recently used line leaves when that makes
    /// one too many.
    pub(crate) fn fill(&mut self, line: LineAddr) {
        self.forget(line);
        self.lines.insert(0, line);
        self.lines
            .truncate(usize::try_from(self.capacity).unwrap_or(usize::MAX));
    }

    /// Drops `line`, if the shadow holds it. An edit that takes a line out
    /// of the cache without an access drops it here too, so that the next
    /// miss on it is compulsory rather than a conflict the cache did not
    /// cause.
    pub(crate) fn forget(&mut self, line: LineAddr) {
        self.lines.retain(|held| *held != line);
    }

    /// The lines it holds, most recently used first.
    #[cfg(test)]
    pub(crate) fn lines(&self) -> &[LineAddr] {
        &self.lines
    }
}

/// The class of a miss, as the counter it moves: compulsory when its line
/// has never been `filled` in this run, and otherwise a conflict when the
/// shadow held the line, `in_shadow`, and a capacity miss when it did not.
pub(crate) fn miss_class(filled: bool, in_shadow: bool) -> Counter {
    match (filled, in_shadow) {
        (false, _) => Counter::Compulsory,
        (true, true) => Counter::Conflict,
        (true, false) => Counter::Capacity,
    }
}

#[cfg(test)]
mod tests;
