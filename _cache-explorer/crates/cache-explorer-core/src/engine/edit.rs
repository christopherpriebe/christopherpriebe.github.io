//! Edits: a way of the cache or a word of memory changed by hand, planned
//! before it is applied.
//!
//! Planning an edit runs its rules in order, and the first that matches
//! decides: the target must exist and the patch be of its kind and shape;
//! then, for a way, the resulting way must change something, hold a line of
//! its own set that no other way holds, keep a clean line equal to memory
//! and every line clean under write-through, and keep the set's ranks a
//! permutation with the valid ways first; for a memory word, the address
//! must be aligned and the value new. A plan says what the edit changes and
//! what follows from it, such as a line brought in from memory, a word left
//! stale, or a dirty line's newer words discarded. A refusal says why, and
//! offers another edit when one would do what was meant: the same line in
//! its own set's empty way, the dirty bit set, a memory word edited instead.
//! An offer is itself an edit the rules accept.
//!
//! A way given no value for a field keeps it, or takes the value the edit
//! implies: a way that takes a line holds memory's copy of it, clean, and
//! ranks first, as a fill would; a way made empty ranks last. Under random
//! replacement, and in a set of one way, ranks never change.
//!
//! An applied edit is one `Edit` event, an entry of its own. It changes no
//! counter, no cycle count and the generator not at all, but a way that
//! takes or loses a line updates what the classification of misses
//! remembers, so the next miss on a line an edit removed is compulsory.
//!
//! The editors send what the student typed. A draft reads it with the rules
//! typed input follows everywhere, builds the smallest patch that makes the
//! way what the form shows, and plans it, so the page parses nothing.

use serde::{Deserialize, Serialize};

use crate::cache::{Dirtiness, Set, Slot, Way};
use crate::config::{Replacement, WritePolicy};
use crate::engine::access::{lost, set_ranks};
use crate::engine::{end_entry, Draft, Emitter, Flow, Halt};
use crate::event::{ChangeRef, EventKind};
use crate::explain::{self, EditAction, MemoryNote, Unfinished, WayItem};
use crate::format;
use crate::geometry::{AccessError, Geometry};
use crate::input::{AddressInput, WordInput};
use crate::machine::Machine;
use crate::memory::{set_word_at, word_at};
use crate::op::{self, EditPatch, EditTarget, OpError, WayPatch};
use crate::replacement::RankMove;
use crate::snapshot::{self, WayView};
use crate::units::{Addr, LineAddr, SetIndex, Tag, WayIndex, Word, WordIndex};

/// How an editor's status bar reads, as the colour of its rule: ready to
/// apply (green), a caution (amber), a move to another way (navy), refused
/// (red), or nothing to apply yet (the rule's grey).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    /// The edit can be applied as it is.
    Ready,
    /// The edit needs a second look: it drops or leaves stale data, or it
    /// needs another field to be applied.
    Caution,
    /// The edit belongs to another way.
    Move,
    /// The edit cannot be applied.
    Refused,
    /// There is nothing to apply yet.
    Idle,
}

/// How a data input of the way editor is marked: underlined when its value
/// differs from the way's word, on a tinted ground when it differs from
/// memory's copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum WordMark {
    /// The way's word, as memory has it.
    Same,
    /// Differs from the way's word, not from memory.
    Changed,
    /// Differs from memory, not from the way's word.
    DiffersFromMemory,
    /// Differs from both.
    ChangedAndDiffers,
}

/// One cell an edit changes, with its text before and after, as the cache
/// table and memory show it: `0` and `1` for a bit, `—` for a field an empty
/// way does not have.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    /// The cell.
    pub cell: ChangeRef,
    /// Its text before the edit.
    pub from: String,
    /// Its text after.
    pub to: String,
}

/// Something that follows from an edit beyond the fields it names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "consequence",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Consequence {
    /// Way `way` of set `set` moves from rank `from` to rank `to`, so the
    /// ranks stay a permutation.
    Rerank {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
        /// Its rank before.
        from: u32,
        /// Its rank after.
        to: u32,
    },
    /// The way takes `line`, and memory's copy of it unless the edit gives
    /// its data.
    LoadsLine {
        /// The line.
        line: LineAddr,
    },
    /// The memory word at `addr` goes from `from` to `to`.
    MemoryWrite {
        /// The word's address.
        addr: Addr,
        /// Its value before.
        from: Word,
        /// Its value after.
        to: Word,
    },
    /// The clean line in way `way` of set `set` holds the word too, and its
    /// copy follows memory.
    CachedCopyUpdated {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
        /// The word within the line.
        word: WordIndex,
        /// The cached value before.
        from: Word,
        /// The cached value after.
        to: Word,
    },
    /// The memory word at `addr` becomes stale: a dirty line now holds
    /// another value for it.
    BecomesStale {
        /// The word's address.
        addr: Addr,
    },
    /// The memory word at `addr` stays stale: the dirty line in way `way`
    /// of set `set` holds another value for it.
    StaysStale {
        /// The word's address.
        addr: Addr,
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
    },
    /// The memory word at `addr` is no longer stale.
    NoLongerStale {
        /// The word's address.
        addr: Addr,
    },
    /// Way `way` of set `set` is dirty and loses its line, so the newer
    /// words at `addrs`, which memory never received, are gone.
    DiscardsDirtyData {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
        /// The addresses of the words discarded, ascending.
        addrs: Vec<Addr>,
    },
}

