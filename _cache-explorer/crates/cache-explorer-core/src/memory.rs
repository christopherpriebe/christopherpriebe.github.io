//! Main memory, and whether memory's copy of a word is stale.
//!
//! Memory keeps a line's bytes once the run has touched it: read by a fill,
//! or written by a write-back, a write-through or a write-around. A line no
//! one has touched reads as the configuration's initial contents, so the map
//! is also the list of lines the run has touched, which the memory view shows
//! even for lines that were only read.
//!
//! Words are little-endian, in memory as in the cache's lines. A word in
//! memory is stale when a valid dirty way holds its line with a different
//! value for it; that is derived when asked for, never stored.

use std::collections::BTreeMap;

use crate::cache::{Dirtiness, Slot};
use crate::config::MemoryInit;
use crate::geometry::Geometry;
use crate::machine::Machine;
use crate::units::{Addr, LineAddr, SetIndex, WayIndex, Word};

/// Main memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Memory {
    /// The lines the run has touched, with their bytes, `line_bytes` each.
    lines: BTreeMap<LineAddr, Vec<u8>>,
    /// What an untouched line reads as.
    init: MemoryInit,
    /// Bytes per line.
    line_bytes: u32,
    /// Bytes per word, which the pattern's values and word reads need.
    word_bytes: u32,
}

/// A stale word of memory: a valid dirty way holds its line with a newer
/// value for it. What the memory view's stale mark and its tooltip show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaleInfo {
    /// The set of the way holding the newer value.
    pub set: SetIndex,
    /// The way holding the newer value.
    pub way: WayIndex,
    /// The newer value, in the cache.
    pub cached: Word,
    /// The older value, in memory.
    pub memory: Word,
}

impl Memory {
    /// The memory a run of `geometry` starts with: nothing touched, every
    /// line reading as `init`.
    pub(crate) fn new(geometry: &Geometry, init: MemoryInit) -> Self {
        Self {
            lines: BTreeMap::new(),
            init,
            line_bytes: geometry.line_bytes(),
            word_bytes: geometry.word_bytes(),
        }
    }

    /// The word at `addr`, which is a multiple of the word size.
    pub(crate) fn word(&self, addr: Addr) -> Word {
        let (line, offset) = self.split(addr);
        let value = match self.lines.get(&line) {
            Some(bytes) => word_at(bytes, offset, self.word_bytes),
            None => self.initial_word(addr.get()),
        };
        Word::new_unchecked(value)
    }

    /// The bytes of `line`, `line_bytes` of them.
    pub(crate) fn line(&self, line: LineAddr) -> Vec<u8> {
        self.lines
            .get(&line)
            .cloned()
            .unwrap_or_else(|| self.initial_line(line))
    }

    /// True when `line`'s bytes are `data`, as a clean cached copy's must be.
    pub(crate) fn holds(&self, line: LineAddr, data: &[u8]) -> bool {
        match self.lines.get(&line) {
            Some(bytes) => bytes == data,
            None => self.initial_line(line) == data,
        }
    }

    /// True when the run has touched `line`.
    pub(crate) fn is_touched(&self, line: LineAddr) -> bool {
        self.lines.contains_key(&line)
    }

    /// Records `line` as touched, with the contents it has now.
    pub(crate) fn touch(&mut self, line: LineAddr) {
        self.touched_line(line);
    }

    /// Writes `value`, already fitted to the word, to the word at `addr`, a
    /// multiple of the word size, and records its line as touched.
    pub(crate) fn write_word(&mut self, addr: Addr, value: Word) {
        let (line, offset) = self.split(addr);
        let word_bytes = self.word_bytes;
        set_word_at(self.touched_line(line), offset, word_bytes, value.get());
    }

    /// Writes a whole line back, and records it as touched.
    pub(crate) fn write_line(&mut self, line: LineAddr, data: &[u8]) {
        let bytes = self.touched_line(line);
        for (byte, value) in bytes.iter_mut().zip(data) {
            *byte = *value;
        }
    }

    /// The bytes of `line` for changing, recorded as touched first.
    fn touched_line(&mut self, line: LineAddr) -> &mut Vec<u8> {
        let initial = if self.lines.contains_key(&line) {
            Vec::new()
        } else {
            self.initial_line(line)
        };
        self.lines.entry(line).or_insert(initial)
    }

    /// The line holding `addr`, and `addr`'s byte within it.
    fn split(&self, addr: Addr) -> (LineAddr, u32) {
        // `line_bytes` is a power of two, so this clears the offset bits.
        let offset = addr.get() & (self.line_bytes - 1);
        (LineAddr::new_unchecked(addr.get() - offset), offset)
    }

