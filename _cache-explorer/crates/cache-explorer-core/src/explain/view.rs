//! The wording of the views: the CPU's status and the Stats lines, the
//! cache's and memory's headings and descriptions, the lanes, the log's
//! entries and headers, the changed summary and the status line.

use super::EntryNoun;
use crate::event::{ChangeRef, Counter, EventKind};
use crate::format;
use crate::geometry::Geometry;
use crate::op::{AccessKind, ProgramFormat};
use crate::session::EntryOutcome;
use crate::units::{Addr, LineAddr, Register, SetIndex, WayIndex, Word};

/// The CPU status for the cursor's place in its entry, which counts steps,
/// one per event: "Access #5 · step 3 of 8", "Edit #6 · step 1 of 1".
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

/// The line beside the hit and miss sparkline: how many of the accesses it
/// shows hit and missed, "1 hit, 2 misses". Once the run has had more
/// accesses than the sparkline shows, `shown` is how many it shows, and the
/// line says the counts are of those: "15 hits, 9 misses (last 24)".
pub(crate) fn recent(hits: u32, misses: u32, shown: Option<u32>) -> String {
    let last = shown
        .map(|shown| format!(" (last {})", format::setting(shown)))
        .unwrap_or_default();
    format!(
        "{} {}, {} {}{last}",
        format::data(hits),
        format::plural(hits, "hit", "hits"),
        format::data(misses),
        format::plural(misses, "miss", "misses")
    )
}

/// The cache table's heading: "L1 cache · 4 sets × 2 ways × 16 B".
pub(crate) fn cache_header(geometry: &Geometry) -> String {
    let (sets, ways) = (geometry.sets(), geometry.ways());
    format!(
        "L1 cache \u{b7} {} {} \u{d7} {} {} \u{d7} {} B",
        format::setting(sets),
        format::plural(sets, "set", "sets"),
        format::setting(ways),
        format::plural(ways, "way", "ways"),
        format::setting(geometry.line_bytes())
    )
}

/// What one way of the cache holds, as its cell describes itself to a
/// screen reader: "Set 2, way 0: valid, tag 0x05, dirty, rank 1, victim." A
/// way has a `tag` when it is valid, and a `rank` when the rank column is
/// shown.
pub(crate) fn way_description(
    set: SetIndex,
    way: WayIndex,
    tag: Option<&str>,
    dirty: bool,
    rank: Option<u32>,
    victim: bool,
) -> String {
    let mut text = format!(
        "Set {}, way {}: ",
        format::data(set.get()),
        format::data(way.get())
    );
    match tag {
        Some(tag) => {
            text.push_str("valid, tag ");
            text.push_str(tag);
        }
        None => text.push_str("invalid"),
    }
    if dirty {
        text.push_str(", dirty");
    }
    if let Some(rank) = rank {
        text.push_str(", rank ");
        text.push_str(&format::data(rank));
    }
    if victim {
        text.push_str(", victim");
    }
    text.push('.');
    text
}

/// The memory table's caption: how many lines the run has touched, "5 lines
/// touched".
pub(crate) fn memory_caption(lines: u32) -> String {
    format!(
        "{} {} touched",
        format::data(lines),
        format::plural(lines, "line", "lines")
    )
}

/// Why a word of memory is marked stale: "Stale: set 2, way 1 holds the
/// newer value 7."
pub(crate) fn stale(set: SetIndex, way: WayIndex, cached: Word) -> String {
    format!(
        "Stale: set {}, way {} holds the newer value {}.",
        format::data(set.get()),
        format::data(way.get()),
        format::data(cached.get())
    )
}

/// What a word of memory holds, as its cell describes itself: "0x0A4 = 53."
/// A stale word goes on with why: "0x1A4 = 0. Stale: set 2, way 1 holds the
/// newer value 7."
pub(crate) fn memory_word(
    geometry: &Geometry,
    addr: Addr,
    value: Word,
    stale: Option<&str>,
) -> String {
    let text = format!(
        "{} = {}.",
        super::address(geometry, addr.get()),
        format::data(value.get())
    );
    match stale {
        Some(stale) => format!("{text} {stale}"),
        None => text,
    }
}

/// The transfer lane while nothing moves.
pub(crate) fn bus_idle() -> String {
    "Bus idle".to_owned()
}

