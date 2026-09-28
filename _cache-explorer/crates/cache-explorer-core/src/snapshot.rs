//! The snapshot: what the page draws at the cursor, as text and flags, so it
//! computes and formats nothing itself.
//!
//! [`State`] is built from the machine at the cursor, the event there, and
//! what the session knows of the cursor's entry. The cache shows every set
//! and way with its texts, rank marks and the victim; while the cursor is on
//! an access, the access's set and the result of its tag compare, measured
//! against the tags the set held before the access. Memory lists the lines
//! the run has touched and the ones the student asked to watch, windowed by
//! [`Session::memory_lines`](crate::Session::memory_lines). The lanes show
//! the transfer of the event at the cursor, lit only on the event that
//! makes it, and the access in the cache. The CPU names the cursor's entry
//! and counts its steps, or a stall's cycles; where the cursor is on no
//! entry's step, and on the last step of the entry that ended the program,
//! it says what the loaded program can do.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::cache::{Set, Slot, Way};
use crate::config::Config;
use crate::cpu::Halted;
use crate::engine::Origin;
use crate::event::{ChangeRef, EventKind, MicroEvent};
use crate::explain::{self, EntryNoun};
use crate::format;
use crate::geometry::{Decoded, Geometry};
use crate::machine::{AccessContext, Machine};
use crate::memory::{word_at, Memory};
use crate::op::{AccessKind, ProgramFormat};
use crate::session::{Boundary, EntryKind, EntryRecord, Position};
use crate::stats::Stats;
use crate::units::{Addr, EventPos, LineAddr, Register, SetIndex, WayIndex, WordIndex};

/// Everything the page draws at the cursor.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct State {
    /// The displayed run's configuration.
    pub config: Config,
    /// Whether the page shows Advanced: the mode chosen, or Advanced while
    /// the displayed run needs it.
    pub advanced: bool,
    /// The displayed run's geometry.
    pub geometry: Geometry,
    /// Where the cursor is.
    pub position: Position,
    /// The event the cursor rests on; absent at position 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<MicroEvent>,
    /// The cache table.
    pub cache: CacheView,
    /// The memory table's heading and size; its rows come from
    /// `memoryLines`.
    pub memory: MemoryView,
    /// The transfer lane between the cache and memory.
    pub bus: BusView,
    /// The request lane between the CPU and the cache.
    pub request: RequestView,
    /// The CPU box.
    pub cpu: CpuView,
    /// The loaded program, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<ProgramView>,
    /// The Stats region.
    pub stats: Stats,
    /// What the cursor's entry has changed so far, net: the cells, words
    /// and counters to draw on the changed ground. Empty at a run's origin
    /// and on a program load.
    pub highlight: Vec<ChangeRef>,
    /// The highlight in words, as the log's "changed" line gives it:
    /// "set 2 way 0 valid, tag, data · r1 · misses", or "nothing changed".
    pub changed_summary: String,
    /// Which controls are live.
    pub controls: Controls,
    /// The status line and the configuration's summary.
    pub labels: Labels,
}

/// The head of the cache table's rank column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum RankHeader {
    /// "LRU": ranks by recency of use.
    Lru,
    /// "Age": ranks by order of filling, under FIFO.
    Age,
}

/// The mark under a rank at either end of its set's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum RankRole {
    /// Rank 0 under LRU: most recently used.
    Mru,
    /// The last rank under LRU: least recently used, the next victim.
    Lru,
    /// Rank 0 under FIFO: filled most recently.
    New,
    /// The last rank under FIFO: filled earliest, the next victim.
    Old,
}

/// A way's rank, and its mark at either end of the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct RankCell {
    /// The rank.
    pub value: u32,
    /// The mark, for the first and the last rank.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<RankRole>,
}

/// What a comparator shows for one way of the selected set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum WayCompare {
    /// `=?`: the compare has not happened yet.
    Pending,
    /// `=`: the way was valid and held the tag.
    Match,
    /// `≠`: the way was valid and held another tag.
    Mismatch,
    /// The way was invalid, so it held no tag to compare.
    Invalid,
}

