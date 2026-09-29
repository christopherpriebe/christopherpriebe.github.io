//! The wording of the editors: each rule's title and text, the preview of
//! an edit, the labels of its actions and the log line of an applied edit.

use super::EntryNoun;
use crate::config::Replacement;
use crate::format;
use crate::geometry::{Geometry, Placement};
use crate::units::{Addr, LineAddr, SetIndex, Tag, WayIndex, Word, WordIndex};

/// Why an edit waits: the cursor is inside an op, which undoes as a whole,
/// so an edit would apply after the op's end rather than where the editor
/// shows the machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unfinished {
    /// The cursor is inside the op's last entry, `noun` `number`.
    Entry {
        /// What the entry is.
        noun: EntryNoun,
        /// Its number within the run.
        number: u32,
    },
    /// The op executed several instructions, entries `first` to `last`, the
    /// last of which may be the error that ended it, and the cursor is
    /// before the end of the last, inside an access or not: the op ends only
    /// there.
    Entries {
        /// The number of the op's first entry.
        first: u32,
        /// The number of its last.
        last: u32,
        /// Whether the last is the error that ended the op rather than an
        /// instruction.
        error: bool,
    },
}

/// The title of an edit refused while an op is unfinished: "Finish this
/// access first", or "Finish these instructions first".
pub(crate) fn unfinished_title(unfinished: Unfinished) -> String {
    match unfinished {
        Unfinished::Entry { noun, .. } => format!("Finish this {} first", noun_name(noun)),
        Unfinished::Entries { .. } => "Finish these instructions first".to_owned(),
    }
}

/// The text of an edit refused while an op is unfinished: "Step to the end
/// of access #5 (Shift+Space), then edit.", or "Instructions #3 to #5 were
/// executed together and undo together. Go to the end of #5 (Shift+Space),
/// then edit." An op that ended in an error names it apart: "Instructions #3
/// to #4 and error #5 were …", or "Instruction #4 and error #5 were …".
pub(crate) fn unfinished_text(unfinished: Unfinished) -> String {
    match unfinished {
        Unfinished::Entry { noun, number } => format!(
            "Step to the end of {} {} (Shift+Space), then edit.",
            noun_name(noun),
            format::entry_number(number)
        ),
        Unfinished::Entries { first, last, error } => {
            let number = format::entry_number;
            let together = match (error, last.saturating_sub(1)) {
                (false, _) => format!("Instructions {} to {}", number(first), number(last)),
                (true, before) if before == first => {
                    format!("Instruction {} and error {}", number(first), number(last))
                }
                (true, before) => format!(
                    "Instructions {} to {} and error {}",
                    number(first),
                    number(before),
                    number(last)
                ),
            };
            format!(
                "{together} were executed together and undo together. Go to the end of {} \
                 (Shift+Space), then edit.",
                number(last)
            )
        }
    }
}

/// An entry's noun inside a sentence.
fn noun_name(noun: EntryNoun) -> &'static str {
    match noun {
        EntryNoun::Access => "access",
        EntryNoun::Instruction => "instruction",
        EntryNoun::Edit => "edit",
        EntryNoun::Error => "error",
    }
}

/// The title of an edit that can be applied.
pub(crate) fn edit_ready() -> String {
    "Ready to apply".to_owned()
}

/// The title of an edit refused with nothing to offer instead.
pub(crate) fn edit_refused() -> String {
    "Refused".to_owned()
}

/// The title of an edit that changes nothing.
pub(crate) fn no_changes() -> String {
    "No changes".to_owned()
}

/// What to do in a way editor whose fields are as they were.
pub(crate) fn no_way_change() -> String {
    "Edit a field to preview what will change.".to_owned()
}

/// What to do in a memory editor whose value is as it was.
pub(crate) fn no_memory_change() -> String {
    "Edit the value to preview what will change.".to_owned()
}

/// The text of an edit of a way that moves ranks.
pub(crate) fn ranks_shift() -> String {
    "Ranks stay a permutation: the other ways shift to make room.".to_owned()
}

/// The text of an edit of a way that moves no rank.
pub(crate) fn recorded_as_step() -> String {
    "This is recorded as a step, so Undo reverts it.".to_owned()
}

/// Why an edit of a set the cache does not have is refused: "This cache has
/// no set 4; its sets are 0–3."
pub(crate) fn no_such_set(geometry: &Geometry, set: SetIndex) -> String {
    format!(
        "This cache has no set {}; {}.",
        format::data(set.get()),
        numbered("set", "sets", geometry.sets())
    )
}