/// The transfer lane at a `Fill`: "Fill · memory line 0x2A0 → set 2, way 1".
pub(crate) fn fill_transfer(
    geometry: &Geometry,
    line: LineAddr,
    set: SetIndex,
    way: WayIndex,
) -> String {
    format!(
        "Fill \u{b7} memory line {} \u{2192} set {}, way {}",
        super::address(geometry, line.get()),
        format::data(set.get()),
        format::data(way.get())
    )
}

/// The transfer lane at a `WriteBack`: "Write-back · set 2, way 1 → memory
/// line 0x1A0".
pub(crate) fn write_back_transfer(
    geometry: &Geometry,
    line: LineAddr,
    set: SetIndex,
    way: WayIndex,
) -> String {
    format!(
        "Write-back \u{b7} set {}, way {} \u{2192} memory line {}",
        format::data(set.get()),
        format::data(way.get()),
        super::address(geometry, line.get())
    )
}

/// The transfer lane at a `WriteThrough`: "Write-through · set 2, way 0 →
/// memory line 0x0A0".
pub(crate) fn write_through_transfer(
    geometry: &Geometry,
    line: LineAddr,
    set: SetIndex,
    way: WayIndex,
) -> String {
    format!(
        "Write-through \u{b7} set {}, way {} \u{2192} memory line {}",
        format::data(set.get()),
        format::data(way.get()),
        super::address(geometry, line.get())
    )
}

/// The transfer lane at a write-around's `MemWrite`, which goes from the CPU
/// straight to memory: "Write-around · CPU → memory line 0x0A0".
pub(crate) fn write_around_transfer(geometry: &Geometry, line: LineAddr) -> String {
    format!(
        "Write-around \u{b7} CPU \u{2192} memory line {}",
        super::address(geometry, line.get())
    )
}

/// The request lane while an access is in the cache: "Load 0x0A4 → cache".
pub(crate) fn request(geometry: &Geometry, kind: AccessKind, addr: Addr) -> String {
    format!("{} \u{2192} cache", typed_request(geometry, kind, addr))
}

/// The request lane for an address typed but not issued: "Load 0x0A4".
pub(crate) fn typed_request(geometry: &Geometry, kind: AccessKind, addr: Addr) -> String {
    let verb = match kind {
        AccessKind::Read => "Load",
        AccessKind::Write => "Store",
    };
    format!("{verb} {}", super::address(geometry, addr.get()))
}

/// The request lane with no request.
pub(crate) fn no_request() -> String {
    "No request".to_owned()
}

/// Where the address strip's address comes from while an access is at the
/// cursor: "from access #5".
pub(crate) fn request_source(entry: u32) -> String {
    format!("from access {}", format::entry_number(entry))
}

/// An access issued by hand, written as the instruction it stands for, as
/// the log's entry and the CPU box name it: `ld r1, [0x0A4]`, and for a
/// store of `value`, `st [0x0A4], #7`.
pub(crate) fn issued(
    geometry: &Geometry,
    addr: Addr,
    into: Register,
    value: Option<u64>,
) -> String {
    let at = super::address(geometry, addr.get());
    match value {
        None => format!("ld {into}, [{at}]"),
        Some(value) => format!("st [{at}], #{}", format::plain(value)),
    }
}

/// An edit of a way, as the log's entry names it: "edit set 2 way 0".
pub(crate) fn edit_way(set: SetIndex, way: WayIndex) -> String {
    format!(
        "edit set {} way {}",
        format::data(set.get()),
        format::data(way.get())
    )
}

/// An edit of a memory word, as the log's entry names it: "edit memory
/// 0x0A4".
pub(crate) fn edit_memory(geometry: &Geometry, addr: Addr) -> String {
    format!("edit memory {}", super::address(geometry, addr.get()))
}

/// A program load, as the log's divider row names it: "Loaded a program of
/// 5 instructions", or for a trace "Loaded a trace of 5 accesses".
pub(crate) fn program_divider(format: ProgramFormat, instructions: u32) -> String {
    let text = super::program_loaded(format, instructions);
    let first = text
        .split_once(". ")
        .map_or(text.as_str(), |(first, _)| first);
    first.to_owned()
}

