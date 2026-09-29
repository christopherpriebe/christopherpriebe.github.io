//! Replacement: which way a miss fills, and how hits and fills reorder a set.
//!
//! The policies are stateless. LRU and FIFO keep their order in the ways'
//! ranks, and random replacement draws from the run's generator, so a set has
//! no replacement state of its own. Every policy starts from ranks equal to
//! the way numbers:
//!
//! | Policy | Hit | Fill | Victim of a full set |
//! |---|---|---|---|
//! | LRU | to the front | to the front | the way ranked last (least recently used) |
//! | FIFO | nothing | to the front | the way ranked last (filled earliest) |
//! | Random | nothing | nothing | a way drawn from the run's generator |
//!
//! Before any of that, a miss takes the lowest-numbered invalid way, and in a
//! one-way set the only way. Under LRU and FIFO a fill moves its way ahead of
//! every invalid way, so valid ways always rank below invalid ones and an
//! invalid way shows a high rank. Random replacement keeps the ranks equal to
//! the way numbers, and so does every one-way set, whose only rank is 0.
//!
//! Crate-private, like `Way`: outside the crate, rank changes are visible only
//! as `Rank` events and in the state views.

use crate::cache::Way;
use crate::config::Replacement;
use crate::prng::Prng;
use crate::units::WayIndex;

/// Replacement behaviour over the ways of one set, in way order.
pub(crate) trait ReplacementPolicy {
    /// A hit on `way`. Returns the rank moves made, in way order; empty if
    /// none.
    fn on_access(&self, set: &mut [Way], way: WayIndex) -> Vec<RankMove>;

    /// `way` has just been filled. Returns the rank moves made, in way order;
    /// empty if none.
    fn on_fill(&self, set: &mut [Way], way: WayIndex) -> Vec<RankMove>;

    /// The victim for a miss: the lowest-numbered invalid way if there is one,
    /// way 0 if the set has one way, otherwise [`choose_full`]. Only random
    /// replacement of a full set draws from `rng`.
    ///
    /// A set with no ways, which no valid geometry has, also gets way 0; the
    /// caller's lookup of it then fails and is reported.
    ///
    /// [`choose_full`]: ReplacementPolicy::choose_full
    fn victim(&self, set: &[Way], rng: &mut Prng) -> VictimChoice {
        if let Some(way) = (0_u32..)
            .zip(set)
            .find_map(|(index, way)| (!way.is_valid()).then_some(index))
        {
            return VictimChoice {
                way: WayIndex::new_unchecked(way),
                reason: VictimReason::Empty,
            };
        }
        if set.len() <= 1 {
            return VictimChoice {
                way: WayIndex::new_unchecked(0),
                reason: VictimReason::OnlyWay,
            };
        }
        self.choose_full(set, rng)
    }

    /// The victim of a set whose ways are all valid, of which there is more
    /// than one. Only [`RandomChoice`] touches `rng`.
    fn choose_full(&self, set: &[Way], rng: &mut Prng) -> VictimChoice;
}

/// Least recently used: hits and fills move a way to the front, and a full
/// set evicts the way ranked last.
pub(crate) struct Lru;

/// First in, first out: fills move a way to the front, hits change nothing,
/// and a full set evicts the way filled earliest, ranked last.
pub(crate) struct Fifo;

/// Random: ranks never change, and a full set evicts a way drawn from the
/// run's generator.
pub(crate) struct RandomChoice;

/// One way's rank before and after a hit or a fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RankMove {
    /// The way whose rank changed.
    pub(crate) way: WayIndex,
    /// Its rank before.
    pub(crate) from: u32,
    /// Its rank after.
    pub(crate) to: u32,
}

/// The way a miss fills, and why that one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VictimChoice {
    /// The way chosen.
    pub(crate) way: WayIndex,
    /// Why it was chosen.
    pub(crate) reason: VictimReason,
}

/// Why a way was chosen for a miss.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VictimReason {
    /// It was the lowest-numbered invalid way.
    Empty,
    /// It is the set's only way, and it was valid.
    OnlyWay,
    /// Under LRU, it was the least recently used way, ranked `rank`.
    LeastRecentlyUsed {
        /// The way's rank, the set's last.
        rank: u32,
    },
    /// Under FIFO, it was the way filled earliest, ranked `rank`.
    FirstIn {
        /// The way's rank, the set's last.
        rank: u32,
    },
    /// Under random replacement, it was drawn from the run's generator.
    Random,
}