/// Why an edit of a way a set does not have is refused: "Set 2 has no way
/// 3; its ways are 0–1."
pub(crate) fn no_such_way(geometry: &Geometry, set: SetIndex, way: WayIndex) -> String {
    format!(
        "Set {} has no way {}; {}.",
        format::data(set.get()),
        format::data(way.get()),
        numbered("way", "ways", geometry.ways())
    )
}

/// The numbers of `count` sets or ways: "its only set is 0", "its ways are
/// 0–1".
fn numbered(one: &str, many: &str, count: u32) -> String {
    match count.checked_sub(1) {
        Some(last) if last > 0 => format!("its {many} are 0\u{2013}{}", format::data(last)),
        _ => format!("its only {one} is 0"),
    }
}

/// Why an edit whose patch is of the other kind than its target is refused.
/// `way` is whether the target is a way.
pub(crate) fn kind_mismatch(way: bool) -> String {
    if way {
        "A way takes a way's fields, not a memory value.".to_owned()
    } else {
        "A memory word takes a value, not a way's fields.".to_owned()
    }
}

/// Why a way's data with the wrong number of words is refused: "Each line
/// holds 4 words; this data has 3."
pub(crate) fn data_length(geometry: &Geometry, given: u32) -> String {
    let words = geometry.words_per_line();
    format!(
        "Each line holds {} {}; this data has {}.",
        format::data(words),
        format::plural(words, "word", "words"),
        format::data(given)
    )
}

/// Why a rank that no way of a set can have is refused: "Set 2 has 2 ways,
/// so its ranks are 0–1."
pub(crate) fn rank_too_high(geometry: &Geometry, set: SetIndex) -> String {
    let ways = geometry.ways();
    let ranks = match ways.checked_sub(1) {
        Some(last) if last > 0 => format!("its ranks are 0\u{2013}{}", format::data(last)),
        _ => "its only rank is 0".to_owned(),
    };
    format!(
        "Set {} has {} {}, so {ranks}.",
        format::data(set.get()),
        format::data(ways),
        format::plural(ways, "way", "ways")
    )
}

/// The title of an edit that would leave a way empty and still gives it
/// fields.
pub(crate) fn empty_way_title() -> String {
    "The way would be empty".to_owned()
}

/// Why an empty way cannot be given fields.
pub(crate) fn empty_way() -> String {
    "An empty way has no tag, data, dirty bit or rank.".to_owned()
}

/// The title of an edit that makes a way valid without saying what it
/// holds.
pub(crate) fn missing_line_title() -> String {
    "No line address".to_owned()
}

/// What an edit that makes a way valid needs.
pub(crate) fn missing_line() -> String {
    "Give the address of the line this way should hold.".to_owned()
}

/// The title of an edit that gives a way a line of another set: "0x0C0
/// belongs in set 0".
pub(crate) fn belongs_in(geometry: &Geometry, line: LineAddr, set: SetIndex) -> String {
    format!(
        "{} belongs in set {}",
        super::address(geometry, line.get()),
        format::data(set.get())
    )
}

/// Why a line that another set already holds cannot go into a way: "0x0C0
/// is already in set 0, way 1."
pub(crate) fn already_in(
    geometry: &Geometry,
    line: LineAddr,
    set: SetIndex,
    way: WayIndex,
) -> String {
    format!(
        "{} is already in set {}, way {}.",
        super::address(geometry, line.get()),
        format::data(set.get()),
        format::data(way.get())
    )
}

/// Why a line of set `index` cannot go into set `set`, while way `way` of
/// its own set is empty: "Its index bits are 00, so it cannot live in set 3.
/// Set 0 has an empty way 0."
pub(crate) fn wrong_set(
    geometry: &Geometry,
    index: SetIndex,
    set: SetIndex,
    way: WayIndex,
) -> String {
    format!(
        "Its index bits are {}, so it cannot live in set {}. Set {} has an empty way {}.",
        format::bits(index.get(), geometry.index_bits()),
        format::data(set.get()),
        format::data(index.get()),
        format::data(way.get())
    )
}