/// One way of the cache, as its cell shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct WayView {
    /// Whether it holds a line.
    pub valid: bool,
    /// Its tag, `0x05`; absent while invalid, which the cell shows as `—`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_text: Option<String>,
    /// The address of its line, `0x0A0`; absent while invalid.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_text: Option<String>,
    /// Whether its line is dirty.
    pub dirty: bool,
    /// Its words, in order; empty while invalid, which the cell shows as a
    /// `·` per word.
    pub words: Vec<String>,
    /// Its rank, when the rank column is shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<RankCell>,
    /// Whether a miss has chosen it and has not filled it yet.
    pub victim: bool,
    /// The word the access at the cursor reads or writes, once the way holds
    /// the access's line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addressed_word: Option<WordIndex>,
    /// What the cell describes itself as: "Set 2, way 0: valid, tag 0x05,
    /// dirty, rank 1, victim."
    pub description: String,
}

/// One set of the cache.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct SetView {
    /// The set.
    pub set: SetIndex,
    /// Its ways, in order.
    pub ways: Vec<WayView>,
}

/// The cache table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct CacheView {
    /// The heading: "L1 cache · 4 sets × 2 ways × 16 B".
    pub header: String,
    /// The heads of the word columns: `+0`, `+4`, `+8`, `+C`.
    pub word_heads: Vec<String>,
    /// Whether the rank column is shown: under LRU and FIFO with more than
    /// one way.
    pub show_rank: bool,
    /// The rank column's head, when it is shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank_header: Option<RankHeader>,
    /// Whether the dirty column is shown: always, in Basics as in Advanced
    /// and under either write policy, since the mode changes only which
    /// controls the page shows.
    pub show_dirty: bool,
    /// Whether a set's ways wrap onto rows, and the comparators become a
    /// badge in each way: with more than four ways.
    pub wrap: bool,
    /// The set of the access at the cursor.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_set: Option<SetIndex>,
    /// One comparator per way of the selected set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compare: Option<Vec<WayCompare>>,
    /// Every set, in order.
    pub sets: Vec<SetView>,
}

/// A stale word of memory: the dirty way holding a newer value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Stale {
    /// The set of the way holding the newer value.
    pub set: SetIndex,
    /// The way.
    pub way: WayIndex,
    /// "Stale: set 2, way 1 holds the newer value 7."
    pub text: String,
}

/// One word of memory, as its cell shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct MemoryWordView {
    /// The word's address.
    pub addr: Addr,
    /// Its value: "53".
    pub text: String,
    /// Its stale mark, when a dirty way holds a newer value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<Stale>,
    /// What the cell describes itself as: "0x0A4 = 53."
    pub description: String,
}

/// A way of the cache, named by its set and its number. A way predicate
/// names its way this way too; one read from JSON is unchecked until the
/// module is checked against the configuration in force.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WayLoc {
    /// The set.
    pub set: SetIndex,
    /// The way.
    pub way: WayIndex,
}

/// One line of memory, as its row shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct MemoryLineView {
    /// The line's address.
    pub line: LineAddr,
    /// The address: `0x0A0`.
    pub line_text: String,
    /// The line's tag: `0x02`.
    pub tag_text: String,
    /// The set it maps to.
    pub set: SetIndex,
    /// The way holding it, when the cache does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resident: Option<WayLoc>,
    /// Whether it is the line the event at the cursor moves.
    pub on_bus: bool,
    /// Whether the student asked to watch it: a "Jump to" row.
    pub watched: bool,
    /// Its words, in order.
    pub words: Vec<MemoryWordView>,
}

/// Rows of the memory table in order, as the boundary returns a window of
/// them: an array in JavaScript, which one serializer call builds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct MemoryLines(pub Vec<MemoryLineView>);

/// The memory table's heading and size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct MemoryView {
    /// "5 lines touched".
    pub caption: String,
    /// The heads of the word columns, as the cache's.
    pub word_heads: Vec<String>,
    /// The rows: the lines touched and the lines watched.
    pub line_count: u32,
    /// The row of the line the event at the cursor moves, to scroll it into
    /// view.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bus_row: Option<u32>,
}

/// What the transfer lane shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum Transfer {
    /// Nothing moves.
    Idle,
    /// A line comes in from memory.
    Fill,
    /// A dirty line goes back to memory.
    WriteBack,
    /// A word goes to memory: written through, or around the cache, which
    /// the lane draws in the same style.
    WriteThrough,
}