impl Replacement {
    /// The behaviour of this policy.
    pub(crate) fn policy(self) -> &'static dyn ReplacementPolicy {
        match self {
            Replacement::Lru => &Lru,
            Replacement::Fifo => &Fifo,
            Replacement::Random => &RandomChoice,
        }
    }
}

impl ReplacementPolicy for Lru {
    fn on_access(&self, set: &mut [Way], way: WayIndex) -> Vec<RankMove> {
        move_to_front(set, way)
    }

    fn on_fill(&self, set: &mut [Way], way: WayIndex) -> Vec<RankMove> {
        move_to_front(set, way)
    }

    fn choose_full(&self, set: &[Way], _rng: &mut Prng) -> VictimChoice {
        let (way, rank) = ranked_last(set);
        VictimChoice {
            way,
            reason: VictimReason::LeastRecentlyUsed { rank },
        }
    }
}

impl ReplacementPolicy for Fifo {
    fn on_access(&self, _set: &mut [Way], _way: WayIndex) -> Vec<RankMove> {
        Vec::new()
    }

    fn on_fill(&self, set: &mut [Way], way: WayIndex) -> Vec<RankMove> {
        move_to_front(set, way)
    }

    fn choose_full(&self, set: &[Way], _rng: &mut Prng) -> VictimChoice {
        let (way, rank) = ranked_last(set);
        VictimChoice {
            way,
            reason: VictimReason::FirstIn { rank },
        }
    }
}

impl ReplacementPolicy for RandomChoice {
    fn on_access(&self, _set: &mut [Way], _way: WayIndex) -> Vec<RankMove> {
        Vec::new()
    }

    fn on_fill(&self, _set: &mut [Way], _way: WayIndex) -> Vec<RankMove> {
        Vec::new()
    }

    fn choose_full(&self, set: &[Way], rng: &mut Prng) -> VictimChoice {
        // A set has at most `MAX_LINES` ways, so its length fits.
        let ways = u32::try_from(set.len()).unwrap_or(u32::MAX);
        VictimChoice {
            way: WayIndex::new_unchecked(rng.draw_way(ways)),
            reason: VictimReason::Random,
        }
    }
}

/// Moves `way` to rank 0: every way ranked ahead of it moves back one place,
/// and the ways behind it keep their ranks. Nothing moves when `way` is
/// already at rank 0, or when the set has no such way. Returns the moves, in
/// way order.
fn move_to_front(set: &mut [Way], way: WayIndex) -> Vec<RankMove> {
    let Some(target) = usize::try_from(way.get())
        .ok()
        .and_then(|index| set.get(index))
        .map(|way| way.rank)
    else {
        return Vec::new();
    };
    if target == 0 {
        return Vec::new();
    }
    let mut moves = Vec::new();
    for (index, other) in (0_u32..).zip(set.iter_mut()) {
        let from = other.rank;
        let to = if index == way.get() {
            0
        } else if from < target {
            // Below `target`, so one more still fits.
            from + 1
        } else {
            continue;
        };
        other.rank = to;
        moves.push(RankMove {
            way: WayIndex::new_unchecked(index),
            from,
            to,
        });
    }
    moves
}