/// Which rule refused an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum RefusalReason {
    /// The session's log has no room for another entry where the edit would
    /// go, so the edit would be refused as an op.
    SessionFull,
    /// The session's log already holds as many ops before where the edit
    /// would go as a link can hold, so the edit would be refused as an op.
    LinkFull,
    /// The cursor is inside an op, which undoes as a whole.
    MidAccess,
    /// The set, way or address does not exist.
    OutOfRange,
    /// The patch is not of the target's kind.
    KindMismatch,
    /// The data does not have one value per word, or the rank is not one a
    /// way of the set can have.
    BadShape,
    /// A value does not fit the word, or does not read as one.
    ValueTooWide,
    /// Nothing would change.
    NoChange,
    /// An empty way would be given a field.
    FieldsOnInvalidWay,
    /// A way would be made valid without a line.
    MissingLine,
    /// The line does not fit the address width, or does not read as an
    /// address.
    LineOutOfRange,
    /// Another way already holds the line.
    DuplicateLine,
    /// The line belongs to another set, which has an empty way.
    WrongSet,
    /// The line belongs to another set, which is full.
    WrongSetFull,
    /// A line would be dirty under write-through.
    DirtyUnderWriteThrough,
    /// A clean line would differ from memory under write-back.
    NeedsDirty,
    /// A line would differ from memory under write-through.
    WriteThroughMismatch,
    /// Ranks do not change under random replacement or in a set of one way.
    RankNotEditable,
    /// A valid way would rank among the empty ones.
    RankOutOfRange,
    /// The memory address is not a multiple of the word size.
    Misaligned,
}

/// Another edit a refusal offers: a complete target and patch the rules
/// accept, the label of the button that applies it, and its preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// The way or word it edits.
    pub target: EditTarget,
    /// What it changes.
    pub patch: EditPatch,
    /// Its button: "Set dirty and apply", "Move to set 0, way 0".
    pub action: String,
    /// What it changes, one line each, as a plan's preview.
    pub preview: Vec<String>,
}

/// An edit the rules accept: what it changes and what follows from it, and
/// how the editor presents it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct EditPlan {
    /// The way or word it edits.
    pub target: EditTarget,
    /// The patch with every field the result has: the line at its start,
    /// and the defaults the edit implies filled in. Planned again, it gives
    /// the same plan.
    pub patch: EditPatch,
    /// The status bar's colour.
    pub tone: Tone,
    /// The status bar's title: "Ready to apply", "Memory only".
    pub title: String,
    /// The status bar's text.
    pub text: String,
    /// Every cell it changes, with its text before and after, in cell order.
    pub changes: Vec<FieldChange>,
    /// What follows from it.
    pub consequences: Vec<Consequence>,
    /// What it changes, one line each: "word 1: 53 → 9", "rank way 1: 1 →
    /// 0".
    pub preview: Vec<String>,
    /// The primary button: "Apply", "Apply to memory", "Apply to both".
    pub action: String,
    /// One mark per data input of the way editor; none for a memory edit.
    pub word_marks: Vec<WordMark>,
}

/// An edit the rules refuse: which rule, why in words, and another edit to
/// offer, if one would do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct EditRefusal {
    /// The rule.
    pub reason: RefusalReason,
    /// The status bar's colour.
    pub tone: Tone,
    /// The status bar's title: "Refused", "Data differs from memory".
    pub title: String,
    /// The status bar's text.
    pub text: String,
    /// The edit offered instead, boxed to keep a refusal small beside a
    /// plan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<Box<Suggestion>>,
    /// One mark per data input of the way editor, as the edit would leave
    /// them; none for a memory edit or a patch that does not fit its way.
    pub word_marks: Vec<WordMark>,
}

/// A plan or a refusal, tagged under `outcome` so that TypeScript can tell
/// them apart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum EditOutcome {
    /// The edit can be applied.
    Plan(EditPlan),
    /// The edit cannot be applied.
    Refusal(EditRefusal),
}

impl From<Result<EditPlan, EditRefusal>> for EditOutcome {
    fn from(result: Result<EditPlan, EditRefusal>) -> Self {
        match result {
            Ok(plan) => EditOutcome::Plan(plan),
            Err(refusal) => EditOutcome::Refusal(refusal),
        }
    }
}

/// The way editor's form, as the student left it: the texts as typed, and
/// the valid and dirty switches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WayForm {
    /// Whether the way should hold a line.
    pub valid: bool,
    /// The line address as typed; any byte of the line will do.
    pub line_text: String,
    /// Each data input's text as typed. An input the student has not typed
    /// in is sent empty, and reads as the word the draft shows for it, which
    /// follows the line typed.
    pub word_texts: Vec<String>,
    /// Whether the line should be dirty.
    pub dirty: bool,
    /// The rank the student chose; none until they choose one, while the
    /// rank shown is the draft's result's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
}

/// One typed field of an editor, read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FieldInput {
    /// Nothing is typed.
    Empty,
    /// It reads.
    Valid {
        /// What it reads as: the line address, `0x0C0`, or the value as the
        /// word stores it.
        text: String,
    },
    /// It does not read.
    Invalid {
        /// Why: `Type a 12-bit hex address, 0x000 to 0xFFF.`, or `300 does
        /// not fit a 1-byte word (−128 to 255).`
        message: String,
    },
}