/// Why a line of set `index` cannot go into set `set` when its own set is
/// full: "0x120 has index 2, so it can only live in set 2, and every way of
/// set 2 is valid. Evict a line there first, or choose an address with index
/// 3."
pub(crate) fn wrong_set_full(
    geometry: &Geometry,
    line: LineAddr,
    index: SetIndex,
    set: SetIndex,
) -> String {
    let index = format::data(index.get());
    format!(
        "{} has index {index}, so it can only live in set {index}, and every way of set \
         {index} is valid. Evict a line there first, or choose an address with index {}.",
        super::address(geometry, line.get()),
        format::data(set.get())
    )
}

/// The title of an edit that gives a way a line another way of its set
/// holds: "0x0A0 is already cached".
pub(crate) fn already_cached(geometry: &Geometry, line: LineAddr) -> String {
    format!("{} is already cached", super::address(geometry, line.get()))
}

/// Why a line another way of the set holds cannot go into a way too: "Set 2
/// already holds 0x0A0 in way 0."
pub(crate) fn already_holds(
    geometry: &Geometry,
    set: SetIndex,
    line: LineAddr,
    way: WayIndex,
) -> String {
    format!(
        "Set {} already holds {} in way {}.",
        format::data(set.get()),
        super::address(geometry, line.get()),
        format::data(way.get())
    )
}

/// The title of an edit that sets a dirty bit under write-through.
pub(crate) fn never_dirty_title() -> String {
    "No dirty lines under write-through".to_owned()
}

/// Why no line is dirty under write-through.
pub(crate) fn never_dirty() -> String {
    "Under write-through lines are never dirty: every store already reached memory.".to_owned()
}

/// The title of an edit that leaves a clean line's data other than memory's.
pub(crate) fn data_differs() -> String {
    "Data differs from memory".to_owned()
}

/// Why a clean line under write-back cannot differ from memory, naming the
/// word: "Word 2 is 40 here but 21 in memory. A clean line must match
/// memory, so this edit needs the dirty bit."
pub(crate) fn needs_dirty(word: WordIndex, value: Word, memory: Word) -> String {
    format!(
        "Word {} is {} here but {} in memory. A clean line must match memory, so this edit \
         needs the dirty bit.",
        format::data(word.get()),
        format::data(value.get()),
        format::data(memory.get())
    )
}

/// Why a line under write-through cannot differ from memory.
pub(crate) fn write_through_agrees() -> String {
    "Under write-through the cache and memory always agree.".to_owned()
}

/// Why a rank cannot be edited: random replacement keeps no order, and a
/// one-way set has only rank 0, "A direct-mapped set has only rank 0.", or
/// for the one-line cache "A one-line cache has only rank 0."
pub(crate) fn rank_fixed(geometry: &Geometry, replacement: Replacement) -> String {
    if geometry.ways() > 1 && replacement == Replacement::Random {
        "Random replacement keeps no order to edit.".to_owned()
    } else if geometry.placement() == Placement::FullyAssociative {
        "A one-line cache has only rank 0.".to_owned()
    } else {
        "A direct-mapped set has only rank 0.".to_owned()
    }
}

/// The title of an edit that ranks a way among the empty ones.
pub(crate) fn rank_out_of_range_title() -> String {
    "Rank out of range".to_owned()
}

/// Why a valid way must rank below the empty ways, when its set will have
/// `valid` valid ways: "With 2 valid ways, ranks 0–1 belong to valid ways;
/// empty ways always rank last."
pub(crate) fn rank_out_of_range(valid: u32) -> String {
    match valid.checked_sub(1) {
        Some(last) if last > 0 => format!(
            "With {} valid ways, ranks 0\u{2013}{} belong to valid ways; empty ways always \
             rank last.",
            format::data(valid),
            format::data(last)
        ),
        _ => "With 1 valid way, rank 0 belongs to it; empty ways always rank last.".to_owned(),
    }
}

/// The caution of an edit that drops a dirty line's newer words: "Way 1 is
/// dirty: invalidating it discards 7 at 0x1A4; memory keeps 12.", or when
/// the way is given `line` instead, "Way 0 is dirty: giving it line 0x1B0
/// discards 5 at 0x0B0; memory keeps 92." Each word is its address, the
/// newer value the way holds and memory's.
pub(crate) fn discards(
    geometry: &Geometry,
    way: WayIndex,
    line: Option<LineAddr>,
    words: &[(Addr, Word, Word)],
) -> String {
    let doing = match line {
        Some(line) => format!("giving it line {}", super::address(geometry, line.get())),
        None => "invalidating it".to_owned(),
    };
    let discarded: Vec<String> = words
        .iter()
        .map(|(addr, cached, _)| {
            format!(
                "{} at {}",
                format::data(cached.get()),
                super::address(geometry, addr.get())
            )
        })
        .collect();
    let kept: Vec<String> = words
        .iter()
        .map(|(_, _, memory)| format::data(memory.get()))
        .collect();
    format!(
        "Way {} is dirty: {doing} discards {}; memory keeps {}.",
        format::data(way.get()),
        listed(&discarded),
        listed(&kept)
    )
}

