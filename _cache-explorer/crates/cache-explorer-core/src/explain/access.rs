//! The wording of an access as it plays out on the machine: the address
//! decode, the hit or miss and why, the victim chosen and the transfers a
//! miss stalls for, the load or store that completes it, an instruction's
//! own `Complete` and `Error` lines, and the line for a broken invariant.

use std::fmt::Write as _;

use crate::cache::Dirtiness;
use crate::config::Replacement;
use crate::format;
use crate::geometry::{Geometry, Location, Placement};
use crate::limits::INSTRUCTION_BUDGET;
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
    let addr = super::address(geometry, addr.get());
    let tag = super::tag(geometry, location.tag);
    let offset = format::Data(u64::from(location.offset));
    let word = format::Data(u64::from(location.word.get()));
    match geometry.placement() {
        Placement::FullyAssociative => format!(
            "{verb} {addr}: tag {tag}, offset {offset} (word {word}). Fully associative: one \
             set, no index bits."
        ),
        Placement::DirectMapped | Placement::SetAssociative => format!(
            "{verb} {addr}: tag {tag}, set {}, offset {offset} (word {word}).",
            format::Data(u64::from(location.set.get()))
        ),
    }
}

/// The `Hit` line: "Set 2, way 0: valid, and tag 0x01 matches. Hit."
pub(crate) fn hit(geometry: &Geometry, set: SetIndex, way: WayIndex, tag: Tag) -> String {
    format!(
        "Set {}, way {}: valid, and tag {} matches. Hit.",
        format::Data(u64::from(set.get())),
        format::Data(u64::from(way.get())),
        super::tag(geometry, tag)
    )
}

/// The `Miss` line. A set of several ways reads "Set 2: no valid way holds
/// tag 0x0A. Miss."; a one-way set names what its way held, `held`, the tag
/// it was compared against: "Set 2 holds tag 0x01, not 0x03. Miss.", or "Set
/// 2 is invalid. Miss." when it held nothing.
pub(crate) fn miss(geometry: &Geometry, set: SetIndex, tag: Tag, held: Option<Tag>) -> String {
    let set = format::Data(u64::from(set.get()));
    let tag = super::tag(geometry, tag);
    if geometry.ways() > 1 {
        return format!("Set {set}: no valid way holds tag {tag}. Miss.");
    }
    match held {
        Some(held) => format!(
            "Set {set} holds tag {}, not {tag}. Miss.",
            super::tag(geometry, held)
        ),
        None => format!("Set {set} is invalid. Miss."),
    }
}