/// The transfer lane between the cache and memory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct BusView {
    /// What moves.
    pub transfer: Transfer,
    /// The lane's label: "Fill · memory line 0x2A0 → set 2, way 1", or "Bus
    /// idle".
    pub text: String,
    /// In timed mode, what the transfer costs: "20 cycles".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycles_text: Option<String>,
    /// The line that moves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<LineAddr>,
}

/// Whether an access is in the cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum RequestStatus {
    /// The cursor rests on an access.
    Issued,
    /// It does not.
    None,
}

/// The request lane between the CPU and the cache, and where the address
/// strip's address comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct RequestView {
    /// Whether an access is in the cache.
    pub status: RequestStatus,
    /// Whether it loads or stores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<AccessKind>,
    /// The lane's label: "Load 0x0A4 → cache", or "No request".
    pub text: String,
    /// The instruction the access stands for: `ld r1, [0x0A4]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The address strip's footer while it shows the access: "from access
    /// #5"; empty otherwise, when the page shows its own copy.
    pub source_text: String,
    /// The access's address, decoded for the address strip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoded: Option<Decoded>,
}

/// What the CPU is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CpuStatus {
    /// On no entry's step, at position 0 or a run's origin, with no program
    /// loaded; accesses can be issued.
    NoProgram,
    /// On no entry's step, at a run's origin or a program load, with an
    /// instruction of the loaded program to execute.
    Ready,
    /// On a step of a numbered entry, its last included.
    Busy {
        /// The entry's number.
        entry: u32,
        /// The step the cursor is on, from 1.
        seq: u32,
        /// The entry's steps.
        of: u32,
    },
    /// Waiting for memory in timed mode.
    Stalled {
        /// The cycles left.
        remaining: u32,
        /// The stall's length in all.
        total: u32,
    },
    /// On the last step of the instruction after which the loaded program
    /// has none left to execute.
    Finished,
    /// On the error that stopped the loaded program on an address it could
    /// not access.
    Faulted,
    /// On the error that stopped the loaded program at the end of its
    /// budget, or of the session's room.
    OutOfBudget,
}

/// The CPU box's hit or miss mark for the access at the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum HitMarker {
    /// The access's tag compare found its line.
    Hit,
    /// It did not.
    Miss,
    /// No compare to show: no access at the cursor, or not compared yet.
    None,
}

/// A register and its value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct RegisterView {
    /// The register.
    pub register: Register,
    /// Its value: "53".
    pub text: String,
}

/// The CPU box.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct CpuView {
    /// The instruction of the cursor's entry: `ld r1, [0x0A4]`; "—" when
    /// there is none.
    pub instruction: String,
    /// What the CPU is doing.
    pub status: CpuStatus,
    /// The status in words: "Access #5 · step 3 of 8", "Stalled: 40
    /// cycles", "Ready", "Finished", "Stopped at line 4".
    pub status_text: String,
    /// The hit or miss mark.
    pub hit_marker: HitMarker,
    /// The registers, `r0` to `r7`.
    pub registers: Vec<RegisterView>,
}

/// One line of the loaded program's source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct ProgramLine {
    /// Its number, from 1.
    pub number: u32,
    /// The line as written.
    pub text: String,
}

/// The loaded program, as the drawer lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct ProgramView {
    /// Whether it is assembly or a trace.
    pub format: ProgramFormat,
    /// Its source as written.
    pub source: String,
    /// Its source, line by line.
    pub lines: Vec<ProgramLine>,
    /// The line of the instruction at the pc: the next to execute, the one
    /// in flight, or the one the program stopped at; absent once it has
    /// finished.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_line: Option<u32>,
    /// Whether that instruction is in flight: an access that has not had
    /// its last event.
    pub executing: bool,
    /// How many instructions it has; for a trace, how many accesses.
    pub instructions: u32,
    /// How many it has executed since it was loaded.
    pub executed: u32,
    /// Whether it has executed its last instruction.
    pub finished: bool,
    /// The drawer's line: `5 instructions · next: ld r2, [0x0A8]`.
    pub summary: String,
}

