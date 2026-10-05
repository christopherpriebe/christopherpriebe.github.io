#![allow(clippy::disallowed_macros)] // assertions are how tests fail

use super::*;
use crate::config::{AllocatePolicy, CacheConfig, MemoryInit, Preset, WordBytes};
use crate::cpu::CpuState;
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
                Dirtiness::Clean => machine.memory.line(line).into_owned(),
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
    let data = machine.memory.line(decoded.line).into_owned();
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
    let machine = start(config);
    assert_eq!(machine.geometry, config.validate().unwrap());
    assert_eq!(machine.cache, Cache::new(&machine.geometry));
    assert_eq!(
        machine.memory,
        Memory::new(&machine.geometry, MemoryInit::Pattern)
    );
    assert_eq!(machine.cpu, CpuState::default());
    assert_eq!(machine.counters, Counters::default());
    assert_eq!(machine.shadow, Shadow::new(machine.geometry.lines()));
    assert!(machine.shadow.lines().is_empty());
    assert!(machine.filled.is_empty());
    assert_eq!(machine.recent, Recent::default());
    assert_eq!(machine.prng, Prng::new(config.seed));
    assert_eq!(machine.access, None);
    assert_eq!((machine.run.run, machine.run.entries), (1, 0));
    assert!(!machine.in_flight());
    assert_eq!(machine.config, config);
    assert_eq!(machine.check_invariants(), Ok(()));
}

#[test]
fn a_run_cannot_start_from_an_invalid_configuration() {
    let mut config = Config::default();
    config.cache.associativity = 3;
    assert_eq!(Machine::start(config), Err(config.validate().unwrap_err()));
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
    let first = run(config);
    assert_eq!(first, run(config));
    assert_ne!(first.prng, Prng::new(config.seed));
    let other = run(Config { seed: 1, ..config });
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
fn i1_reports_a_rank_equal_to_the_widest_sets_way_count() {
    // 64 is the most ways any set gets, so this is the narrowest rank
    // `rank >= count` refuses on its own, before the bitmask it folds
    // ranks into is ever consulted.
    let mut machine = start(config(1024, 64, Replacement::Lru, WritePolicy::WriteBack));
    machine.cache.way_mut(set(0), way(0)).unwrap().rank = 64;
    assert_eq!(
        machine.check_invariants(),
        Err(InvariantViolation::RanksNotAPermutation {
            set: set(0),
            ways: 64
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
fn i3_reports_a_duplicate_tag_found_above_the_pairwise_scan_width() {
    // Above 8 ways, I3 sorts a fixed-size array instead of scanning
    // pairwise; a 64-way set with a distinct tag per way exercises that
    // path once two of them collide.
    let mut machine = start(config(1024, 64, Replacement::Lru, WritePolicy::WriteBack));
    for way_index in 0..64 {
        let slot = line(&machine, way_index, Dirtiness::Clean);
        put(&mut machine, 0, way_index, slot, way_index);
    }
    assert_eq!(machine.check_invariants(), Ok(()));
    // Way 63 takes way 0's tag; its rank is unaffected and stays valid.
    let slot = line(&machine, 0, Dirtiness::Clean);
    put(&mut machine, 0, 63, slot, 63);
    assert_eq!(
        machine.check_invariants(),
        Err(InvariantViolation::DuplicateTag { set: set(0) })
    );
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
    let Slot::Valid { data, dirty, .. } = &mut machine.cache.way_mut(set(2), way(1)).unwrap().slot
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
    if let Slot::Valid { dirty, .. } = &mut machine.cache.way_mut(set(2), way(1)).unwrap().slot {
        *dirty = Dirtiness::Dirty;
    }
    assert_eq!(machine.check_invariants(), Ok(()));
}

#[test]
fn i6_reports_an_untouched_clean_line_that_differs_from_its_initial_bytes() {
    // Zero memory: an untouched line reads as zero bytes. This way's
    // data does not, so it must report I6, not I8, even though the line
    // was never recorded as touched either.
    let mut machine = start(Config::default());
    let mut slot = line(&machine, 1, Dirtiness::Clean);
    let Slot::Valid { data, .. } = &mut slot else {
        panic!("way 0 holds a line");
    };
    data[0] ^= 0xFF;
    machine.cache.way_mut(set(3), way(0)).unwrap().slot = slot;
    assert_eq!(
        machine.check_invariants(),
        Err(InvariantViolation::CleanLineDiffersFromMemory {
            set: set(3),
            way: way(0)
        })
    );
}

#[test]
fn i7_is_reported_before_i8_when_a_set_breaks_both() {
    let mut machine = start(config(128, 2, Replacement::Random, WritePolicy::WriteBack));
    // An untouched line, which I8 alone would report.
    machine.cache.way_mut(set(3), way(0)).unwrap().slot = line(&machine, 1, Dirtiness::Clean);
    // A valid permutation (I1 holds) that is not the way numbers random
    // replacement requires (I7).
    machine.cache.way_mut(set(3), way(0)).unwrap().rank = 1;
    machine.cache.way_mut(set(3), way(1)).unwrap().rank = 0;
    assert_eq!(
        machine.check_invariants(),
        Err(InvariantViolation::RanksNotWayNumbers { set: set(3) })
    );
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
            core: (),
            addr,
            into: crate::units::Register::R1,
            origin: crate::request::Origin::Hand,
        },
        location: geometry.locate(addr),
        lookup: None,
        victim: None,
        way: None,
        stall: None,
        done: false,
        counters_before: machine.counters,
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
