//! How an edit reads: the plan of an edit the rules accept and the refusal of
//! one they refuse, with the edit the refusal offers instead; the cells,
//! items and consequences an edit lists; and the marks of the way editor's
//! data inputs.

use super::{
    check_shape, editable, fields_of, line_words, plan_edit, propose, tag_of, Consequence,
    EditPlan, EditRefusal, FieldChange, Fields, Holder, Malformed, MemoryChange, RefusalReason,
    Refused, Suggestion, Tone, WayChange, WordMark,
};
use crate::event::ChangeRef;
use crate::explain::{self, EditAction, WayItem};
use crate::format;
use crate::geometry::{AccessError, Geometry};
use crate::machine::Machine;
use crate::op::{self, EditPatch, EditTarget, WayPatch};
use crate::units::{Addr, SetIndex, Word, WordIndex};

/// The plan of `change`, an edit of `target` the rules accept.
pub(super) fn way_plan(machine: &Machine, target: EditTarget, change: &WayChange) -> EditPlan {
    let geometry = &machine.geometry;
    let discarded = discarded(machine, change);
    let (tone, text) = if discarded.is_empty() {
        let text = if change.moves.is_empty() {
            explain::recorded_as_step()
        } else {
            explain::ranks_shift()
        };
        (Tone::Ready, text)
    } else {
        let line = change.after.line;
        (
            Tone::Caution,
            explain::discards(geometry, change.way, line, &discarded),
        )
    };
    EditPlan {
        target,
        patch: normalised(machine, change),
        tone,
        title: explain::edit_ready(),
        text,
        changes: field_changes(machine, change),
        consequences: consequences(machine, change, &discarded),
        preview: items(machine, change)
            .into_iter()
            .map(|item| explain::way_item(geometry, item))
            .collect(),
        action: explain::edit_action(geometry, EditAction::Apply),
        word_marks: marks(machine, &change.before, &change.after),
    }
}

/// The plan of `change`, an edit of a memory word the rules accept.
pub(super) fn memory_plan(machine: &Machine, target: EditTarget, change: MemoryChange) -> EditPlan {
    let geometry = &machine.geometry;
    let MemoryChange {
        addr,
        from,
        to,
        holder,
    } = change;
    let written = Consequence::MemoryWrite { addr, from, to };
    let mut changes = Vec::new();
    let mut preview = vec![explain::memory_item(geometry, addr, from, to)];
    let (tone, title, text, action, consequences) = match holder {
        None => (
            Tone::Ready,
            explain::memory_only_title(),
            explain::memory_only(),
            EditAction::Apply,
            vec![written],
        ),
        Some(holder) if !holder.dirty => {
            let Holder {
                set,
                way,
                word,
                cached,
                ..
            } = holder;
            changes.push(FieldChange {
                cell: ChangeRef::Word { set, way, word },
                from: format::data(cached.get()),
                to: format::data(to.get()),
            });
            preview.push(explain::cached_item(set, way, word, cached, to));
            let copy = Consequence::CachedCopyUpdated {
                set,
                way,
                word,
                from: cached,
                to,
            };
            (
                Tone::Move,
                explain::clean_copy_title(set, way),
                explain::clean_copy(),
                EditAction::ApplyToBoth,
                vec![written, copy],
            )
        }
        Some(holder) if holder.cached != to => {
            let Holder {
                set, way, cached, ..
            } = holder;
            // The word was stale unless the line agreed with memory until
            // now; either way it is stale after the edit.
            let was_stale = cached != from;
            let stale = if was_stale {
                Consequence::StaysStale { addr, set, way }
            } else {
                Consequence::BecomesStale { addr }
            };
            (
                Tone::Caution,
                explain::newer_value_title(set, way),
                explain::newer_value(cached, was_stale),
                EditAction::ApplyToMemory,
                vec![written, stale],
            )
        }
        Some(holder) => (
            Tone::Ready,
            explain::matches_dirty_title(holder.set, holder.way),
            explain::matches_dirty(),
            EditAction::ApplyToMemory,
            vec![written, Consequence::NoLongerStale { addr }],
        ),
    };
    changes.push(FieldChange {
        cell: ChangeRef::Memory { addr },
        from: format::data(from.get()),
        to: format::data(to.get()),
    });
    EditPlan {
        target,
        patch: EditPatch::Memory { value: to },
        tone,
        title,
        text,
        changes,
        consequences,
        preview,
        action: explain::edit_action(geometry, action),
        word_marks: Vec::new(),
    }
}