/// `items` in a sentence: "a", "a and b", "a, b and c".
fn listed(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The title of a memory edit no cached line holds.
pub(crate) fn memory_only_title() -> String {
    "Memory only".to_owned()
}

/// What a memory edit no cached line holds changes.
pub(crate) fn memory_only() -> String {
    "No cache line holds this word, so only memory changes.".to_owned()
}

/// The title of a memory edit of a word a clean line holds: "Set 2, way 0
/// holds this word, clean".
pub(crate) fn clean_copy_title(set: SetIndex, way: WayIndex) -> String {
    format!(
        "Set {}, way {} holds this word, clean",
        format::data(set.get()),
        format::data(way.get())
    )
}

/// What a memory edit of a word a clean line holds changes.
pub(crate) fn clean_copy() -> String {
    "A clean line must match memory, so the cached copy is updated as well.".to_owned()
}

/// The title of a memory edit of a word a dirty line holds with another
/// value: "Set 3, way 0 holds a newer value".
pub(crate) fn newer_value_title(set: SetIndex, way: WayIndex) -> String {
    format!(
        "Set {}, way {} holds a newer value",
        format::data(set.get()),
        format::data(way.get())
    )
}

/// Why a memory edit of a word a dirty line holds with the value `cached`
/// leaves it stale: "The dirty line there has 5 for this word. Editing
/// memory does not change that; the word stays stale until the line is
/// written back." Unless `was_stale`, the line held memory's value until the
/// edit, and the word "becomes stale" instead.
pub(crate) fn newer_value(cached: Word, was_stale: bool) -> String {
    format!(
        "The dirty line there has {} for this word. Editing memory does not change that; the \
         word {} stale until the line is written back.",
        format::data(cached.get()),
        if was_stale { "stays" } else { "becomes" }
    )
}

/// The title of a memory edit that gives a word the value a dirty line
/// holds for it: "Set 3, way 0 holds this value".
pub(crate) fn matches_dirty_title(set: SetIndex, way: WayIndex) -> String {
    format!(
        "Set {}, way {} holds this value",
        format::data(set.get()),
        format::data(way.get())
    )
}

/// What a memory edit that matches a dirty line does.
pub(crate) fn matches_dirty() -> String {
    "Memory now matches the dirty line, so the word is no longer stale.".to_owned()
}

/// The title of a way editor whose line address does not read.
pub(crate) fn not_an_address() -> String {
    "Not an address".to_owned()
}

/// The title of a way editor whose data input `word` does not read: "Check
/// word 2".
pub(crate) fn check_word(word: WordIndex) -> String {
    format!("Check word {}", format::data(word.get()))
}

/// The title of a memory editor whose value does not read.
pub(crate) fn check_value() -> String {
    "Check the value".to_owned()
}

/// An editor's action: the primary button, or a suggestion's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditAction {
    /// Apply an edit of a way, or of a memory word no line holds.
    Apply,
    /// Apply a memory edit that a dirty line does not follow.
    ApplyToMemory,
    /// Apply a memory edit and its clean cached copy.
    ApplyToBoth,
    /// Invalidate the way and nothing else.
    JustInvalidate,
    /// Put the line into way `way` of set `set`, its own.
    MoveTo {
        /// The line's set.
        set: SetIndex,
        /// Its empty way.
        way: WayIndex,
    },
    /// Edit way `way` instead, of set `set` when that is another set.
    EditInstead {
        /// The other set, if it is not the target's.
        set: Option<SetIndex>,
        /// The way.
        way: WayIndex,
    },
    /// Apply with the dirty bit clear.
    ClearDirty,
    /// Apply with the dirty bit set.
    SetDirty,
    /// Edit the memory word at `addr` instead.
    EditMemory {
        /// The word's address.
        addr: Addr,
    },
    /// Apply with rank `rank`.
    UseRank {
        /// The rank.
        rank: u32,
    },
}