/// What an entry did, as its header in the log names it: an access's "hit"
/// or "miss", followed by what else it did, as in "miss · evict ·
/// write-back"; "CPU only" for an instruction without a memory access;
/// "edit"; "program" or "trace" for a load; "error". `kinds` are the kinds
/// of the entry's events, and `format` the loaded program's.
pub(crate) fn entry_outcome(
    outcome: EntryOutcome,
    kinds: &[EventKind],
    format: Option<ProgramFormat>,
) -> String {
    let access = |result: &str| {
        let mut parts = vec![result];
        for (kind, name) in [
            (EventKind::Evict, "evict"),
            (EventKind::WriteBack, "write-back"),
            (EventKind::WriteThrough, "write-through"),
            (EventKind::MemWrite, "write-around"),
        ] {
            if kinds.contains(&kind) {
                parts.push(name);
            }
        }
        parts.join(" \u{b7} ")
    };
    match outcome {
        EntryOutcome::Hit => access("hit"),
        EntryOutcome::Miss => access("miss"),
        EntryOutcome::Edit => "edit".to_owned(),
        EntryOutcome::CpuOnly => "CPU only".to_owned(),
        EntryOutcome::Program => {
            super::source_noun(format.unwrap_or(ProgramFormat::Asm)).to_owned()
        }
        EntryOutcome::Error => "error".to_owned(),
    }
}

/// The sentence the page's live region reads after the cursor moves: the
/// log line of the event at the cursor. After a move across more than one
/// event, `entry` gives the entry's number, label and outcome, which open
/// the sentence: `#5 ld r3, [0x2A4]: miss · evict · write-back. Read word 1
/// of way 1 (68) into r3.`
pub(crate) fn live(entry: Option<(u32, &str, &str)>, explain: &str) -> String {
    match entry {
        Some((number, label, outcome)) => format!(
            "{} {label}: {outcome}. {explain}",
            format::entry_number(number)
        ),
        None => explain.to_owned(),
    }
}

/// The log's header: which step of the run the cursor is on, "step 5 of
/// 14", or "no steps yet" while the run has none.
pub(crate) fn log_header(step: u32, steps: u32) -> String {
    if steps == 0 {
        "no steps yet".to_owned()
    } else {
        format!("step {} of {}", format::data(step), format::data(steps))
    }
}

/// The status line over the workbench: the mode and the run's steps, then
/// "timed" in timed mode: "Basics · 14 steps in this run", "Advanced · 1
/// step in this run · timed".
pub(crate) fn status_line(advanced: bool, steps: u32, timed: bool) -> String {
    format!(
        "{} \u{b7} {} {} in this run{}",
        if advanced { "Advanced" } else { "Basics" },
        format::data(steps),
        format::plural(steps, "step", "steps"),
        if timed { " \u{b7} timed" } else { "" }
    )
}

/// The CPU's status where the cursor is on no entry's step, whether or not
/// a program is loaded: it can take the next step or access.
pub(crate) fn ready() -> String {
    "Ready".to_owned()
}

/// The CPU's status once the loaded program has executed its last
/// instruction.
pub(crate) fn finished() -> String {
    "Finished".to_owned()
}

/// The CPU's status once the loaded program has stopped before its end, at
/// the instruction on line `line`: "Stopped at line 4".
pub(crate) fn stopped_at(line: u32) -> String {
    format!("Stopped at line {}", format::position(line))
}

/// What the CPU box shows as its instruction when there is none.
pub(crate) fn no_instruction() -> String {
    format::NO_FIGURE.to_owned()
}