/// A control of the page that the snapshot can switch off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum ControlAction {
    /// Undo, Z.
    Undo,
    /// Redo, Y.
    Redo,
    /// Step, Space.
    Step,
    /// Step access, Shift+Space.
    StepAccess,
    /// Run, R.
    Run,
    /// Reset.
    Reset,
    /// Issue an access.
    Issue,
    /// Open a way or memory editor.
    Edit,
    /// Change the configuration.
    Configure,
    /// Load a program from the drawer.
    Program,
    /// Load a saved run or a trace.
    Load,
}

/// Which controls are live.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Controls {
    /// The live controls; every other is disabled.
    pub enabled: Vec<ControlAction>,
    /// The furthest position the cursor may reach, while a prediction is
    /// unanswered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seek_limit: Option<EventPos>,
    /// Why controls are disabled, while a prediction is unanswered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard: Option<String>,
}

/// The texts over the workbench.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub struct Labels {
    /// The status line: "Basics · 14 steps in this run", "Advanced · 14
    /// steps in this run · timed".
    pub status_text: String,
    /// The configuration's summary in the mode shown: "128 B · 16 B lines ·
    /// 2-way · LRU".
    pub config_summary: String,
}

impl CacheView {
    /// The cache of `machine`, with the access in progress or just made.
    pub(crate) fn new(machine: &Machine) -> Self {
        let geometry = &machine.geometry;
        let rank_header = machine
            .config
            .cache
            .replacement
            .policy()
            .rank_header(geometry.ways());
        let access = machine.access.as_ref();
        let sets = (0..geometry.sets())
            .map(SetIndex::new_unchecked)
            .map(|set| SetView {
                set,
                ways: machine
                    .cache
                    .set(set)
                    .map(Set::ways)
                    .unwrap_or_default()
                    .iter()
                    .zip(0_u32..)
                    .map(|(way, index)| {
                        way_view(
                            machine,
                            set,
                            WayIndex::new_unchecked(index),
                            way,
                            rank_header,
                            access,
                        )
                    })
                    .collect(),
            })
            .collect();
        Self {
            header: explain::cache_header(geometry),
            word_heads: word_heads(geometry),
            show_rank: rank_header.is_some(),
            rank_header,
            show_dirty: true,
            wrap: geometry.ways() > WAYS_IN_A_ROW,
            selected_set: access.map(|access| access.location.set),
            compare: access.map(|access| compare(geometry, access)),
            sets,
        }
    }
}

/// The most ways a set shows side by side, as the design draws them; more
/// wrap onto rows.
const WAYS_IN_A_ROW: u32 = 4;

/// The view of `way`, way `index` of `set`.
fn way_view(
    machine: &Machine,
    set: SetIndex,
    index: WayIndex,
    way: &Way,
    rank_header: Option<RankHeader>,
    access: Option<&AccessContext>,
) -> WayView {
    let geometry = &machine.geometry;
    let tag = way.tag();
    let tag_text = tag.map(|tag| format::tag(tag.get(), geometry.tag_bits()));
    let words = match &way.slot {
        Slot::Valid { data, .. } => (0..geometry.words_per_line())
            .map(|word| {
                let start = word.saturating_mul(geometry.word_bytes());
                format::data(word_at(data, start, geometry.word_bytes()))
            })
            .collect(),
        Slot::Invalid => Vec::new(),
    };
    let rank = rank_header.map(|header| RankCell {
        value: way.rank,
        role: rank_role(header, way.rank, geometry.ways()),
    });
    let at_set = access.filter(|access| access.location.set == set);
    let victim = at_set
        .and_then(|access| access.victim)
        .is_some_and(|victim| victim.way == index);
    let addressed_word = at_set
        .filter(|access| access.way == Some(index) && tag == Some(access.location.tag))
        .map(|access| access.location.word);
    let description = explain::way_description(
        set,
        index,
        tag_text.as_deref(),
        way.is_dirty(),
        rank.map(|rank| rank.value),
        victim,
    );
    WayView {
        valid: way.is_valid(),
        line_text: tag.map(|tag| {
            format::address(geometry.line_addr(tag, set).get(), geometry.address_bits())
        }),
        tag_text,
        dirty: way.is_dirty(),
        words,
        rank,
        victim,
        addressed_word,
        description,
    }
}