/// The label of `action`: "Apply", "Move to set 0, way 0", "Edit memory
/// 0x0A4 instead".
pub(crate) fn edit_action(geometry: &Geometry, action: EditAction) -> String {
    match action {
        EditAction::Apply => "Apply".to_owned(),
        EditAction::ApplyToMemory => "Apply to memory".to_owned(),
        EditAction::ApplyToBoth => "Apply to both".to_owned(),
        EditAction::JustInvalidate => "Just invalidate it".to_owned(),
        EditAction::MoveTo { set, way } => format!(
            "Move to set {}, way {}",
            format::data(set.get()),
            format::data(way.get())
        ),
        EditAction::EditInstead {
            set: Some(set),
            way,
        } => format!(
            "Edit set {}, way {} instead",
            format::data(set.get()),
            format::data(way.get())
        ),
        EditAction::EditInstead { set: None, way } => {
            format!("Edit way {} instead", format::data(way.get()))
        }
        EditAction::ClearDirty => "Clear dirty and apply".to_owned(),
        EditAction::SetDirty => "Set dirty and apply".to_owned(),
        EditAction::EditMemory { addr } => {
            format!(
                "Edit memory {} instead",
                super::address(geometry, addr.get())
            )
        }
        EditAction::UseRank { rank } => format!("Use rank {}", format::data(rank)),
    }
}

/// One change an edit of a way makes, as its preview lists it and its log
/// line names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WayItem {
    /// The valid bit becomes `to`.
    Valid {
        /// The new bit.
        to: bool,
    },
    /// The way takes the line with tag `to`, in place of `from`'s.
    Tag {
        /// The tag it held; none while it was invalid.
        from: Option<Tag>,
        /// The tag it holds.
        to: Tag,
    },
    /// Word `word` goes from `from` to `to`; `from` is memory's copy for a
    /// line the way takes.
    Word {
        /// The word within the line.
        word: WordIndex,
        /// Its value before.
        from: Word,
        /// Its value after.
        to: Word,
    },
    /// The dirty bit becomes `to`.
    Dirty {
        /// The new bit.
        to: bool,
    },
    /// Way `way`'s rank goes from `from` to `to`.
    Rank {
        /// The way.
        way: WayIndex,
        /// Its rank before.
        from: u32,
        /// Its rank after.
        to: u32,
    },
}

/// One line of an edit's preview: "valid 0 → 1", "tag — → 0x03", "word 1:
/// 53 → 9", "dirty 0 → 1", "rank way 1: 1 → 0".
pub(crate) fn way_item(geometry: &Geometry, item: WayItem) -> String {
    match item {
        WayItem::Valid { to } => format!("valid {} \u{2192} {}", format::bit(!to), format::bit(to)),
        WayItem::Tag { from, to } => format!(
            "tag {} \u{2192} {}",
            from.map_or_else(
                || format::NO_FIGURE.to_owned(),
                |from| super::tag(geometry, from).to_string()
            ),
            super::tag(geometry, to)
        ),
        WayItem::Word { word, from, to } => format!(
            "word {}: {} \u{2192} {}",
            format::data(word.get()),
            format::data(from.get()),
            format::data(to.get())
        ),
        WayItem::Dirty { to } => format!("dirty {} \u{2192} {}", format::bit(!to), format::bit(to)),
        WayItem::Rank { way, from, to } => format!(
            "rank way {}: {} \u{2192} {}",
            format::data(way.get()),
            format::data(from),
            format::data(to)
        ),
    }
}

/// The `Edit` line of an edit of way `way` of set `set` that makes the
/// changes `items`, the target's rank before any other's: "Edited set 2,
/// way 0: word 1: 53 → 9; dirty 0 → 1; rank 0 → 1 (way 1: 1 → 0)."
pub(crate) fn edited_way(
    geometry: &Geometry,
    set: SetIndex,
    way: WayIndex,
    items: &[WayItem],
) -> String {
    let mut parts = Vec::new();
    let mut others = Vec::new();
    let mut own = None;
    for &item in items {
        match item {
            WayItem::Rank {
                way: moved,
                from,
                to,
            } if moved == way => own = Some((from, to)),
            WayItem::Rank {
                way: moved,
                from,
                to,
            } => others.push(format!(
                "way {}: {} \u{2192} {}",
                format::Data(u64::from(moved.get())),
                format::Data(u64::from(from)),
                format::Data(u64::from(to))
            )),
            item => parts.push(way_item(geometry, item)),
        }
    }
    match own {
        Some((from, to)) if others.is_empty() => parts.push(format!(
            "rank {} \u{2192} {}",
            format::Data(u64::from(from)),
            format::Data(u64::from(to))
        )),
        Some((from, to)) => parts.push(format!(
            "rank {} \u{2192} {} ({})",
            format::Data(u64::from(from)),
            format::Data(u64::from(to)),
            others.join(", ")
        )),
        None => parts.extend(others.iter().map(|other| format!("rank {other}"))),
    }
    format!(
        "Edited set {}, way {}: {}.",
        format::Data(u64::from(set.get())),
        format::Data(u64::from(way.get())),
        parts.join("; ")
    )
}

