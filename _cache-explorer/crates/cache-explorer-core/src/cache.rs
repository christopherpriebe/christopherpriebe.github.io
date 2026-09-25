//! The cache's contents: sets of ways, each way empty or holding one line.
//!
//! An invalid way has no tag, dirty bit or data, so it has exactly one
//! representation, and a dirty way is always valid: both hold by construction
//! rather than by a check, and two caches in the same state compare equal.
//! The replacement order lives in the ways' ranks; there is no other
//! replacement state per set.
//!
//! Every lookup goes through an accessor that returns `Option`. A lookup that
//! finds nothing can only mean an index that was never checked against the
//! geometry, or a broken invariant; the caller reports it rather than
//! panicking.

use crate::geometry::Geometry;
use crate::units::{SetIndex, Tag, WayIndex};

/// Every set of the cache, in set order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cache {
    /// The sets, `geometry.sets()` of them.
    sets: Vec<Set>,
}

/// The ways of one set, in way order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Set {
    /// The ways, `geometry.ways()` of them.
    ways: Vec<Way>,
}

/// One way of a set: what it holds and where it stands in its set's
/// replacement order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Way {
    /// The line the way holds, if any.
    pub(crate) slot: Slot,
    /// The way's place in its set's replacement order. Under LRU, 0 is the
    /// most recently used way; under FIFO, 0 is the newest fill. The ranks of
    /// a set are always a permutation of `0..ways`, and under random
    /// replacement and in a one-way set they stay equal to the way numbers.
    pub(crate) rank: u32,
}

/// What a way holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    /// Nothing: no tag, no dirty bit, no data.
    Invalid,
    /// One line of memory.
    #[cfg_attr(not(test), expect(dead_code, reason = "the access engine fills ways"))]
    Valid {
        /// The line's tag; with the way's set, it gives the line's address.
        tag: Tag,
        /// Whether the line differs from memory and must be written back
        /// before it is evicted. Only a write-back cache has dirty lines.
        dirty: Dirtiness,
        /// The line's bytes, `line_bytes` of them, holding its words
        /// little-endian.
        data: Vec<u8>,
    },
}

/// Whether a valid line has been written since it was filled, under
/// write-back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the access engine fills and writes lines")
)]
pub(crate) enum Dirtiness {
    /// The line matches what it was filled with.
    Clean,
    /// The line was written, and memory has not seen the write yet.
    Dirty,
}

impl Cache {
    /// The empty cache a run starts with: every way invalid, and ranked by
    /// its way number, so way 0 is taken first and every policy starts from
    /// the same order.
    pub(crate) fn new(geometry: &Geometry) -> Self {
        let set = Set {
            ways: (0..geometry.ways())
                .map(|rank| Way {
                    slot: Slot::Invalid,
                    rank,
                })
                .collect(),
        };
        Self {
            sets: (0..geometry.sets()).map(|_| set.clone()).collect(),
        }
    }

    /// The set `set`, if the cache has it.
    pub(crate) fn set(&self, set: SetIndex) -> Option<&Set> {
        self.sets.get(usize::try_from(set.get()).ok()?)
    }

    /// The set `set` for changing, if the cache has it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the access engine fills sets and ranks them")
    )]
    pub(crate) fn set_mut(&mut self, set: SetIndex) -> Option<&mut Set> {
        self.sets.get_mut(usize::try_from(set.get()).ok()?)
    }

    /// Way `way` of set `set`, if the cache has it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the access engine looks ways up")
    )]
    pub(crate) fn way(&self, set: SetIndex, way: WayIndex) -> Option<&Way> {
        self.set(set)?.way(way)
    }

    /// Way `way` of set `set` for changing, if the cache has it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the access engine fills, writes and evicts ways")
    )]
    pub(crate) fn way_mut(&mut self, set: SetIndex, way: WayIndex) -> Option<&mut Way> {
        self.set_mut(set)?.way_mut(way)
    }
}

impl Set {
    /// The set's ways, in way order.
    pub(crate) fn ways(&self) -> &[Way] {
        &self.ways
    }

    /// The set's ways for changing, in way order, as the replacement policies
    /// rank them.
    #[cfg_attr(not(test), expect(dead_code, reason = "the access engine ranks ways"))]
    pub(crate) fn ways_mut(&mut self) -> &mut [Way] {
        &mut self.ways
    }