/// The view of `way` as way `index` of set `set` would show it on its own,
/// outside any access: what an editor shows of the way an edit would leave.
pub(crate) fn lone_way_view(
    machine: &Machine,
    set: SetIndex,
    index: WayIndex,
    way: &Way,
) -> WayView {
    let rank_header = machine
        .config
        .cache
        .replacement
        .policy()
        .rank_header(machine.geometry.ways());
    way_view(machine, set, index, way, rank_header, None)
}

/// The mark of `rank` in a set of `ways` ways ranked as `header` says.
fn rank_role(header: RankHeader, rank: u32, ways: u32) -> Option<RankRole> {
    let last = ways.saturating_sub(1);
    match header {
        RankHeader::Lru if rank == 0 => Some(RankRole::Mru),
        RankHeader::Lru if rank == last => Some(RankRole::Lru),
        RankHeader::Age if rank == 0 => Some(RankRole::New),
        RankHeader::Age if rank == last => Some(RankRole::Old),
        RankHeader::Lru | RankHeader::Age => None,
    }
}

/// The comparators of `access`'s set: `=?` on every way until the tag
/// compare, then what each way held when it was compared.
fn compare(geometry: &Geometry, access: &AccessContext) -> Vec<WayCompare> {
    match &access.lookup {
        None => (0..geometry.ways()).map(|_| WayCompare::Pending).collect(),
        Some(lookup) => lookup
            .tags
            .iter()
            .map(|held| match held {
                None => WayCompare::Invalid,
                Some(tag) if *tag == access.location.tag => WayCompare::Match,
                Some(_) => WayCompare::Mismatch,
            })
            .collect(),
    }
}

/// The heads of the word columns of `geometry`'s lines.
fn word_heads(geometry: &Geometry) -> Vec<String> {
    (0..geometry.words_per_line())
        .map(|word| format::word_head(word.saturating_mul(geometry.word_bytes())))
        .collect()
}

/// The rows of the memory table: every line a run has touched and every
/// watched line that fits its addresses, taken to the start of its line, in
/// ascending order and each once, with whether it is watched. The touched
/// lines are walked where the memory keeps them, never copied, so a run that
/// has touched many lines costs a render no more than a walk along them.
pub(crate) struct MemoryRows<'m> {
    /// The run's memory, whose touched lines are rows.
    memory: &'m Memory,
    /// The watched lines that fit the run's addresses, each at the start of
    /// its line.
    watched: BTreeSet<LineAddr>,
}

impl<'m> MemoryRows<'m> {
    /// The rows of `machine`'s memory table with the lines of `watch`.
    pub(crate) fn new(machine: &'m Machine, watch: &BTreeSet<LineAddr>) -> Self {
        let geometry = &machine.geometry;
        // Extended rather than collected: collecting sorts the lines first,
        // with a sort the wasm would carry for this type alone.
        let mut watched = BTreeSet::new();
        watched.extend(
            watch
                .iter()
                .filter_map(|line| geometry.addr(line.get()))
                .map(|addr| geometry.locate(addr).line),
        );
        Self {
            memory: &machine.memory,
            watched,
        }
    }

    /// The watched lines the run has not touched, which are rows of their
    /// own.
    fn untouched(&self) -> impl Iterator<Item = LineAddr> + '_ {
        self.watched
            .iter()
            .copied()
            .filter(|line| !self.memory.is_touched(*line))
    }

    /// How many rows there are.
    pub(crate) fn count(&self) -> u32 {
        let untouched = u32::try_from(self.untouched().count()).unwrap_or(u32::MAX);
        self.memory.touched_count().saturating_add(untouched)
    }

    /// The row of `line`, when it is one.
    pub(crate) fn row_of(&self, line: LineAddr) -> Option<u32> {
        let listed = self.memory.is_touched(line) || self.watched.contains(&line);
        listed.then(|| {
            let untouched = self
                .untouched()
                .take_while(|watched| *watched < line)
                .count();
            self.memory
                .touched_before(line)
                .saturating_add(u32::try_from(untouched).unwrap_or(u32::MAX))
        })
    }

    /// The rows in order, each with whether it is watched.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (LineAddr, bool)> + '_ {
        let mut touched = self.memory.touched().peekable();
        let mut watched = self.watched.iter().copied().peekable();
        std::iter::from_fn(
            move || match (touched.peek().copied(), watched.peek().copied()) {
                (Some(both), Some(line)) if both == line => {
                    touched.next();
                    watched.next();
                    Some((both, true))
                }
                (Some(line), Some(next)) if line < next => {
                    touched.next();
                    Some((line, false))
                }
                (_, Some(line)) => {
                    watched.next();
                    Some((line, true))
                }
                (Some(line), None) => {
                    touched.next();
                    Some((line, false))
                }
                (None, None) => None,
            },
        )
    }
}