    /// What the word at `addr` reads before the run writes it: 0, or the
    /// design's filler, `(addr / word_bytes × 13 + 5) mod 97`, which fits
    /// every word size.
    fn initial_word(&self, addr: u32) -> u64 {
        match self.init {
            MemoryInit::Zero => 0,
            MemoryInit::Pattern => {
                let index = u64::from(addr)
                    .checked_div(u64::from(self.word_bytes))
                    .unwrap_or(0);
                (index * 13 + 5) % 97
            }
        }
    }

    /// The bytes of `line` before the run writes it.
    fn initial_line(&self, line: LineAddr) -> Vec<u8> {
        let mut bytes = vec![0; usize::try_from(self.line_bytes).unwrap_or(0)];
        if self.init == MemoryInit::Pattern {
            let words = self.line_bytes.checked_div(self.word_bytes).unwrap_or(0);
            for offset in (0..words).map(|word| word * self.word_bytes) {
                let value = self.initial_word(line.get().saturating_add(offset));
                set_word_at(&mut bytes, offset, self.word_bytes, value);
            }
        }
        bytes
    }
}

impl Machine {
    /// The stale mark of the word holding `addr`: `Some` when a valid dirty
    /// way holds its line and has a value for that word that differs from
    /// memory's. Derived from the cache and memory each time, never stored;
    /// at most one way can hold the line, since no set holds a tag twice.
    pub fn stale(&self, addr: Addr) -> Option<StaleInfo> {
        let location = self.geometry.locate(addr);
        let word_bytes = self.geometry.word_bytes();
        let start = location.word.get() * word_bytes;
        let set = self.cache.set(location.set)?;
        let (way, cached) = (0_u32..)
            .zip(set.ways())
            .find_map(|(index, way)| match &way.slot {
                Slot::Valid {
                    tag,
                    dirty: Dirtiness::Dirty,
                    data,
                } if *tag == location.tag => Some((index, word_at(data, start, word_bytes))),
                _ => None,
            })?;
        let memory = self
            .memory
            .word(Addr::new_unchecked(location.line.get() + start));
        (memory.get() != cached).then(|| StaleInfo {
            set: location.set,
            way: WayIndex::new_unchecked(way),
            cached: Word::new_unchecked(cached),
            memory,
        })
    }
}

/// The little-endian word of `word_bytes` bytes at byte `offset` of `bytes`.
/// A line always has room for each of its words, so no byte is ever missing;
/// one that were would read as 0.
pub(crate) fn word_at(bytes: &[u8], offset: u32, word_bytes: u32) -> u64 {
    (0..word_bytes).fold(0, |word, index| {
        let byte = offset
            .checked_add(index)
            .and_then(|at| usize::try_from(at).ok())
            .and_then(|at| bytes.get(at))
            .copied()
            .unwrap_or(0);
        word | u64::from(byte).checked_shl(index * u8::BITS).unwrap_or(0)
    })
}