/// How a refusal reads: its rule, colour, title and text, and whether the
/// colour holds when there is nothing to offer.
struct Wording {
    /// The rule.
    reason: RefusalReason,
    /// The status bar's colour when the offer plans, or when `firm`.
    tone: Tone,
    /// The title.
    title: String,
    /// The text.
    text: String,
    /// Whether the tone holds without an offer; otherwise a refusal left
    /// with nothing to offer reads as refused.
    firm: bool,
}

impl Wording {
    /// A refusal in the tone `tone`, which holds with or without an offer.
    fn firm(reason: RefusalReason, tone: Tone, title: String, text: String) -> Self {
        Self {
            reason,
            tone,
            title,
            text,
            firm: true,
        }
    }

    /// A refusal in the tone `tone` while it has an offer, and refused
    /// without one.
    fn offering(reason: RefusalReason, tone: Tone, title: String, text: String) -> Self {
        Self {
            firm: false,
            ..Self::firm(reason, tone, title, text)
        }
    }

    /// A refusal with nothing to fix: "Refused".
    fn refused(reason: RefusalReason, text: String) -> Self {
        Self::firm(reason, Tone::Refused, explain::edit_refused(), text)
    }
}

/// The refusal of `patch` on `target` for `refused`, with its offer if the
/// rules accept the offer.
pub(super) fn refusal(
    machine: &Machine,
    target: &EditTarget,
    patch: &EditPatch,
    refused: &Refused,
) -> EditRefusal {
    let set = match *target {
        EditTarget::Way { set, .. } => set,
        EditTarget::Memory { addr } => machine.geometry.locate(addr).set,
    };
    let wording = wording(machine, set, refused);
    let suggestion = offer(*target, patch, refused)
        .and_then(|(target, patch, action)| suggest(machine, target, patch, action));
    let tone = if suggestion.is_some() || wording.firm {
        wording.tone
    } else {
        Tone::Refused
    };
    EditRefusal {
        reason: wording.reason,
        tone,
        title: wording.title,
        text: wording.text,
        suggestion,
        word_marks: word_marks(machine, target, patch),
    }
}

/// How `refused` reads, for an edit whose target is in set `set`.
fn wording(machine: &Machine, set: SetIndex, refused: &Refused) -> Wording {
    use RefusalReason as Reason;
    let geometry = &machine.geometry;
    match *refused {
        Refused::Malformed(malformed) => malformed_wording(geometry, malformed),
        Refused::NoChange => Wording::firm(
            Reason::NoChange,
            Tone::Idle,
            explain::no_changes(),
            explain::no_way_change(),
        ),
        Refused::FieldsOnInvalidWay => Wording::offering(
            Reason::FieldsOnInvalidWay,
            Tone::Caution,
            explain::empty_way_title(),
            explain::empty_way(),
        ),
        Refused::MissingLine => Wording::firm(
            Reason::MissingLine,
            Tone::Idle,
            explain::missing_line_title(),
            explain::missing_line(),
        ),
        Refused::LineOutOfRange { addr } => {
            Wording::refused(Reason::LineOutOfRange, out_of_range(geometry, addr))
        }
        Refused::AlreadyElsewhere {
            line,
            set: home,
            way,
        } => Wording::offering(
            Reason::DuplicateLine,
            Tone::Move,
            explain::belongs_in(geometry, line, home),
            explain::already_in(geometry, line, home, way),
        ),
        Refused::WrongSet {
            line,
            set: home,
            way,
        } => Wording::firm(
            Reason::WrongSet,
            Tone::Move,
            explain::belongs_in(geometry, line, home),
            explain::wrong_set(geometry, home, set, way),
        ),
        Refused::WrongSetFull { line, set: home } => Wording::refused(
            Reason::WrongSetFull,
            explain::wrong_set_full(geometry, line, home, set),
        ),
        Refused::AlreadyHere { line, way } => Wording::offering(
            Reason::DuplicateLine,
            Tone::Move,
            explain::already_cached(geometry, line),
            explain::already_holds(geometry, set, line, way),
        ),
        Refused::DirtyUnderWriteThrough => Wording::offering(
            Reason::DirtyUnderWriteThrough,
            Tone::Caution,
            explain::never_dirty_title(),
            explain::never_dirty(),
        ),
        Refused::NeedsDirty {
            word,
            value,
            memory,
        } => Wording::firm(
            Reason::NeedsDirty,
            Tone::Caution,
            explain::data_differs(),
            explain::needs_dirty(word, value, memory),
        ),
        Refused::WriteThroughMismatch { .. } => Wording::offering(
            Reason::WriteThroughMismatch,
            Tone::Caution,
            explain::data_differs(),
            explain::write_through_agrees(),
        ),
        Refused::RankNotEditable => Wording::refused(
            Reason::RankNotEditable,
            explain::rank_fixed(geometry, machine.config.cache.replacement),
        ),
        Refused::RankOutOfRange { valid } => Wording::offering(
            Reason::RankOutOfRange,
            Tone::Caution,
            explain::rank_out_of_range_title(),
            explain::rank_out_of_range(valid),
        ),
        Refused::MemoryNoChange { stale } => unchanged_memory(stale),
    }
}

