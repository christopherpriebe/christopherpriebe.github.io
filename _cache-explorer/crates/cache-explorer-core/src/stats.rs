//! The run's counters, and the figures derived from them.
//!
//! Counters live in the machine, move at the events that name them in their
//! `changed` lists, and reset with each run. In timed mode two figures are
//! derived from them: AMAT, which predicts the cost of an access from the
//! miss rate, and the measured cost, which is what the run actually spent
//! per access. They differ by the write-backs AMAT leaves out, and that
//! difference is itself a lesson.
//!
//! Each figure's text is computed in whole numbers of tenths and hundredths,
//! so no float is ever formatted; its value is an `f64` for the page's own
//! use.

use serde::Serialize;

use crate::config::Timing;
use crate::event::Counter;
use crate::explain;
use crate::format;
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

    /// The average memory access time `timing` predicts from the miss rate:
    /// `hit + (misses / accesses) × miss`. Its text is the design's, "AMAT =
    /// 1 + 0.40 × 20 = 9.0 cycles", and is true as printed: the miss rate is
    /// rounded half up to two decimals, and the result is computed from the
    /// printed rate and rounded half up to one, so 1 miss in 3 accesses reads
    /// "AMAT = 1 + 0.33 × 20 = 7.6 cycles" although the value is 7.67. `None`
    /// before the run's first access.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the Stats region shows AMAT in timed mode")
    )]
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
    /// so the two agree but for rounding; in the middle of an access the
    /// miss is counted before its penalty is charged. `None` before the
    /// run's first access.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the Stats region shows the measured cost in timed mode"
        )
    )]
    #[allow(
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
}
