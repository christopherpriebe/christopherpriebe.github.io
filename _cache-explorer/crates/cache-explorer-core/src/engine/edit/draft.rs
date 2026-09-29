//! The editors' drafts: what each editor draws for what the student typed.
//! A typed field is read as typed input is read everywhere, the smallest
//! patch that makes the way or the word what the editor shows is built from
//! it, and the patch is planned or refused.

use crate::cache::{Dirtiness, Slot, Way};
use crate::explain;
use crate::format;
use crate::geometry::Geometry;
use crate::input::{AddressInput, WordInput};
use crate::machine::Machine;
use crate::op::{EditPatch, EditTarget, WayPatch};
use crate::snapshot::{self, WayView};
use crate::units::{Addr, LineAddr, SetIndex, WayIndex, Word, WordIndex};

use super::wording::word_marks;
use super::{
    bytes_of, fields_of, line_words, plan_edit, propose, tag_of, Blocked, EditDraft, EditOutcome,
    EditRefusal, FieldInput, Fields, MemoryEditDraft, RefusalReason, Tone, WayForm,
};

/// The way editor's draft for `form` on way `way` of set `set`: each field
/// read, the smallest patch that makes the way what the form shows, and
/// its plan, or the refusal of a field that does not read, or the session's
/// refusal of the patch, which `blocked` gives when there is one.
///
/// A field that leaves the way as the edit would anyway is left out of the
/// patch: the valid bit and the line as the way has them; the data inputs as
/// `word_texts` shows them, which is memory's copy of a line the way takes,
/// an empty input reading as the word it shows; the dirty bit the edit
/// implies, clear for a line the way takes; and a rank the student did not
/// choose, or chose as the edit implies it. So the way the patch leaves is
/// the one the form shows. For a way made empty, each field is compared with
/// the way as it is, so one the student changed is sent, and refused.
pub(crate) fn draft_way(
    machine: &Machine,
    set: SetIndex,
    way: WayIndex,
    form: &WayForm,
    blocked: impl Fn(&EditTarget, &EditPatch) -> Option<Blocked>,
) -> EditDraft {
    let geometry = &machine.geometry;
    let target = EditTarget::Way { set, way };
    let Some(current) = machine.cache.way(set, way) else {
        let typed = TypedLine::read(geometry, &form.line_text);
        let texts = WordTexts::read(geometry, form, &[]);
        let patch = EditPatch::Way(WayPatch::default());
        let empty = Way {
            slot: Slot::Invalid,
            rank: 0,
        };
        return EditDraft {
            line: typed.input,
            words: texts.inputs,
            tag_text: typed.tag_text,
            index_text: typed.index_text,
            word_texts: Vec::new(),
            outcome: outcome(machine, &target, &patch, blocked(&target, &patch), None),
            patch: Some(patch),
            result: snapshot::lone_way_view(machine, set, way, &empty),
        };
    };
    let before = fields_of(machine, set, current);
    let read = FormRead::read(machine, &before, form);
    let patch = EditPatch::Way(read.patch(machine, &before, form));
    let invalid = unread_field(read.typed.invalid, read.texts.invalid);
    let reads = invalid.is_none();
    EditDraft {
        line: read.typed.input,
        words: read.texts.inputs,
        tag_text: read.typed.tag_text,
        index_text: read.typed.index_text,
        word_texts: (0..geometry.words_per_line())
            .map(|word| {
                usize::try_from(word)
                    .ok()
                    .and_then(|index| read.shown.get(index))
                    .map_or_else(String::new, |value| format::plain(*value))
            })
            .collect(),
        outcome: outcome(machine, &target, &patch, blocked(&target, &patch), invalid),
        result: result_view(machine, set, way, current, &patch),
        patch: reads.then_some(patch),
    }
}

/// The memory editor's draft for `value_text` on the word at `addr`: the
/// value read, its patch and its plan, or the refusal of a value that does
/// not read, or the session's refusal of the patch, which `blocked` gives
/// when there is one. An empty value is the word's own, which changes
/// nothing.
pub(crate) fn draft_memory(
    machine: &Machine,
    addr: Addr,
    value_text: &str,
    blocked: impl Fn(&EditTarget, &EditPatch) -> Option<Blocked>,
) -> MemoryEditDraft {
    let target = EditTarget::Memory { addr };
    let current = EditPatch::Memory {
        value: machine.memory.word(addr),
    };
    let (value, patch, invalid) = match machine.geometry.read_word(value_text) {
        WordInput::Empty => (FieldInput::Empty, current, None),
        WordInput::Valid { value, text } => (
            FieldInput::Valid { text },
            EditPatch::Memory { value },
            None,
        ),
        WordInput::Invalid { message } => (
            FieldInput::Invalid {
                message: message.clone(),
            },
            current,
            Some((RefusalReason::ValueTooWide, explain::check_value(), message)),
        ),
    };
    let reads = invalid.is_none();
    MemoryEditDraft {
        value,
        outcome: outcome(machine, &target, &patch, blocked(&target, &patch), invalid),
        patch: reads.then_some(patch),
    }
}

