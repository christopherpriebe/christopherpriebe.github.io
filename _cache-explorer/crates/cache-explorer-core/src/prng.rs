//! The run's pseudo-random generator, from which random replacement draws its
//! victims.
//!
//! One generator per run, seeded from the configuration's seed when the run
//! starts, whichever sets draw from it: the same seed and the same accesses
//! give the same victims on every machine, so a share link replays them.
//! The stream is xoshiro256++ as `rand_xoshiro` computes it, pinned to one
//! version because saved runs depend on it.

use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

/// The run's xoshiro256++ generator.
///
/// Deliberately not serializable: nothing outside the core sees its state,
/// and a run's draws are reproduced by replaying the run from its seed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prng(Xoshiro256PlusPlus);

impl Prng {
    /// The generator a run with `seed` starts from, seeded as
    /// `Xoshiro256PlusPlus::seed_from_u64` seeds it.
    pub(crate) fn new(seed: u64) -> Self {
        Self(Xoshiro256PlusPlus::seed_from_u64(seed))
    }

    /// Draws a way of a set with `ways` ways, a power of two: the top
    /// `log2(ways)` bits of the next output.
    ///
    /// Taking the high bits gives an unbiased draw without a range-sampling
    /// algorithm from another crate, so the victims depend only on the
    /// generator's stream. Every call consumes one output. A one-way set
    /// draws 0; random replacement never draws for one, since its only way
    /// is the victim.
    pub(crate) fn draw_way(&mut self, ways: u32) -> u32 {
        let bits = ways.checked_ilog2().unwrap_or(0);
        let draw = self.0.next_u64().checked_shr(64 - bits).unwrap_or(0);
        // At most 31 bits remain, since `ways` is a `u32`.
        u32::try_from(draw).unwrap_or(0)
    }

    /// The next raw output, for the tests that pin the stream.
    #[cfg(test)]
    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    #[test]
    fn seed_zero_gives_the_reference_stream() {
        let mut prng = Prng::new(0);
        let outputs: Vec<u64> = (0..4).map(|_| prng.next_u64()).collect();
        assert_eq!(
            outputs,
            [
                5_987_356_902_031_041_503,
                7_051_070_477_665_621_255,
                6_633_766_593_972_829_180,
                211_316_841_551_650_330,
            ]
        );
    }

    #[test]
    fn draw_way_takes_the_top_bits_of_each_output() {
        // The top bits of the four outputs above: 1, 1, 1, 0 for four ways and
        // 2, 3, 2, 0 for eight.
        let mut prng = Prng::new(0);
        let draws: Vec<u32> = (0..4).map(|_| prng.draw_way(2)).collect();
        assert_eq!(draws, [0, 0, 0, 0]);
        let mut prng = Prng::new(0);
        let draws: Vec<u32> = (0..4).map(|_| prng.draw_way(4)).collect();
        assert_eq!(draws, [1, 1, 1, 0]);
        let mut prng = Prng::new(0);
        let draws: Vec<u32> = (0..4).map(|_| prng.draw_way(8)).collect();
        assert_eq!(draws, [2, 3, 2, 0]);
    }

    #[test]
    fn draws_stay_below_the_way_count() {
        for ways in [2, 4, 8, 16, 32, 64, 1 << 31] {
            let mut prng = Prng::new(12_345);
            for _ in 0..200 {
                assert!(prng.draw_way(ways) < ways);
            }
        }
    }

    #[test]
    fn a_one_way_draw_is_0_and_still_consumes_an_output() {
        let mut prng = Prng::new(7);
        let mut reference = Prng::new(7);
        assert_eq!(prng.draw_way(1), 0);
        reference.next_u64();
        assert_eq!(prng, reference);
    }

    #[test]
    fn the_same_seed_gives_the_same_draws() {
        let mut first = Prng::new(12_345);
        let mut second = Prng::new(12_345);
        let a: Vec<u32> = (0..64).map(|_| first.draw_way(8)).collect();
        let b: Vec<u32> = (0..64).map(|_| second.draw_way(8)).collect();
        assert_eq!(a, b);
        let mut other = Prng::new(12_346);
        let c: Vec<u32> = (0..64).map(|_| other.draw_way(8)).collect();
        assert_ne!(a, c);
    }
}