/// Everything the way editor draws for its form: each field read, the
/// swatches under the line address, what the untouched data inputs show,
/// the smallest patch that makes the way what the form shows, its plan or
/// refusal, and the way as it would be.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct EditDraft {
    /// The line address, read.
    pub line: FieldInput,
    /// Each data input, read.
    pub words: Vec<FieldInput>,
    /// The typed line's tag, `0x05`, for its swatch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_text: Option<String>,
    /// The typed line's index, `2`, for its swatch; absent when the cache
    /// has no index bits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_text: Option<String>,
    /// What each data input shows while the student has not typed in it:
    /// memory's copy of the line when the way takes a line, and otherwise
    /// the way's words.
    pub word_texts: Vec<String>,
    /// The edit Apply sends; absent while a field does not read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<EditPatch>,
    /// The patch's plan or refusal; a refusal naming the field while one
    /// does not read.
    pub outcome: EditOutcome,
    /// The way as the edit would leave it.
    pub result: WayView,
}

/// Everything the memory editor draws for its typed value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct MemoryEditDraft {
    /// The value, read.
    pub value: FieldInput,
    /// The edit Apply sends; absent while the value does not read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<EditPatch>,
    /// The patch's plan or refusal.
    pub outcome: EditOutcome,
}

/// An edit the rules accept, ready to apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    /// A way's new fields.
    Way(WayChange),
    /// A memory word's new value.
    Memory(MemoryChange),
}

/// A way's fields before and after an edit, and every rank it moves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WayChange {
    /// The set.
    set: SetIndex,
    /// The way.
    way: WayIndex,
    /// Its fields before.
    before: Fields,
    /// Its fields after.
    after: Fields,
    /// The ways whose ranks move, the edited one included, in way order.
    moves: Vec<RankMove>,
}

/// A memory word's value before and after an edit, and the line that holds
/// it, if one does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemoryChange {
    /// The word's address.
    addr: Addr,
    /// Its value before.
    from: Word,
    /// Its value after.
    to: Word,
    /// The valid way holding its line.
    holder: Option<Holder>,
}

/// The valid way that holds a memory word's line, and what it holds for
/// the word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Holder {
    /// The set.
    set: SetIndex,
    /// The way.
    way: WayIndex,
    /// The word within the line.
    word: WordIndex,
    /// Whether the line is dirty.
    dirty: bool,
    /// The way's value for the word.
    cached: Word,
}

/// A way's fields, as the rules compare them.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fields {
    /// The line it holds; none while it is empty, or while an edit makes it
    /// valid without saying which line.
    line: Option<LineAddr>,
    /// Its words, one per word of the line; none while it is empty.
    words: Vec<u64>,
    /// Its dirty bit; clear while it is empty.
    dirty: bool,
    /// Its rank.
    rank: u32,
}

/// A way as a patch would leave it, before any rule is checked.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Proposal {
    /// Whether it holds a line.
    valid: bool,
    /// Its fields.
    fields: Fields,
}

/// Why an edit cannot be planned at all: its target does not exist, its
/// patch is not of the target's kind or shape, or a memory word's address is
/// not aligned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Malformed {
    /// The cache has no set `set`.
    NoSuchSet {
        /// The set.
        set: SetIndex,
    },
    /// Set `set` has no way `way`.
    NoSuchWay {
        /// The set.
        set: SetIndex,
        /// The way.
        way: WayIndex,
    },
    /// `addr` does not fit the address width.
    AddressOutOfRange {
        /// The address.
        addr: Addr,
    },
    /// The patch is not of the target's kind; `way` is whether the target
    /// is a way.
    KindMismatch {
        /// Whether the target is a way.
        way: bool,
    },
    /// The data has `given` values, not one per word.
    DataLength {
        /// How many it has.
        given: u32,
    },
    /// `value` does not fit the word.
    ValueTooWide {
        /// The value.
        value: Word,
    },
    /// No way of set `set` can have the rank given.
    RankTooHigh {
        /// The set.
        set: SetIndex,
    },
    /// `addr` is not a multiple of the word size.
    Misaligned {
        /// The address.
        addr: Addr,
    },
}

/// Why the rules refuse an edit, with what its wording needs.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Refused {
    /// The target or the patch is malformed.
    Malformed(Malformed),
    /// Nothing would change.
    NoChange,
    /// An empty way would be given a field.
    FieldsOnInvalidWay,
    /// A way would be made valid without a line.
    MissingLine,
    /// The line given, `addr`, does not fit the address width.
    LineOutOfRange {
        /// The address given.
        addr: Addr,
    },
    /// `line` belongs to set `set`, where way `way` already holds it.
    AlreadyElsewhere {
        /// The line.
        line: LineAddr,
        /// Its set.
        set: SetIndex,
        /// The way holding it.
        way: WayIndex,
    },
    /// `line` belongs to set `set`, whose way `way` is empty.
    WrongSet {
        /// The line.
        line: LineAddr,
        /// Its set.
        set: SetIndex,
        /// The lowest empty way there.
        way: WayIndex,
    },
    /// `line` belongs to set `set`, which is full.
    WrongSetFull {
        /// The line.
        line: LineAddr,
        /// Its set.
        set: SetIndex,
    },
    /// Way `way` of the same set already holds `line`.
    AlreadyHere {
        /// The line.
        line: LineAddr,
        /// The way holding it.
        way: WayIndex,
    },
    /// The line would be dirty under write-through.
    DirtyUnderWriteThrough,
    /// The line would be clean under write-back while word `word` is
    /// `value`, not memory's `memory`.
    NeedsDirty {
        /// The word named: the first edited word that differs from memory,
        /// or the first that differs.
        word: WordIndex,
        /// Its value in the way.
        value: Word,
        /// Its value in memory.
        memory: Word,
    },
    /// The line would differ from memory under write-through, at each of
    /// `differing`: a word's address and the way's value for it.
    WriteThroughMismatch {
        /// The words that differ.
        differing: Vec<(Addr, Word)>,
    },
    /// A rank is given where ranks do not change.
    RankNotEditable,
    /// The rank given is not below `valid`, the valid ways the set would
    /// have.
    RankOutOfRange {
        /// The valid ways the set would have.
        valid: u32,
    },
    /// The memory word already holds the value. `stale` is the dirty way
    /// holding another value for it, if one does.
    MemoryNoChange {
        /// The dirty way with another value.
        stale: Option<Holder>,
    },
}