/// The log's "changed" line for `changes`, the cells and counters an entry
/// has changed: ways by set, then way, each with its fields in the order
/// valid, tag, data, dirty, rank; then memory, addresses ascending; then
/// registers; then the counters the student would not otherwise notice, in
/// the Stats region's order: "set 2 way 0 valid, tag, data · r1 · misses",
/// "memory 0x1A4 · misses, write-backs". Every access moves `accesses` and
/// `loads` or `stores`, every miss its class, every fill `fills` and timed
/// runs `cycles`, so those are left out. The pc is left out too. With
/// nothing to list it reads "nothing changed".
pub(crate) fn changed_summary(geometry: &Geometry, changes: &[ChangeRef]) -> String {
    // A way's fields, in the order the line names them.
    const VALID: usize = 0;
    const TAG: usize = 1;
    const DATA: usize = 2;
    const DIRTY: usize = 3;
    const RANK: usize = 4;
    const FIELDS: [&str; 5] = ["valid", "tag", "data", "dirty", "rank"];
    // Each way once, in ascending order, kept sorted as it grows.
    let mut ways: Vec<((SetIndex, WayIndex), [bool; 5])> = Vec::new();
    let mut memory = Vec::new();
    let mut registers = Vec::new();
    let mut counters = Vec::new();
    for &change in changes {
        let (place, field) = match change {
            ChangeRef::Valid { set, way } => ((set, way), VALID),
            ChangeRef::Tag { set, way } => ((set, way), TAG),
            ChangeRef::Word { set, way, .. } => ((set, way), DATA),
            ChangeRef::Dirty { set, way } => ((set, way), DIRTY),
            ChangeRef::Rank { set, way } => ((set, way), RANK),
            ChangeRef::Memory { addr } => {
                memory.push(addr.get());
                continue;
            }
            ChangeRef::Register { register } => {
                registers.push(register);
                continue;
            }
            ChangeRef::Counter { counter } => {
                counters.extend(counter_name(counter));
                continue;
            }
            ChangeRef::Pc => continue,
        };
        let index = ways
            .binary_search_by_key(&place, |&(place, _)| place)
            .unwrap_or_else(|index| {
                ways.insert(index, (place, [false; 5]));
                index
            });
        if let Some(flag) = ways
            .get_mut(index)
            .and_then(|(_, fields)| fields.get_mut(field))
        {
            *flag = true;
        }
    }
    // The addresses sort as plain numbers and the registers go in the order
    // of `Register::ALL`, which is theirs, so that neither type needs a sort,
    // nor the ways a map, of its own in the wasm.
    memory.sort_unstable();
    memory.dedup();
    let registers: Vec<Register> = Register::ALL
        .into_iter()
        .filter(|register| registers.contains(register))
        .collect();
    counters.dedup();
    let mut parts: Vec<String> = ways
        .into_iter()
        .map(|((set, way), fields)| {
            let names: Vec<&str> = FIELDS
                .iter()
                .zip(fields)
                .filter_map(|(name, on)| on.then_some(*name))
                .collect();
            format!(
                "set {} way {} {}",
                format::Data(u64::from(set.get())),
                format::Data(u64::from(way.get())),
                names.join(", ")
            )
        })
        .collect();
    if !memory.is_empty() {
        let addrs: Vec<String> = memory
            .iter()
            .map(|&addr| super::address(geometry, addr).to_string())
            .collect();
        parts.push(format!("memory {}", addrs.join(", ")));
    }
    if !registers.is_empty() {
        let names: Vec<String> = registers.iter().map(ToString::to_string).collect();
        parts.push(names.join(", "));
    }
    if !counters.is_empty() {
        parts.push(counters.join(", "));
    }
    if parts.is_empty() {
        "nothing changed".to_owned()
    } else {
        parts.join(" \u{b7} ")
    }
}