/// How a memory edit that keeps the word's value reads: as nothing to apply,
/// or, while `stale` is a dirty way holding another value for the word, with
/// that way's warning, so it shows as the editor opens.
fn unchanged_memory(stale: Option<Holder>) -> Wording {
    match stale {
        Some(holder) => Wording::firm(
            RefusalReason::NoChange,
            Tone::Caution,
            explain::newer_value_title(holder.set, holder.way),
            explain::newer_value(holder.cached, true),
        ),
        None => Wording::firm(
            RefusalReason::NoChange,
            Tone::Idle,
            explain::no_changes(),
            explain::no_memory_change(),
        ),
    }
}

/// How `malformed` reads: refused, saying what does not exist or fit.
fn malformed_wording(geometry: &Geometry, malformed: Malformed) -> Wording {
    use RefusalReason as Reason;
    let (reason, text) = match malformed {
        Malformed::NoSuchSet { set } => (Reason::OutOfRange, explain::no_such_set(geometry, set)),
        Malformed::NoSuchWay { set, way } => {
            (Reason::OutOfRange, explain::no_such_way(geometry, set, way))
        }
        Malformed::AddressOutOfRange { addr } => (Reason::OutOfRange, out_of_range(geometry, addr)),
        Malformed::KindMismatch { way } => (Reason::KindMismatch, explain::kind_mismatch(way)),
        Malformed::DataLength { given } => {
            (Reason::BadShape, explain::data_length(geometry, given))
        }
        Malformed::ValueTooWide { value } => (
            Reason::ValueTooWide,
            op::value_too_wide(value, geometry.word_bytes()),
        ),
        Malformed::RankTooHigh { set } => (Reason::BadShape, explain::rank_too_high(geometry, set)),
        Malformed::Misaligned { addr } => {
            let error = AccessError::Misaligned {
                addr,
                address_bits: geometry.address_bits(),
                word_bytes: geometry.word_bytes(),
            };
            (Reason::Misaligned, error.to_string())
        }
    };
    Wording::refused(reason, text)
}

/// The message of an address that does not fit the address width, as an
/// issued access's: "0x1000 does not fit a 12-bit address (0x000–0xFFF)."
fn out_of_range(geometry: &Geometry, addr: Addr) -> String {
    AccessError::OutOfRange {
        addr: u64::from(addr.get()),
        address_bits: geometry.address_bits(),
    }
    .to_string()
}

