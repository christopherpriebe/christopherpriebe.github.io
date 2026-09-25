//! The run's counters.
//!
//! Counters live in the machine, move at the events that name them in their
//! `changed` lists, and reset with each run.

use serde::Serialize;

use crate::event::Counter;
use crate::units::Cycle;

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
    /// Misses on a line never filled in this run.
    pub compulsory: u32,
    /// Misses a fully associative LRU cache of the same size would also
    /// have.
    pub capacity: u32,
    /// The remaining misses, caused by where lines may go.
    pub conflict: u32,
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
}