impl MemoryView {
    /// The memory table's heading for `machine`, whose table has `rows`, with
    /// `bus` the line on the bus.
    pub(crate) fn new(machine: &Machine, rows: &MemoryRows<'_>, bus: Option<LineAddr>) -> Self {
        Self {
            caption: explain::memory_caption(machine.memory.touched_count()),
            word_heads: word_heads(&machine.geometry),
            line_count: rows.count(),
            bus_row: bus.and_then(|bus| rows.row_of(bus)),
        }
    }
}

impl MemoryLineView {
    /// Line `line` of `machine`'s memory, `watched` or not, with `bus` the
    /// line on the bus.
    pub(crate) fn new(
        machine: &Machine,
        line: LineAddr,
        watched: bool,
        bus: Option<LineAddr>,
    ) -> Self {
        let geometry = &machine.geometry;
        let location = geometry.locate(Addr::new_unchecked(line.get()));
        let resident = machine.cache.set(location.set).and_then(|set| {
            (0_u32..)
                .zip(set.ways())
                .find(|(_, way)| way.tag() == Some(location.tag))
                .map(|(way, _)| WayLoc {
                    set: location.set,
                    way: WayIndex::new_unchecked(way),
                })
        });
        let words = (0..geometry.words_per_line())
            .map(|word| {
                let addr = Addr::new_unchecked(
                    line.get()
                        .saturating_add(word.saturating_mul(geometry.word_bytes())),
                );
                let value = machine.memory.word(addr);
                let stale = machine.stale(addr).map(|info| Stale {
                    set: info.set,
                    way: info.way,
                    text: explain::stale(info.set, info.way, info.cached),
                });
                MemoryWordView {
                    addr,
                    text: format::data(value.get()),
                    description: explain::memory_word(
                        geometry,
                        addr,
                        value,
                        stale.as_ref().map(|stale| stale.text.as_str()),
                    ),
                    stale,
                }
            })
            .collect();
        Self {
            line,
            line_text: format::address(line.get(), geometry.address_bits()),
            tag_text: format::tag(location.tag.get(), geometry.tag_bits()),
            set: location.set,
            resident,
            on_bus: bus == Some(line),
            watched,
            words,
        }
    }
}

/// What the transfer lane shows at `event`: the transfer it makes and the
/// line it moves; none for an event that moves nothing between the cache and
/// memory.
fn transfer(geometry: &Geometry, event: &MicroEvent) -> Option<(Transfer, LineAddr)> {
    let line = geometry.locate(event.addr?).line;
    let transfer = match event.kind {
        EventKind::Fill => Transfer::Fill,
        EventKind::WriteBack => Transfer::WriteBack,
        EventKind::WriteThrough | EventKind::MemWrite => Transfer::WriteThrough,
        _ => return None,
    };
    Some((transfer, line))
}

/// The line the event at the cursor moves between the cache and memory.
pub(crate) fn bus_line(machine: &Machine, event: Option<&MicroEvent>) -> Option<LineAddr> {
    event
        .and_then(|event| transfer(&machine.geometry, event))
        .map(|(_, line)| line)
}

