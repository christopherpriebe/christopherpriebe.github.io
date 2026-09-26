//! The wording of the log: every line built from machine state is written
//! here, so the tool's voice has one source. That includes the CPU's status
//! and the timed Stats lines, AMAT and the measured cost. Error messages are
//! the exception; each is the `Display` of its error type, next to that type.
//!
//! An access's lines keep the design's wording where it has one, as in "Way 1
//! chosen: least recently used (rank 1).", and the lines it does not draw
//! follow its voice: short declarative sentences that give the number that
//! matters. Addresses, lines and tags are padded to the configuration's
//! widths, and values follow the data rule. No line depends on the cursor or
//! on Basics and Advanced, so the same event always reads the same.

use crate::cache::Dirtiness;
use crate::config::Replacement;
use crate::format;
use crate::geometry::{Geometry, Location, Placement};
use crate::machine::InvariantViolation;
use crate::op::AccessKind;
use crate::replacement::{VictimChoice, VictimReason};
use crate::units::{Addr, LineAddr, Register, SetIndex, Tag, WayIndex, Word, WordIndex};

/// What a store's `Write` line says after the word it wrote, which depends on
/// the write policy and on what the store found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WriteNote {
    /// Under write-through the line says nothing more: the `WriteThrough`
    /// event that follows writes memory.
    Through,
    /// Under write-back the line names the dirty bit and memory's copy of the
    /// word.
    Back {
        /// The line's dirty bit before the store.
        before: Dirtiness,
        /// Whether memory's copy of the word now differs from the cache's.
        stale: bool,
    },
}

/// What a miss's stall waits for in timed mode, which its `Stall` line names.
/// Each transfer to or from memory costs one miss penalty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StallFor {
    /// Memory supplying the new line.
    Fill {
        /// The line coming in.
        line: LineAddr,
    },
    /// A dirty victim's line going back to memory, then memory supplying the
    /// new line: two transfers.
    WriteBackThenFill {
        /// The dirty line going out.
        victim: LineAddr,
        /// The line coming in.
        line: LineAddr,
    },
    /// A store that missed under write-no-allocate going to memory.
    WriteAround,
}

impl StallFor {
    /// The transfers the stall waits for, each costing one miss penalty.
    pub(crate) fn transfers(self) -> u32 {
        match self {
            StallFor::Fill { .. } | StallFor::WriteAround => 1,
            StallFor::WriteBackThenFill { .. } => 2,
        }
    }
}

/// What an entry is, as the CPU status names it. The design calls every
/// entry an access; here an instruction without a memory access, an edit and
/// an error that ends an op are named for what they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the CPU status names the cursor's entry")
)]
pub(crate) enum EntryNoun {
    /// A load or store, issued by hand or executed by a program.
    Access,
    /// An instruction without a memory access, such as `add` or `bnez`.
    Instruction,
    /// An edit of a way or a memory word.
    Edit,
    /// An error that ended an op.
    Error,
}

/// The `Decode` line: the address split into its fields, as in "Load 0x2A4:
/// tag 0x0A, set 2, offset 4 (word 1)." A fully associative cache has no
/// index, and the line says so instead of naming a set.
pub(crate) fn decode(
    geometry: &Geometry,
    kind: AccessKind,
    addr: Addr,
    location: Location,
) -> String {
    let verb = match kind {
        AccessKind::Read => "Load",
        AccessKind::Write => "Store",
    };
    let addr = address(geometry, addr.get());
    let tag = tag(geometry, location.tag);
    let offset = format::data(location.offset);
    let word = format::data(location.word.get());
    match geometry.placement() {
        Placement::FullyAssociative => format!(
            "{verb} {addr}: tag {tag}, offset {offset} (word {word}). Fully associative: one \
             set, no index bits."
        ),
        Placement::DirectMapped | Placement::SetAssociative => format!(
            "{verb} {addr}: tag {tag}, set {}, offset {offset} (word {word}).",
            format::data(location.set.get())
        ),
    }
}