impl From<Malformed> for Refused {
    fn from(malformed: Malformed) -> Self {
        Refused::Malformed(malformed)
    }
}

/// Plans `patch` on `target` against `machine`: the rules for ways and
/// memory words, in order, the first match deciding. The session's own
/// checks, that its log has room and its cursor is at the end of an op, are
/// not among them.
///
/// # Errors
///
/// The [`EditRefusal`] of the first rule that refuses the edit.
pub(crate) fn plan_edit(
    machine: &Machine,
    target: &EditTarget,
    patch: &EditPatch,
) -> Result<EditPlan, EditRefusal> {
    match decide(machine, target, patch) {
        Ok(Decision::Way(change)) => Ok(way_plan(machine, *target, &change)),
        Ok(Decision::Memory(change)) => Ok(memory_plan(machine, *target, change)),
        Err(refused) => Err(refusal(machine, target, patch, &refused)),
    }
}

/// Checks an `Edit` op against `machine`, as the engine checks every op
/// before it executes it.
///
/// # Errors
///
/// [`OpError::Edit`] with the refusal of the first rule that refuses it.
pub(crate) fn check(
    machine: &Machine,
    target: &EditTarget,
    patch: &EditPatch,
) -> Result<Decision, OpError> {
    decide(machine, target, patch)
        .map_err(|refused| OpError::Edit(Box::new(refusal(machine, target, patch, &refused))))
}

/// Checks what of an `Edit` op holds whatever the cache and memory contain,
/// against `machine`'s configuration: the target exists, the patch is of
/// the target's kind and shape, a way's line fits the address width, and a
/// memory word's address fits and is aligned and its value fits the word.
/// The rules that read the contents are left to the op's own check when it
/// is applied. A guided module's ops are checked this way when the module
/// is read.
///
/// # Errors
///
/// [`OpError::Edit`] with the refusal of the first of these the edit
/// breaks.
pub(crate) fn check_form(
    machine: &Machine,
    target: &EditTarget,
    patch: &EditPatch,
) -> Result<(), OpError> {
    let geometry = &machine.geometry;
    let form = match (*target, patch) {
        (EditTarget::Way { set, way }, EditPatch::Way(patch)) => way_form(machine, set, way, patch),
        (EditTarget::Memory { addr }, EditPatch::Memory { value }) => {
            if geometry.addr(addr.get()).is_none() {
                Err(Malformed::AddressOutOfRange { addr }.into())
            } else if geometry.fit_word(value.get()) != *value {
                Err(Malformed::ValueTooWide { value: *value }.into())
            } else if addr.get() % geometry.word_bytes() != 0 {
                Err(Malformed::Misaligned { addr }.into())
            } else {
                Ok(())
            }
        }
        // A patch of the other kind is refused before any rule reads the
        // contents.
        _ => decide(machine, target, patch).map(|_| ()),
    };
    form.map_err(|refused| OpError::Edit(Box::new(refusal(machine, target, patch, &refused))))
}

/// The form of a way's edit: the way exists, the patch has its shape, and
/// the line it gives fits the address width.
fn way_form(
    machine: &Machine,
    set: SetIndex,
    way: WayIndex,
    patch: &WayPatch,
) -> Result<(), Refused> {
    let geometry = &machine.geometry;
    let ways = machine.cache.set(set).ok_or(Malformed::NoSuchSet { set })?;
    ways.way(way).ok_or(Malformed::NoSuchWay { set, way })?;
    check_shape(geometry, set, patch)?;
    match patch
        .line
        .filter(|addr| geometry.addr(addr.get()).is_none())
    {
        Some(addr) => Err(Refused::LineOutOfRange { addr }),
        None => Ok(()),
    }
}

/// Why the session refuses any edit at its cursor before the rules see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Blocked {
    /// The log has no room for another entry where the edit would go, so
    /// the edit would be refused as an op, as any op would.
    Full,
    /// The log before where the edit would go holds as many ops as a link
    /// can hold, so the edit would be refused as an op, as any op would.
    LinkFull,
    /// The cursor is inside an op, which undoes as a whole, so an edit would
    /// apply after the op's end, not where the editor shows the machine.
    Unfinished(Unfinished),
}

impl Blocked {
    /// The refusal of `patch` on `target`, `machine` being the machine at
    /// the cursor: the session's limit or the link's, refused, or what to
    /// finish first, with caution; none offers another edit.
    pub(crate) fn refusal(
        self,
        machine: &Machine,
        target: &EditTarget,
        patch: &EditPatch,
    ) -> EditRefusal {
        let (reason, tone, title, text) = match self {
            Blocked::Full => (
                RefusalReason::SessionFull,
                Tone::Refused,
                explain::edit_refused(),
                OpError::SessionFull.to_string(),
            ),
            Blocked::LinkFull => (
                RefusalReason::LinkFull,
                Tone::Refused,
                explain::edit_refused(),
                OpError::LinkFull.to_string(),
            ),
            Blocked::Unfinished(unfinished) => (
                RefusalReason::MidAccess,
                Tone::Caution,
                explain::unfinished_title(unfinished),
                explain::unfinished_text(unfinished),
            ),
        };
        EditRefusal {
            reason,
            tone,
            title,
            text,
            suggestion: None,
            word_marks: word_marks(machine, target, patch),
        }
    }
}