/// The edit `refused` offers instead of `patch` on `target`, with its
/// button, before the rules have planned it: making the way empty and no
/// more; the rest of the patch on the way that already holds the line; the
/// same patch, valid, on the empty way of the line's own set; the patch with
/// the dirty bit cleared under write-through, or set under write-back; the
/// one memory word that differs, edited instead; or the highest rank a valid
/// way can have.
fn offer(
    target: EditTarget,
    patch: &EditPatch,
    refused: &Refused,
) -> Option<(EditTarget, EditPatch, EditAction)> {
    let EditPatch::Way(patch) = patch else {
        return None;
    };
    let EditTarget::Way { set, .. } = target else {
        return None;
    };
    let rest = || WayPatch {
        line: None,
        ..patch.clone()
    };
    let (target, patch, action) = match *refused {
        Refused::FieldsOnInvalidWay => (
            target,
            WayPatch {
                valid: Some(false),
                ..WayPatch::default()
            },
            EditAction::JustInvalidate,
        ),
        Refused::AlreadyElsewhere { set: home, way, .. } => (
            EditTarget::Way { set: home, way },
            rest(),
            EditAction::EditInstead {
                set: Some(home),
                way,
            },
        ),
        Refused::WrongSet { set: home, way, .. } => (
            EditTarget::Way { set: home, way },
            WayPatch {
                valid: Some(true),
                ..patch.clone()
            },
            EditAction::MoveTo { set: home, way },
        ),
        Refused::AlreadyHere { way, .. } => (
            EditTarget::Way { set, way },
            rest(),
            EditAction::EditInstead { set: None, way },
        ),
        Refused::DirtyUnderWriteThrough => (
            target,
            WayPatch {
                dirty: Some(false),
                ..patch.clone()
            },
            EditAction::ClearDirty,
        ),
        Refused::NeedsDirty { .. } => (
            target,
            WayPatch {
                dirty: Some(true),
                ..patch.clone()
            },
            EditAction::SetDirty,
        ),
        Refused::WriteThroughMismatch { ref differing } => {
            let &[(addr, value)] = differing.as_slice() else {
                return None;
            };
            return Some((
                EditTarget::Memory { addr },
                EditPatch::Memory { value },
                EditAction::EditMemory { addr },
            ));
        }
        Refused::RankOutOfRange { valid } => {
            let rank = valid.saturating_sub(1);
            (
                target,
                WayPatch {
                    rank: Some(rank),
                    ..patch.clone()
                },
                EditAction::UseRank { rank },
            )
        }
        _ => return None,
    };
    Some((target, EditPatch::Way(patch), action))
}

/// The offer of `patch` on `target` with the button `action`, if the rules
/// accept it.
fn suggest(
    machine: &Machine,
    target: EditTarget,
    patch: EditPatch,
    action: EditAction,
) -> Option<Box<Suggestion>> {
    let plan = plan_edit(machine, &target, &patch).ok()?;
    Some(Box::new(Suggestion {
        target,
        patch,
        action: explain::edit_action(&machine.geometry, action),
        preview: plan.preview,
    }))
}

/// The marks of the way editor's data inputs for `patch` on `target`; none
/// for a memory edit, or for a patch that does not fit its way.
pub(super) fn word_marks(
    machine: &Machine,
    target: &EditTarget,
    patch: &EditPatch,
) -> Vec<WordMark> {
    let (EditTarget::Way { set, way }, EditPatch::Way(patch)) = (*target, patch) else {
        return Vec::new();
    };
    let Some(current) = machine.cache.way(set, way) else {
        return Vec::new();
    };
    if check_shape(&machine.geometry, set, patch).is_err() {
        return Vec::new();
    }
    let before = fields_of(machine, set, current);
    let proposal = propose(machine, &before, patch);
    marks(machine, &before, &proposal.fields)
}

/// The marks of the data inputs showing `after`'s words, for a way whose
/// fields are `before`: whether each differs from the way's word, and from
/// memory's copy of `after`'s line. Inputs mark nothing while `after` holds
/// no line.
fn marks(machine: &Machine, before: &Fields, after: &Fields) -> Vec<WordMark> {
    let geometry = &machine.geometry;
    let memory = after
        .line
        .filter(|line| geometry.addr(line.get()).is_some())
        .map(|line| line_words(machine, line))
        .unwrap_or_default();
    let words = usize::try_from(geometry.words_per_line()).unwrap_or(0);
    (0..words)
        .map(|index| {
            let Some(value) = after.words.get(index).filter(|_| after.line.is_some()) else {
                return WordMark::Same;
            };
            let changed = before.words.get(index) != Some(value);
            let differs = memory.get(index).is_some_and(|memory| memory != value);
            match (changed, differs) {
                (false, false) => WordMark::Same,
                (true, false) => WordMark::Changed,
                (false, true) => WordMark::DiffersFromMemory,
                (true, true) => WordMark::ChangedAndDiffers,
            }
        })
        .collect()
}