/// The `Hit` line: "Set 2, way 0: valid, and tag 0x01 matches. Hit."
pub(crate) fn hit(geometry: &Geometry, set: SetIndex, way: WayIndex, tag: Tag) -> String {
    format!(
        "Set {}, way {}: valid, and tag {} matches. Hit.",
        format::data(set.get()),
        format::data(way.get()),
        self::tag(geometry, tag)
    )
}

/// The `Miss` line. A set of several ways reads "Set 2: no valid way holds
/// tag 0x0A. Miss."; a one-way set names what its way held, `held`, the tag
/// it was compared against: "Set 2 holds tag 0x01, not 0x03. Miss.", or "Set
/// 2 is invalid. Miss." when it held nothing.
pub(crate) fn miss(geometry: &Geometry, set: SetIndex, tag: Tag, held: Option<Tag>) -> String {
    let set = format::data(set.get());
    let tag = self::tag(geometry, tag);
    if geometry.ways() > 1 {
        return format!("Set {set}: no valid way holds tag {tag}. Miss.");
    }
    match held {
        Some(held) => format!(
            "Set {set} holds tag {}, not {tag}. Miss.",
            self::tag(geometry, held)
        ),
        None => format!("Set {set} is invalid. Miss."),
    }
}

/// The `Victim` line: the way a miss chose, and why that one.
pub(crate) fn victim(geometry: &Geometry, set: SetIndex, choice: VictimChoice) -> String {
    let way = format::data(choice.way.get());
    match choice.reason {
        VictimReason::Empty if geometry.ways() == 1 => format!(
            "Set {} has one way; the line goes there.",
            format::data(set.get())
        ),
        VictimReason::Empty => format!("Way {way} chosen: it is invalid (empty)."),
        // A one-line cache also has a single way, but it is fully associative,
        // so only a direct-mapped cache is named as the reason.
        VictimReason::OnlyWay => match geometry.placement() {
            Placement::DirectMapped => format!(
                "Direct-mapped: way {way} is the only place for this line, so its current line \
                 must go."
            ),
            Placement::SetAssociative | Placement::FullyAssociative => {
                format!("Way {way} is the only place for this line, so its current line must go.")
            }
        },
        VictimReason::LeastRecentlyUsed { rank } => format!(
            "Way {way} chosen: least recently used (rank {}).",
            format::data(rank)
        ),
        VictimReason::FirstIn { .. } => format!("Way {way} chosen: filled earliest (FIFO)."),
        VictimReason::Random => format!("Way {way} chosen at random."),
    }
}

/// The `Stall` line of timed mode: how long the CPU waits for memory, and
/// what for. Each transfer costs `miss` cycles, the miss penalty: "The CPU
/// stalls for 20 cycles while memory supplies line 0x2A0.", or with a dirty
/// victim "The CPU stalls for 40 cycles: 20 to write line 0x1A0 back, then 20
/// to fetch line 0x2A0."
pub(crate) fn stall(geometry: &Geometry, miss: u32, wait: StallFor) -> String {
    let total = format::cycles(miss.saturating_mul(wait.transfers()));
    match wait {
        StallFor::Fill { line } => format!(
            "The CPU stalls for {total} while memory supplies line {}.",
            address(geometry, line.get())
        ),
        StallFor::WriteBackThenFill { victim, line } => {
            let each = format::setting(miss);
            format!(
                "The CPU stalls for {total}: {each} to write line {} back, then {each} to fetch \
                 line {}.",
                address(geometry, victim.get()),
                address(geometry, line.get())
            )
        }
        StallFor::WriteAround => {
            format!("The CPU stalls for {total} while the store goes to memory.")
        }
    }
}