/// The way ranked last in `set`, with its rank: the highest rank, which is
/// `ways − 1` while the ranks are a permutation. Of equal ranks, which only a
/// broken set could hold, the lowest-numbered way; way 0 for a set with no
/// ways.
fn ranked_last(set: &[Way]) -> (WayIndex, u32) {
    let mut last = (0, 0);
    for (index, way) in (0_u32..).zip(set) {
        if index == 0 || way.rank > last.1 {
            last = (index, way.rank);
        }
    }
    (WayIndex::new_unchecked(last.0), last.1)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::cache::{Dirtiness, Slot};
    use crate::limits::MAX_LINES;
    use crate::units::Tag;

    /// A set of `ways` invalid ways, ranked by way number, as a run starts.
    fn empty_set(ways: u32) -> Vec<Way> {
        (0..ways)
            .map(|rank| Way {
                slot: Slot::Invalid,
                rank,
            })
            .collect()
    }

    fn valid(tag: u32) -> Slot {
        Slot::Valid {
            tag: Tag::new_unchecked(tag),
            dirty: Dirtiness::Clean,
            data: vec![0; 4],
        }
    }

    fn ranks(set: &[Way]) -> Vec<u32> {
        set.iter().map(|way| way.rank).collect()
    }

    fn way(index: u32) -> WayIndex {
        WayIndex::new_unchecked(index)
    }

    fn rank_move(index: u32, from: u32, to: u32) -> RankMove {
        RankMove {
            way: way(index),
            from,
            to,
        }
    }

    /// What one access did to its set.
    #[derive(Debug, PartialEq, Eq)]
    enum Outcome {
        Hit(u32),
        Miss(VictimChoice),
    }

    /// One access to `tag` on `set`, as the engine will make it: a hit is
    /// ranked, and a miss fills its victim and ranks the fill.
    fn access(
        policy: &dyn ReplacementPolicy,
        set: &mut [Way],
        rng: &mut Prng,
        tag: u32,
    ) -> Outcome {
        access_ranked(policy, set, rng, tag).0
    }

    /// As [`access`], also returning the rank moves the hit or the fill
    /// reported.
    fn access_ranked(
        policy: &dyn ReplacementPolicy,
        set: &mut [Way],
        rng: &mut Prng,
        tag: u32,
    ) -> (Outcome, Vec<RankMove>) {
        let held = Some(Tag::new_unchecked(tag));
        let hit = (0_u32..)
            .zip(set.iter())
            .find_map(|(index, way)| (way.tag() == held).then_some(index));
        if let Some(index) = hit {
            let moves = policy.on_access(set, way(index));
            return (Outcome::Hit(index), moves);
        }
        let choice = policy.victim(set, rng);
        set[choice.way.get() as usize].slot = valid(tag);
        let moves = policy.on_fill(set, choice.way);
        (Outcome::Miss(choice), moves)
    }

    fn run(policy: Replacement, ways: u32, seed: u64, tags: &[u32]) -> Vec<Outcome> {
        let mut set = empty_set(ways);
        let mut rng = Prng::new(seed);
        tags.iter()
            .map(|&tag| access(policy.policy(), &mut set, &mut rng, tag))
            .collect()
    }

    fn chosen(index: u32, reason: VictimReason) -> VictimChoice {
        VictimChoice {
            way: way(index),
            reason,
        }
    }

    /// A miss that filled way `index`, which was invalid.
    fn empty(index: u32) -> Outcome {
        Outcome::Miss(chosen(index, VictimReason::Empty))
    }

    /// A miss that evicted way `index` for `reason`.
    fn evicts(index: u32, reason: VictimReason) -> Outcome {
        Outcome::Miss(chosen(index, reason))
    }

    /// Asserts the ranks of `set` are a permutation of `0..ways`.
    fn assert_permutation(set: &[Way]) {
        let mut sorted = ranks(set);
        sorted.sort_unstable();
        assert!(
            sorted
                .iter()
                .copied()
                .eq(0..u32::try_from(set.len()).unwrap()),
            "ranks {:?} are not a permutation",
            ranks(set)
        );
    }

    /// Asserts every valid way of `set` ranks below every invalid way.
    fn assert_valid_below_invalid(set: &[Way]) {
        let valid_max = set.iter().filter(|w| w.is_valid()).map(|w| w.rank).max();
        let invalid_min = set.iter().filter(|w| !w.is_valid()).map(|w| w.rank).min();
        if let (Some(valid_max), Some(invalid_min)) = (valid_max, invalid_min) {
            assert!(valid_max < invalid_min, "ranks {:?}", ranks(set));
        }
    }

    #[test]
    fn move_to_front_moves_the_ways_ahead_of_it_back_one_place() {
        let mut set = empty_set(4);
        for (index, rank) in [3, 1, 0, 2].into_iter().enumerate() {
            set[index].rank = rank;
        }
        let moves = move_to_front(&mut set, way(3));
        assert_eq!(ranks(&set), [3, 2, 1, 0]);
        assert_eq!(
            moves,
            [rank_move(1, 1, 2), rank_move(2, 0, 1), rank_move(3, 2, 0)]
        );
    }

    #[test]
    fn move_to_front_moves_nothing_for_the_front_way_or_a_missing_one() {
        let mut set = empty_set(4);
        assert!(move_to_front(&mut set, way(0)).is_empty());
        assert_eq!(ranks(&set), [0, 1, 2, 3]);
        assert!(move_to_front(&mut set, way(4)).is_empty());
        assert_eq!(ranks(&set), [0, 1, 2, 3]);
    }

    #[test]
    fn an_lru_hit_moves_its_way_to_the_front() {
        let mut set = empty_set(4);
        let moves = Lru.on_access(&mut set, way(2));
        assert_eq!(ranks(&set), [1, 2, 0, 3]);
        assert_eq!(
            moves,
            [rank_move(0, 0, 1), rank_move(1, 1, 2), rank_move(2, 2, 0)]
        );
    }

    #[test]
    fn a_fifo_hit_moves_nothing_and_a_fifo_fill_moves_to_the_front() {
        let mut set = empty_set(4);
        assert!(Fifo.on_access(&mut set, way(3)).is_empty());
        assert_eq!(ranks(&set), [0, 1, 2, 3]);
        let moves = Fifo.on_fill(&mut set, way(1));
        assert_eq!(ranks(&set), [1, 0, 2, 3]);
        assert_eq!(moves, [rank_move(0, 0, 1), rank_move(1, 1, 0)]);
    }

    #[test]
    fn random_replacement_never_moves_a_rank() {
        let mut set = empty_set(4);
        assert!(RandomChoice.on_access(&mut set, way(3)).is_empty());
        assert!(RandomChoice.on_fill(&mut set, way(2)).is_empty());
        assert_eq!(ranks(&set), [0, 1, 2, 3]);
    }

    #[test]
    fn a_one_way_set_never_moves_its_rank() {
        for policy in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
            let mut set = empty_set(1);
            assert!(policy.policy().on_fill(&mut set, way(0)).is_empty());
            assert!(policy.policy().on_access(&mut set, way(0)).is_empty());
            assert_eq!(ranks(&set), [0]);
        }
    }

    #[test]
    fn a_miss_takes_the_lowest_numbered_invalid_way_first() {
        for policy in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
            let mut set = empty_set(4);
            set[0].slot = valid(1);
            set[2].slot = valid(2);
            let mut rng = Prng::new(0);
            let choice = policy.policy().victim(&set, &mut rng);
            assert_eq!(choice, chosen(1, VictimReason::Empty));
            set[1].slot = valid(3);
            let choice = policy.policy().victim(&set, &mut rng);
            assert_eq!(choice, chosen(3, VictimReason::Empty));
            assert_eq!(rng, Prng::new(0));
        }
    }

    #[test]
    fn a_one_way_set_evicts_its_only_way() {
        for policy in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
            let mut set = empty_set(1);
            let mut rng = Prng::new(0);
            let choice = policy.policy().victim(&set, &mut rng);
            assert_eq!(choice, chosen(0, VictimReason::Empty));
            set[0].slot = valid(1);
            let choice = policy.policy().victim(&set, &mut rng);
            assert_eq!(choice, chosen(0, VictimReason::OnlyWay));
            assert_eq!(rng, Prng::new(0));
        }
    }

    #[test]
    fn a_set_with_no_ways_gets_way_0() {
        let mut rng = Prng::new(0);
        for policy in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
            let choice = policy.policy().victim(&[], &mut rng);
            assert_eq!(choice, chosen(0, VictimReason::OnlyWay));
        }
    }

    #[test]
    fn lru_evicts_the_least_recently_used_way() {
        // A B C D fill the four ways; the hit on A makes B the least recently
        // used, so E replaces B, F replaces C, and after a second hit on A, G
        // replaces D.
        let lru = |rank| VictimReason::LeastRecentlyUsed { rank };
        assert_eq!(
            run(
                Replacement::Lru,
                4,
                0,
                &[10, 11, 12, 13, 10, 14, 15, 10, 16]
            ),
            [
                empty(0),
                empty(1),
                empty(2),
                empty(3),
                Outcome::Hit(0),
                evicts(1, lru(3)),
                evicts(2, lru(3)),
                Outcome::Hit(0),
                evicts(3, lru(3)),
            ]
        );
    }

    #[test]
    fn fifo_evicts_the_way_filled_earliest_whatever_hits_it() {
        // The same accesses as under LRU: the hit on A does not save it, so E
        // replaces A, F replaces B, A misses and replaces C, and G replaces D.
        let fifo = |rank| VictimReason::FirstIn { rank };
        assert_eq!(
            run(
                Replacement::Fifo,
                4,
                0,
                &[10, 11, 12, 13, 10, 14, 15, 10, 16]
            ),
            [
                empty(0),
                empty(1),
                empty(2),
                empty(3),
                Outcome::Hit(0),
                evicts(0, fifo(3)),
                evicts(1, fifo(3)),
                evicts(2, fifo(3)),
                evicts(3, fifo(3)),
            ]
        );
    }

    #[test]
    fn lru_and_fifo_ranks_after_a_sequence() {
        let mut set = empty_set(4);
        let mut rng = Prng::new(0);
        for tag in [10, 11, 12, 13, 10] {
            access(&Lru, &mut set, &mut rng, tag);
        }
        // A was used last, then D, C and B.
        assert_eq!(ranks(&set), [0, 3, 2, 1]);
        let mut set = empty_set(4);
        for tag in [10, 11, 12, 13, 10] {
            access(&Fifo, &mut set, &mut rng, tag);
        }
        // D was filled last, then C, B and A; the hit on A changed nothing.
        assert_eq!(ranks(&set), [3, 2, 1, 0]);
        assert_eq!(rng, Prng::new(0));
    }

    #[test]
    fn random_victims_follow_the_seeded_stream() {
        // Under seed 12345, the default, the top two bits of the generator's
        // first outputs are 2, 0, 0, 0, 1, 2, 0, 3: the victims of eight
        // misses on a full four-way set. The first four misses fill empty
        // ways and draw nothing.
        let tags: Vec<u32> = (100..112).collect();
        let outcomes = run(Replacement::Random, 4, 12_345, &tags);
        let random = |index| evicts(index, VictimReason::Random);
        assert_eq!(
            outcomes,
            [
                empty(0),
                empty(1),
                empty(2),
                empty(3),
                random(2),
                random(0),
                random(0),
                random(0),
                random(1),
                random(2),
                random(0),
                random(3),
            ]
        );
        assert_eq!(run(Replacement::Random, 4, 12_345, &tags), outcomes);
    }

    #[test]
    fn random_draws_only_when_no_way_is_invalid() {
        let mut set = empty_set(4);
        let mut rng = Prng::new(12_345);
        for index in 0..3 {
            set[index].slot = valid(u32::try_from(index).unwrap());
            assert_eq!(
                RandomChoice.victim(&set, &mut rng).reason,
                VictimReason::Empty
            );
        }
        assert_eq!(rng, Prng::new(12_345));
        set[3].slot = valid(3);
        assert_eq!(
            RandomChoice.victim(&set, &mut rng),
            chosen(2, VictimReason::Random)
        );
        assert_ne!(rng, Prng::new(12_345));
    }

    #[test]
    fn lru_and_fifo_never_draw() {
        let mut set = empty_set(4);
        for (tag, way) in (0..).zip(set.iter_mut()) {
            way.slot = valid(tag);
        }
        let mut rng = Prng::new(12_345);
        assert_eq!(
            Lru.victim(&set, &mut rng),
            chosen(3, VictimReason::LeastRecentlyUsed { rank: 3 })
        );
        assert_eq!(
            Fifo.victim(&set, &mut rng),
            chosen(3, VictimReason::FirstIn { rank: 3 })
        );
        assert_eq!(rng, Prng::new(12_345));
    }

    #[test]
    fn ranks_stay_a_permutation_under_every_policy() {
        // A pseudo-random stream of tags from twice as many lines as the set
        // holds, so there are hits, fills into empty ways and evictions. After
        // every access the ranks are a permutation; under LRU and FIFO every
        // valid way ranks below every invalid way, and under random
        // replacement, as in any one-way set, the ranks equal the way numbers.
        let mut ways = 1;
        while ways <= MAX_LINES {
            for policy in [Replacement::Lru, Replacement::Fifo, Replacement::Random] {
                let mut set = empty_set(ways);
                let mut rng = Prng::new(12_345);
                let mut tags = Prng::new(u64::from(ways));
                for _ in 0..400 {
                    let tag = tags.draw_way(ways * 2);
                    access(policy.policy(), &mut set, &mut rng, tag);
                    assert_permutation(&set);
                    if policy == Replacement::Random || ways == 1 {
                        assert!(ranks(&set).into_iter().eq(0..ways));
                    } else {
                        assert_valid_below_invalid(&set);
                    }
                }
            }
            ways *= 2;
        }
    }

    #[test]
    fn a_hit_or_fill_reports_every_rank_it_changes() {
        // The moves reported are exactly the ranks that differ before and
        // after, in way order.
        for policy in [Replacement::Lru, Replacement::Fifo] {
            let mut set = empty_set(8);
            let mut rng = Prng::new(12_345);
            let mut tags = Prng::new(3);
            for _ in 0..200 {
                let before = ranks(&set);
                let tag = tags.draw_way(16);
                let (_, moves) = access_ranked(policy.policy(), &mut set, &mut rng, tag);
                let after = ranks(&set);
                let changed: Vec<RankMove> = (0_u32..)
                    .zip(before.iter().zip(&after))
                    .filter(|(_, (from, to))| from != to)
                    .map(|(index, (&from, &to))| rank_move(index, from, to))
                    .collect();
                assert_eq!(moves, changed);
            }
        }
    }
}