/// The patch of `change` with every field the result has: `{ valid: false
/// }` for a way made empty, and otherwise the line at its start, the words,
/// the dirty bit and, where ranks can change, the rank.
fn normalised(machine: &Machine, change: &WayChange) -> EditPatch {
    let after = &change.after;
    EditPatch::Way(match after.line {
        None => WayPatch {
            valid: Some(false),
            ..WayPatch::default()
        },
        Some(line) => WayPatch {
            valid: Some(true),
            line: Some(Addr::new_unchecked(line.get())),
            data: Some(
                after
                    .words
                    .iter()
                    .copied()
                    .map(Word::new_unchecked)
                    .collect(),
            ),
            dirty: Some(after.dirty),
            rank: editable(machine).then_some(after.rank),
        },
    })
}

/// The dirty words `change` drops: when a dirty way loses its line, each
/// word that differs from memory, with its address, the way's value and
/// memory's.
fn discarded(machine: &Machine, change: &WayChange) -> Vec<(Addr, Word, Word)> {
    let before = &change.before;
    let Some(line) = before
        .line
        .filter(|line| before.dirty && change.after.line != Some(*line))
    else {
        return Vec::new();
    };
    let memory = line_words(machine, line);
    let word_bytes = machine.geometry.word_bytes();
    (0_u32..)
        .zip(before.words.iter().zip(&memory))
        .filter(|(_, (cached, memory))| cached != memory)
        .map(|(word, (cached, memory))| {
            (
                Addr::new_unchecked(line.get() + word * word_bytes),
                Word::new_unchecked(*cached),
                Word::new_unchecked(*memory),
            )
        })
        .collect()
}

/// What follows from `change` beyond its fields: the dirty data it drops,
/// `discarded`; the line it brings in; the memory words it makes stale or no
/// longer stale; and the ranks it moves.
fn consequences(
    machine: &Machine,
    change: &WayChange,
    discarded: &[(Addr, Word, Word)],
) -> Vec<Consequence> {
    let (before, after) = (&change.before, &change.after);
    let mut consequences = Vec::new();
    if !discarded.is_empty() {
        consequences.push(Consequence::DiscardsDirtyData {
            set: change.set,
            way: change.way,
            addrs: discarded.iter().map(|(addr, _, _)| *addr).collect(),
        });
    }
    if let Some(line) = after.line {
        let same = before.line == Some(line);
        if !same {
            consequences.push(Consequence::LoadsLine { line });
        }
        let memory = line_words(machine, line);
        let word_bytes = machine.geometry.word_bytes();
        for (word, memory) in (0_u32..).zip(&memory) {
            let index = usize::try_from(word).unwrap_or(usize::MAX);
            let stale = |fields: &Fields| {
                fields.dirty && fields.words.get(index).is_some_and(|value| value != memory)
            };
            let addr = Addr::new_unchecked(line.get() + word * word_bytes);
            match (same && stale(before), stale(after)) {
                (false, true) => consequences.push(Consequence::BecomesStale { addr }),
                (true, false) => consequences.push(Consequence::NoLongerStale { addr }),
                _ => {}
            }
        }
    }
    consequences.extend(change.moves.iter().map(|moved| Consequence::Rerank {
        set: change.set,
        way: moved.way,
        from: moved.from,
        to: moved.to,
    }));
    consequences
}