/// The `WriteBack` line: "Way 1 is dirty: write line 0x1A0 back to memory
/// first."
pub(crate) fn write_back(geometry: &Geometry, way: WayIndex, line: LineAddr) -> String {
    format!(
        "Way {} is dirty: write line {} back to memory first.",
        format::data(way.get()),
        address(geometry, line.get())
    )
}

/// The `Evict` line: "Evict line 0x1A0 (tag 0x06) from way 1."
pub(crate) fn evict(geometry: &Geometry, way: WayIndex, line: LineAddr, tag: Tag) -> String {
    format!(
        "Evict line {} (tag {}) from way {}.",
        address(geometry, line.get()),
        self::tag(geometry, tag),
        format::data(way.get())
    )
}

/// The `Fill` line: "Fill way 1 with line 0x2A0–0x2AF from memory."
pub(crate) fn fill(geometry: &Geometry, way: WayIndex, line: LineAddr) -> String {
    format!(
        "Fill way {} with line {} from memory.",
        format::data(way.get()),
        format::line_range(line.get(), geometry.line_bytes(), geometry.address_bits())
    )
}

/// The `Rank` line: the way the hit or fill moved to the front, then every
/// other way of the set in way order, with the ranks `ranks` gives in way
/// order after the move. "LRU ranks in set 2: way 1 becomes 0 (most recent);
/// way 0 is 1." or "FIFO order in set 0: way 0 becomes 0 (newest); way 1 is
/// 1."
pub(crate) fn rank(
    replacement: Replacement,
    set: SetIndex,
    way: WayIndex,
    ranks: &[u32],
) -> String {
    let (order, front) = match replacement {
        Replacement::Fifo => ("FIFO order", "newest"),
        // Random replacement never moves a rank, so no `Rank` event has it.
        Replacement::Lru | Replacement::Random => ("LRU ranks", "most recent"),
    };
    let promoted = usize::try_from(way.get())
        .ok()
        .and_then(|index| ranks.get(index))
        .copied()
        .unwrap_or(0);
    let others: Vec<String> = (0_u32..)
        .zip(ranks)
        .filter(|(other, _)| *other != way.get())
        .map(|(other, rank)| format!("way {} is {}", format::data(other), format::data(*rank)))
        .collect();
    let moved = format!(
        "{order} in set {}: way {} becomes {} ({front})",
        format::data(set.get()),
        format::data(way.get()),
        format::data(promoted)
    );
    if others.is_empty() {
        format!("{moved}.")
    } else {
        format!("{moved}; {}.", others.join(", "))
    }
}

/// A load's `Complete` line: "Read word 1 of way 1 (68) into r3." A load into
/// r0 reads the word and drops it, since r0 always reads 0.
pub(crate) fn load(word: WordIndex, way: WayIndex, value: u64, into: Register) -> String {
    let read = format!(
        "Read word {} of way {} ({})",
        format::data(word.get()),
        format::data(way.get()),
        format::data(value)
    );
    if into == Register::R0 {
        format!("{read}; r0 always reads 0, so the value is dropped.")
    } else {
        format!("{read} into {into}.")
    }
}

/// A store's `Write` line: the word written into the way, then, under
/// write-back, the dirty bit and memory's copy of the word at `addr`: "Write
/// 7 into word 1 of way 0 and set its dirty bit. Memory at 0x0A4 is now
/// stale."
pub(crate) fn write(
    geometry: &Geometry,
    addr: Addr,
    word: WordIndex,
    way: WayIndex,
    value: Word,
    note: WriteNote,
) -> String {
    let value = format::data(value.get());
    let wrote = format!(
        "Write {value} into word {} of way {}",
        format::data(word.get()),
        format::data(way.get())
    );
    let (before, stale) = match note {
        WriteNote::Through => return format!("{wrote}."),
        WriteNote::Back { before, stale } => (before, stale),
    };
    let addr = address(geometry, addr.get());
    let dirty = match before {
        Dirtiness::Clean => " and set its dirty bit.",
        Dirtiness::Dirty => "; the line was already dirty.",
    };
    let memory = match (stale, before) {
        (false, _) => format!("Memory at {addr} already holds {value}."),
        (true, Dirtiness::Clean) => format!("Memory at {addr} is now stale."),
        (true, Dirtiness::Dirty) => format!("Memory at {addr} is stale."),
    };
    format!("{wrote}{dirty} {memory}")
}