/// An editor's outcome for `patch` on `target`: the session's refusal when
/// it is `blocked`; the refusal of a field that does not read, `invalid`,
/// with its reason, title and message; or the patch's plan or refusal.
fn outcome(
    machine: &Machine,
    target: &EditTarget,
    patch: &EditPatch,
    blocked: Option<Blocked>,
    invalid: Option<(RefusalReason, String, String)>,
) -> EditOutcome {
    if let Some(blocked) = blocked {
        return EditOutcome::Refusal(blocked.refusal(machine, target, patch));
    }
    if let Some((reason, title, text)) = invalid {
        return EditOutcome::Refusal(EditRefusal {
            reason,
            tone: Tone::Refused,
            title,
            text,
            suggestion: None,
            word_marks: word_marks(machine, target, patch),
        });
    }
    plan_edit(machine, target, patch).into()
}

/// The way `current`, way `way` of set `set`, as `patch` would leave it:
/// the way itself when the patch does not give it a line it could hold.
fn result_view(
    machine: &Machine,
    set: SetIndex,
    way: WayIndex,
    current: &Way,
    patch: &EditPatch,
) -> WayView {
    let geometry = &machine.geometry;
    let EditPatch::Way(patch) = patch else {
        return snapshot::lone_way_view(machine, set, way, current);
    };
    let proposal = propose(machine, &fields_of(machine, set, current), patch);
    let fields = &proposal.fields;
    // A line the way could hold fits the address width and belongs to the
    // way's set, and the data gives each of its words.
    let holdable = fields.line.filter(|line| {
        geometry.addr(line.get()).is_some()
            && geometry.locate(Addr::new_unchecked(line.get())).set == set
            && u32::try_from(fields.words.len()).ok() == Some(geometry.words_per_line())
    });
    let slot = if !proposal.valid {
        Slot::Invalid
    } else if let Some(line) = holdable {
        Slot::Valid {
            tag: tag_of(geometry, line),
            dirty: if fields.dirty {
                Dirtiness::Dirty
            } else {
                Dirtiness::Clean
            },
            data: bytes_of(geometry, &fields.words),
        }
    } else {
        return snapshot::lone_way_view(machine, set, way, current);
    };
    let rank = fields.rank.min(geometry.ways().saturating_sub(1));
    snapshot::lone_way_view(machine, set, way, &Way { slot, rank })
}

/// The refusal of a field of the way editor's form that does not read: the
/// line address, `line`, or else the first data input, `word`, as the rule,
/// the status bar's title and the message.
fn unread_field(
    line: Option<String>,
    word: Option<(WordIndex, String)>,
) -> Option<(RefusalReason, String, String)> {
    line.map(|message| {
        (
            RefusalReason::LineOutOfRange,
            explain::not_an_address(),
            message,
        )
    })
    .or_else(|| {
        word.map(|(word, message)| {
            (
                RefusalReason::ValueTooWide,
                explain::check_word(word),
                message,
            )
        })
    })
}

/// The way editor's form, read against the way it edits.
struct FormRead {
    /// The line address, read.
    typed: TypedLine,
    /// The data inputs, read.
    texts: WordTexts,
    /// What the data inputs show while untouched: memory's copy of a line the
    /// way takes, and otherwise the way's words.
    shown: Vec<u64>,
    /// Whether the form gives the way a line other than the one it holds.
    moved: bool,
}

impl FormRead {
    /// Reads `form` against a way whose fields are `before`.
    fn read(machine: &Machine, before: &Fields, form: &WayForm) -> Self {
        let geometry = &machine.geometry;
        let typed = TypedLine::read(geometry, &form.line_text);
        let line = if form.valid {
            typed.line.or(before.line)
        } else {
            None
        };
        let moved = form.valid && line != before.line;
        let shown = match line {
            Some(line) if moved => line_words(machine, line),
            None if form.valid => Vec::new(),
            _ => before.words.clone(),
        };
        let texts = WordTexts::read(geometry, form, &shown);
        Self {
            typed,
            texts,
            shown,
            moved,
        }
    }