/// One line of a memory edit's preview for the word itself: "memory 0x0B0:
/// 92 → 12".
pub(crate) fn memory_item(geometry: &Geometry, addr: Addr, from: Word, to: Word) -> String {
    format!(
        "memory {}: {} \u{2192} {}",
        super::address(geometry, addr.get()),
        format::data(from.get()),
        format::data(to.get())
    )
}

/// One line of a memory edit's preview for the clean cached copy it updates:
/// "set 2 way 0 word 1: 53 → 12".
pub(crate) fn cached_item(
    set: SetIndex,
    way: WayIndex,
    word: WordIndex,
    from: Word,
    to: Word,
) -> String {
    format!(
        "set {} way {} word {}: {} \u{2192} {}",
        format::data(set.get()),
        format::data(way.get()),
        format::data(word.get()),
        format::data(from.get()),
        format::data(to.get())
    )
}

/// What a memory edit does to the line that holds its word, as its log line
/// adds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemoryNote {
    /// No line holds the word.
    MemoryOnly,
    /// The clean line in way `way` of set `set` was updated too.
    CleanCopy {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
    },
    /// The dirty line in way `way` of set `set` keeps another value, as it
    /// did before the edit.
    StaysStale {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
    },
    /// The dirty line in way `way` of set `set` held memory's value until
    /// the edit, and keeps it.
    BecomesStale {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
    },
    /// Memory now matches the dirty line in way `way` of set `set`.
    MatchesDirty {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
    },
}