impl BusView {
    /// The transfer lane at `event`, on `machine`.
    pub(crate) fn new(machine: &Machine, event: Option<&MicroEvent>) -> Self {
        let geometry = &machine.geometry;
        let Some((event, (transfer, line))) =
            event.and_then(|event| Some((event, transfer(geometry, event)?)))
        else {
            return Self {
                transfer: Transfer::Idle,
                text: explain::bus_idle(),
                cycles_text: None,
                line: None,
            };
        };
        let text = match (event.kind, event.set.zip(event.way)) {
            (EventKind::Fill, Some((set, way))) => explain::fill_transfer(geometry, line, set, way),
            (EventKind::WriteBack, Some((set, way))) => {
                explain::write_back_transfer(geometry, line, set, way)
            }
            (EventKind::WriteThrough, Some((set, way))) => {
                explain::write_through_transfer(geometry, line, set, way)
            }
            _ => explain::write_around_transfer(geometry, line),
        };
        // A transfer that costs the miss penalty: every one but a
        // write-through, which a hit's time covers.
        let charged = matches!(
            event.kind,
            EventKind::Fill | EventKind::WriteBack | EventKind::MemWrite
        );
        Self {
            transfer,
            text,
            cycles_text: machine
                .config
                .timing
                .filter(|_| charged)
                .map(|timing| format::cycles(timing.miss_cycles)),
            line: Some(line),
        }
    }
}

impl RequestView {
    /// The request lane on `machine`, whose cursor rests on `entry`.
    pub(crate) fn new(machine: &Machine, entry: Option<&EntryRecord>) -> Self {
        let geometry = &machine.geometry;
        match (machine.access.as_ref(), entry) {
            (Some(access), Some(entry)) if entry.kind == EntryKind::Access => {
                let (kind, addr) = (access.request.kind(), access.request.addr());
                Self {
                    status: RequestStatus::Issued,
                    kind: Some(kind),
                    text: explain::request(geometry, kind, addr),
                    source: Some(entry.label.clone()),
                    source_text: entry
                        .number
                        .map(explain::request_source)
                        .unwrap_or_default(),
                    decoded: geometry.decode(addr.get()).ok(),
                }
            }
            _ => Self {
                status: RequestStatus::None,
                kind: None,
                text: explain::no_request(),
                source: None,
                source_text: String::new(),
                decoded: None,
            },
        }
    }
}

impl CpuView {
    /// The CPU box on `machine`, with the cursor at `position` on `entry`.
    pub(crate) fn new(machine: &Machine, entry: Option<&EntryRecord>, position: &Position) -> Self {
        let instruction = entry
            .filter(|entry| entry.kind != EntryKind::Program)
            .map_or_else(explain::no_instruction, |entry| entry.label.clone());
        // Every event of a numbered entry names it, its last included,
        // except the last event of the entry that finished or stopped the
        // program, where the program's state takes over at once.
        let busy = entry
            .filter(|entry| {
                position.boundary == Boundary::MidEntry || !ends_program(machine, entry)
            })
            .and_then(|entry| Some((entry.number?, noun(entry.kind))));
        let (status, status_text) = if let Some(stall) = machine.stall() {
            (
                CpuStatus::Stalled {
                    remaining: stall.remaining,
                    total: stall.total,
                },
                explain::stalled(stall.remaining),
            )
        } else if let Some((number, noun)) = busy {
            (
                CpuStatus::Busy {
                    entry: number,
                    seq: position.seq,
                    of: position.entry_length,
                },
                explain::entry_status(noun, number, position.seq, position.entry_length),
            )
        } else {
            between_entries(machine)
        };
        let hit_marker = match machine
            .access
            .as_ref()
            .and_then(|access| access.lookup.as_ref())
        {
            Some(lookup) if lookup.hit.is_some() => HitMarker::Hit,
            Some(_) => HitMarker::Miss,
            None => HitMarker::None,
        };
        Self {
            instruction,
            status,
            status_text,
            hit_marker,
            registers: Register::ALL
                .into_iter()
                .map(|register| RegisterView {
                    register,
                    text: format::data(machine.registers.get(register)),
                })
                .collect(),
        }
    }
}

/// Whether `entry`, complete on `machine`, is the one that ended the loaded
/// program: the error that stopped it, or the instruction after which it
/// had none left to execute.
fn ends_program(machine: &Machine, entry: &EntryRecord) -> bool {
    let Some(program) = &machine.program else {
        return false;
    };
    match entry.kind {
        EntryKind::Error => machine.cpu.halted.is_some(),
        EntryKind::Instruction | EntryKind::Access => {
            // An access issued by hand is none of the program's.
            let executed = entry.kind == EntryKind::Instruction
                || machine
                    .access
                    .as_ref()
                    .is_some_and(|access| access.request.origin() == Origin::Program);
            executed && machine.cpu.halted.is_none() && program.get(machine.cpu.pc).is_none()
        }
        EntryKind::Edit | EntryKind::Program => false,
    }
}

