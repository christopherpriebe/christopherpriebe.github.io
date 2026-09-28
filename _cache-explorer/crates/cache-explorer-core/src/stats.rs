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

use crate::config::Timing;
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
    /// Advanced when `advanced` and in Basics otherwise. `highlight` is what
    /// the cursor's entry has changed so far; a row is changed when one of
    /// its counters is among it. The hit rate, which no event names, never
    /// is. The counters, the rows, the traffic, the hit rate and the
    /// sparkline are those where the machine stands; AMAT and the measured
    /// cost are those of the run's last completed access, so while an
    /// access is in flight they are the figures from before it began.
    pub(crate) fn new(machine: &Machine, advanced: bool, highlight: &[ChangeRef]) -> Self {
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
            rows: rows(&counters, advanced, timing.is_some(), highlight),
            hit_rate: counters.hit_rate(),
            amat: timing.and_then(|timing| completed.amat(timing)),
            measured: timing.and_then(|timing| completed.measured(timing)),
            recent,
            recent_text,
        }
    }
}

/// The rows of the Stats region: in Advanced when `advanced`, with the
/// cycles row when `timed`, and in Basics otherwise.
fn rows(counters: &Counters, advanced: bool, timed: bool, highlight: &[ChangeRef]) -> Vec<StatRow> {
    let mut keys = vec![
        StatKey::Accesses,
        StatKey::Hits,
        StatKey::Misses,
        StatKey::HitRate,
        StatKey::Evictions,
    ];
    if advanced {
        keys.extend([
            StatKey::Writebacks,
            StatKey::Writethroughs,
            StatKey::Writearounds,
        ]);
        if timed {
            keys.push(StatKey::Cycles);
        }
        keys.extend([StatKey::Compulsory, StatKey::Capacity, StatKey::Conflict]);
    } else {
        keys.push(StatKey::MemoryWrites);
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
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use serde_json::json;

    #[test]
    fn counters_start_at_zero_and_serialize_in_camel_case() {
        let counters = Counters::default();
        assert_eq!(
            serde_json::to_value(counters).unwrap(),
            json!({
                "accesses": 0, "loads": 0, "stores": 0, "hits": 0, "misses": 0,
                "evictions": 0, "writebacks": 0, "writethroughs": 0, "writearounds": 0,
                "fills": 0, "cycles": 0, "compulsory": 0, "capacity": 0, "conflict": 0
            })
        );
    }

    #[test]
    fn add_moves_only_the_named_counter() {
        let all = [
            Counter::Accesses,
            Counter::Loads,
            Counter::Stores,
            Counter::Hits,
            Counter::Misses,
            Counter::Evictions,
            Counter::Writebacks,
            Counter::Writethroughs,
            Counter::Writearounds,
            Counter::Fills,
            Counter::Cycles,
            Counter::Compulsory,
            Counter::Capacity,
            Counter::Conflict,
        ];
        for (index, counter) in all.into_iter().enumerate() {
            let mut counters = Counters::default();
            counters.add(counter, 3);
            let value = serde_json::to_value(counters).unwrap();
            let name = serde_json::to_value(counter).unwrap();
            let fields = value.as_object().unwrap();
            assert_eq!(fields.len(), all.len());
            for (field, count) in fields {
                let expected = if json!(field) == name { 3 } else { 0 };
                assert_eq!(
                    count,
                    &json!(expected),
                    "{counter:?} moved {field} ({index})"
                );
            }
        }
    }

    /// Hit time `hit`, miss penalty `miss`.
    fn timing(hit: u32, miss: u32) -> Timing {
        Timing {
            hit_cycles: hit,
            miss_cycles: miss,
        }
    }

    /// Counters after `accesses` accesses with `misses` misses and
    /// `writebacks` write-backs, which spent `cycles` cycles.
    fn run(accesses: u32, misses: u32, writebacks: u32, cycles: u64) -> Counters {
        Counters {
            accesses,
            hits: accesses - misses,
            misses,
            writebacks,
            cycles: Cycle::new_unchecked(cycles),
            ..Counters::default()
        }
    }

    fn text(figure: Option<Figure>) -> Option<String> {
        figure.map(|figure| figure.text)
    }

    #[test]
    fn amat_is_true_as_printed() {
        let one_twenty = timing(1, 20);
        let lines = [
            ((10, 4), "AMAT = 1 + 0.40 \u{d7} 20 = 9.0 cycles"),
            // The rate is printed to two places, and the result is computed
            // from the printed rate: 1 + 0.33 × 20 = 7.6, not 7.67.
            ((3, 1), "AMAT = 1 + 0.33 \u{d7} 20 = 7.6 cycles"),
            ((3, 2), "AMAT = 1 + 0.67 \u{d7} 20 = 14.4 cycles"),
            ((1, 0), "AMAT = 1 + 0.00 \u{d7} 20 = 1.0 cycles"),
            ((5, 5), "AMAT = 1 + 1.00 \u{d7} 20 = 21.0 cycles"),
            // 1 / 8 = 0.125 rounds half up to 0.13.
            ((8, 1), "AMAT = 1 + 0.13 \u{d7} 20 = 3.6 cycles"),
        ];
        for ((accesses, misses), line) in lines {
            let counters = run(accesses, misses, 0, 0);
            assert_eq!(
                text(counters.amat(one_twenty)).as_deref(),
                Some(line),
                "{misses} in {accesses}"
            );
        }
    }

    #[test]
    fn the_amat_result_rounds_half_up_to_one_place() {
        // 1 + 0.25 × 3 = 1.75, which rounds up to 1.8.
        assert_eq!(
            text(run(4, 1, 0, 0).amat(timing(1, 3))).as_deref(),
            Some("AMAT = 1 + 0.25 \u{d7} 3 = 1.8 cycles")
        );
        // 2 + 0.33 × 7 = 4.31, which rounds down to 4.3.
        assert_eq!(
            text(run(3, 1, 0, 0).amat(timing(2, 7))).as_deref(),
            Some("AMAT = 2 + 0.33 \u{d7} 7 = 4.3 cycles")
        );
        // Long costs are grouped like settings.
        assert_eq!(
            text(run(2, 1, 0, 0).amat(timing(3, 2_500))).as_deref(),
            Some("AMAT = 3 + 0.50 \u{d7} 2,500 = 1,253.0 cycles")
        );
    }

    #[test]
    fn the_amat_value_is_exact() {
        let figure = run(3, 1, 0, 0).amat(timing(1, 20)).unwrap();
        assert!((figure.value - (1.0 + 20.0 / 3.0)).abs() < 1e-12);
        let figure = run(10, 4, 0, 0).amat(timing(1, 20)).unwrap();
        assert!((figure.value - 9.0).abs() < 1e-12);
    }

    #[test]
    fn the_measured_cost_names_the_write_backs() {
        // 10 accesses, 4 misses and 1 write-back at 1/20 spend
        // 10 + 4 × 20 + 20 = 110 cycles: AMAT 9.0 plus 2.0.
        let counters = run(10, 4, 1, 110);
        assert_eq!(
            text(counters.amat(timing(1, 20))).as_deref(),
            Some("AMAT = 1 + 0.40 \u{d7} 20 = 9.0 cycles")
        );
        let measured = counters.measured(timing(1, 20)).unwrap();
        assert_eq!(
            measured.text,
            "Measured: 11.0 cycles per access (AMAT plus 2.0 for 1 write-back)."
        );
        assert!((measured.value - 11.0).abs() < 1e-12);
        assert_eq!(
            text(run(5, 4, 2, 125).measured(timing(1, 20))).as_deref(),
            Some("Measured: 25.0 cycles per access (AMAT plus 8.0 for 2 write-backs).")
        );
        assert_eq!(
            text(run(2, 1, 0, 22).measured(timing(1, 20))).as_deref(),
            Some("Measured: 11.0 cycles per access (AMAT plus 0.0 for 0 write-backs).")
        );
    }

    #[test]
    fn the_measured_numbers_round_half_up_on_their_own() {
        // 3 accesses, 1 miss and 1 write-back at 1/20 spend 3 + 20 + 20 =
        // 43 cycles: 14.33 rounds down to 14.3, and 20 / 3 = 6.67 up to 6.7.
        assert_eq!(
            text(run(3, 1, 1, 43).measured(timing(1, 20))).as_deref(),
            Some("Measured: 14.3 cycles per access (AMAT plus 6.7 for 1 write-back).")
        );
        // At 1/5: 4 + 3 × 5 + 2 × 5 = 29 cycles in 4 accesses, 7.25, rounds
        // up to 7.3; 4 + 5 + 5 = 14 leaves 5 / 4 = 1.25, which rounds up to
        // 1.3.
        assert_eq!(
            text(run(4, 3, 2, 29).measured(timing(1, 5))).as_deref(),
            Some("Measured: 7.3 cycles per access (AMAT plus 2.5 for 2 write-backs).")
        );
        assert_eq!(
            text(run(4, 1, 1, 14).measured(timing(1, 5))).as_deref(),
            Some("Measured: 3.5 cycles per access (AMAT plus 1.3 for 1 write-back).")
        );
        let measured = run(3, 1, 1, 43).measured(timing(1, 20)).unwrap();
        assert!((measured.value - 43.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn the_printed_rate_can_move_amat_by_half_a_hundredth_of_the_miss_penalty() {
        // 1 miss at 1/1,000 spends 1,000 cycles more than the accesses' hit
        // time. In 201 accesses, 1,201 cycles are 5.98 per access, but the
        // rate 0.004975 prints as 0.00; in 200, 1,200 cycles are 6.0 per
        // access, but the rate 0.005 prints as 0.01, and AMAT reads 5 cycles
        // above the exact 6.0. Each line is still true as printed.
        let slow = timing(1, 1_000);
        for (accesses, amat) in [
            (201, "AMAT = 1 + 0.00 \u{d7} 1,000 = 1.0 cycles"),
            (200, "AMAT = 1 + 0.01 \u{d7} 1,000 = 11.0 cycles"),
        ] {
            let counters = run(accesses, 1, 0, u64::from(accesses) + 1_000);
            assert_eq!(text(counters.amat(slow)).as_deref(), Some(amat));
            assert_eq!(
                text(counters.measured(slow)).as_deref(),
                Some("Measured: 6.0 cycles per access (AMAT plus 0.0 for 0 write-backs).")
            );
        }
    }

    #[test]
    fn neither_figure_exists_before_the_first_access() {
        let counters = Counters::default();
        assert_eq!(counters.amat(timing(1, 20)), None);
        assert_eq!(counters.measured(timing(1, 20)), None);
    }

    #[test]
    fn a_figure_serializes_its_value_and_text() {
        let figure = run(10, 4, 0, 0).amat(timing(1, 20)).unwrap();
        assert_eq!(
            serde_json::to_value(figure).unwrap(),
            json!({ "value": 9.0, "text": "AMAT = 1 + 0.40 \u{d7} 20 = 9.0 cycles" })
        );
    }

    #[test]
    fn counts_saturate() {
        let mut counters = Counters {
            hits: u32::MAX - 1,
            cycles: Cycle::new_unchecked(u64::MAX - 1),
            ..Counters::default()
        };
        counters.add(Counter::Hits, 5);
        counters.add(Counter::Cycles, 5);
        assert_eq!(counters.hits, u32::MAX);
        assert_eq!(counters.cycles.get(), u64::MAX);
    }

    /// Every counter, in the order the `Counter` enum declares them.
    const ALL: [Counter; 14] = [
        Counter::Accesses,
        Counter::Loads,
        Counter::Stores,
        Counter::Hits,
        Counter::Misses,
        Counter::Evictions,
        Counter::Writebacks,
        Counter::Writethroughs,
        Counter::Writearounds,
        Counter::Fills,
        Counter::Cycles,
        Counter::Compulsory,
        Counter::Capacity,
        Counter::Conflict,
    ];

    #[test]
    fn get_reads_back_what_add_moved() {
        for counter in ALL {
            let mut counters = Counters::default();
            counters.add(counter, 7);
            for other in ALL {
                let expected = if other == counter { 7 } else { 0 };
                assert_eq!(
                    counters.get(other),
                    expected,
                    "{counter:?} read as {other:?}"
                );
            }
        }
        let counters = Counters {
            cycles: Cycle::new_unchecked(u64::MAX),
            ..Counters::default()
        };
        assert_eq!(counters.get(Counter::Cycles), u64::MAX);
    }

    #[test]
    fn the_hit_rate_is_a_whole_percent_of_an_exact_share() {
        assert_eq!(Counters::default().hit_rate(), None);
        let rate = run(3, 1, 0, 0).hit_rate().unwrap();
        assert_eq!(rate.text, "67%");
        assert!((rate.value - 2.0 / 3.0).abs() < 1e-12);
        // 1 hit in 8 is 12.5 %, which rounds up.
        assert_eq!(text(run(8, 7, 0, 0).hit_rate()).as_deref(), Some("13%"));
        assert_eq!(text(run(4, 4, 0, 0).hit_rate()).as_deref(), Some("0%"));
        assert_eq!(text(run(4, 0, 0, 0).hit_rate()).as_deref(), Some("100%"));
    }

    /// The geometry of `Config::default()`: 16 B lines of 4-byte words.
    fn geometry() -> Geometry {
        crate::config::Config::default().validate().unwrap()
    }

    #[test]
    fn traffic_counts_lines_and_words_and_their_bytes() {
        let counters = Counters {
            fills: 3,
            writebacks: 2,
            writethroughs: 5,
            writearounds: 1,
            ..Counters::default()
        };
        let geometry = geometry();
        let (line, word) = (
            u64::from(geometry.line_bytes()),
            u64::from(geometry.word_bytes()),
        );
        assert_eq!(
            Traffic::new(&counters, &geometry),
            Traffic {
                line_reads: 3,
                line_writes: 2,
                word_writes: 6,
                bytes_read: 3 * line,
                bytes_written: 2 * line + 6 * word,
            }
        );
        assert_eq!(
            serde_json::to_value(Traffic::new(&Counters::default(), &geometry)).unwrap(),
            json!({
                "lineReads": 0, "lineWrites": 0, "wordWrites": 0, "bytesRead": 0,
                "bytesWritten": 0
            })
        );
    }

    #[test]
    fn a_shadow_keeps_its_lines_in_lru_order() {
        let line = |at: u32| LineAddr::new_unchecked(at);
        let mut shadow = Shadow::new(3);
        // Misses that do not allocate leave it empty.
        assert!(!shadow.access(line(0x00), false));
        assert!(shadow.lines().is_empty());
        for at in [0x00, 0x10, 0x20] {
            assert!(!shadow.access(line(at), true));
        }
        assert_eq!(shadow.lines(), [line(0x20), line(0x10), line(0x00)]);
        // A hit makes its line the most recent, whether or not a miss there
        // would allocate.
        assert!(shadow.access(line(0x00), false));
        assert_eq!(shadow.lines(), [line(0x00), line(0x20), line(0x10)]);
        // A fourth line pushes out the least recently used.
        assert!(!shadow.access(line(0x30), true));
        assert_eq!(shadow.lines(), [line(0x30), line(0x00), line(0x20)]);
        assert!(!shadow.access(line(0x10), true));
        assert_eq!(shadow.lines(), [line(0x10), line(0x30), line(0x00)]);
        // An edit's fill and removal: a filled line becomes the most recent,
        // and a forgotten one is gone, so the shadow misses on it.
        shadow.fill(line(0x00));
        assert_eq!(shadow.lines(), [line(0x00), line(0x10), line(0x30)]);
        shadow.forget(line(0x10));
        shadow.forget(line(0x70));
        assert_eq!(shadow.lines(), [line(0x00), line(0x30)]);
        assert!(!shadow.access(line(0x10), false));
    }

    #[test]
    fn a_miss_is_compulsory_until_its_line_is_filled_then_the_shadow_decides() {
        assert_eq!(miss_class(false, false), Counter::Compulsory);
        // A line the shadow holds was filled; if an edit removed it from
        // the filled lines, it was removed from the shadow too, so this
        // only arises by construction.
        assert_eq!(miss_class(false, true), Counter::Compulsory);
        assert_eq!(miss_class(true, false), Counter::Capacity);
        assert_eq!(miss_class(true, true), Counter::Conflict);
    }

    #[test]
    fn the_recent_outcomes_keep_the_latest() {
        let mut recent = Recent::default();
        assert_eq!(recent.outcomes().count(), 0);
        let limit = RECENT_OUTCOMES as usize;
        // One more miss than fits, then a hit: the first miss and the one
        // after it are dropped.
        for _ in 0..=limit {
            recent.push(Outcome::Miss);
        }
        recent.push(Outcome::Hit);
        let outcomes: Vec<Outcome> = recent.outcomes().collect();
        assert_eq!(outcomes.len(), limit);
        let (last, older) = outcomes.split_last().unwrap();
        assert_eq!(*last, Outcome::Hit);
        assert!(older.iter().all(|&o| o == Outcome::Miss));
    }

    /// A machine at the start of a run of `config`, with `outcomes` recorded
    /// as its accesses' outcomes and its counters moved to match.
    fn after(config: crate::config::Config, outcomes: &[Outcome]) -> Machine {
        let mut machine = Machine::start(config).unwrap();
        for &outcome in outcomes {
            machine.recent.push(outcome);
            machine.counters.add(Counter::Accesses, 1);
            machine.counters.add(Counter::Loads, 1);
            let counter = match outcome {
                Outcome::Hit => Counter::Hits,
                Outcome::Miss => Counter::Misses,
            };
            machine.counters.add(counter, 1);
        }
        machine
    }

    fn recent_text(outcomes: &[Outcome]) -> Option<String> {
        Stats::new(
            &after(crate::config::Config::default(), outcomes),
            false,
            &[],
        )
        .recent_text
    }

    #[test]
    fn the_sparkline_text_counts_its_outcomes_in_the_singular_and_plural() {
        use Outcome::{Hit, Miss};
        let stats = Stats::new(&after(crate::config::Config::default(), &[]), false, &[]);
        assert!(stats.recent.is_empty());
        assert_eq!(stats.recent_text, None);
        assert_eq!(recent_text(&[Miss]).as_deref(), Some("0 hits, 1 miss"));
        assert_eq!(recent_text(&[Hit]).as_deref(), Some("1 hit, 0 misses"));
        assert_eq!(
            recent_text(&[Miss, Hit, Miss]).as_deref(),
            Some("1 hit, 2 misses")
        );
        assert_eq!(
            recent_text(&[Miss, Miss, Hit, Hit]).as_deref(),
            Some("2 hits, 2 misses")
        );
    }

    #[test]
    fn the_sparkline_says_last_only_once_the_run_has_more_accesses_than_it_shows() {
        use Outcome::{Hit, Miss};
        let limit = RECENT_OUTCOMES as usize;
        // Exactly as many accesses as it shows: every one is counted, with
        // no suffix. One miss, the rest hits.
        let mut outcomes = vec![Miss];
        outcomes.resize(limit, Hit);
        // Saturating, so that a limit lowered to 0 still builds and fails
        // in limits.rs, which says why.
        let hit_count = RECENT_OUTCOMES.saturating_sub(1);
        let hits = format::data(hit_count);
        let stats = Stats::new(
            &after(crate::config::Config::default(), &outcomes),
            false,
            &[],
        );
        assert_eq!(stats.recent, outcomes);
        assert_eq!(
            stats.recent_text,
            Some(format!(
                "{hits} {}, 1 miss",
                format::plural(hit_count, "hit", "hits")
            ))
        );
        // One more, a miss: the first access drops out of the counts, which
        // are of the last RECENT_OUTCOMES, and the text says so.
        outcomes.push(Miss);
        let stats = Stats::new(
            &after(crate::config::Config::default(), &outcomes),
            false,
            &[],
        );
        assert_eq!(stats.recent, outcomes[1..]);
        assert_eq!(stats.counters.accesses, RECENT_OUTCOMES + 1);
        assert_eq!(
            stats.recent_text,
            Some(format!(
                "{hits} {}, 1 miss (last {})",
                format::plural(hit_count, "hit", "hits"),
                format::setting(RECENT_OUTCOMES)
            ))
        );
    }

    /// The keys of `rows`, in order.
    fn keys(rows: &[StatRow]) -> Vec<StatKey> {
        rows.iter().map(|row| row.key).collect()
    }

    #[test]
    fn basics_shows_the_writes_to_memory_and_advanced_each_kind_and_the_classes() {
        use StatKey::{
            Accesses, Capacity, Compulsory, Conflict, Cycles, Evictions, HitRate, Hits,
            MemoryWrites, Misses, Writearounds, Writebacks, Writethroughs,
        };
        let untimed = crate::config::Config::default();
        let machine = after(untimed.clone(), &[]);
        assert_eq!(
            keys(&Stats::new(&machine, false, &[]).rows),
            [Accesses, Hits, Misses, HitRate, Evictions, MemoryWrites]
        );
        assert_eq!(
            keys(&Stats::new(&machine, true, &[]).rows),
            [
                Accesses,
                Hits,
                Misses,
                HitRate,
                Evictions,
                Writebacks,
                Writethroughs,
                Writearounds,
                Compulsory,
                Capacity,
                Conflict
            ]
        );
        // Timed mode adds the cycles, in Advanced, where timing lives.
        let machine = after(
            crate::config::Config {
                timing: Some(timing(1, 20)),
                ..untimed
            },
            &[],
        );
        assert_eq!(
            keys(&Stats::new(&machine, true, &[]).rows),
            [
                Accesses,
                Hits,
                Misses,
                HitRate,
                Evictions,
                Writebacks,
                Writethroughs,
                Writearounds,
                Cycles,
                Compulsory,
                Capacity,
                Conflict
            ]
        );
    }

    #[test]
    fn rows_give_counts_as_data_and_the_hit_rate_as_a_percent() {
        let mut machine = after(crate::config::Config::default(), &[]);
        let texts = |stats: &Stats| -> Vec<(StatKey, String)> {
            stats
                .rows
                .iter()
                .map(|row| (row.key, row.text.clone()))
                .collect()
        };
        // Before the first access the hit rate has nothing to show.
        let stats = Stats::new(&machine, false, &[]);
        assert_eq!(
            stats.rows[3],
            StatRow {
                key: StatKey::HitRate,
                text: format::NO_FIGURE.to_owned(),
                changed: false
            }
        );
        machine.counters = Counters {
            accesses: 12_000,
            hits: 8_000,
            misses: 4_000,
            evictions: 3_990,
            writebacks: 7,
            writethroughs: 5,
            writearounds: 2,
            compulsory: 10,
            capacity: 3_000,
            conflict: 990,
            ..Counters::default()
        };
        assert_eq!(
            texts(&Stats::new(&machine, false, &[])),
            [
                (StatKey::Accesses, "12,000".to_owned()),
                (StatKey::Hits, "8000".to_owned()),
                (StatKey::Misses, "4000".to_owned()),
                (StatKey::HitRate, "67%".to_owned()),
                (StatKey::Evictions, "3990".to_owned()),
                // Write-backs, write-throughs and write-arounds together.
                (StatKey::MemoryWrites, "14".to_owned()),
            ]
        );
        let advanced = texts(&Stats::new(&machine, true, &[]));
        assert_eq!(
            advanced[5..],
            [
                (StatKey::Writebacks, "7".to_owned()),
                (StatKey::Writethroughs, "5".to_owned()),
                (StatKey::Writearounds, "2".to_owned()),
                (StatKey::Compulsory, "10".to_owned()),
                (StatKey::Capacity, "3000".to_owned()),
                (StatKey::Conflict, "990".to_owned()),
            ]
        );
    }

    #[test]
    fn a_row_is_changed_when_the_entry_moved_one_of_its_counters() {
        let config = crate::config::Config {
            timing: Some(timing(1, 20)),
            ..crate::config::Config::default()
        };
        let machine = after(config, &[Outcome::Miss]);
        let moved = |counters: &[Counter]| -> Vec<ChangeRef> {
            counters
                .iter()
                .map(|&counter| ChangeRef::Counter { counter })
                .collect()
        };
        let changed = |advanced: bool, highlight: &[ChangeRef]| -> Vec<StatKey> {
            Stats::new(&machine, advanced, highlight)
                .rows
                .iter()
                .filter(|row| row.changed)
                .map(|row| row.key)
                .collect()
        };
        // A miss that evicts a dirty line, as far as its write-back. The
        // hit rate, which no event names, is never changed; loads and fills
        // have no row.
        let highlight = moved(&[
            Counter::Accesses,
            Counter::Loads,
            Counter::Misses,
            Counter::Compulsory,
            Counter::Cycles,
            Counter::Writebacks,
        ]);
        assert_eq!(
            changed(true, &highlight),
            [
                StatKey::Accesses,
                StatKey::Misses,
                StatKey::Writebacks,
                StatKey::Cycles,
                StatKey::Compulsory
            ]
        );
        assert_eq!(
            changed(false, &highlight),
            [StatKey::Accesses, StatKey::Misses, StatKey::MemoryWrites]
        );
        // Any of the three writes changes the writes to memory; cells that
        // are not counters change no row.
        for counter in [
            Counter::Writebacks,
            Counter::Writethroughs,
            Counter::Writearounds,
        ] {
            let mut highlight = moved(&[counter]);
            highlight.push(ChangeRef::Pc);
            assert_eq!(changed(false, &highlight), [StatKey::MemoryWrites]);
        }
        assert!(changed(true, &[ChangeRef::Pc]).is_empty());
    }

    #[test]
    fn amat_and_the_measured_cost_are_there_in_timed_mode_only() {
        let untimed = crate::config::Config::default();
        let mut machine = after(untimed.clone(), &[Outcome::Miss, Outcome::Hit]);
        let stats = Stats::new(&machine, true, &[]);
        assert_eq!((stats.amat, stats.measured), (None, None));
        assert_eq!(stats.hit_rate.map(|rate| rate.text).as_deref(), Some("50%"));

        let config = crate::config::Config {
            timing: Some(timing(1, 20)),
            ..untimed
        };
        machine.config = config.clone();
        machine.counters.cycles = Cycle::new_unchecked(22);
        let stats = Stats::new(&machine, true, &[]);
        assert_eq!(stats.amat, machine.counters.amat(timing(1, 20)));
        assert_eq!(stats.measured, machine.counters.measured(timing(1, 20)));
        assert_eq!(
            text(stats.amat).as_deref(),
            Some("AMAT = 1 + 0.50 \u{d7} 20 = 11.0 cycles")
        );
        // Before the run's first access there is nothing to average.
        let stats = Stats::new(&after(config, &[]), true, &[]);
        assert_eq!(
            (stats.hit_rate, stats.amat, stats.measured),
            (None, None, None)
        );
    }

    #[test]
    fn stats_serialize_in_camel_case_without_absent_figures() {
        let machine = after(crate::config::Config::default(), &[]);
        let value = serde_json::to_value(Stats::new(&machine, false, &[])).unwrap();
        let fields: Vec<&String> = value.as_object().unwrap().keys().collect();
        assert_eq!(fields, ["counters", "recent", "rows", "traffic"]);
        assert_eq!(value["recent"], json!([]));
        assert_eq!(
            value["rows"][3],
            json!({ "key": "hitRate", "text": "\u{2014}", "changed": false })
        );

        let config = crate::config::Config {
            timing: Some(timing(1, 20)),
            ..crate::config::Config::default()
        };
        let machine = after(config, &[Outcome::Miss, Outcome::Hit]);
        let value = serde_json::to_value(Stats::new(&machine, true, &[])).unwrap();
        let fields: Vec<&String> = value.as_object().unwrap().keys().collect();
        assert_eq!(
            fields,
            [
                "amat",
                "counters",
                "hitRate",
                "measured",
                "recent",
                "recentText",
                "rows",
                "traffic"
            ]
        );
        assert_eq!(value["recent"], json!(["miss", "hit"]));
        assert_eq!(value["recentText"], json!("1 hit, 1 miss"));
        assert_eq!(value["hitRate"], json!({ "value": 0.5, "text": "50%" }));
    }

    #[test]
    fn keys_and_outcomes_serialize_by_camel_case_name() {
        let keys = [
            (StatKey::Accesses, "accesses"),
            (StatKey::Hits, "hits"),
            (StatKey::Misses, "misses"),
            (StatKey::HitRate, "hitRate"),
            (StatKey::Evictions, "evictions"),
            (StatKey::MemoryWrites, "memoryWrites"),
            (StatKey::Writebacks, "writebacks"),
            (StatKey::Writethroughs, "writethroughs"),
            (StatKey::Writearounds, "writearounds"),
            (StatKey::Cycles, "cycles"),
            (StatKey::Compulsory, "compulsory"),
            (StatKey::Capacity, "capacity"),
            (StatKey::Conflict, "conflict"),
        ];
        for (key, name) in keys {
            assert_eq!(serde_json::to_value(key).unwrap(), json!(name));
        }
        for (outcome, name) in [(Outcome::Hit, "hit"), (Outcome::Miss, "miss")] {
            assert_eq!(serde_json::to_value(outcome).unwrap(), json!(name));
            assert_eq!(
                serde_json::from_value::<Outcome>(json!(name)).unwrap(),
                outcome
            );
        }
        assert!(serde_json::from_value::<Outcome>(json!("Hit")).is_err());
    }
}