/// What `change` changes, as its preview and its log line list it: the
/// valid bit, the tag of a line the way takes, each word whose value moves
/// (from memory's copy for a line it takes), the dirty bit, and each rank.
pub(super) fn items(machine: &Machine, change: &WayChange) -> Vec<WayItem> {
    let (before, after) = (&change.before, &change.after);
    let mut items = Vec::new();
    if before.line.is_some() != after.line.is_some() {
        items.push(WayItem::Valid {
            to: after.line.is_some(),
        });
    }
    if let Some(line) = after.line {
        let moved = before.line != Some(line);
        if moved {
            items.push(WayItem::Tag {
                from: before.line.map(|line| tag_of(&machine.geometry, line)),
                to: tag_of(&machine.geometry, line),
            });
        }
        let base = if moved {
            line_words(machine, line)
        } else {
            before.words.clone()
        };
        for (word, (&from, &to)) in (0_u32..).zip(base.iter().zip(&after.words)) {
            if from != to {
                items.push(WayItem::Word {
                    word: WordIndex::new_unchecked(word),
                    from: Word::new_unchecked(from),
                    to: Word::new_unchecked(to),
                });
            }
        }
    }
    if before.dirty != after.dirty {
        items.push(WayItem::Dirty { to: after.dirty });
    }
    items.extend(change.moves.iter().map(|moved| WayItem::Rank {
        way: moved.way,
        from: moved.from,
        to: moved.to,
    }));
    items
}

/// The cells `change` changes, as its event lists them: a way whose valid
/// bit or line changes lists its tag and every word, since a different line
/// now occupies it; otherwise each word whose value changes. Then the dirty
/// bit, and each rank that moves. Sorted.
pub(super) fn cells(machine: &Machine, change: &WayChange) -> Vec<ChangeRef> {
    let (set, way) = (change.set, change.way);
    let (before, after) = (&change.before, &change.after);
    let mut cells = Vec::new();
    if before.line.is_some() != after.line.is_some() {
        cells.push(ChangeRef::Valid { set, way });
    }
    if before.line == after.line {
        cells.extend(
            (0_u32..)
                .zip(before.words.iter().zip(&after.words))
                .filter(|(_, (from, to))| from != to)
                .map(|(word, _)| ChangeRef::Word {
                    set,
                    way,
                    word: WordIndex::new_unchecked(word),
                }),
        );
    } else {
        cells.push(ChangeRef::Tag { set, way });
        cells.extend(
            (0..machine.geometry.words_per_line()).map(|word| ChangeRef::Word {
                set,
                way,
                word: WordIndex::new_unchecked(word),
            }),
        );
    }
    if before.dirty != after.dirty {
        cells.push(ChangeRef::Dirty { set, way });
    }
    cells.extend(change.moves.iter().map(|moved| ChangeRef::Rank {
        set,
        way: moved.way,
    }));
    cells.sort_unstable();
    cells
}

/// The cells `change` changes, each with its text before and after.
fn field_changes(machine: &Machine, change: &WayChange) -> Vec<FieldChange> {
    let geometry = &machine.geometry;
    let (before, after) = (&change.before, &change.after);
    let absent = || format::NO_FIGURE.to_owned();
    let tag = |fields: &Fields| {
        fields.line.map_or_else(absent, |line| {
            format::tag(tag_of(geometry, line).get(), geometry.tag_bits())
        })
    };
    let word = |fields: &Fields, word: WordIndex| {
        usize::try_from(word.get())
            .ok()
            .and_then(|index| fields.words.get(index))
            .map_or_else(absent, |value| format::data(*value))
    };
    cells(machine, change)
        .into_iter()
        .filter_map(|cell| {
            let (from, to) = match cell {
                ChangeRef::Valid { .. } => (
                    format::bit(before.line.is_some()),
                    format::bit(after.line.is_some()),
                ),
                ChangeRef::Tag { .. } => (tag(before), tag(after)),
                ChangeRef::Word { word: index, .. } => (word(before, index), word(after, index)),
                ChangeRef::Dirty { .. } => (format::bit(before.dirty), format::bit(after.dirty)),
                ChangeRef::Rank { way, .. } => {
                    let moved = change.moves.iter().find(|moved| moved.way == way)?;
                    (format::data(moved.from), format::data(moved.to))
                }
                _ => return None,
            };
            Some(FieldChange { cell, from, to })
        })
        .collect()
}
