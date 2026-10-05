//! The ops of a saved file as its JSON holds them, and the room the file has
//! left for more.
//!
//! A session keeps each op's JSON as the op is appended, so that a file or a
//! link written after a cursor move serializes only the rest of the
//! document, and the file's size is known op by op without writing it. The
//! file's frame, the most bytes it takes besides its ops' JSON, and its limit
//! are the caller's: this module only counts.

/// A session's ops as its saved file writes them, kept as ops are appended
/// and dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedOps {
    /// The most bytes the file takes besides its ops' JSON, whatever the
    /// cursor's place and the mode.
    frame: usize,
    /// The most bytes the file may take.
    limit: usize,
    /// Each op's JSON, in order, with a comma between two: what the
    /// document's `ops` array holds between its brackets.
    text: String,
    /// Where each op's JSON ends in `text`.
    ends: Vec<usize>,
}

/// The room a saved file has for more ops, taken as ops are checked in
/// turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileRoom {
    /// The bytes left under the limit.
    left: usize,
    /// Whether the file holds an op already, from which a comma separates
    /// the next.
    after_op: bool,
}

/// The file has no room for an op.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

impl SavedOps {
    /// No op yet, for a file that takes `frame` bytes besides its ops' JSON
    /// and may take `limit` in all.
    pub fn new(frame: usize, limit: usize) -> Self {
        Self {
            frame,
            limit,
            text: String::new(),
            ends: Vec::new(),
        }
    }

    /// The ops' JSON, a comma between two.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Keeps the JSON of the first `ops` ops only.
    pub fn truncate(&mut self, ops: usize) {
        // Each end is where an op's JSON ended as it was appended, so it
        // falls between two characters.
        self.text.truncate(self.len_of(ops));
        self.ends.truncate(ops);
    }

    /// Appends the next op's JSON, `json`.
    pub fn push(&mut self, json: &str) {
        if !self.ends.is_empty() {
            self.text.push(',');
        }
        self.text.push_str(json);
        self.ends.push(self.text.len());
    }

    /// The room the file has for ops after its first `ops`, those after
    /// them not counted, since an op appended there drops them.
    pub fn room_after(&self, ops: usize) -> FileRoom {
        let used = self.frame.saturating_add(self.len_of(ops));
        FileRoom {
            left: self.limit.saturating_sub(used),
            after_op: ops.min(self.ends.len()) > 0,
        }
    }

    /// The bytes of the first `ops` ops' JSON, the commas between them
    /// included.
    fn len_of(&self, ops: usize) -> usize {
        ops.min(self.ends.len())
            .checked_sub(1)
            .and_then(|last| self.ends.get(last))
            .copied()
            .unwrap_or(0)
    }
}

impl FileRoom {
    /// Whether an op whose JSON is `json` fits.
    pub fn fits(self, json: &str) -> bool {
        self.needed(json) <= self.left
    }

    /// Takes the room of an op whose JSON is `json`.
    ///
    /// # Errors
    ///
    /// [`Full`] when it does not fit; nothing is taken.
    pub fn take(&mut self, json: &str) -> Result<(), Full> {
        if !self.fits(json) {
            return Err(Full);
        }
        self.left = self.left.saturating_sub(self.needed(json));
        self.after_op = true;
        Ok(())
    }

    /// The bytes an op whose JSON is `json` adds to the file, with the comma
    /// before it.
    fn needed(self, json: &str) -> usize {
        json.len().saturating_add(usize::from(self.after_op))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    /// A frame of 10 bytes in a limit of 100.
    fn empty_log() -> SavedOps {
        SavedOps::new(10, 100)
    }

    #[test]
    fn a_log_with_no_op_has_the_room_the_frame_leaves() {
        let room = empty_log().room_after(0);
        assert!(room.fits(&"x".repeat(90)));
        assert!(!room.fits(&"x".repeat(91)));
    }

    #[test]
    fn ops_are_kept_with_a_comma_between_two() {
        let mut saved = empty_log();
        saved.push("{\"a\":1}");
        assert_eq!(saved.text(), "{\"a\":1}");
        saved.push("{\"b\":2}");
        assert_eq!(saved.text(), "{\"a\":1},{\"b\":2}");
    }

    #[test]
    fn an_op_takes_its_json_and_a_comma_before_it_after_the_first() {
        let mut saved = empty_log();
        let mut room = saved.room_after(0);
        for (kept, json) in (1..).zip(["aaaa", "bb"]) {
            room.take(json).unwrap();
            saved.push(json);
            assert_eq!(room, saved.room_after(kept));
        }
        // The frame, "aaaa", a comma and "bb" take 17 of the 100 bytes; the
        // next op's comma takes one of the 83 left.
        assert!(room.fits(&"x".repeat(82)));
        assert!(!room.fits(&"x".repeat(83)));
    }

    #[test]
    fn an_op_that_fills_the_room_to_the_byte_fits_and_one_byte_more_does_not() {
        let mut room = empty_log().room_after(0);
        let exact = "x".repeat(90);
        let over = "x".repeat(91);
        assert!(room.fits(&exact));
        assert!(!room.fits(&over));
        let before = room;
        assert_eq!(room.take(&over), Err(Full));
        assert_eq!(room, before, "a refused op takes nothing");
        room.take(&exact).unwrap();
        assert!(!room.fits(""), "the next op needs at least its comma");
    }

    #[test]
    fn dropping_ops_gives_their_room_back() {
        let mut saved = empty_log();
        saved.push("aaaa");
        saved.push("bb");
        saved.truncate(1);
        assert_eq!(saved.text(), "aaaa");
        assert_eq!(saved.room_after(1), {
            let mut room = empty_log().room_after(0);
            room.take("aaaa").unwrap();
            room
        });
        saved.truncate(0);
        assert_eq!(saved.text(), "");
        assert_eq!(saved.room_after(0), empty_log().room_after(0));
    }

    #[test]
    fn counting_past_the_ops_kept_counts_those() {
        let mut saved = empty_log();
        saved.push("aaaa");
        assert_eq!(saved.room_after(5), saved.room_after(1));
    }

    #[test]
    fn a_frame_over_the_limit_leaves_no_room() {
        let room = SavedOps::new(200, 100).room_after(0);
        assert!(!room.fits("x"));
    }
}