/// What the CPU says of the loaded program where the cursor is on no
/// entry's step, and on the last step of the entry that ended it.
fn between_entries(machine: &Machine) -> (CpuStatus, String) {
    let Some(program) = &machine.program else {
        return (CpuStatus::NoProgram, explain::ready());
    };
    let at = program.get(machine.cpu.pc);
    let stopped = |status| {
        let line = at.map_or(0, |placed| placed.line);
        (status, explain::stopped_at(line))
    };
    match (machine.cpu.halted, at) {
        (Some(Halted::Faulted), _) => stopped(CpuStatus::Faulted),
        (Some(Halted::OutOfBudget), _) => stopped(CpuStatus::OutOfBudget),
        (None, None) => (CpuStatus::Finished, explain::finished()),
        (None, Some(_)) => (CpuStatus::Ready, explain::ready()),
    }
}

/// What the CPU status calls an entry of `kind`.
pub(crate) fn noun(kind: EntryKind) -> EntryNoun {
    match kind {
        EntryKind::Access | EntryKind::Program => EntryNoun::Access,
        EntryKind::Instruction => EntryNoun::Instruction,
        EntryKind::Edit => EntryNoun::Edit,
        EntryKind::Error => EntryNoun::Error,
    }
}

impl ProgramView {
    /// The program loaded on `machine`, if any.
    pub(crate) fn new(machine: &Machine) -> Option<Self> {
        let program = machine.program.as_ref()?;
        let pc = machine.cpu.pc;
        let at = program.get(pc);
        let executing = machine.in_flight()
            && machine
                .access
                .as_ref()
                .is_some_and(|access| access.request.origin() == Origin::Program);
        Some(Self {
            format: program.format(),
            source: program.source().to_owned(),
            lines: (1_u32..)
                .zip(program.source().lines())
                .map(|(number, text)| ProgramLine {
                    number,
                    text: text.to_owned(),
                })
                .collect(),
            current_line: at.map(|placed| placed.line),
            executing,
            instructions: program.len(),
            executed: machine.cpu.executed,
            finished: at.is_none(),
            summary: program.summary(pc),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use std::collections::BTreeMap;

    use super::*;
    use crate::config::Preset;

    fn line(value: u32) -> LineAddr {
        LineAddr::new_unchecked(value)
    }

    #[test]
    fn memory_rows_walk_the_touched_and_watched_lines_as_one_ordered_list() {
        let mut machine = Machine::start(Preset::TwoWayLru.config()).unwrap();
        for touched in [0x0A0, 0x0B0, 0x1A0, 0x2A0] {
            machine.memory.touch(line(touched));
        }
        let watches: [&[u32]; 5] = [
            &[],
            // Below, between, above and on touched lines, one of them given
            // by a byte inside its line.
            &[0x000, 0x124, 0x0A0, 0x2A4, 0xFF0],
            // Beyond the 12-bit addresses: left out.
            &[0x1000, 0x0B0],
            &[0x300, 0x310],
            &[0x010],
        ];
        for watch in watches {
            let watch: BTreeSet<LineAddr> = watch.iter().copied().map(line).collect();
            let rows = MemoryRows::new(&machine, &watch);
            // The list as a copy would build it.
            let mut expected: BTreeMap<LineAddr, bool> =
                machine.memory.touched().map(|line| (line, false)).collect();
            for watched in &watch {
                if let Some(addr) = machine.geometry.addr(watched.get()) {
                    expected.insert(machine.geometry.locate(addr).line, true);
                }
            }
            let expected: Vec<(LineAddr, bool)> = expected.into_iter().collect();
            assert_eq!(rows.iter().collect::<Vec<_>>(), expected, "{watch:?}");
            assert_eq!(usize::try_from(rows.count()).unwrap(), expected.len());
            for ((at, _), row) in expected.iter().zip(0_u32..) {
                assert_eq!(rows.row_of(*at), Some(row), "{at:?} in {watch:?}");
            }
            assert_eq!(rows.row_of(line(0x0C0)), None);
        }
    }
}