/// Applies `decision`, an edit checked against `machine`, as the run's next
/// entry: one `Edit` event, then the invariant check of an entry's end.
///
/// Returns [`Flow::Stop`] when the sink stopped the edit, or when an
/// invariant no longer holds.
pub(crate) fn edit(machine: &mut Machine, decision: &Decision, out: &mut Emitter<'_>) -> Flow {
    out.begin_entry(machine);
    machine.access = None;
    let result = match decision {
        Decision::Way(change) => edit_way(machine, change, out),
        Decision::Memory(change) => edit_memory(machine, *change, out),
    };
    end_entry(machine, result, out)
}

/// The rules on `machine`, in order.
fn decide(machine: &Machine, target: &EditTarget, patch: &EditPatch) -> Result<Decision, Refused> {
    match (*target, patch) {
        (EditTarget::Way { set, way }, EditPatch::Way(patch)) => {
            decide_way(machine, set, way, patch).map(Decision::Way)
        }
        (EditTarget::Memory { addr }, EditPatch::Memory { value }) => {
            decide_memory(machine, addr, *value).map(Decision::Memory)
        }
        (EditTarget::Way { set, way }, EditPatch::Memory { .. }) => {
            let ways = machine.cache.set(set).ok_or(Malformed::NoSuchSet { set })?;
            ways.way(way).ok_or(Malformed::NoSuchWay { set, way })?;
            Err(Malformed::KindMismatch { way: true }.into())
        }
        (EditTarget::Memory { addr }, EditPatch::Way(_)) => {
            machine
                .geometry
                .addr(addr.get())
                .ok_or(Malformed::AddressOutOfRange { addr })?;
            Err(Malformed::KindMismatch { way: false }.into())
        }
    }
}

/// The rules for an edit of way `way` of set `set`.
fn decide_way(
    machine: &Machine,
    set: SetIndex,
    way: WayIndex,
    patch: &WayPatch,
) -> Result<WayChange, Refused> {
    let geometry = &machine.geometry;
    let ways = machine.cache.set(set).ok_or(Malformed::NoSuchSet { set })?;
    let current = ways.way(way).ok_or(Malformed::NoSuchWay { set, way })?;
    check_shape(geometry, set, patch)?;
    let before = fields_of(machine, set, current);
    let proposal = propose(machine, &before, patch);
    let after = proposal.fields;
    let was_valid = before.line.is_some();
    if proposal.valid == was_valid && after == before {
        return Err(Refused::NoChange);
    }
    if !proposal.valid {
        let given = patch.line.is_some()
            || patch.data.is_some()
            || patch.dirty.is_some()
            || patch.rank.is_some();
        if given {
            return Err(Refused::FieldsOnInvalidWay);
        }
        return Ok(way_change(ways.ways(), set, way, before, after));
    }
    let line = after.line.ok_or(Refused::MissingLine)?;
    if let Some(addr) = patch
        .line
        .filter(|addr| geometry.addr(addr.get()).is_none())
    {
        return Err(Refused::LineOutOfRange { addr });
    }
    let location = geometry.locate(Addr::new_unchecked(line.get()));
    if location.set != set {
        let home = machine
            .cache
            .set(location.set)
            .map(Set::ways)
            .unwrap_or_default();
        let held = (0_u32..)
            .zip(home)
            .find_map(|(index, other)| (other.tag() == Some(location.tag)).then_some(index));
        if let Some(index) = held {
            return Err(Refused::AlreadyElsewhere {
                line,
                set: location.set,
                way: WayIndex::new_unchecked(index),
            });
        }
        let empty = (0_u32..)
            .zip(home)
            .find_map(|(index, other)| (!other.is_valid()).then_some(index));
        return Err(match empty {
            Some(index) => Refused::WrongSet {
                line,
                set: location.set,
                way: WayIndex::new_unchecked(index),
            },
            None => Refused::WrongSetFull {
                line,
                set: location.set,
            },
        });
    }
    let held = (0_u32..).zip(ways.ways()).find_map(|(index, other)| {
        (index != way.get() && other.tag() == Some(location.tag)).then_some(index)
    });
    if let Some(index) = held {
        return Err(Refused::AlreadyHere {
            line,
            way: WayIndex::new_unchecked(index),
        });
    }
    check_memory(machine, &before, &after, line)?;
    if let Some(rank) = patch.rank {
        if !editable(machine) {
            return Err(Refused::RankNotEditable);
        }
        let valid = ways.ways().iter().filter(|other| other.is_valid()).count();
        let valid = u32::try_from(valid)
            .unwrap_or(u32::MAX)
            .saturating_add(u32::from(!was_valid));
        if rank >= valid {
            return Err(Refused::RankOutOfRange { valid });
        }
    }
    Ok(way_change(ways.ways(), set, way, before, after))
}

/// The shape of a way's patch: one value per word, each fitting the word,
/// and a rank some way of the set can have.
fn check_shape(geometry: &Geometry, set: SetIndex, patch: &WayPatch) -> Result<(), Malformed> {
    if let Some(data) = &patch.data {
        let given = u32::try_from(data.len()).unwrap_or(u32::MAX);
        if given != geometry.words_per_line() {
            return Err(Malformed::DataLength { given });
        }
        if let Some(&value) = data
            .iter()
            .find(|value| geometry.fit_word(value.get()) != **value)
        {
            return Err(Malformed::ValueTooWide { value });
        }
    }
    if patch.rank.is_some_and(|rank| rank >= geometry.ways()) {
        return Err(Malformed::RankTooHigh { set });
    }
    Ok(())
}