    /// The smallest patch that makes a way whose fields are `before` what
    /// `form` shows, as [`draft_way`] describes.
    fn patch(&self, machine: &Machine, before: &Fields, form: &WayForm) -> WayPatch {
        let mut patch = WayPatch::default();
        if form.valid != before.line.is_some() {
            patch.valid = Some(form.valid);
        }
        if let Some(line) = self.typed.line.filter(|line| Some(*line) != before.line) {
            patch.line = Some(Addr::new_unchecked(line.get()));
        }
        if let Some(words) = self
            .texts
            .words
            .as_ref()
            .filter(|words| **words != self.shown)
        {
            patch.data = Some(words.iter().copied().map(Word::new_unchecked).collect());
        }
        let dirty = if self.moved { false } else { before.dirty };
        if form.dirty != dirty {
            patch.dirty = Some(form.dirty);
        }
        let rank = propose(
            machine,
            before,
            &WayPatch {
                rank: None,
                ..patch.clone()
            },
        )
        .fields
        .rank;
        if let Some(chosen) = form.rank.filter(|chosen| *chosen != rank) {
            patch.rank = Some(chosen);
        }
        patch
    }
}

/// The way editor's line address, read.
struct TypedLine {
    /// The field, read.
    input: FieldInput,
    /// The line it names, when it reads.
    line: Option<LineAddr>,
    /// The line's tag, for its swatch.
    tag_text: Option<String>,
    /// The line's index, for its swatch, when the cache has index bits.
    index_text: Option<String>,
    /// Why it does not read, if it does not.
    invalid: Option<String>,
}

impl TypedLine {
    /// Reads `text` as any address inside a line, as the address box reads
    /// an address, aligned to a word or not.
    fn read(geometry: &Geometry, text: &str) -> Self {
        match geometry.read_address(text) {
            AddressInput::Valid { decoded } | AddressInput::Misaligned { decoded, .. } => Self {
                input: FieldInput::Valid {
                    text: decoded.line_text.clone(),
                },
                line: Some(decoded.line),
                index_text: (geometry.index_bits() > 0).then(|| format::data(decoded.index.get())),
                tag_text: Some(decoded.tag_text),
                invalid: None,
            },
            AddressInput::Invalid { message } => Self {
                input: FieldInput::Invalid {
                    message: message.clone(),
                },
                line: None,
                tag_text: None,
                index_text: None,
                invalid: Some(message),
            },
            AddressInput::Empty => Self {
                input: FieldInput::Empty,
                line: None,
                tag_text: None,
                index_text: None,
                invalid: None,
            },
        }
    }
}

/// The way editor's data inputs, read.
struct WordTexts {
    /// Each field, read.
    inputs: Vec<FieldInput>,
    /// The words they give, an empty input giving the word it shows; none
    /// while an input does not read, or an empty one shows no word.
    words: Option<Vec<u64>>,
    /// The first input that does not read, and why.
    invalid: Option<(WordIndex, String)>,
}

impl WordTexts {
    /// Reads the form's data inputs, one per word of a line, as the Value box
    /// reads a value; an input the form lacks is empty. `shown` is what the
    /// inputs show untouched.
    fn read(geometry: &Geometry, form: &WayForm, shown: &[u64]) -> Self {
        let mut inputs = Vec::new();
        let mut words = Some(Vec::new());
        let mut invalid = None;
        for word in 0..geometry.words_per_line() {
            let index = usize::try_from(word).unwrap_or(usize::MAX);
            let text = form.word_texts.get(index).map_or("", String::as_str);
            let (input, value) = match geometry.read_word(text) {
                WordInput::Empty => (FieldInput::Empty, shown.get(index).copied()),
                WordInput::Valid { value, text } => (FieldInput::Valid { text }, Some(value.get())),
                WordInput::Invalid { message } => {
                    if invalid.is_none() {
                        invalid = Some((WordIndex::new_unchecked(word), message.clone()));
                    }
                    (FieldInput::Invalid { message }, None)
                }
            };
            inputs.push(input);
            words = words.zip(value).map(|(mut words, value)| {
                words.push(value);
                words
            });
        }
        Self {
            inputs,
            words,
            invalid,
        }
    }
}