    /// Way `way`, if the set has it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the access engine looks ways up")
    )]
    pub(crate) fn way(&self, way: WayIndex) -> Option<&Way> {
        self.ways.get(usize::try_from(way.get()).ok()?)
    }

    /// Way `way` for changing, if the set has it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the access engine fills, writes and evicts ways")
    )]
    pub(crate) fn way_mut(&mut self, way: WayIndex) -> Option<&mut Way> {
        self.ways.get_mut(usize::try_from(way.get()).ok()?)
    }
}

impl Way {
    /// True when the way holds a line.
    pub(crate) fn is_valid(&self) -> bool {
        matches!(self.slot, Slot::Valid { .. })
    }

    /// The tag of the line the way holds, if it holds one.
    pub(crate) fn tag(&self) -> Option<Tag> {
        match self.slot {
            Slot::Valid { tag, .. } => Some(tag),
            Slot::Invalid => None,
        }
    }

    /// True when the way holds a dirty line.
    pub(crate) fn is_dirty(&self) -> bool {
        matches!(
            self.slot,
            Slot::Valid {
                dirty: Dirtiness::Dirty,
                ..
            }
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::{Config, Preset};

    fn geometry(config: &Config) -> Geometry {
        config.validate().unwrap()
    }

    #[test]
    fn a_new_cache_is_empty_with_ranks_equal_to_way_numbers() {
        let geometry = geometry(&Preset::FourWayWriteBack.config());
        let cache = Cache::new(&geometry);
        assert_eq!(cache.sets.len(), 4);
        for set in &cache.sets {
            let ranks: Vec<u32> = set.ways().iter().map(|way| way.rank).collect();
            assert_eq!(ranks, [0, 1, 2, 3]);
            assert!(set.ways().iter().all(|way| way.slot == Slot::Invalid));
        }
    }

    #[test]
    fn a_new_direct_mapped_cache_has_one_way_per_set() {
        let geometry = geometry(&Config::default());
        let cache = Cache::new(&geometry);
        assert_eq!(cache.sets.len(), 8);
        assert!(cache.sets.iter().all(|set| set.ways().len() == 1));
    }

    #[test]
    fn accessors_find_what_the_geometry_allows_and_nothing_else() {
        let geometry = geometry(&Preset::TwoWayLru.config());
        let mut cache = Cache::new(&geometry);
        let set = geometry.set_index(3).unwrap();
        let way = geometry.way_index(1).unwrap();
        assert_eq!(cache.way(set, way).map(|way| way.rank), Some(1));
        assert!(cache.set(set).is_some());
        assert!(cache.set(SetIndex::new_unchecked(4)).is_none());
        assert!(cache.way(set, WayIndex::new_unchecked(2)).is_none());
        assert!(cache.way(SetIndex::new_unchecked(4), way).is_none());
        assert!(cache.set_mut(SetIndex::new_unchecked(4)).is_none());
        assert!(cache.way_mut(set, WayIndex::new_unchecked(2)).is_none());
        assert!(cache
            .set(set)
            .and_then(|set| set.way(WayIndex::new_unchecked(u32::MAX)))
            .is_none());
    }

    #[test]
    fn way_mut_changes_only_its_way() {
        let geometry = geometry(&Preset::TwoWayLru.config());
        let mut cache = Cache::new(&geometry);
        let before = cache.clone();
        let set = geometry.set_index(2).unwrap();
        let way = geometry.way_index(0).unwrap();
        let tag = geometry.tag(5).unwrap();
        cache.way_mut(set, way).unwrap().slot = Slot::Valid {
            tag,
            dirty: Dirtiness::Dirty,
            data: vec![0; 16],
        };
        assert_ne!(cache, before);
        let changed = cache.way(set, way).unwrap();
        assert!(changed.is_valid() && changed.is_dirty());
        assert_eq!(changed.tag(), Some(tag));
        cache.way_mut(set, way).unwrap().slot = Slot::Invalid;
        assert_eq!(cache, before);
    }

    #[test]
    fn an_invalid_way_has_no_tag_and_is_never_dirty() {
        let way = Way {
            slot: Slot::Invalid,
            rank: 0,
        };
        assert!(!way.is_valid());
        assert!(!way.is_dirty());
        assert_eq!(way.tag(), None);
        let clean = Way {
            slot: Slot::Valid {
                tag: Tag::new_unchecked(1),
                dirty: Dirtiness::Clean,
                data: vec![0; 4],
            },
            rank: 0,
        };
        assert!(clean.is_valid());
        assert!(!clean.is_dirty());
    }
}