/// The rules that compare a valid way's data with memory's copy of its
/// line, `line`: no dirty line under write-through, no clean line other than
/// memory under write-back, and no line other than memory under
/// write-through.
fn check_memory(
    machine: &Machine,
    before: &Fields,
    after: &Fields,
    line: LineAddr,
) -> Result<(), Refused> {
    let write_through = machine.config.cache.write_policy == WritePolicy::WriteThrough;
    if after.dirty && write_through {
        return Err(Refused::DirtyUnderWriteThrough);
    }
    let memory = line_words(machine, line);
    let differing: Vec<usize> = (0..after.words.len())
        .filter(|&index| after.words.get(index) != memory.get(index))
        .collect();
    if differing.is_empty() || after.dirty {
        return Ok(());
    }
    if write_through {
        let word_bytes = machine.geometry.word_bytes();
        let differing = differing
            .iter()
            .filter_map(|&index| {
                let value = after.words.get(index)?;
                let word = u32::try_from(index).ok()?;
                let addr = Addr::new_unchecked(line.get() + word * word_bytes);
                Some((addr, Word::new_unchecked(*value)))
            })
            .collect();
        return Err(Refused::WriteThroughMismatch { differing });
    }
    // The word named is the one the student edited, when one of those
    // differs; a way that takes a line holds memory's copy of every word it
    // is not given, so each of its differing words was given.
    let moved = after.line != before.line;
    let named = differing
        .iter()
        .find(|&&index| moved || before.words.get(index) != after.words.get(index))
        .or(differing.first())
        .copied()
        .unwrap_or(0);
    Err(Refused::NeedsDirty {
        word: WordIndex::new_unchecked(u32::try_from(named).unwrap_or(0)),
        value: Word::new_unchecked(after.words.get(named).copied().unwrap_or(0)),
        memory: Word::new_unchecked(memory.get(named).copied().unwrap_or(0)),
    })
}

/// The rules for an edit of the memory word at `addr` to `value`.
fn decide_memory(machine: &Machine, addr: Addr, value: Word) -> Result<MemoryChange, Refused> {
    let geometry = &machine.geometry;
    if geometry.addr(addr.get()).is_none() {
        return Err(Malformed::AddressOutOfRange { addr }.into());
    }
    if geometry.fit_word(value.get()) != value {
        return Err(Malformed::ValueTooWide { value }.into());
    }
    if addr.get() % geometry.word_bytes() != 0 {
        return Err(Malformed::Misaligned { addr }.into());
    }
    let from = machine.memory.word(addr);
    let holder = holder(machine, addr);
    if value == from {
        return Err(Refused::MemoryNoChange {
            stale: holder.filter(|holder| holder.dirty && holder.cached != from),
        });
    }
    Ok(MemoryChange {
        addr,
        from,
        to: value,
        holder,
    })
}

/// The valid way that holds the line of the memory word at `addr`, and its
/// value for the word.
fn holder(machine: &Machine, addr: Addr) -> Option<Holder> {
    let geometry = &machine.geometry;
    let location = geometry.locate(addr);
    let word_bytes = geometry.word_bytes();
    let set = machine.cache.set(location.set)?;
    (0_u32..)
        .zip(set.ways())
        .find_map(|(index, way)| match &way.slot {
            Slot::Valid { tag, dirty, data } if *tag == location.tag => Some(Holder {
                set: location.set,
                way: WayIndex::new_unchecked(index),
                word: location.word,
                dirty: *dirty == Dirtiness::Dirty,
                cached: Word::new_unchecked(word_at(
                    data,
                    location.word.get() * word_bytes,
                    word_bytes,
                )),
            }),
            _ => None,
        })
}

/// The fields of `way`, a way of set `set`.
fn fields_of(machine: &Machine, set: SetIndex, way: &Way) -> Fields {
    match &way.slot {
        Slot::Valid { tag, dirty, data } => Fields {
            line: Some(machine.geometry.line_addr(*tag, set)),
            words: words_of(&machine.geometry, data),
            dirty: *dirty == Dirtiness::Dirty,
            rank: way.rank,
        },
        Slot::Invalid => Fields {
            line: None,
            words: Vec::new(),
            dirty: false,
            rank: way.rank,
        },
    }
}

/// The way `patch` makes of a way whose fields are `before`. A field the
/// patch leaves out keeps its value, or takes the one the edit implies: a
/// way that takes a line holds memory's copy of it, clean, and ranks first,
/// as a fill would; a way made empty ranks last; ranks that never change do
/// not. An empty way has no rank to give, so a way left empty takes none
/// from the patch: one kept empty changes nothing, and a rank sent with an
/// invalidation is refused as a field of an empty way.
fn propose(machine: &Machine, before: &Fields, patch: &WayPatch) -> Proposal {
    let was_valid = before.line.is_some();
    let valid = patch.valid.unwrap_or(was_valid);
    let rank = if editable(machine) && valid {
        patch
            .rank
            .unwrap_or(if was_valid { before.rank } else { 0 })
    } else if editable(machine) && was_valid {
        machine.geometry.ways().saturating_sub(1)
    } else {
        before.rank
    };
    if !valid {
        return Proposal {
            valid,
            fields: Fields {
                line: None,
                words: Vec::new(),
                dirty: false,
                rank,
            },
        };
    }
    let line = patch
        .line
        .map(|addr| line_of(&machine.geometry, addr))
        .or(before.line);
    let moved = line != before.line;
    let words = match &patch.data {
        Some(data) => data.iter().map(|value| value.get()).collect(),
        None if moved => line
            .map(|line| line_words(machine, line))
            .unwrap_or_default(),
        None => before.words.clone(),
    };
    Proposal {
        valid,
        fields: Fields {
            line,
            words,
            dirty: patch.dirty.unwrap_or(!moved && before.dirty),
            rank,
        },
    }
}