/// The `Edit` line of a memory edit: "Edited memory 0x0B0: 92 → 12.", then
/// what happened to the line that holds the word: " The clean copy in set
/// 3, way 0 was updated too." or " Set 3, way 0 holds this line dirty, so
/// the word stays stale." ("becomes stale" when it was not)
pub(crate) fn edited_memory(
    geometry: &Geometry,
    addr: Addr,
    from: Word,
    to: Word,
    note: MemoryNote,
) -> String {
    let edited = format!(
        "Edited memory {}: {} \u{2192} {}.",
        super::address(geometry, addr.get()),
        format::data(from.get()),
        format::data(to.get())
    );
    let place = |set: SetIndex, way: WayIndex| {
        format!(
            "set {}, way {}",
            format::data(set.get()),
            format::data(way.get())
        )
    };
    match note {
        MemoryNote::MemoryOnly => edited,
        MemoryNote::CleanCopy { set, way } => {
            format!(
                "{edited} The clean copy in {} was updated too.",
                place(set, way)
            )
        }
        MemoryNote::StaysStale { set, way } => format!(
            "{edited} Set {}, way {} holds this line dirty, so the word stays stale.",
            format::data(set.get()),
            format::data(way.get())
        ),
        MemoryNote::BecomesStale { set, way } => format!(
            "{edited} Set {}, way {} holds this line dirty, so the word becomes stale.",
            format::data(set.get()),
            format::data(way.get())
        ),
        MemoryNote::MatchesDirty { set, way } => format!(
            "{edited} Memory now matches the dirty line in {}.",
            place(set, way)
        ),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::{CacheConfig, Config, Preset};

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

    fn set(index: u32) -> SetIndex {
        SetIndex::new_unchecked(index)
    }

    fn way(index: u32) -> WayIndex {
        WayIndex::new_unchecked(index)
    }

    #[test]
    fn an_unfinished_op_says_what_to_finish_first() {
        let access = Unfinished::Entry {
            noun: EntryNoun::Access,
            number: 5,
        };
        assert_eq!(unfinished_title(access), "Finish this access first");
        assert_eq!(
            unfinished_text(access),
            "Step to the end of access #5 (Shift+Space), then edit."
        );
        let several = Unfinished::Entries {
            first: 3,
            last: 5,
            error: false,
        };
        assert_eq!(unfinished_title(several), "Finish these instructions first");
        assert_eq!(
            unfinished_text(several),
            "Instructions #3 to #5 were executed together and undo together. Go to the end of \
             #5 (Shift+Space), then edit."
        );
        let stopped = Unfinished::Entries {
            first: 3,
            last: 5,
            error: true,
        };
        assert_eq!(unfinished_title(stopped), "Finish these instructions first");
        assert_eq!(
            unfinished_text(stopped),
            "Instructions #3 to #4 and error #5 were executed together and undo together. Go to \
             the end of #5 (Shift+Space), then edit."
        );
        let faulted = Unfinished::Entries {
            first: 4,
            last: 5,
            error: true,
        };
        assert_eq!(
            unfinished_text(faulted),
            "Instruction #4 and error #5 were executed together and undo together. Go to the end \
             of #5 (Shift+Space), then edit."
        );
        let error = Unfinished::Entry {
            noun: EntryNoun::Error,
            number: 1,
        };
        assert_eq!(unfinished_title(error), "Finish this error first");
        assert_eq!(
            unfinished_text(error),
            "Step to the end of error #1 (Shift+Space), then edit."
        );
    }

    #[test]
    fn editor_messages_count_in_the_singular_and_the_plural() {
        let two_way = preset(Preset::TwoWayLru);
        let direct = preset(Preset::DirectMapped8);
        let one_line = one_line();
        assert_eq!(
            no_such_set(&two_way, set(4)),
            "This cache has no set 4; its sets are 0\u{2013}3."
        );
        assert_eq!(
            no_such_set(&one_line, set(1)),
            "This cache has no set 1; its only set is 0."
        );
        assert_eq!(
            no_such_way(&direct, set(2), way(1)),
            "Set 2 has no way 1; its only way is 0."
        );
        assert_eq!(
            rank_too_high(&two_way, set(2)),
            "Set 2 has 2 ways, so its ranks are 0\u{2013}1."
        );
        assert_eq!(
            rank_too_high(&direct, set(2)),
            "Set 2 has 1 way, so its only rank is 0."
        );
        assert_eq!(
            data_length(&two_way, 1),
            "Each line holds 4 words; this data has 1."
        );
        assert_eq!(
            rank_out_of_range(1),
            "With 1 valid way, rank 0 belongs to it; empty ways always rank last."
        );
        assert_eq!(
            rank_out_of_range(4),
            "With 4 valid ways, ranks 0\u{2013}3 belong to valid ways; empty ways always rank \
             last."
        );
        assert_eq!(
            rank_fixed(&ways(2), Replacement::Random),
            "Random replacement keeps no order to edit."
        );
        assert_eq!(
            rank_fixed(&direct, Replacement::Random),
            "A direct-mapped set has only rank 0."
        );
        assert_eq!(
            rank_fixed(&one_line, Replacement::Lru),
            "A one-line cache has only rank 0."
        );
    }

    #[test]
    fn a_discarded_dirty_line_names_each_word_it_drops() {
        let geometry = preset(Preset::TwoWayLru);
        let word = Word::new_unchecked;
        assert_eq!(
            discards(&geometry, way(1), None, &[(addr(0x1A4), word(7), word(12))]),
            "Way 1 is dirty: invalidating it discards 7 at 0x1A4; memory keeps 12."
        );
        assert_eq!(
            discards(
                &geometry,
                way(0),
                Some(LineAddr::new_unchecked(0x1B0)),
                &[
                    (addr(0x0B0), word(5), word(92)),
                    (addr(0x0B4), word(6), word(8)),
                    (addr(0x0BC), word(10_000), word(34)),
                ]
            ),
            "Way 0 is dirty: giving it line 0x1B0 discards 5 at 0x0B0, 6 at 0x0B4 and 10,000 at \
             0x0BC; memory keeps 92, 8 and 34."
        );
    }

    #[test]
    fn an_edit_of_a_way_names_its_own_rank_before_the_others() {
        let geometry = preset(Preset::FourWayWriteBack);
        let word = Word::new_unchecked;
        let items = [
            WayItem::Valid { to: true },
            WayItem::Tag {
                from: None,
                to: Tag::new_unchecked(3),
            },
            WayItem::Word {
                word: WordIndex::new_unchecked(1),
                from: word(53),
                to: word(9),
            },
            WayItem::Dirty { to: true },
            WayItem::Rank {
                way: way(0),
                from: 0,
                to: 1,
            },
            WayItem::Rank {
                way: way(2),
                from: 3,
                to: 0,
            },
            WayItem::Rank {
                way: way(3),
                from: 2,
                to: 3,
            },
        ];
        let previews: Vec<String> = items
            .iter()
            .map(|&item| way_item(&geometry, item))
            .collect();
        assert_eq!(
            previews,
            [
                "valid 0 \u{2192} 1",
                "tag \u{2014} \u{2192} 0x03",
                "word 1: 53 \u{2192} 9",
                "dirty 0 \u{2192} 1",
                "rank way 0: 0 \u{2192} 1",
                "rank way 2: 3 \u{2192} 0",
                "rank way 3: 2 \u{2192} 3",
            ]
        );
        assert_eq!(
            edited_way(&geometry, set(1), way(2), &items),
            "Edited set 1, way 2: valid 0 \u{2192} 1; tag \u{2014} \u{2192} 0x03; word 1: 53 \
             \u{2192} 9; dirty 0 \u{2192} 1; rank 3 \u{2192} 0 (way 0: 0 \u{2192} 1, way 3: 2 \
             \u{2192} 3)."
        );
        assert_eq!(
            edited_way(&geometry, set(1), way(2), &[WayItem::Valid { to: false }]),
            "Edited set 1, way 2: valid 1 \u{2192} 0."
        );
    }

    #[test]
    fn an_edit_of_memory_says_what_the_line_that_holds_it_does() {
        let geometry = preset(Preset::TwoWayLru);
        let (from, to) = (Word::new_unchecked(92), Word::new_unchecked(12));
        let line = |note| edited_memory(&geometry, addr(0x0B0), from, to, note);
        assert_eq!(
            line(MemoryNote::MemoryOnly),
            "Edited memory 0x0B0: 92 \u{2192} 12."
        );
        assert_eq!(
            line(MemoryNote::CleanCopy {
                set: set(3),
                way: way(0)
            }),
            "Edited memory 0x0B0: 92 \u{2192} 12. The clean copy in set 3, way 0 was updated \
             too."
        );
        assert_eq!(
            line(MemoryNote::StaysStale {
                set: set(3),
                way: way(0)
            }),
            "Edited memory 0x0B0: 92 \u{2192} 12. Set 3, way 0 holds this line dirty, so the \
             word stays stale."
        );
        assert_eq!(
            line(MemoryNote::BecomesStale {
                set: set(3),
                way: way(0)
            }),
            "Edited memory 0x0B0: 92 \u{2192} 12. Set 3, way 0 holds this line dirty, so the \
             word becomes stale."
        );
        assert_eq!(
            line(MemoryNote::MatchesDirty {
                set: set(3),
                way: way(0)
            }),
            "Edited memory 0x0B0: 92 \u{2192} 12. Memory now matches the dirty line in set 3, \
             way 0."
        );
        assert_eq!(
            cached_item(set(2), way(0), WordIndex::new_unchecked(1), from, to),
            "set 2 way 0 word 1: 92 \u{2192} 12"
        );
    }

    #[test]
    fn edit_actions_name_what_they_do() {
        let geometry = preset(Preset::TwoWayLru);
        let labels: Vec<String> = [
            EditAction::Apply,
            EditAction::ApplyToMemory,
            EditAction::ApplyToBoth,
            EditAction::JustInvalidate,
            EditAction::MoveTo {
                set: set(0),
                way: way(0),
            },
            EditAction::EditInstead {
                set: Some(set(0)),
                way: way(1),
            },
            EditAction::EditInstead {
                set: None,
                way: way(0),
            },
            EditAction::ClearDirty,
            EditAction::SetDirty,
            EditAction::EditMemory { addr: addr(0x0A4) },
            EditAction::UseRank { rank: 1 },
        ]
        .into_iter()
        .map(|action| edit_action(&geometry, action))
        .collect();
        assert_eq!(
            labels,
            [
                "Apply",
                "Apply to memory",
                "Apply to both",
                "Just invalidate it",
                "Move to set 0, way 0",
                "Edit set 0, way 1 instead",
                "Edit way 0 instead",
                "Clear dirty and apply",
                "Set dirty and apply",
                "Edit memory 0x0A4 instead",
                "Use rank 1",
            ]
        );
    }
}
