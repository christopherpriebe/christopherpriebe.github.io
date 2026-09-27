//! Edits at the cursor: the plan of an edit and the editors' drafts.
//!
//! An edit is appended at the end of the op holding the cursor, as every op
//! is, and that op undoes as a whole. So the session refuses an edit unless
//! the cursor is at the end of an op: the machine the editor shows is then
//! the one the edit applies to, and a plan shown is the plan applied. The
//! refusal names what to finish first; Shift+Space goes to the op's end.
//! Before that, as for every op, the session refuses an edit while its log
//! has no room for another entry there, or already holds as many ops before
//! it as a link can hold. Then the machine's own rules decide.

use super::{EntryKind, EntryRecord, Session};
use crate::engine::{self, Blocked, EditDraft, EditPlan, EditRefusal, MemoryEditDraft, WayForm};
use crate::explain::Unfinished;
use crate::op::{EditPatch, EditTarget};
use crate::snapshot;
use crate::units::{Addr, EventPos, OpIndex, SetIndex, WayIndex};

impl Session {
    /// Plans `patch` on `target` at the cursor: refused while the log has no
    /// room for another entry where the edit would go, or for another op a
    /// link could hold, or while the cursor is inside an op, and otherwise
    /// planned by the rules on the machine at the cursor, which is the
    /// machine [`Session::apply`] checks the edit against.
    ///
    /// # Errors
    ///
    /// The [`EditRefusal`] of the session's limit or the link's, of the
    /// cursor's place, or of the first rule that refuses the edit.
    pub fn plan_edit(
        &self,
        target: &EditTarget,
        patch: &EditPatch,
    ) -> Result<EditPlan, EditRefusal> {
        match self.blocked() {
            Some(blocked) => Err(blocked.refusal(&self.current, target, patch)),
            None => engine::plan_edit(&self.current, target, patch),
        }
    }

    /// The way editor's draft of `form` for way `way` of set `set` at the
    /// cursor: every field read, the patch Apply sends, its plan or refusal,
    /// and the way as it would be.
    pub fn draft_edit(&self, set: SetIndex, way: WayIndex, form: &WayForm) -> EditDraft {
        engine::draft_way(&self.current, set, way, form, self.blocked())
    }

    /// The memory editor's draft of `value_text` for the word at `addr` at
    /// the cursor: the value read, the patch Apply sends, and its plan or
    /// refusal.
    pub fn draft_memory_edit(&self, addr: Addr, value_text: &str) -> MemoryEditDraft {
        engine::draft_memory(&self.current, addr, value_text, self.blocked())
    }

    /// Why the session refuses any edit at the cursor before the rules see
    /// it, if it does: the log up to where the edit would go, the end of the
    /// op holding the cursor, leaves no room for another entry, or holds as
    /// many ops as a link can, which [`Session::apply`] checks first, in
    /// that order; or the cursor is inside that op.
    fn blocked(&self) -> Option<Blocked> {
        let (base, kept) = self.base();
        if !engine::has_room(u64::from(base.get())) {
            return Some(Blocked::Full);
        }
        if !engine::has_op_room(kept) {
            return Some(Blocked::LinkFull);
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