/// The `Victim` line: the way a miss chose, and why that one.
pub(crate) fn victim(geometry: &Geometry, set: SetIndex, choice: VictimChoice) -> String {
    let way = format::Data(u64::from(choice.way.get()));
    match choice.reason {
        VictimReason::Empty if geometry.ways() == 1 => format!(
            "Set {} has one way; the line goes there.",
            format::Data(u64::from(set.get()))
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
            format::Data(u64::from(rank))
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
            super::address(geometry, line.get())
        ),
        StallFor::WriteBackThenFill { victim, line } => {
            let each = format::Setting(u64::from(miss));
            format!(
                "The CPU stalls for {total}: {each} to write line {} back, then {each} to fetch \
                 line {}.",
                super::address(geometry, victim.get()),
                super::address(geometry, line.get())
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
        format::Data(u64::from(way.get())),
        super::address(geometry, line.get())
    )
}

/// The `Evict` line: "Evict line 0x1A0 (tag 0x06) from way 1."
pub(crate) fn evict(geometry: &Geometry, way: WayIndex, line: LineAddr, tag: Tag) -> String {
    format!(
        "Evict line {} (tag {}) from way {}.",
        super::address(geometry, line.get()),
        super::tag(geometry, tag),
        format::Data(u64::from(way.get()))
    )
}

/// The `Fill` line: "Fill way 1 with line 0x2A0–0x2AF from memory."
pub(crate) fn fill(geometry: &Geometry, way: WayIndex, line: LineAddr) -> String {
    format!(
        "Fill way {} with line {} from memory.",
        format::Data(u64::from(way.get())),
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
    // The opening clause plus one "; way N is R" or ", way N is R" per other
    // way, sized so the loop below never has to grow the buffer.
    let mut line = String::with_capacity(48 + ranks.len() * 16);
    // Writing to a `String` cannot fail: `String::write_str` never errs, and
    // `format::Data`'s `Display` impl only ever passes on the writer's
    // result, so it cannot return `Err` either.
    let _ = write!(
        line,
        "{order} in set {}: way {} becomes {} ({front})",
        format::Data(u64::from(set.get())),
        format::Data(u64::from(way.get())),
        format::Data(u64::from(promoted))
    );
    let mut joined_any = false;
    for (other, &rank) in (0_u32..).zip(ranks) {
        if other == way.get() {
            continue;
        }
        line.push_str(if joined_any { ", " } else { "; " });
        joined_any = true;
        let _ = write!(
            line,
            "way {} is {}",
            format::Data(u64::from(other)),
            format::Data(u64::from(rank))
        );
    }
    line.push('.');
    line
}

/// A load's `Complete` line: "Read word 1 of way 1 (68) into r3." A load into
/// r0 reads the word and drops it, since r0 always reads 0. A load issued by
/// hand while an assembly program is loaded and has not finished,
/// `beside_program`, says that the program's register changed: "Read word 1
/// of way 0 (53) into r1. A program is loaded, so its r1 now holds 53."
pub(crate) fn load(
    word: WordIndex,
    way: WayIndex,
    value: u64,
    into: Register,
    beside_program: bool,
) -> String {
    let shown = format::Data(value);
    let read = format!(
        "Read word {} of way {} ({shown})",
        format::Data(u64::from(word.get())),
        format::Data(u64::from(way.get()))
    );
    if into == Register::R0 {
        format!("{read}; r0 always reads 0, so the value is dropped.")
    } else if beside_program {
        format!("{read} into {into}. A program is loaded, so its {into} now holds {shown}.")
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
    let value = format::Data(value.get());
    let wrote = format!(
        "Write {value} into word {} of way {}",
        format::Data(u64::from(word.get())),
        format::Data(u64::from(way.get()))
    );
    let (before, stale) = match note {
        WriteNote::Through => return format!("{wrote}."),
        WriteNote::Back { before, stale } => (before, stale),
    };
    let addr = super::address(geometry, addr.get());
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
        format::Data(value.get()),
        super::address(geometry, addr.get())
    )
}

/// The `MemWrite` line of a store that missed under write-no-allocate:
/// "Write-no-allocate: 7 goes straight to memory at 0x0A4. The cache is
/// unchanged."
pub(crate) fn write_around(geometry: &Geometry, addr: Addr, value: Word) -> String {
    format!(
        "Write-no-allocate: {} goes straight to memory at {}. The cache is unchanged.",
        format::Data(value.get()),
        super::address(geometry, addr.get())
    )
}

/// The sentence a store's result line starts with when the register it
/// stores holds more than the word: "r2 = 300 does not fit a 1-byte word, so
/// its low 8 bits, 44, are stored. "
pub(crate) fn truncated(register: Register, value: u64, word_bytes: u32, stored: Word) -> String {
    format!(
        "{register} = {} does not fit {} {}-byte word, so its low {} bits, {}, are stored. ",
        format::data(value),
        format::a_or_an(word_bytes),
        format::setting(word_bytes),
        format::setting(word_bytes.saturating_mul(u8::BITS)),
        format::data(stored.get())
    )
}

/// An `add`'s `Complete` line: "add: r2 ← 176. No memory access." A sum for
/// r0 is computed and dropped.
pub(crate) fn add(into: Register, value: u64) -> String {
    let value = format::data(value);
    if into == Register::R0 {
        format!("add: r0 always reads 0, so the result {value} is dropped.")
    } else {
        format!("add: {into} \u{2190} {value}. No memory access.")
    }
}

/// A `bnez`'s `Complete` line: "bnez: r1 is 3, branch taken to loop." when
/// `taken` names the label, or "bnez: r1 is 0, branch not taken."
pub(crate) fn bnez(test: Register, value: u64, taken: Option<&str>) -> String {
    let value = format::data(value);
    match taken {
        Some(label) => format!("bnez: {test} is {value}, branch taken to {label}."),
        None => format!("bnez: {test} is {value}, branch not taken."),
    }
}

/// The `Error` line of an instruction on line `line` whose address, held in
/// `register`, is `value` and does not fit the address width: "Line 4: r2
/// holds 4096, which does not fit a 12-bit address. The program stopped."
pub(crate) fn fault_out_of_range(
    geometry: &Geometry,
    line: u32,
    register: Register,
    value: u64,
) -> String {
    let bits = geometry.address_bits();
    format!(
        "Line {}: {register} holds {}, which does not fit {} {}-bit address. The program stopped.",
        format::position(line),
        format::data(value),
        format::a_or_an(bits),
        format::setting(bits)
    )
}

/// The `Error` line of an instruction on line `line` whose address, held in
/// `register`, is `addr` and not a multiple of the word size: "Line 4: r2
/// holds 166 (0x0A6), which is not aligned to a 4-byte word. The program
/// stopped."
pub(crate) fn fault_misaligned(
    geometry: &Geometry,
    line: u32,
    register: Register,
    addr: Addr,
) -> String {
    let word_bytes = geometry.word_bytes();
    format!(
        "Line {}: {register} holds {} ({}), which is not aligned to {} {}-byte word. The program \
         stopped.",
        format::position(line),
        format::data(addr.get()),
        super::address(geometry, addr.get()),
        format::a_or_an(word_bytes),
        format::setting(word_bytes)
    )
}

/// The `Error` line that replaces the instruction after the budget's last:
/// "The program used its budget of 10,000 instructions and stopped. Is a loop
/// missing its exit?"
pub(crate) fn out_of_budget() -> String {
    format!(
        "The program used its budget of {} instructions and stopped. Is a loop missing its exit?",
        format::setting(INSTRUCTION_BUDGET)
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
#[cfg(test)]
mod tests {
    #![expect(clippy::disallowed_macros, reason = "assertions are how tests fail")]

    use super::*;
    use crate::config::{Config, Preset, WordBytes};
    use crate::explain::testing::{addr, geometry, line, one_line, preset, set, way, ways};

    fn tag_of(value: u32) -> Tag {
        Tag::new_unchecked(value)
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
    fn a_rank_line_lists_every_other_way_of_a_64_way_set() {
        let ranks: Vec<u32> = (0..64).map(|index| (index + 1) % 64).collect();
        let others = (1..64_u32)
            .map(|other| format!("way {other} is {}", (other + 1) % 64))
            .collect::<Vec<String>>()
            .join(", ");
        let expected = format!("LRU ranks in set 5: way 0 becomes 1 (most recent); {others}.");

        assert_eq!(rank(Replacement::Lru, set(5), way(0), &ranks), expected);
    }

    #[test]
    fn a_load_names_the_word_the_value_and_the_register() {
        assert_eq!(
            load(word(1), way(1), 68, Register::R3, false),
            "Read word 1 of way 1 (68) into r3."
        );
        assert_eq!(
            load(word(0), way(0), 12_345, Register::R0, false),
            "Read word 0 of way 0 (12,345); r0 always reads 0, so the value is dropped."
        );
    }

    #[test]
    fn a_load_issued_beside_a_program_says_the_program_register_changed() {
        assert_eq!(
            load(word(1), way(0), 53, Register::R1, true),
            "Read word 1 of way 0 (53) into r1. A program is loaded, so its r1 now holds 53."
        );
        assert_eq!(
            load(word(1), way(0), 12_345, Register::R1, true),
            "Read word 1 of way 0 (12,345) into r1. A program is loaded, so its r1 now holds \
             12,345."
        );
    }

    #[test]
    fn a_truncated_store_names_the_register_and_the_bits_kept() {
        assert_eq!(
            truncated(Register::R2, 300, 1, value(44)),
            "r2 = 300 does not fit a 1-byte word, so its low 8 bits, 44, are stored. "
        );
        assert_eq!(
            truncated(Register::R7, u64::MAX, 4, value(u64::from(u32::MAX))),
            "r7 = 18,446,744,073,709,551,615 does not fit a 4-byte word, so its low 32 bits, \
             4,294,967,295, are stored. "
        );
        assert_eq!(
            truncated(Register::R1, 70_000, 8, value(70_000)),
            "r1 = 70,000 does not fit an 8-byte word, so its low 64 bits, 70,000, are stored. "
        );
    }

    #[test]
    fn alu_lines_name_the_register_and_the_value() {
        assert_eq!(
            add(Register::R2, 176),
            "add: r2 \u{2190} 176. No memory access."
        );
        assert_eq!(
            add(Register::R1, u64::MAX),
            "add: r1 \u{2190} 18,446,744,073,709,551,615. No memory access."
        );
        assert_eq!(
            add(Register::R0, 5),
            "add: r0 always reads 0, so the result 5 is dropped."
        );
        assert_eq!(
            bnez(Register::R1, 3, Some("loop")),
            "bnez: r1 is 3, branch taken to loop."
        );
        assert_eq!(
            bnez(Register::R1, 0, None),
            "bnez: r1 is 0, branch not taken."
        );
    }

    #[test]
    fn a_fault_names_the_line_the_register_and_its_value() {
        let direct = preset(Preset::DirectMapped8);
        assert_eq!(
            fault_out_of_range(&direct, 4, Register::R2, 4_096),
            "Line 4: r2 holds 4096, which does not fit a 12-bit address. The program stopped."
        );
        assert_eq!(
            fault_out_of_range(&direct, 12, Register::R3, u64::MAX),
            "Line 12: r3 holds 18,446,744,073,709,551,615, which does not fit a 12-bit address. \
             The program stopped."
        );
        assert_eq!(
            fault_misaligned(&direct, 4, Register::R2, addr(0x0A6)),
            "Line 4: r2 holds 166 (0x0A6), which is not aligned to a 4-byte word. The program \
             stopped."
        );
        let eight_bits = geometry(&Config {
            address_bits: 8,
            word_bytes: WordBytes::Eight,
            ..Preset::DirectMapped8.config()
        });
        assert_eq!(
            fault_out_of_range(&eight_bits, 1, Register::R1, 256),
            "Line 1: r1 holds 256, which does not fit an 8-bit address. The program stopped."
        );
        assert_eq!(
            fault_misaligned(&eight_bits, 1, Register::R1, addr(0xA4)),
            "Line 1: r1 holds 164 (0xA4), which is not aligned to an 8-byte word. The program \
             stopped."
        );
    }

    #[test]
    fn the_budget_line_states_the_budget() {
        assert_eq!(
            out_of_budget(),
            format!(
                "The program used its budget of {} instructions and stopped. Is a loop missing \
                 its exit?",
                format::setting(INSTRUCTION_BUDGET)
            )
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
    fn an_internal_error_names_the_invariant() {
        let violation = InvariantViolation::DuplicateTag { set: set(2) };
        assert_eq!(
            internal_error(&violation),
            "Internal error: invariant I3 (no two valid ways in set 2 hold the same tag) no \
             longer holds. The run stopped here; please report it with a share link."
        );
    }
}