/// Whether ranks can be edited: under LRU and FIFO, in sets of more than
/// one way.
fn editable(machine: &Machine) -> bool {
    machine.config.cache.replacement != Replacement::Random && machine.geometry.ways() > 1
}

/// The change of way `way` of set `set`, whose ways are `ways`, from
/// `before` to `after`, with the ranks it moves.
fn way_change(
    ways: &[Way],
    set: SetIndex,
    way: WayIndex,
    before: Fields,
    after: Fields,
) -> WayChange {
    let moves = rank_moves(ways, way, after.rank);
    WayChange {
        set,
        way,
        before,
        after,
        moves,
    }
}

/// The rank moves that take way `target` of `ways` to rank `to` and keep
/// the ranks a permutation: from rank `o`, the ways ranked from `to` up to
/// `o` move back one place, or those from `o` up to `to` move forward one.
fn rank_moves(ways: &[Way], target: WayIndex, to: u32) -> Vec<RankMove> {
    let Some(from) = usize::try_from(target.get())
        .ok()
        .and_then(|index| ways.get(index))
        .map(|way| way.rank)
    else {
        return Vec::new();
    };
    if from == to {
        return Vec::new();
    }
    (0_u32..)
        .zip(ways)
        .filter_map(|(index, way)| {
            let rank = way.rank;
            let moved = if index == target.get() {
                to
            } else if to < from && (to..from).contains(&rank) {
                rank.saturating_add(1)
            } else if to > from && rank > from && rank <= to {
                rank.saturating_sub(1)
            } else {
                return None;
            };
            Some(RankMove {
                way: WayIndex::new_unchecked(index),
                from: rank,
                to: moved,
            })
        })
        .collect()
}