/// The `WriteThrough` line: "Write-through: 7 also goes to memory at 0x0A4."
pub(crate) fn write_through(geometry: &Geometry, addr: Addr, value: Word) -> String {
    format!(
        "Write-through: {} also goes to memory at {}.",
        format::data(value.get()),
        address(geometry, addr.get())
    )
}

/// The `MemWrite` line of a store that missed under write-no-allocate:
/// "Write-no-allocate: 7 goes straight to memory at 0x0A4. The cache is
/// unchanged."
pub(crate) fn write_around(geometry: &Geometry, addr: Addr, value: Word) -> String {
    format!(
        "Write-no-allocate: {} goes straight to memory at {}. The cache is unchanged.",
        format::data(value.get()),
        address(geometry, addr.get())
    )
}

/// The CPU status for the cursor's place in its entry, which counts steps,
/// one per event: "Access #5 · step 3 of 8", "Edit #6 · step 1 of 1".
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the CPU status names the cursor's entry")
)]
pub(crate) fn entry_status(noun: EntryNoun, number: u32, step: u32, steps: u32) -> String {
    let noun = match noun {
        EntryNoun::Access => "Access",
        EntryNoun::Instruction => "Instruction",
        EntryNoun::Edit => "Edit",
        EntryNoun::Error => "Error",
    };
    format!(
        "{noun} {} \u{b7} step {} of {}",
        format::entry_number(number),
        format::data(step),
        format::data(steps)
    )
}

/// The CPU status while a miss stalls it, counting down the cycles left:
/// "Stalled: 40 cycles".
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the CPU status counts a stall down")
)]
pub(crate) fn stalled(remaining: u32) -> String {
    format!("Stalled: {}", format::cycles(remaining))
}

/// The AMAT line, as the design writes it: "AMAT = 1 + 0.40 × 20 = 9.0
/// cycles", from the hit time `hit`, the miss rate `rate` in hundredths, the
/// miss penalty `miss` and the result `amat` in tenths of a cycle, both
/// already rounded.
pub(crate) fn amat(hit: u32, rate: u64, miss: u32, amat: u64) -> String {
    format!(
        "AMAT = {} + {} \u{d7} {} = {} cycles",
        format::setting(hit),
        format::decimal(rate, 2),
        format::setting(miss),
        format::decimal(amat, 1)
    )
}

/// The measured line under AMAT: the cycles the run spent per access, and
/// what its write-backs, which AMAT leaves out, added. "Measured: 11.0
/// cycles per access (AMAT plus 2.0 for 1 write-back)." `measured` and
/// `plus` are in tenths of a cycle, already rounded.
pub(crate) fn measured(measured: u64, plus: u64, writebacks: u32) -> String {
    format!(
        "Measured: {} cycles per access (AMAT plus {} for {} {}).",
        format::decimal(measured, 1),
        format::decimal(plus, 1),
        format::data(writebacks),
        format::plural(writebacks, "write-back", "write-backs")
    )
}

/// The line of the `Error` event that ends an op when an invariant no longer
/// holds.
pub(crate) fn internal_error(violation: &InvariantViolation) -> String {
    format!(
        "Internal error: {violation} no longer holds. The run stopped here; please report it \
         with a share link."
    )
}

/// An address or a line address, padded to the address width.
fn address(geometry: &Geometry, value: u32) -> String {
    format::address(value, geometry.address_bits())
}