/// Writes the low `word_bytes` bytes of `value`, little-endian, at byte
/// `offset` of `bytes`.
pub(crate) fn set_word_at(bytes: &mut [u8], offset: u32, word_bytes: u32, value: u64) {
    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    let width = usize::try_from(word_bytes).unwrap_or(0);
    for (byte, value) in bytes
        .iter_mut()
        .skip(start)
        .take(width)
        .zip(value.to_le_bytes())
    {
        *byte = value;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::{Config, Preset, WordBytes};

    fn memory(config: &Config) -> Memory {
        Memory::new(&config.validate().unwrap(), config.memory_init)
    }

    fn addr(value: u32) -> Addr {
        Addr::new_unchecked(value)
    }

    fn line(value: u32) -> LineAddr {
        LineAddr::new_unchecked(value)
    }

    #[test]
    fn zero_memory_reads_zero_and_touches_nothing() {
        let memory = memory(&Config::default());
        assert_eq!(memory.word(addr(0x0A4)), Word::new_unchecked(0));
        assert_eq!(memory.line(line(0x0A0)), vec![0; 16]);
        assert!(!memory.is_touched(line(0x0A0)));
    }

    #[test]
    fn pattern_memory_reads_the_design_filler() {
        // 0x0A0 to 0x0AC read 40, 53, 66, 79 with 4-byte words.
        let memory = memory(&Preset::DirectMapped8.config());
        let words: Vec<u64> = [0x0A0, 0x0A4, 0x0A8, 0x0AC]
            .into_iter()
            .map(|at| memory.word(addr(at)).get())
            .collect();
        assert_eq!(words, [40, 53, 66, 79]);
        // Word index 22, 0x058, is the one word in 97 that reads 0.
        assert_eq!(memory.word(addr(0x058)).get(), 0);
        // Little-endian: the value is in the word's lowest byte.
        let bytes = memory.line(line(0x0A0));
        assert_eq!(&bytes[..8], [40, 0, 0, 0, 53, 0, 0, 0]);
        assert!(!memory.is_touched(line(0x0A0)));
    }

    #[test]
    fn the_pattern_follows_the_word_size() {
        for word_bytes in WordBytes::ALL {
            let config = Config {
                word_bytes,
                ..Preset::DirectMapped8.config()
            };
            let memory = memory(&config);
            let size = word_bytes.bytes();
            for index in 0..16 / size {
                let at = 0x100 + index * size;
                let expected = (u64::from(at / size) * 13 + 5) % 97;
                assert_eq!(memory.word(addr(at)).get(), expected, "{size}-byte words");
                let offset = at % 16;
                assert_eq!(word_at(&memory.line(line(0x100)), offset, size), expected);
            }
        }
    }

    #[test]
    fn touching_records_a_line_with_its_current_contents() {
        let mut memory = memory(&Preset::DirectMapped8.config());
        let before = memory.line(line(0x0A0));
        memory.touch(line(0x0A0));
        assert!(memory.is_touched(line(0x0A0)));
        assert_eq!(memory.line(line(0x0A0)), before);
        assert!(memory.holds(line(0x0A0), &before));
        // Touching again keeps what was written.
        memory.write_word(addr(0x0A4), Word::new_unchecked(7));
        memory.touch(line(0x0A0));
        assert_eq!(memory.word(addr(0x0A4)).get(), 7);
    }

    #[test]
    fn writing_a_word_touches_its_line_and_keeps_the_rest() {
        let mut memory = memory(&Preset::DirectMapped8.config());
        memory.write_word(addr(0x0A8), Word::new_unchecked(0x0102));
        assert!(memory.is_touched(line(0x0A0)));
        assert!(!memory.is_touched(line(0x0B0)));
        let words: Vec<u64> = [0x0A0, 0x0A4, 0x0A8, 0x0AC]
            .into_iter()
            .map(|at| memory.word(addr(at)).get())
            .collect();
        assert_eq!(words, [40, 53, 0x0102, 79]);
        assert_eq!(&memory.line(line(0x0A0))[8..12], [0x02, 0x01, 0, 0]);
    }

    #[test]
    fn writing_a_line_replaces_its_bytes() {
        let mut memory = memory(&Config::default());
        let data: Vec<u8> = (1..=16).collect();
        memory.write_line(line(0x1A0), &data);
        assert!(memory.is_touched(line(0x1A0)));
        assert_eq!(memory.line(line(0x1A0)), data);
        assert!(memory.holds(line(0x1A0), &data));
        assert!(!memory.holds(line(0x1A0), &[0; 16]));
        assert_eq!(memory.word(addr(0x1A4)).get(), 0x0807_0605);
    }

    #[test]
    fn words_are_little_endian_at_every_size() {
        let mut bytes = vec![0; 16];
        set_word_at(&mut bytes, 8, 8, 0x0102_0304_0506_0708);
        assert_eq!(&bytes[8..], [8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(word_at(&bytes, 8, 8), 0x0102_0304_0506_0708);
        assert_eq!(word_at(&bytes, 8, 2), 0x0708);
        assert_eq!(word_at(&bytes, 9, 1), 7);
        // Only the word's own bytes are written.
        set_word_at(&mut bytes, 0, 2, 0xFFFF_FFFF);
        assert_eq!(&bytes[..4], [0xFF, 0xFF, 0, 0]);
        // Past the end, nothing is written and missing bytes read as 0.
        set_word_at(&mut bytes, 14, 4, u64::MAX);
        assert_eq!(&bytes[14..], [0xFF, 0xFF]);
        assert_eq!(word_at(&bytes, 14, 4), 0xFFFF);
    }

    #[test]
    fn an_untouched_line_differs_from_other_bytes() {
        let memory = memory(&Preset::DirectMapped8.config());
        let initial = memory.line(line(0x0A0));
        assert!(memory.holds(line(0x0A0), &initial));
        assert!(!memory.holds(line(0x0A0), &[0; 16]));
        assert!(!memory.is_touched(line(0x0A0)));
    }
}