/// The plan of `change`, an edit of `target` the rules accept.
fn way_plan(machine: &Machine, target: EditTarget, change: &WayChange) -> EditPlan {
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
fn memory_plan(machine: &Machine, target: EditTarget, change: MemoryChange) -> EditPlan {
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
fn refusal(
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
fn word_marks(machine: &Machine, target: &EditTarget, patch: &EditPatch) -> Vec<WordMark> {
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
fn items(machine: &Machine, change: &WayChange) -> Vec<WayItem> {
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
fn cells(machine: &Machine, change: &WayChange) -> Vec<ChangeRef> {
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

/// Applies `change` to its way: the new fields and ranks, memory's record
/// of a line the way takes, and what the classification of misses remembers
/// of the lines it takes and loses; then its `Edit` event.
fn edit_way(machine: &mut Machine, change: &WayChange, out: &mut Emitter<'_>) -> Result<(), Halt> {
    let geometry = machine.geometry;
    let (set, way) = (change.set, change.way);
    let (before, after) = (&change.before, &change.after);
    let slot = match after.line {
        Some(line) => Slot::Valid {
            tag: tag_of(&geometry, line),
            dirty: if after.dirty {
                Dirtiness::Dirty
            } else {
                Dirtiness::Clean
            },
            data: bytes_of(&geometry, &after.words),
        },
        None => Slot::Invalid,
    };
    let Some(ways) = machine.cache.set_mut(set) else {
        return Err(lost(machine, set));
    };
    let Some(target) = ways.way_mut(way) else {
        return Err(lost(machine, set));
    };
    target.slot = slot;
    set_ranks(ways.ways_mut(), &change.moves, |moved| moved.to);
    if before.line != after.line {
        if let Some(old) = before.line {
            machine.filled.remove(&old);
            machine.shadow.forget(old);
        }
        if let Some(new) = after.line {
            machine.memory.touch(new);
            machine.filled.insert(new);
            machine.shadow.fill(new);
        }
    }
    let draft = Draft {
        set: Some(set),
        way: Some(way),
        changed: cells(machine, change),
        ..Draft::new(EventKind::Edit, None)
    };
    out.emit(machine, draft, || {
        explain::edited_way(&geometry, set, way, &items(machine, change))
    })
}

/// Applies `change` to memory, and to the clean line that holds the word,
/// if one does; then its `Edit` event.
fn edit_memory(
    machine: &mut Machine,
    change: MemoryChange,
    out: &mut Emitter<'_>,
) -> Result<(), Halt> {
    let geometry = machine.geometry;
    let MemoryChange {
        addr,
        from,
        to,
        holder,
    } = change;
    machine.memory.write_word(addr, to);
    let mut changed = vec![ChangeRef::Memory { addr }];
    let note = match holder {
        None => MemoryNote::MemoryOnly,
        Some(Holder {
            set,
            way,
            word,
            dirty: false,
            cached,
        }) => {
            let word_bytes = geometry.word_bytes();
            match machine.cache.way_mut(set, way).map(|held| &mut held.slot) {
                Some(Slot::Valid { data, .. }) => {
                    set_word_at(data, word.get() * word_bytes, word_bytes, to.get());
                }
                _ => return Err(lost(machine, set)),
            }
            if cached != to {
                changed.push(ChangeRef::Word { set, way, word });
            }
            MemoryNote::CleanCopy { set, way }
        }
        Some(Holder {
            set, way, cached, ..
        }) if cached == to => MemoryNote::MatchesDirty { set, way },
        Some(Holder {
            set, way, cached, ..
        }) if cached == from => MemoryNote::BecomesStale { set, way },
        Some(Holder { set, way, .. }) => MemoryNote::StaysStale { set, way },
    };
    changed.sort_unstable();
    let draft = Draft {
        addr: Some(addr),
        changed,
        ..Draft::new(EventKind::Edit, None)
    };
    out.emit(machine, draft, || {
        explain::edited_memory(&geometry, addr, from, to, note)
    })
}

/// The way editor's draft for `form` on way `way` of set `set`: each field
/// read, the smallest patch that makes the way what the form shows, and
/// its plan, or the refusal of a field that does not read, or the session's
/// refusal while it is `blocked`.
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
    blocked: Option<Blocked>,
) -> EditDraft {
    let geometry = &machine.geometry;
    let target = EditTarget::Way { set, way };
    let typed = TypedLine::read(geometry, &form.line_text);
    let Some(current) = machine.cache.way(set, way) else {
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
            outcome: outcome(machine, &target, &patch, blocked, None),
            patch: Some(patch),
            result: snapshot::lone_way_view(machine, set, way, &empty),
        };
    };
    let before = fields_of(machine, set, current);
    let was_valid = before.line.is_some();
    let valid = form.valid;
    let line = if valid {
        typed.line.or(before.line)
    } else {
        None
    };
    let moved = valid && line != before.line;
    let shown = match line {
        Some(line) if moved => line_words(machine, line),
        None if valid => Vec::new(),
        _ => before.words.clone(),
    };
    let texts = WordTexts::read(geometry, form, &shown);
    let mut patch = WayPatch::default();
    if valid != was_valid {
        patch.valid = Some(valid);
    }
    if let Some(typed) = typed.line.filter(|typed| Some(*typed) != before.line) {
        patch.line = Some(Addr::new_unchecked(typed.get()));
    }
    if let Some(words) = texts.words.as_ref().filter(|words| **words != shown) {
        patch.data = Some(words.iter().copied().map(Word::new_unchecked).collect());
    }
    let dirty = if moved { false } else { before.dirty };
    if form.dirty != dirty {
        patch.dirty = Some(form.dirty);
    }
    let rank = propose(
        machine,
        &before,
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
    let invalid = typed
        .invalid
        .map(|message| {
            (
                RefusalReason::LineOutOfRange,
                explain::not_an_address(),
                message,
            )
        })
        .or_else(|| {
            texts.invalid.map(|(word, message)| {
                (
                    RefusalReason::ValueTooWide,
                    explain::check_word(word),
                    message,
                )
            })
        });
    let patch = EditPatch::Way(patch);
    let reads = invalid.is_none();
    EditDraft {
        line: typed.input,
        words: texts.inputs,
        tag_text: typed.tag_text,
        index_text: typed.index_text,
        word_texts: (0..geometry.words_per_line())
            .map(|word| {
                usize::try_from(word)
                    .ok()
                    .and_then(|index| shown.get(index))
                    .map_or_else(String::new, |value| format::plain(*value))
            })
            .collect(),
        outcome: outcome(machine, &target, &patch, blocked, invalid),
        result: result_view(machine, set, way, current, &patch),
        patch: reads.then_some(patch),
    }
}

/// The memory editor's draft for `value_text` on the word at `addr`: the
/// value read, its patch and its plan, or the refusal of a value that does
/// not read, or the session's refusal while it is `blocked`. An empty value
/// is the word's own, which changes nothing.
pub(crate) fn draft_memory(
    machine: &Machine,
    addr: Addr,
    value_text: &str,
    blocked: Option<Blocked>,
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
        outcome: outcome(machine, &target, &patch, blocked, invalid),
        patch: reads.then_some(patch),
    }
}

/// An editor's outcome for `patch` on `target`: the session's refusal while
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

/// `addr`'s line: the address with its offset bits cleared.
fn line_of(geometry: &Geometry, addr: Addr) -> LineAddr {
    LineAddr::new_unchecked(addr.get() & !geometry.line_bytes().saturating_sub(1))
}

/// The tag of `line`.
fn tag_of(geometry: &Geometry, line: LineAddr) -> Tag {
    geometry.locate(Addr::new_unchecked(line.get())).tag
}

/// Memory's copy of `line`, one value per word.
fn line_words(machine: &Machine, line: LineAddr) -> Vec<u64> {
    words_of(&machine.geometry, &machine.memory.line(line))
}

/// The words of a line's bytes, `data`.
fn words_of(geometry: &Geometry, data: &[u8]) -> Vec<u64> {
    let word_bytes = geometry.word_bytes();
    (0..geometry.words_per_line())
        .map(|word| word_at(data, word * word_bytes, word_bytes))
        .collect()
}

/// A line's bytes, holding `words` little-endian.
fn bytes_of(geometry: &Geometry, words: &[u64]) -> Vec<u8> {
    let word_bytes = geometry.word_bytes();
    let mut data = vec![0; usize::try_from(geometry.line_bytes()).unwrap_or(0)];
    for (word, value) in (0_u32..).zip(words) {
        set_word_at(&mut data, word * word_bytes, word_bytes, *value);
    }
    data
}