/// A counter as the changed summary names it, or `None` for one it leaves
/// out: the counters every access, miss, fill or timed step moves.
fn counter_name(counter: Counter) -> Option<&'static str> {
    match counter {
        Counter::Hits => Some("hits"),
        Counter::Misses => Some("misses"),
        Counter::Evictions => Some("evictions"),
        Counter::Writebacks => Some("write-backs"),
        Counter::Writethroughs => Some("write-throughs"),
        Counter::Writearounds => Some("write-arounds"),
        Counter::Accesses
        | Counter::Loads
        | Counter::Stores
        | Counter::Fills
        | Counter::Cycles
        | Counter::Compulsory
        | Counter::Capacity
        | Counter::Conflict => None,
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::disallowed_macros, reason = "assertions are how tests fail")]

    use super::*;
    use crate::config::Preset;
    use crate::explain::testing::{addr, line, one_line, preset, set, way};
    use crate::units::WordIndex;

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
    fn the_sparkline_line_counts_hits_and_misses_in_the_singular_and_plural() {
        assert_eq!(recent(1, 2, None), "1 hit, 2 misses");
        assert_eq!(recent(2, 1, None), "2 hits, 1 miss");
        assert_eq!(recent(0, 1, None), "0 hits, 1 miss");
        assert_eq!(recent(1, 0, None), "1 hit, 0 misses");
        assert_eq!(recent(15, 9, Some(24)), "15 hits, 9 misses (last 24)");
        // The number shown is a limit, grouped like a setting.
        assert_eq!(
            recent(1_000, 200, Some(1_200)),
            "1000 hits, 200 misses (last 1,200)"
        );
    }

    #[test]
    fn the_cache_heading_counts_sets_ways_and_line_bytes() {
        assert_eq!(
            cache_header(&preset(Preset::TwoWayLru)),
            "L1 cache \u{b7} 4 sets \u{d7} 2 ways \u{d7} 16 B"
        );
        assert_eq!(
            cache_header(&one_line()),
            "L1 cache \u{b7} 1 set \u{d7} 1 way \u{d7} 16 B"
        );
    }

    #[test]
    fn a_way_describes_what_it_holds() {
        assert_eq!(
            way_description(set(2), way(0), Some("0x05"), true, Some(1), true),
            "Set 2, way 0: valid, tag 0x05, dirty, rank 1, victim."
        );
        assert_eq!(
            way_description(set(2), way(1), None, false, Some(1), false),
            "Set 2, way 1: invalid, rank 1."
        );
        assert_eq!(
            way_description(set(0), way(0), Some("0x01"), false, None, false),
            "Set 0, way 0: valid, tag 0x01."
        );
    }

    #[test]
    fn memory_texts_count_lines_and_name_stale_words() {
        assert_eq!(memory_caption(5), "5 lines touched");
        assert_eq!(memory_caption(1), "1 line touched");
        assert_eq!(memory_caption(0), "0 lines touched");
        let stale_text = stale(set(2), way(1), Word::new_unchecked(7));
        assert_eq!(stale_text, "Stale: set 2, way 1 holds the newer value 7.");
        let geometry = preset(Preset::TwoWayLru);
        assert_eq!(
            memory_word(&geometry, addr(0x0A4), Word::new_unchecked(53), None),
            "0x0A4 = 53."
        );
        assert_eq!(
            memory_word(
                &geometry,
                addr(0x1A4),
                Word::new_unchecked(0),
                Some(&stale_text)
            ),
            "0x1A4 = 0. Stale: set 2, way 1 holds the newer value 7."
        );
    }

    #[test]
    fn the_lanes_name_what_moves_where() {
        let geometry = preset(Preset::TwoWayLru);
        assert_eq!(bus_idle(), "Bus idle");
        assert_eq!(
            fill_transfer(&geometry, line(0x2A0), set(2), way(1)),
            "Fill \u{b7} memory line 0x2A0 \u{2192} set 2, way 1"
        );
        assert_eq!(
            write_back_transfer(&geometry, line(0x1A0), set(2), way(1)),
            "Write-back \u{b7} set 2, way 1 \u{2192} memory line 0x1A0"
        );
        assert_eq!(
            write_through_transfer(&geometry, line(0x0A0), set(2), way(0)),
            "Write-through \u{b7} set 2, way 0 \u{2192} memory line 0x0A0"
        );
        assert_eq!(
            write_around_transfer(&geometry, line(0x0A0)),
            "Write-around \u{b7} CPU \u{2192} memory line 0x0A0"
        );
        assert_eq!(
            request(&geometry, AccessKind::Read, addr(0x0A4)),
            "Load 0x0A4 \u{2192} cache"
        );
        assert_eq!(
            typed_request(&geometry, AccessKind::Write, addr(0x0A4)),
            "Store 0x0A4"
        );
        assert_eq!(no_request(), "No request");
        assert_eq!(request_source(5), "from access #5");
    }

    #[test]
    fn entries_are_named_by_what_they_are() {
        let geometry = preset(Preset::TwoWayLru);
        assert_eq!(
            issued(&geometry, addr(0x0A4), Register::R1, None),
            "ld r1, [0x0A4]"
        );
        assert_eq!(
            issued(&geometry, addr(0x0A4), Register::R1, Some(10_000)),
            "st [0x0A4], #10000"
        );
        assert_eq!(edit_way(set(2), way(0)), "edit set 2 way 0");
        assert_eq!(edit_memory(&geometry, addr(0x0A4)), "edit memory 0x0A4");
        assert_eq!(
            program_divider(ProgramFormat::Asm, 5),
            "Loaded a program of 5 instructions"
        );
        assert_eq!(
            program_divider(ProgramFormat::Trace, 1),
            "Loaded a trace of 1 access"
        );
    }

    #[test]
    fn an_outcome_names_the_result_then_what_else_happened() {
        use EventKind::{Complete, Decode, Evict, Fill, Hit, MemWrite, Miss, Victim, WriteBack};
        let dirty = [Decode, Miss, Victim, WriteBack, Evict, Fill, Complete];
        assert_eq!(
            entry_outcome(EntryOutcome::Miss, &dirty, None),
            "miss \u{b7} evict \u{b7} write-back"
        );
        assert_eq!(
            entry_outcome(EntryOutcome::Hit, &[Decode, Hit], None),
            "hit"
        );
        assert_eq!(
            entry_outcome(EntryOutcome::Miss, &[Decode, Miss, MemWrite], None),
            "miss \u{b7} write-around"
        );
        let through = [Decode, Hit, EventKind::Write, EventKind::WriteThrough];
        assert_eq!(
            entry_outcome(EntryOutcome::Hit, &through, None),
            "hit \u{b7} write-through"
        );
        assert_eq!(
            entry_outcome(EntryOutcome::CpuOnly, &[Complete], None),
            "CPU only"
        );
        assert_eq!(entry_outcome(EntryOutcome::Edit, &[], None), "edit");
        assert_eq!(entry_outcome(EntryOutcome::Error, &[], None), "error");
        assert_eq!(
            entry_outcome(EntryOutcome::Program, &[], Some(ProgramFormat::Trace)),
            "trace"
        );
        assert_eq!(
            entry_outcome(EntryOutcome::Program, &[], Some(ProgramFormat::Asm)),
            "program"
        );
    }

    #[test]
    fn the_live_sentence_opens_with_the_entry_after_a_jump() {
        let line = "Read word 1 of way 1 (68) into r3.";
        assert_eq!(live(None, line), line);
        assert_eq!(
            live(
                Some((5, "ld r3, [0x2A4]", "miss \u{b7} evict \u{b7} write-back")),
                line
            ),
            "#5 ld r3, [0x2A4]: miss \u{b7} evict \u{b7} write-back. Read word 1 of way 1 (68) \
             into r3."
        );
    }

    #[test]
    fn headers_and_status_lines_count_steps() {
        assert_eq!(log_header(5, 14), "step 5 of 14");
        assert_eq!(log_header(0, 14), "step 0 of 14");
        assert_eq!(log_header(0, 0), "no steps yet");
        assert_eq!(log_header(12_000, 12_345), "step 12,000 of 12,345");
        assert_eq!(
            status_line(false, 14, false),
            "Basics \u{b7} 14 steps in this run"
        );
        assert_eq!(
            status_line(true, 1, true),
            "Advanced \u{b7} 1 step in this run \u{b7} timed"
        );
        assert_eq!(ready(), "Ready");
        assert_eq!(finished(), "Finished");
        assert_eq!(stopped_at(4), "Stopped at line 4");
        assert_eq!(no_instruction(), "\u{2014}");
    }

    #[test]
    fn the_changed_summary_lists_ways_memory_registers_and_counters_in_order() {
        let geometry = preset(Preset::TwoWayLru);
        let counter = |counter| ChangeRef::Counter { counter };
        let changes = [
            ChangeRef::Valid {
                set: set(2),
                way: way(0),
            },
            ChangeRef::Tag {
                set: set(2),
                way: way(0),
            },
            ChangeRef::Rank {
                set: set(2),
                way: way(1),
            },
            ChangeRef::Word {
                set: set(2),
                way: way(0),
                word: WordIndex::new_unchecked(1),
            },
            ChangeRef::Word {
                set: set(2),
                way: way(0),
                word: WordIndex::new_unchecked(2),
            },
            ChangeRef::Memory { addr: addr(0x1A8) },
            ChangeRef::Memory { addr: addr(0x1A4) },
            ChangeRef::Register {
                register: Register::R3,
            },
            ChangeRef::Register {
                register: Register::R1,
            },
            ChangeRef::Pc,
            counter(Counter::Accesses),
            counter(Counter::Loads),
            counter(Counter::Misses),
            counter(Counter::Evictions),
            counter(Counter::Writebacks),
            counter(Counter::Fills),
            counter(Counter::Cycles),
            counter(Counter::Conflict),
        ];
        assert_eq!(
            changed_summary(&geometry, &changes),
            "set 2 way 0 valid, tag, data \u{b7} set 2 way 1 rank \u{b7} memory 0x1A4, 0x1A8 \
             \u{b7} r1, r3 \u{b7} misses, evictions, write-backs"
        );
        assert_eq!(
            changed_summary(
                &geometry,
                &[
                    ChangeRef::Register {
                        register: Register::R2
                    },
                    counter(Counter::Accesses),
                    counter(Counter::Hits)
                ]
            ),
            "r2 \u{b7} hits"
        );
        assert_eq!(
            changed_summary(
                &geometry,
                &[
                    ChangeRef::Memory { addr: addr(0x0A4) },
                    counter(Counter::Misses),
                    counter(Counter::Writearounds),
                    counter(Counter::Compulsory)
                ]
            ),
            "memory 0x0A4 \u{b7} misses, write-arounds"
        );
        assert_eq!(
            changed_summary(&geometry, &[ChangeRef::Pc]),
            "nothing changed"
        );
        assert_eq!(changed_summary(&geometry, &[]), "nothing changed");
    }
}