/// A tag, padded to the tag width.
fn tag(geometry: &Geometry, tag: Tag) -> String {
    format::tag(tag.get(), geometry.tag_bits())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::{CacheConfig, Config, Preset, WordBytes};

    /// The tests' address width: 12 bits, as in every design frame. The lines
    /// these tests expect are written for it, so the width is set here rather
    /// than taken from `Config::default()`, and changing the default does not
    /// break these tests.
    const ADDRESS_BITS: u8 = 12;

    fn geometry(config: &Config) -> Geometry {
        config.validate().unwrap()
    }

    /// A named configuration's geometry at the tests' address width.
    fn preset(preset: Preset) -> Geometry {
        geometry(&Config {
            address_bits: ADDRESS_BITS,
            ..preset.config()
        })
    }

    /// A 128 B cache of 16 B lines with `ways` ways, at the tests' address
    /// width.
    fn ways(ways: u32) -> Geometry {
        geometry(&Config {
            address_bits: ADDRESS_BITS,
            cache: CacheConfig {
                associativity: ways,
                ..Preset::DirectMapped8.config().cache
            },
            ..Preset::DirectMapped8.config()
        })
    }

    /// The one-line cache: 16 B of 16 B lines, fully associative with one
    /// way, at the tests' address width.
    fn one_line() -> Geometry {
        geometry(&Config {
            address_bits: ADDRESS_BITS,
            cache: CacheConfig {
                size_bytes: 16,
                ..Preset::DirectMapped8.config().cache
            },
            ..Preset::DirectMapped8.config()
        })
    }

    fn addr(value: u32) -> Addr {
        Addr::new_unchecked(value)
    }

    fn line(value: u32) -> LineAddr {
        LineAddr::new_unchecked(value)
    }

    fn tag_of(value: u32) -> Tag {
        Tag::new_unchecked(value)
    }

    fn set(index: u32) -> SetIndex {
        SetIndex::new_unchecked(index)
    }

    fn way(index: u32) -> WayIndex {
        WayIndex::new_unchecked(index)
    }

    fn word(index: u32) -> WordIndex {
        WordIndex::new_unchecked(index)
    }

    fn value(value: u64) -> Word {
        Word::new_unchecked(value)
    }

    fn choice(index: u32, reason: VictimReason) -> VictimChoice {
        VictimChoice {
            way: way(index),
            reason,
        }
    }

    #[test]
    fn decode_names_the_fields_of_the_address() {
        let two_way = preset(Preset::TwoWayLru);
        let at = addr(0x2A4);
        assert_eq!(
            decode(&two_way, AccessKind::Read, at, two_way.locate(at)),
            "Load 0x2A4: tag 0x0A, set 2, offset 4 (word 1)."
        );
        let direct = preset(Preset::DirectMapped8);
        let at = addr(0x0A4);
        assert_eq!(
            decode(&direct, AccessKind::Write, at, direct.locate(at)),
            "Store 0x0A4: tag 0x01, set 2, offset 4 (word 1)."
        );
    }

    #[test]
    fn decode_pads_to_the_configured_widths() {
        let config = Config {
            address_bits: 16,
            word_bytes: WordBytes::Eight,
            ..Preset::DirectMapped8.config()
        };
        let wide = geometry(&config);
        let at = addr(0x0A8);
        assert_eq!(
            decode(&wide, AccessKind::Read, at, wide.locate(at)),
            "Load 0x00A8: tag 0x001, set 2, offset 8 (word 1)."
        );
    }

    #[test]
    fn a_fully_associative_decode_has_no_set() {
        for geometry in [ways(8), one_line()] {
            let at = addr(0x0A4);
            assert_eq!(
                decode(&geometry, AccessKind::Read, at, geometry.locate(at)),
                "Load 0x0A4: tag 0x0A, offset 4 (word 1). Fully associative: one set, no index \
                 bits."
            );
        }
    }

    #[test]
    fn a_hit_names_the_way_and_the_tag() {
        assert_eq!(
            hit(&preset(Preset::DirectMapped8), set(2), way(0), tag_of(1)),
            "Set 2, way 0: valid, and tag 0x01 matches. Hit."
        );
    }

    #[test]
    fn a_miss_in_a_one_way_set_names_what_the_way_held() {
        let direct = preset(Preset::DirectMapped8);
        assert_eq!(
            miss(&direct, set(2), tag_of(3), Some(tag_of(1))),
            "Set 2 holds tag 0x01, not 0x03. Miss."
        );
        assert_eq!(
            miss(&direct, set(2), tag_of(1), None),
            "Set 2 is invalid. Miss."
        );
        assert_eq!(
            miss(&one_line(), set(0), tag_of(0x1A), Some(tag_of(0x0A))),
            "Set 0 holds tag 0x0A, not 0x1A. Miss."
        );
    }

    #[test]
    fn a_miss_in_a_set_of_several_ways_names_only_the_tag() {
        let two_way = preset(Preset::TwoWayLru);
        for held in [None, Some(tag_of(6))] {
            assert_eq!(
                miss(&two_way, set(2), tag_of(0x0A), held),
                "Set 2: no valid way holds tag 0x0A. Miss."
            );
        }
    }

    #[test]
    fn a_victim_line_gives_the_reason_for_the_way() {
        let two_way = preset(Preset::TwoWayLru);
        let lines = [
            (
                choice(0, VictimReason::Empty),
                "Way 0 chosen: it is invalid (empty).",
            ),
            (
                choice(1, VictimReason::LeastRecentlyUsed { rank: 1 }),
                "Way 1 chosen: least recently used (rank 1).",
            ),
            (
                choice(0, VictimReason::FirstIn { rank: 1 }),
                "Way 0 chosen: filled earliest (FIFO).",
            ),
            (choice(1, VictimReason::Random), "Way 1 chosen at random."),
        ];
        for (chosen, line) in lines {
            assert_eq!(victim(&two_way, set(0), chosen), line);
        }
    }

    #[test]
    fn a_one_way_set_names_its_only_way() {
        let direct = preset(Preset::DirectMapped8);
        assert_eq!(
            victim(&direct, set(2), choice(0, VictimReason::Empty)),
            "Set 2 has one way; the line goes there."
        );
        assert_eq!(
            victim(&direct, set(2), choice(0, VictimReason::OnlyWay)),
            "Direct-mapped: way 0 is the only place for this line, so its current line must go."
        );
        // A one-line cache is fully associative, not direct-mapped.
        assert_eq!(
            victim(&one_line(), set(0), choice(0, VictimReason::Empty)),
            "Set 0 has one way; the line goes there."
        );
        assert_eq!(
            victim(&one_line(), set(0), choice(0, VictimReason::OnlyWay)),
            "Way 0 is the only place for this line, so its current line must go."
        );
    }

    #[test]
    fn the_allocation_lines_name_the_old_and_the_new_line() {
        let two_way = preset(Preset::TwoWayLru);
        assert_eq!(
            write_back(&two_way, way(1), line(0x1A0)),
            "Way 1 is dirty: write line 0x1A0 back to memory first."
        );
        assert_eq!(
            evict(&two_way, way(1), line(0x1A0), tag_of(6)),
            "Evict line 0x1A0 (tag 0x06) from way 1."
        );
        assert_eq!(
            fill(&two_way, way(1), line(0x2A0)),
            "Fill way 1 with line 0x2A0\u{2013}0x2AF from memory."
        );
    }

    #[test]
    fn a_stall_names_its_length_and_what_it_waits_for() {
        let two_way = preset(Preset::TwoWayLru);
        assert_eq!(
            stall(&two_way, 20, StallFor::Fill { line: line(0x2A0) }),
            "The CPU stalls for 20 cycles while memory supplies line 0x2A0."
        );
        assert_eq!(
            stall(
                &two_way,
                20,
                StallFor::WriteBackThenFill {
                    victim: line(0x1A0),
                    line: line(0x2A0)
                }
            ),
            "The CPU stalls for 40 cycles: 20 to write line 0x1A0 back, then 20 to fetch line \
             0x2A0."
        );
        assert_eq!(
            stall(&two_way, 20, StallFor::WriteAround),
            "The CPU stalls for 20 cycles while the store goes to memory."
        );
    }

    #[test]
    fn a_stall_of_one_cycle_is_singular_and_long_ones_are_grouped() {
        let direct = preset(Preset::DirectMapped8);
        assert_eq!(
            stall(&direct, 1, StallFor::Fill { line: line(0x0A0) }),
            "The CPU stalls for 1 cycle while memory supplies line 0x0A0."
        );
        assert_eq!(
            stall(
                &direct,
                1,
                StallFor::WriteBackThenFill {
                    victim: line(0x0A0),
                    line: line(0x1A0)
                }
            ),
            "The CPU stalls for 2 cycles: 1 to write line 0x0A0 back, then 1 to fetch line \
             0x1A0."
        );
        assert_eq!(
            stall(
                &direct,
                1_500,
                StallFor::WriteBackThenFill {
                    victim: line(0x0A0),
                    line: line(0x1A0)
                }
            ),
            "The CPU stalls for 3,000 cycles: 1,500 to write line 0x0A0 back, then 1,500 to \
             fetch line 0x1A0."
        );
        assert_eq!(
            stall(&direct, 1, StallFor::WriteAround),
            "The CPU stalls for 1 cycle while the store goes to memory."
        );
    }

    #[test]
    fn a_stall_waits_for_one_transfer_or_two() {
        let fill = StallFor::Fill { line: line(0x2A0) };
        let both = StallFor::WriteBackThenFill {
            victim: line(0x1A0),
            line: line(0x2A0),
        };
        assert_eq!(fill.transfers(), 1);
        assert_eq!(StallFor::WriteAround.transfers(), 1);
        assert_eq!(both.transfers(), 2);
    }

    #[test]
    fn the_stall_status_counts_the_cycles_left() {
        assert_eq!(stalled(40), "Stalled: 40 cycles");
        assert_eq!(stalled(1), "Stalled: 1 cycle");
        assert_eq!(stalled(2_000), "Stalled: 2,000 cycles");
    }

    #[test]
    fn the_amat_line_shows_its_arithmetic() {
        assert_eq!(
            amat(1, 40, 20, 90),
            "AMAT = 1 + 0.40 \u{d7} 20 = 9.0 cycles"
        );
        assert_eq!(
            amat(1, 33, 20, 76),
            "AMAT = 1 + 0.33 \u{d7} 20 = 7.6 cycles"
        );
        assert_eq!(
            amat(3, 100, 1_500, 15_030),
            "AMAT = 3 + 1.00 \u{d7} 1,500 = 1,503.0 cycles"
        );
    }

    #[test]
    fn the_measured_line_names_the_write_backs_amat_leaves_out() {
        assert_eq!(
            measured(110, 20, 1),
            "Measured: 11.0 cycles per access (AMAT plus 2.0 for 1 write-back)."
        );
        assert_eq!(
            measured(77, 0, 0),
            "Measured: 7.7 cycles per access (AMAT plus 0.0 for 0 write-backs)."
        );
        assert_eq!(
            measured(12_345, 10_005, 12_000),
            "Measured: 1,234.5 cycles per access (AMAT plus 1,000.5 for 12,000 write-backs)."
        );
    }

    #[test]
    fn a_rank_line_lists_every_other_way_in_way_order() {
        assert_eq!(
            rank(Replacement::Lru, set(2), way(1), &[1, 0]),
            "LRU ranks in set 2: way 1 becomes 0 (most recent); way 0 is 1."
        );
        assert_eq!(
            rank(Replacement::Fifo, set(0), way(0), &[0, 1]),
            "FIFO order in set 0: way 0 becomes 0 (newest); way 1 is 1."
        );
        assert_eq!(
            rank(Replacement::Lru, set(3), way(2), &[1, 3, 0, 2]),
            "LRU ranks in set 3: way 2 becomes 0 (most recent); way 0 is 1, way 1 is 3, way 3 \
             is 2."
        );
    }

    #[test]
    fn a_load_names_the_word_the_value_and_the_register() {
        assert_eq!(
            load(word(1), way(1), 68, Register::R3),
            "Read word 1 of way 1 (68) into r3."
        );
        assert_eq!(
            load(word(0), way(0), 12_345, Register::R0),
            "Read word 0 of way 0 (12,345); r0 always reads 0, so the value is dropped."
        );
    }

    #[test]
    fn a_write_back_store_names_the_dirty_bit_and_memory() {
        let direct = preset(Preset::DirectMapped8);
        let lines = [
            (
                Dirtiness::Clean,
                true,
                "Write 7 into word 1 of way 0 and set its dirty bit. Memory at 0x0A4 is now \
                 stale.",
            ),
            (
                Dirtiness::Clean,
                false,
                "Write 7 into word 1 of way 0 and set its dirty bit. Memory at 0x0A4 already \
                 holds 7.",
            ),
            (
                Dirtiness::Dirty,
                true,
                "Write 7 into word 1 of way 0; the line was already dirty. Memory at 0x0A4 is \
                 stale.",
            ),
            (
                Dirtiness::Dirty,
                false,
                "Write 7 into word 1 of way 0; the line was already dirty. Memory at 0x0A4 \
                 already holds 7.",
            ),
        ];
        for (before, stale, line) in lines {
            let note = WriteNote::Back { before, stale };
            assert_eq!(
                write(&direct, addr(0x0A4), word(1), way(0), value(7), note),
                line
            );
        }
    }

    #[test]
    fn a_write_through_store_writes_the_cache_then_memory() {
        let direct = preset(Preset::DirectMapped8);
        assert_eq!(
            write(
                &direct,
                addr(0x0A4),
                word(1),
                way(0),
                value(7),
                WriteNote::Through
            ),
            "Write 7 into word 1 of way 0."
        );
        assert_eq!(
            write_through(&direct, addr(0x0A4), value(7)),
            "Write-through: 7 also goes to memory at 0x0A4."
        );
    }

    #[test]
    fn a_write_around_leaves_the_cache_alone() {
        assert_eq!(
            write_around(&preset(Preset::DirectMapped8), addr(0x0A4), value(70_000)),
            "Write-no-allocate: 70,000 goes straight to memory at 0x0A4. The cache is unchanged."
        );
    }

    #[test]
    fn the_cpu_status_names_the_entry_and_counts_its_steps() {
        let lines = [
            (EntryNoun::Access, 5, 3, 8, "Access #5 \u{b7} step 3 of 8"),
            (EntryNoun::Edit, 6, 1, 1, "Edit #6 \u{b7} step 1 of 1"),
            (
                EntryNoun::Instruction,
                7,
                1,
                1,
                "Instruction #7 \u{b7} step 1 of 1",
            ),
            (EntryNoun::Error, 8, 1, 1, "Error #8 \u{b7} step 1 of 1"),
            (
                EntryNoun::Access,
                12_345,
                10,
                10,
                "Access #12345 \u{b7} step 10 of 10",
            ),
        ];
        for (noun, number, step, steps, line) in lines {
            assert_eq!(entry_status(noun, number, step, steps), line);
        }
    }

    #[test]
    fn an_internal_error_names_the_invariant() {
        let violation = InvariantViolation::DuplicateTag { set: set(2) };
        assert_eq!(
            internal_error(&violation),
            "Internal error: invariant I3 (no two valid ways in set 2 hold the same tag) no \
             longer holds. The run stopped here; please report it with a share link."
        );
    }
}
