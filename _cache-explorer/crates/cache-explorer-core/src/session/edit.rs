//! Edits at the cursor: the plan of an edit and the editors' drafts.
//!
//! An edit is appended at the end of the op holding the cursor, as every op
//! is, and that op undoes as a whole. So the session refuses an edit unless
//! the cursor is at the end of an op: the machine the editor shows is then
//! the one the edit applies to, and a plan shown is the plan applied. The
//! refusal names what to finish first; Shift+Space goes to the op's end.
//! Before that, as for every op, the session refuses an edit while a guided
//! module's prediction is unanswered, while its log has no room for another
//! entry there, already holds as many ops before it as a link can hold, or
//! would take its saved file past what a file can hold with the edit. Then
//! the machine's own rules decide.

use super::{slot_of, EntryKind, EntryRecord, Session};
use crate::engine::{self, Blocked, EditDraft, EditPlan, EditRefusal, MemoryEditDraft, WayForm};
use crate::explain::Unfinished;
use crate::op::{EditPatch, EditTarget, Op};
use crate::share;
use crate::snapshot;
use crate::units::{Addr, EventPos, OpIndex, SetIndex, WayIndex};

impl Session {
    /// Plans `patch` on `target` at the cursor: refused while a guided
    /// module's prediction is unanswered, while the log has no room for
    /// another entry where the edit would go, or for another op a link
    /// could hold, or for the edit in a saved file, or while the cursor is
    /// inside an op, and otherwise planned by the rules on the machine at
    /// the cursor, which is the machine [`Session::apply`] checks the edit
    /// against.
    ///
    /// # Errors
    ///
    /// The [`EditRefusal`] of the gate, the session's limit, the link's or
    /// the saved file's, of the cursor's place, or of the first rule that
    /// refuses the edit.
    pub fn plan_edit(
        &self,
        target: &EditTarget,
        patch: &EditPatch,
    ) -> Result<EditPlan, EditRefusal> {
        match self.blocked(target, patch) {
            Some(blocked) => Err(blocked.refusal(&self.current, target, patch)),
            None => engine::plan_edit(&self.current, target, patch),
        }
    }

    /// The way editor's draft of `form` for way `way` of set `set` at the
    /// cursor: every field read, the patch Apply sends, its plan or refusal,
    /// and the way as it would be.
    pub fn draft_edit(&self, set: SetIndex, way: WayIndex, form: &WayForm) -> EditDraft {
        engine::draft_way(&self.current, set, way, form, |target, patch| {
            self.blocked(target, patch)
        })
    }

    /// The memory editor's draft of `value_text` for the word at `addr` at
    /// the cursor: the value read, the patch Apply sends, and its plan or
    /// refusal.
    pub fn draft_memory_edit(&self, addr: Addr, value_text: &str) -> MemoryEditDraft {
        engine::draft_memory(&self.current, addr, value_text, |target, patch| {
            self.blocked(target, patch)
        })
    }

    /// Why the session refuses the edit of `patch` on `target` at the
    /// cursor before the rules see it, if it does: a guided module's
    /// unanswered prediction, or, as [`Session::apply`] checks in the same
    /// order, the log up to where the edit would go, the end of the op
    /// holding the cursor, leaves no room for another entry, holds as many
    /// ops as a link can, or leaves its saved file no room for the edit's
    /// op; or the cursor is inside that op.
    fn blocked(&self, target: &EditTarget, patch: &EditPatch) -> Option<Blocked> {
        if self.gate.is_some() {
            return Some(Blocked::Gated);
        }
        let (base, kept) = self.base();
        if !engine::has_room(u64::from(base.get())) {
            return Some(Blocked::Full);
        }
        if !engine::has_op_room(kept) {
            return Some(Blocked::LinkFull);
        }
        let op = Op::Edit {
            target: *target,
            patch: patch.clone(),
        };
        if !self
            .saved
            .room_after(slot_of(kept))
            .fits(&share::op_json(&op))
        {
            return Some(Blocked::FileFull);
        }
        self.unfinished().map(Blocked::Unfinished)
    }

    /// What the cursor must finish before an edit, when it is inside an op:
    /// the entry it is in, when that is the op's last numbered entry or its
    /// only one, and otherwise every entry of the op, since Shift+Space goes
    /// to the op's end, not to the end of the access the cursor may be in.
    /// None at the end of an op, position 0 included.
    fn unfinished(&self) -> Option<Unfinished> {
        if self.is_op_end(self.cursor) {
            return None;
        }
        let op = self.op_holding(self.cursor)?;
        let end = self.op_end(op)?;
        let start = op
            .get()
            .checked_sub(1)
            .and_then(|before| self.op_end(OpIndex::new_unchecked(before)))
            .unwrap_or(EventPos::new_unchecked(0));
        let from = self.entries.partition_point(|record| record.first <= start);
        let to = self.entries.partition_point(|record| record.first <= end);
        let numbered: Vec<(u32, &EntryRecord)> = self
            .entries
            .get(from..to)
            .unwrap_or_default()
            .iter()
            .filter_map(|record| record.number.map(|number| (number, record)))
            .collect();
        let (&(first, _), &(last, record)) = (numbered.first()?, numbered.last()?);
        if first == last || record.holds(self.cursor) {
            Some(Unfinished::Entry {
                noun: snapshot::noun(record.kind),
                number: last,
            })
        } else {
            Some(Unfinished::Entries {
                first,
                last,
                error: record.kind == EntryKind::Error,
            })
        }
    }
}
