//! The wording of the log: every line built from machine state is written
//! here, so the tool's voice has one source. That includes the CPU's status
//! and the Stats lines: AMAT and the measured cost in timed mode, and the
//! count beside the hit and miss sparkline; the program drawer's lines: its
//! summary and the messages of a program or trace that does not parse; the
//! configuration's summaries, the labels of its repairs and of the
//! configuration bar's options; the texts of the views: the cache's and
//! memory's headings and descriptions, the lanes, the log's headers, the
//! changed summary and the status line; and the editors' messages: each
//! rule's title and text, the preview of an edit, the labels of its actions
//! and the log line of an applied edit; and the sharing texts: a shared
//! run's summary, the line Share shows once it has copied a link, and the
//! heading, lead and prefix action of a link or file that cannot be opened;
//! and the module panel's texts: a step's kind, the progress, the picker's
//! line, a prediction's feedback and the done screen.
//! Error messages are the exception; each is the `Display` of its error
//! type, next to that type. The drawer shares three of those: an address out
//! of range or misaligned, and a value the word cannot hold.
//!
//! An access's lines keep the design's wording where it has one, as in "Way 1
//! chosen: least recently used (rank 1).", and the lines it does not draw
//! follow its voice: short declarative sentences that give the number that
//! matters. Addresses, lines and tags are padded to the configuration's
//! widths, and values follow the data rule. No line depends on the cursor or
//! on Basics and Advanced, so the same event always reads the same.

use std::fmt::Write as _;

use crate::cache::Dirtiness;
use crate::config::{
    AllocatePolicy, CacheConfig, Config, ConfigError, Replacement, WordBytes, WritePolicy,
};
use crate::event::{ChangeRef, Counter, EventKind};
use crate::format;
use crate::geometry::{highest_word, Geometry, Location, Placement};
use crate::isa::{Mnemonic, ParseError};
use crate::limits::{
    INSTRUCTION_BUDGET, MAX_LABEL_CHARS, MAX_PROGRAM_BYTES, MAX_PROGRAM_INSTRUCTIONS,
    MAX_PROGRAM_LINES, MIN_CYCLES, SESSION_EVENT_LIMIT,
};
use crate::machine::InvariantViolation;
use crate::module::{FeedbackTone, ModuleStepKind};
use crate::op::{AccessKind, ProgramFormat};
use crate::replacement::{VictimChoice, VictimReason};
use crate::session::EntryOutcome;
use crate::units::{Addr, LineAddr, Register, SetIndex, Tag, WayIndex, Word, WordIndex};

/// What a store's `Write` line says after the word it wrote, which depends on
/// the write policy and on what the store found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WriteNote {
    /// Under write-through the line says nothing more: the `WriteThrough`
    /// event that follows writes memory.
    Through,
    /// Under write-back the line names the dirty bit and memory's copy of the
    /// word.
    Back {
        /// The line's dirty bit before the store.
        before: Dirtiness,
        /// Whether memory's copy of the word now differs from the cache's.
        stale: bool,
    },
}

/// What a miss's stall waits for in timed mode, which its `Stall` line names.
/// Each transfer to or from memory costs one miss penalty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StallFor {
    /// Memory supplying the new line.
    Fill {
        /// The line coming in.
        line: LineAddr,
    },
    /// A dirty victim's line going back to memory, then memory supplying the
    /// new line: two transfers.
    WriteBackThenFill {
        /// The dirty line going out.
        victim: LineAddr,
        /// The line coming in.
        line: LineAddr,
    },
    /// A store that missed under write-no-allocate going to memory.
    WriteAround,
}

impl StallFor {
    /// The transfers the stall waits for, each costing one miss penalty.
    pub(crate) fn transfers(self) -> u32 {
        match self {
            StallFor::Fill { .. } | StallFor::WriteAround => 1,
            StallFor::WriteBackThenFill { .. } => 2,
        }
    }
}

/// Why a line of a program or trace does not parse, as the program drawer
/// lists it. An address out of range or misaligned, and an immediate the word
/// cannot hold, are not here: typed input gives the same messages, which
/// live with their error types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParseProblem<'a> {
    /// A mnemonic other than `ld`, `st`, `add` and `bnez`.
    UnknownInstruction {
        /// The word where the mnemonic goes, as written.
        name: &'a str,
    },
    /// A trace line that is not `R` or `W`.
    UnknownAccess {
        /// The line's first word, as written.
        name: &'a str,
    },
    /// `r` and digits that name no register, such as `r8`.
    NotARegister {
        /// The word, as written.
        name: &'a str,
    },
    /// An address written in decimal.
    DecimalAddress {
        /// The digits, as written.
        text: &'a str,
        /// Their value.
        value: u64,
    },
    /// A number that does not fit 64 bits.
    TooLong {
        /// The number, as written.
        text: &'a str,
    },
    /// An instruction whose operands are missing, extra or of the wrong kind.
    Operands {
        /// The instruction.
        mnemonic: Mnemonic,
    },
    /// A trace line whose operands are missing, extra or of the wrong kind.
    TraceOperands {
        /// Whether the line is an `R` or a `W`.
        kind: AccessKind,
    },
    /// A trace store with an address and no value.
    StoreWithoutValue {
        /// Its address.
        addr: Addr,
    },
    /// A trace store's value that the word cannot hold. A trace value has no
    /// sign, so the range the message gives is unsigned.
    ValueTooWide {
        /// The value, as its number.
        value: u64,
    },
    /// A word where a label goes that is not spelled like one.
    NotALabel {
        /// The word, as written.
        text: &'a str,
    },
    /// A register or a mnemonic where a label goes.
    ReservedLabel {
        /// The word, as written.
        name: &'a str,
        /// Whether it is a register rather than a mnemonic.
        register: bool,
    },
    /// A label longer than [`MAX_LABEL_CHARS`].
    LabelTooLong {
        /// Its length, in characters.
        chars: u32,
    },
    /// A label defined a second time.
    DuplicateLabel {
        /// The label.
        name: &'a str,
        /// The line that defines it first.
        line: u32,
    },
    /// A branch to a label that is never defined.
    UndefinedLabel {
        /// The label.
        name: &'a str,
    },
    /// A source without a single instruction or access.
    Empty {
        /// Whether it is a program or a trace.
        format: ProgramFormat,
    },
    /// A source of more than [`MAX_PROGRAM_BYTES`].
    TooLarge {
        /// Whether it is a program or a trace.
        format: ProgramFormat,
    },
    /// A source of more than [`MAX_PROGRAM_LINES`] lines.
    TooManyLines {
        /// Whether it is a program or a trace.
        format: ProgramFormat,
    },
    /// A program of more than [`MAX_PROGRAM_INSTRUCTIONS`] instructions.
    TooManyInstructions,
}

/// What an entry is, as the CPU status names it. The design calls every
/// entry an access; here an instruction without a memory access, an edit and
/// an error that ends an op are named for what they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EntryNoun {
    /// A load or store, issued by hand or executed by a program.
    Access,
    /// An instruction without a memory access, such as `add` or `bnez`.
    Instruction,
    /// An edit of a way or a memory word.
    Edit,
    /// An error that ended an op.
    Error,
}

/// The `Decode` line: the address split into its fields, as in "Load 0x2A4:
/// tag 0x0A, set 2, offset 4 (word 1)." A fully associative cache has no
/// index, and the line says so instead of naming a set.
pub(crate) fn decode(
    geometry: &Geometry,
    kind: AccessKind,
    addr: Addr,
    location: Location,
) -> String {
    let verb = match kind {
        AccessKind::Read => "Load",
        AccessKind::Write => "Store",
    };
    let addr = address(geometry, addr.get());
    let tag = tag(geometry, location.tag);
    let offset = format::Data(u64::from(location.offset));
    let word = format::Data(u64::from(location.word.get()));
    match geometry.placement() {
        Placement::FullyAssociative => format!(
            "{verb} {addr}: tag {tag}, offset {offset} (word {word}). Fully associative: one \
             set, no index bits."
        ),
        Placement::DirectMapped | Placement::SetAssociative => format!(
            "{verb} {addr}: tag {tag}, set {}, offset {offset} (word {word}).",
            format::Data(u64::from(location.set.get()))
        ),
    }
}

/// The `Hit` line: "Set 2, way 0: valid, and tag 0x01 matches. Hit."
pub(crate) fn hit(geometry: &Geometry, set: SetIndex, way: WayIndex, tag: Tag) -> String {
    format!(
        "Set {}, way {}: valid, and tag {} matches. Hit.",
        format::Data(u64::from(set.get())),
        format::Data(u64::from(way.get())),
        self::tag(geometry, tag)
    )
}

/// The `Miss` line. A set of several ways reads "Set 2: no valid way holds
/// tag 0x0A. Miss."; a one-way set names what its way held, `held`, the tag
/// it was compared against: "Set 2 holds tag 0x01, not 0x03. Miss.", or "Set
/// 2 is invalid. Miss." when it held nothing.
pub(crate) fn miss(geometry: &Geometry, set: SetIndex, tag: Tag, held: Option<Tag>) -> String {
    let set = format::Data(u64::from(set.get()));
    let tag = self::tag(geometry, tag);
    if geometry.ways() > 1 {
        return format!("Set {set}: no valid way holds tag {tag}. Miss.");
    }
    match held {
        Some(held) => format!(
            "Set {set} holds tag {}, not {tag}. Miss.",
            self::tag(geometry, held)
        ),
        None => format!("Set {set} is invalid. Miss."),
    }
}

/// The `Victim` line: the way a miss chose, and why that one.
pub(crate) fn victim(geometry: &Geometry, set: SetIndex, choice: VictimChoice) -> String {
    let way = format::Data(u64::from(choice.way.get()));
    match choice.reason {
        VictimReason::Empty if geometry.ways() == 1 => format!(
            "Set {} has one way; the line goes there.",
            format::Data(u64::from(set.get()))
        ),
        VictimReason::Empty => format!("Way {way} chosen: it is invalid (empty)."),
        // A one-line cache also has a single way, but it is fully associative,
        // so only a direct-mapped cache is named as the reason.
        VictimReason::OnlyWay => match geometry.placement() {
            Placement::DirectMapped => format!(
                "Direct-mapped: way {way} is the only place for this line, so its current line \
                 must go."
            ),
            Placement::SetAssociative | Placement::FullyAssociative => {
                format!("Way {way} is the only place for this line, so its current line must go.")
            }
        },
        VictimReason::LeastRecentlyUsed { rank } => format!(
            "Way {way} chosen: least recently used (rank {}).",
            format::Data(u64::from(rank))
        ),
        VictimReason::FirstIn { .. } => format!("Way {way} chosen: filled earliest (FIFO)."),
        VictimReason::Random => format!("Way {way} chosen at random."),
    }
}

/// The `Stall` line of timed mode: how long the CPU waits for memory, and
/// what for. Each transfer costs `miss` cycles, the miss penalty: "The CPU
/// stalls for 20 cycles while memory supplies line 0x2A0.", or with a dirty
/// victim "The CPU stalls for 40 cycles: 20 to write line 0x1A0 back, then 20
/// to fetch line 0x2A0."
pub(crate) fn stall(geometry: &Geometry, miss: u32, wait: StallFor) -> String {
    let total = format::cycles(miss.saturating_mul(wait.transfers()));
    match wait {
        StallFor::Fill { line } => format!(
            "The CPU stalls for {total} while memory supplies line {}.",
            address(geometry, line.get())
        ),
        StallFor::WriteBackThenFill { victim, line } => {
            let each = format::Setting(u64::from(miss));
            format!(
                "The CPU stalls for {total}: {each} to write line {} back, then {each} to fetch \
                 line {}.",
                address(geometry, victim.get()),
                address(geometry, line.get())
            )
        }
        StallFor::WriteAround => {
            format!("The CPU stalls for {total} while the store goes to memory.")
        }
    }
}

/// The `WriteBack` line: "Way 1 is dirty: write line 0x1A0 back to memory
/// first."
pub(crate) fn write_back(geometry: &Geometry, way: WayIndex, line: LineAddr) -> String {
    format!(
        "Way {} is dirty: write line {} back to memory first.",
        format::Data(u64::from(way.get())),
        address(geometry, line.get())
    )
}

/// The `Evict` line: "Evict line 0x1A0 (tag 0x06) from way 1."
pub(crate) fn evict(geometry: &Geometry, way: WayIndex, line: LineAddr, tag: Tag) -> String {
    format!(
        "Evict line {} (tag {}) from way {}.",
        address(geometry, line.get()),
        self::tag(geometry, tag),
        format::Data(u64::from(way.get()))
    )
}

/// The `Fill` line: "Fill way 1 with line 0x2A0–0x2AF from memory."
pub(crate) fn fill(geometry: &Geometry, way: WayIndex, line: LineAddr) -> String {
    format!(
        "Fill way {} with line {} from memory.",
        format::Data(u64::from(way.get())),
        format::line_range(line.get(), geometry.line_bytes(), geometry.address_bits())
    )
}

/// The `Rank` line: the way the hit or fill moved to the front, then every
/// other way of the set in way order, with the ranks `ranks` gives in way
/// order after the move. "LRU ranks in set 2: way 1 becomes 0 (most recent);
/// way 0 is 1." or "FIFO order in set 0: way 0 becomes 0 (newest); way 1 is
/// 1."
pub(crate) fn rank(
    replacement: Replacement,
    set: SetIndex,
    way: WayIndex,
    ranks: &[u32],
) -> String {
    let (order, front) = match replacement {
        Replacement::Fifo => ("FIFO order", "newest"),
        // Random replacement never moves a rank, so no `Rank` event has it.
        Replacement::Lru | Replacement::Random => ("LRU ranks", "most recent"),
    };
    let promoted = usize::try_from(way.get())
        .ok()
        .and_then(|index| ranks.get(index))
        .copied()
        .unwrap_or(0);
    // The opening clause plus one "; way N is R" or ", way N is R" per other
    // way, sized so the loop below never has to grow the buffer.
    let mut line = String::with_capacity(48 + ranks.len() * 16);
    // Writing to a `String` cannot fail: `String::write_str` never errs, and
    // `format::Data`'s `Display` impl only ever passes on the writer's
    // result, so it cannot return `Err` either.
    let _ = write!(
        line,
        "{order} in set {}: way {} becomes {} ({front})",
        format::Data(u64::from(set.get())),
        format::Data(u64::from(way.get())),
        format::Data(u64::from(promoted))
    );
    let mut joined_any = false;
    for (other, &rank) in (0_u32..).zip(ranks) {
        if other == way.get() {
            continue;
        }
        line.push_str(if joined_any { ", " } else { "; " });
        joined_any = true;
        let _ = write!(
            line,
            "way {} is {}",
            format::Data(u64::from(other)),
            format::Data(u64::from(rank))
        );
    }
    line.push('.');
    line
}

/// A load's `Complete` line: "Read word 1 of way 1 (68) into r3." A load into
/// r0 reads the word and drops it, since r0 always reads 0. A load issued by
/// hand while an assembly program is loaded and has not finished,
/// `beside_program`, says that the program's register changed: "Read word 1
/// of way 0 (53) into r1. A program is loaded, so its r1 now holds 53."
pub(crate) fn load(
    word: WordIndex,
    way: WayIndex,
    value: u64,
    into: Register,
    beside_program: bool,
) -> String {
    let shown = format::Data(value);
    let read = format!(
        "Read word {} of way {} ({shown})",
        format::Data(u64::from(word.get())),
        format::Data(u64::from(way.get()))
    );
    if into == Register::R0 {
        format!("{read}; r0 always reads 0, so the value is dropped.")
    } else if beside_program {
        format!("{read} into {into}. A program is loaded, so its {into} now holds {shown}.")
    } else {
        format!("{read} into {into}.")
    }
}

/// A store's `Write` line: the word written into the way, then, under
/// write-back, the dirty bit and memory's copy of the word at `addr`: "Write
/// 7 into word 1 of way 0 and set its dirty bit. Memory at 0x0A4 is now
/// stale."
pub(crate) fn write(
    geometry: &Geometry,
    addr: Addr,
    word: WordIndex,
    way: WayIndex,
    value: Word,
    note: WriteNote,
) -> String {
    let value = format::Data(value.get());
    let wrote = format!(
        "Write {value} into word {} of way {}",
        format::Data(u64::from(word.get())),
        format::Data(u64::from(way.get()))
    );
    let (before, stale) = match note {
        WriteNote::Through => return format!("{wrote}."),
        WriteNote::Back { before, stale } => (before, stale),
    };
    let addr = address(geometry, addr.get());
    let dirty = match before {
        Dirtiness::Clean => " and set its dirty bit.",
        Dirtiness::Dirty => "; the line was already dirty.",
    };
    let memory = match (stale, before) {
        (false, _) => format!("Memory at {addr} already holds {value}."),
        (true, Dirtiness::Clean) => format!("Memory at {addr} is now stale."),
        (true, Dirtiness::Dirty) => format!("Memory at {addr} is stale."),
    };
    format!("{wrote}{dirty} {memory}")
}

/// The `WriteThrough` line: "Write-through: 7 also goes to memory at 0x0A4."
pub(crate) fn write_through(geometry: &Geometry, addr: Addr, value: Word) -> String {
    format!(
        "Write-through: {} also goes to memory at {}.",
        format::Data(value.get()),
        address(geometry, addr.get())
    )
}

/// The `MemWrite` line of a store that missed under write-no-allocate:
/// "Write-no-allocate: 7 goes straight to memory at 0x0A4. The cache is
/// unchanged."
pub(crate) fn write_around(geometry: &Geometry, addr: Addr, value: Word) -> String {
    format!(
        "Write-no-allocate: {} goes straight to memory at {}. The cache is unchanged.",
        format::Data(value.get()),
        address(geometry, addr.get())
    )
}

/// The sentence a store's result line starts with when the register it
/// stores holds more than the word: "r2 = 300 does not fit a 1-byte word, so
/// its low 8 bits, 44, are stored. "
pub(crate) fn truncated(register: Register, value: u64, word_bytes: u32, stored: Word) -> String {
    format!(
        "{register} = {} does not fit {} {}-byte word, so its low {} bits, {}, are stored. ",
        format::data(value),
        format::a_or_an(word_bytes),
        format::setting(word_bytes),
        format::setting(word_bytes.saturating_mul(u8::BITS)),
        format::data(stored.get())
    )
}

/// An `add`'s `Complete` line: "add: r2 ← 176. No memory access." A sum for
/// r0 is computed and dropped.
pub(crate) fn add(into: Register, value: u64) -> String {
    let value = format::data(value);
    if into == Register::R0 {
        format!("add: r0 always reads 0, so the result {value} is dropped.")
    } else {
        format!("add: {into} \u{2190} {value}. No memory access.")
    }
}

/// A `bnez`'s `Complete` line: "bnez: r1 is 3, branch taken to loop." when
/// `taken` names the label, or "bnez: r1 is 0, branch not taken."
pub(crate) fn bnez(test: Register, value: u64, taken: Option<&str>) -> String {
    let value = format::data(value);
    match taken {
        Some(label) => format!("bnez: {test} is {value}, branch taken to {label}."),
        None => format!("bnez: {test} is {value}, branch not taken."),
    }
}

/// The `Error` line of an instruction on line `line` whose address, held in
/// `register`, is `value` and does not fit the address width: "Line 4: r2
/// holds 4096, which does not fit a 12-bit address. The program stopped."
pub(crate) fn fault_out_of_range(
    geometry: &Geometry,
    line: u32,
    register: Register,
    value: u64,
) -> String {
    let bits = geometry.address_bits();
    format!(
        "Line {}: {register} holds {}, which does not fit {} {}-bit address. The program stopped.",
        format::position(line),
        format::data(value),
        format::a_or_an(bits),
        format::setting(bits)
    )
}

/// The `Error` line of an instruction on line `line` whose address, held in
/// `register`, is `addr` and not a multiple of the word size: "Line 4: r2
/// holds 166 (0x0A6), which is not aligned to a 4-byte word. The program
/// stopped."
pub(crate) fn fault_misaligned(
    geometry: &Geometry,
    line: u32,
    register: Register,
    addr: Addr,
) -> String {
    let word_bytes = geometry.word_bytes();
    format!(
        "Line {}: {register} holds {} ({}), which is not aligned to {} {}-byte word. The program \
         stopped.",
        format::position(line),
        format::data(addr.get()),
        address(geometry, addr.get()),
        format::a_or_an(word_bytes),
        format::setting(word_bytes)
    )
}

/// The `Error` line that replaces the instruction after the budget's last:
/// "The program used its budget of 10,000 instructions and stopped. Is a loop
/// missing its exit?"
pub(crate) fn out_of_budget() -> String {
    format!(
        "The program used its budget of {} instructions and stopped. Is a loop missing its exit?",
        format::setting(INSTRUCTION_BUDGET)
    )
}

/// The `Setup` line of a program load: "Loaded a program of 5 instructions.
/// Registers and the pc are reset; the cache and memory keep their
/// contents." A trace counts accesses.
pub(crate) fn program_loaded(format: ProgramFormat, instructions: u32) -> String {
    format!(
        "Loaded {} {}. Registers and the pc are reset; the cache and memory keep their contents.",
        match format {
            ProgramFormat::Asm => "a program of",
            ProgramFormat::Trace => "a trace of",
        },
        count_of(format, instructions)
    )
}

/// The `Setup` line of a new run: "New run: 128 B · 16 B lines · 2-way · LRU.
/// The cache is empty and memory, registers and counters are reset." With a
/// program kept, written in `format` and restarting at its first
/// instruction, on line `line`: "… The program is kept and restarts at line
/// 1." A trace is "The trace".
///
/// The summary names the write and allocate policies when the configuration
/// needs Advanced, as a write-through, no-allocate or timed one does, so the
/// line depends on the configuration alone, never on the mode the student is
/// in.
pub(crate) fn new_run(config: &Config, restart: Option<(ProgramFormat, u32)>) -> String {
    let kept = restart
        .map(|(format, line)| {
            format!(
                " The {} is kept and restarts at line {}.",
                source_noun(format),
                format::position(line)
            )
        })
        .unwrap_or_default();
    format!(
        "New run: {}. The cache is empty and memory, registers and counters are reset.{kept}",
        config.summary(!config.fits_basics())
    )
}

/// The `Error` line after a new run's `Setup` when the program it would keep
/// does not parse under the new configuration, quoting the first error: "The
/// program no longer fits this configuration (line 3: 0x1000 does not fit a
/// 12-bit address (0x000–0xFFF)); it was unloaded." A trace is "The trace".
pub(crate) fn program_unloaded(format: ProgramFormat, first: &ParseError) -> String {
    let source = match format {
        ProgramFormat::Asm => "program",
        ProgramFormat::Trace => "trace",
    };
    let message = first.message.strip_suffix('.').unwrap_or(&first.message);
    format!(
        "The {source} no longer fits this configuration (line {}: {message}); it was unloaded.",
        format::position(first.line)
    )
}

/// The `Error` line that stops a program when the log has no room for
/// another entry: "The session reached its limit of 40,000 steps, so the
/// program stopped here."
pub(crate) fn session_limit() -> String {
    format!(
        "The session reached its limit of {} steps, so the program stopped here.",
        format::setting(SESSION_EVENT_LIMIT)
    )
}

/// Why Space executes nothing once the program has executed its last
/// instruction.
pub(crate) fn end_of_program() -> String {
    "End of program.".to_owned()
}

/// The question "Reset to apply?" asks before a configuration change starts
/// a new run. `kept` counts the steps of the displayed run that stay in the
/// log, and `dropped` the undone steps after them that the new run would
/// drop: "Configuration is fixed for a run. Applying starts a new, empty run;
/// this one has 14 steps and stays reachable with Undo." or, with steps to
/// drop, "Configuration is fixed for a run. Applying starts a new, empty run
/// here and drops the 6 undone steps after this point; this run's 3 steps
/// stay reachable with Undo."
pub(crate) fn needs_reset(kept: u32, dropped: u32) -> String {
    let steps = |n: u32| format!("{} {}", format::data(n), format::plural(n, "step", "steps"));
    if dropped == 0 {
        format!(
            "Configuration is fixed for a run. Applying starts a new, empty run; this one has {} \
             and stays reachable with Undo.",
            steps(kept)
        )
    } else {
        format!(
            "Configuration is fixed for a run. Applying starts a new, empty run here and drops \
             the {} undone {} after this point; this run's {} {} reachable with Undo.",
            format::data(dropped),
            format::plural(dropped, "step", "steps"),
            steps(kept),
            format::plural(kept, "stays", "stay")
        )
    }
}

/// The program drawer's line: how many instructions, and the next one, or
/// `end` past the last: `5 instructions · next: ld r1, [0x0A4]`. A trace
/// counts accesses: "5 accesses · next: R 0x0A4".
pub(crate) fn program_summary(
    format: ProgramFormat,
    instructions: u32,
    next: Option<&str>,
) -> String {
    format!(
        "{} \u{b7} next: {}",
        count_of(format, instructions),
        next.unwrap_or("end")
    )
}

/// A parse error's message with its place: "Line 3, column 5: unknown
/// instruction `lw`; use ld, st, add or bnez."
pub(crate) fn parse_error(line: u32, column: u32, message: &str) -> String {
    format!(
        "Line {}, column {}: {message}",
        format::position(line),
        format::position(column)
    )
}

/// The message of `problem`, a reason a line does not parse under
/// `geometry`. The examples it gives are written for the geometry: an
/// address at its width and aligned to its word.
pub(crate) fn parse_problem(geometry: &Geometry, problem: ParseProblem<'_>) -> String {
    match problem {
        ParseProblem::UnknownInstruction { name } => {
            format!("unknown instruction `{name}`; use ld, st, add or bnez.")
        }
        ParseProblem::UnknownAccess { name } => {
            format!("unknown access `{name}`; use R to load or W to store.")
        }
        ParseProblem::NotARegister { name } => {
            format!("`{name}` is not a register; use r0\u{2013}r7.")
        }
        ParseProblem::DecimalAddress { text, value } => format!(
            "addresses are hex: write {}, not {text}.",
            format::address(value, geometry.address_bits())
        ),
        ParseProblem::TooLong { text } => format!("`{text}` does not fit 64 bits."),
        ParseProblem::Operands { mnemonic } => operands(geometry, mnemonic),
        ParseProblem::TraceOperands { kind } => {
            let example = example_address(geometry);
            match kind {
                AccessKind::Read => format!("`R` needs an address, like `R {example}`."),
                AccessKind::Write => {
                    format!("`W` needs an address and a value, like `W {example} 7`.")
                }
            }
        }
        ParseProblem::StoreWithoutValue { addr } => format!(
            "a store needs a value: W {} 7.",
            address(geometry, addr.get())
        ),
        ParseProblem::ValueTooWide { value } => format!(
            "{} does not fit {} {}-byte word (0\u{2013}{}).",
            format::data(value),
            format::a_or_an(geometry.word_bytes()),
            format::setting(geometry.word_bytes()),
            format::setting(highest_word(geometry.word_bytes()))
        ),
        ParseProblem::NotALabel { text } => format!(
            "`{text}` cannot be a label; start it with a letter or `_`, and use only letters, \
             digits and `_`."
        ),
        ParseProblem::ReservedLabel { name, register } => format!(
            "`{name}` is {}, so it cannot be a label.",
            if register {
                "a register"
            } else {
                "an instruction"
            }
        ),
        ParseProblem::LabelTooLong { chars } => format!(
            "a label has at most {} characters; this one has {}.",
            format::setting(MAX_LABEL_CHARS),
            format::data(chars)
        ),
        ParseProblem::DuplicateLabel { name, line } => format!(
            "label `{name}` is already defined on line {}.",
            format::position(line)
        ),
        ParseProblem::UndefinedLabel { name } => format!("label `{name}` is not defined."),
        ParseProblem::Empty { format } => match format {
            ProgramFormat::Asm => "the program has no instructions.".to_owned(),
            ProgramFormat::Trace => "the trace has no accesses.".to_owned(),
        },
        ParseProblem::TooLarge { format } => format!(
            "the {} is larger than {} bytes.",
            source_noun(format),
            format::setting(MAX_PROGRAM_BYTES)
        ),
        ParseProblem::TooManyLines { format } => format!(
            "the {} has more than {} lines.",
            source_noun(format),
            format::setting(MAX_PROGRAM_LINES)
        ),
        ParseProblem::TooManyInstructions => format!(
            "the program has more than {} instructions.",
            format::setting(MAX_PROGRAM_INSTRUCTIONS)
        ),
    }
}

/// The message of an instruction whose operands do not fit its shape, with
/// an example of the shape: "`ld` needs a register and an address, like `ld
/// r1, [0x0A4]`."
fn operands(geometry: &Geometry, mnemonic: Mnemonic) -> String {
    let example = example_address(geometry);
    let needs = match mnemonic {
        Mnemonic::Ld => format!("a register and an address, like `ld r1, [{example}]`"),
        Mnemonic::St => format!("an address and a register or a value, like `st [{example}], #7`"),
        Mnemonic::Add => {
            "two registers and a register or a value, like `add r1, r1, #4`".to_owned()
        }
        Mnemonic::Bnez => "a register and a label, like `bnez r1, loop`".to_owned(),
    };
    format!("`{}` needs {needs}.", mnemonic.name())
}

/// The address the parse messages give as an example: the design's 0x0A4,
/// aligned down to the word and written at the address width. It fits the
/// narrowest width.
fn example_address(geometry: &Geometry) -> String {
    const DESIGN_EXAMPLE: u32 = 0x0A4;
    let word_bytes = geometry.word_bytes().max(1);
    address(geometry, DESIGN_EXAMPLE - DESIGN_EXAMPLE % word_bytes).to_string()
}

/// "program" or "trace".
fn source_noun(format: ProgramFormat) -> &'static str {
    match format {
        ProgramFormat::Asm => "program",
        ProgramFormat::Trace => "trace",
    }
}

/// A program's size as the drawer and the log count it: "5 instructions",
/// "1 access".
fn count_of(format: ProgramFormat, instructions: u32) -> String {
    let (one, many) = match format {
        ProgramFormat::Asm => ("instruction", "instructions"),
        ProgramFormat::Trace => ("access", "accesses"),
    };
    format!(
        "{} {}",
        format::data(instructions),
        format::plural(instructions, one, many)
    )
}

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

/// The line of the `Error` event that ends an op when an invariant no longer
/// holds.
pub(crate) fn internal_error(violation: &InvariantViolation) -> String {
    format!(
        "Internal error: {violation} no longer holds. The run stopped here; please report it \
         with a share link."
    )
}

/// The configuration's summary line, as the design's `cfgSummary` writes
/// it: the size, the line size and the placement, then the replacement
/// policy unless the cache is direct-mapped: "128 B · 16 B lines · 2-way ·
/// LRU". With `advanced`, then the write and allocate policies: " ·
/// write-back · allocate". The placement is read from the numbers as given,
/// so an invalid configuration has a summary too.
pub(crate) fn config_summary(config: &Config, advanced: bool) -> String {
    let cache = &config.cache;
    let mut text = format!(
        "{} B \u{b7} {} B lines \u{b7} {}",
        format::setting(cache.size_bytes),
        format::setting(cache.line_bytes),
        placement_name(cache)
    );
    if cache.placement() != Placement::DirectMapped {
        text.push_str(" \u{b7} ");
        text.push_str(replacement_name(cache.replacement));
    }
    if advanced {
        text.push_str(" \u{b7} ");
        text.push_str(match cache.write_policy {
            WritePolicy::WriteBack => "write-back",
            WritePolicy::WriteThrough => "write-through",
        });
        text.push_str(" \u{b7} ");
        text.push_str(match cache.allocate_policy {
            AllocatePolicy::WriteAllocate => "allocate",
            AllocatePolicy::WriteNoAllocate => "no-allocate",
        });
    }
    text
}

/// The short summary a shared run's loading screen opens with: the
/// placement and the replacement policy, then the size, as in "2-way LRU ·
/// 128 B". It starts a sentence, so its first word has a capital:
/// "Direct-mapped · 128 B", "Fully associative LRU · 128 B".
pub(crate) fn short_summary(config: &Config) -> String {
    let cache = &config.cache;
    let replacement = replacement_name(cache.replacement);
    let placement = match cache.placement() {
        Placement::DirectMapped => "Direct-mapped".to_owned(),
        Placement::FullyAssociative => format!("Fully associative {replacement}"),
        Placement::SetAssociative => {
            format!("{}-way {replacement}", format::setting(cache.associativity))
        }
    };
    format!("{placement} \u{b7} {} B", format::setting(cache.size_bytes))
}

/// The label of the action that opens `repaired`, the repair of a
/// configuration whose first broken rule is `first`: it names that rule's
/// repair, read from the repaired configuration and its `geometry`, as in
/// "Open with 2-way instead" or "Open as a 1,024 B cache instead".
pub(crate) fn repair_label(first: &ConfigError, repaired: &Config, geometry: &Geometry) -> String {
    let size = repaired.cache.size_bytes;
    match first {
        ConfigError::AddressBits { .. } => format!(
            "Open with {}-bit addresses instead",
            format::setting(repaired.address_bits)
        ),
        ConfigError::SizeNotPowerOfTwo { .. }
        | ConfigError::LineLargerThanCache { .. }
        | ConfigError::NoTagBits { .. }
        | ConfigError::TooManyLines { .. } => format!(
            "Open as {} {} B cache instead",
            format::a_or_an(size),
            format::setting(size)
        ),
        ConfigError::LineNotPowerOfTwo { .. }
        | ConfigError::LineSmallerThanWord { .. }
        | ConfigError::TooManyWords { .. } => format!(
            "Open with {} B lines instead",
            format::setting(repaired.cache.line_bytes)
        ),
        ConfigError::AssociativityZero
        | ConfigError::AssociativityNotPowerOfTwo { .. }
        | ConfigError::AssociativityTooLarge { .. } => match geometry.placement() {
            Placement::FullyAssociative => "Open as fully associative instead".to_owned(),
            Placement::DirectMapped => "Open as direct-mapped instead".to_owned(),
            Placement::SetAssociative => {
                format!("Open with {}-way instead", format::setting(geometry.ways()))
            }
        },
        ConfigError::Timing { .. } => {
            let (hit, miss) = repaired.timing.map_or((MIN_CYCLES, MIN_CYCLES), |timing| {
                (timing.hit_cycles, timing.miss_cycles)
            });
            format!(
                "Open with {} {}/{}-cycle timing instead",
                format::a_or_an(hit),
                format::setting(hit),
                format::setting(miss)
            )
        }
        ConfigError::SeedTooLarge { .. } => {
            format!("Open with seed {} instead", format::setting(repaired.seed))
        }
    }
}

/// The label of a cache-size or line-size option: "64 B".
pub(crate) fn bytes_option(bytes: u32) -> String {
    format!("{} B", format::setting(bytes))
}

/// The label of an associativity option other than Full: "Direct" for one
/// way, and otherwise "2-way".
pub(crate) fn ways_option(ways: u32) -> String {
    if ways == 1 {
        "Direct".to_owned()
    } else {
        format!("{}-way", format::setting(ways))
    }
}

/// The label of the fully associative option.
pub(crate) fn full_option() -> String {
    "Full".to_owned()
}

/// Why the associativity option with as many ways as a cache of `lines`
/// lines is disabled: "Same as Full for an 8-line cache."
pub(crate) fn same_as_full(lines: u32) -> String {
    format!(
        "Same as Full for {} {}-line cache.",
        format::a_or_an(lines),
        format::setting(lines)
    )
}

/// The label of a replacement option: "LRU", "FIFO", "Random".
pub(crate) fn replacement_option(replacement: Replacement) -> String {
    replacement_name(replacement).to_owned()
}

/// The label of an address-width option: "12-bit".
pub(crate) fn address_option(bits: u8) -> String {
    format!("{}-bit", format::setting(bits))
}

/// The label of a word-size option: "4 B".
pub(crate) fn word_option(word: WordBytes) -> String {
    bytes_option(word.bytes())
}

/// The label of a write-policy option: "Write-back", "Write-through".
pub(crate) fn write_option(policy: WritePolicy) -> String {
    match policy {
        WritePolicy::WriteBack => "Write-back",
        WritePolicy::WriteThrough => "Write-through",
    }
    .to_owned()
}

/// The label of an allocate-policy option, in the group headed "On a store
/// miss": "Allocate", "No allocate".
pub(crate) fn allocate_option(policy: AllocatePolicy) -> String {
    match policy {
        AllocatePolicy::WriteAllocate => "Allocate",
        AllocatePolicy::WriteNoAllocate => "No allocate",
    }
    .to_owned()
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
        address(geometry, addr.get()),
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
        address(geometry, line.get()),
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
        address(geometry, line.get())
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
        address(geometry, line.get())
    )
}

/// The transfer lane at a write-around's `MemWrite`, which goes from the CPU
/// straight to memory: "Write-around · CPU → memory line 0x0A0".
pub(crate) fn write_around_transfer(geometry: &Geometry, line: LineAddr) -> String {
    format!(
        "Write-around \u{b7} CPU \u{2192} memory line {}",
        address(geometry, line.get())
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
    format!("{verb} {}", address(geometry, addr.get()))
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
    let at = address(geometry, addr.get());
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
    format!("edit memory {}", address(geometry, addr.get()))
}

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
        address(geometry, line.get()),
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
        address(geometry, line.get()),
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
        address(geometry, line.get()),
        format::data(set.get())
    )
}

/// The title of an edit that gives a way a line another way of its set
/// holds: "0x0A0 is already cached".
pub(crate) fn already_cached(geometry: &Geometry, line: LineAddr) -> String {
    format!("{} is already cached", address(geometry, line.get()))
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
        address(geometry, line.get()),
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
        Some(line) => format!("giving it line {}", address(geometry, line.get())),
        None => "invalidating it".to_owned(),
    };
    let discarded: Vec<String> = words
        .iter()
        .map(|(addr, cached, _)| {
            format!(
                "{} at {}",
                format::data(cached.get()),
                address(geometry, addr.get())
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
            format!("Edit memory {} instead", address(geometry, addr.get()))
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
                |from| tag(geometry, from).to_string()
            ),
            tag(geometry, to)
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
        address(geometry, addr.get()),
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
        address(geometry, addr.get()),
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

/// A program load, as the log's divider row names it: "Loaded a program of
/// 5 instructions", or for a trace "Loaded a trace of 5 accesses".
pub(crate) fn program_divider(format: ProgramFormat, instructions: u32) -> String {
    let text = program_loaded(format, instructions);
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
        EntryOutcome::Program => source_noun(format.unwrap_or(ProgramFormat::Asm)).to_owned(),
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

/// The line a shared run's loading screen shows under its heading: the
/// opening run's short summary, then its steps and the step it opens at,
/// "2-way LRU · 128 B · 14 steps, opening at step 9."; with several runs,
/// their count first: "2-way LRU · 128 B · 3 runs; this one has 14 steps,
/// opening at step 9." `summary` is the opening run's short summary, `runs`
/// the runs in the log, `steps` the opening run's steps and `opening` the
/// step it opens at, 0 before the first.
pub(crate) fn share_text(summary: &str, runs: u32, steps: u32, opening: u32) -> String {
    let steps = if steps == 0 {
        "no steps yet".to_owned()
    } else if opening == 0 {
        format!("{}, opening before the first", step_count(steps))
    } else {
        format!(
            "{}, opening at step {}",
            step_count(steps),
            format::data(opening)
        )
    };
    if runs > 1 {
        format!(
            "{summary} \u{b7} {} runs; this one has {steps}.",
            format::data(runs)
        )
    } else {
        format!("{summary} \u{b7} {steps}.")
    }
}

/// The status line once Share has copied the link: what the link encodes
/// and where it opens, "Link copied. It encodes the configuration and all
/// 14 steps, and opens at step 9."; with several runs, "Link copied. It
/// encodes all 3 runs, and opens this one at step 9 of 14." The counts are
/// as for [`share_text`].
pub(crate) fn share_copied(runs: u32, steps: u32, opening: u32) -> String {
    if runs > 1 {
        let place = if steps == 0 {
            ", which has no steps yet".to_owned()
        } else if opening == 0 {
            format!(" before step 1 of {}", format::data(steps))
        } else {
            format!(
                " at step {} of {}",
                format::data(opening),
                format::data(steps)
            )
        };
        return format!(
            "Link copied. It encodes all {} runs, and opens this one{place}.",
            format::data(runs)
        );
    }
    if steps == 0 {
        return "Link copied. It encodes the configuration; there are no steps yet.".to_owned();
    }
    let all = if steps == 1 {
        "its 1 step".to_owned()
    } else {
        format!("all {}", step_count(steps))
    };
    let opens = if opening == 0 {
        "opens before the first".to_owned()
    } else {
        format!("opens at step {}", format::data(opening))
    };
    format!("Link copied. It encodes the configuration and {all}, and {opens}.")
}

/// `steps` steps: "14 steps", "1 step".
fn step_count(steps: u32) -> String {
    format!(
        "{} {}",
        format::data(steps),
        format::plural(steps, "step", "steps")
    )
}

/// The heading of the page's state for a link or a file that cannot be
/// opened: "Could not open link", or for a file "Could not open file".
pub(crate) fn load_title(file: bool) -> String {
    format!("Could not open {}", if file { "file" } else { "link" })
}

/// The lead under that heading: "This link doesn't describe a cache the
/// Interactive Cache Explorer can build.", or the same of a file.
pub(crate) fn load_lead(file: bool) -> String {
    format!(
        "This {} doesn't describe a cache the Interactive Cache Explorer can build.",
        if file { "file" } else { "link" }
    )
}

/// The label of the option that opens the actions, the ops, a link or a
/// file holds before the one that could not be replayed: "Open the first 8
/// actions", or with one, "Open the first action".
pub(crate) fn prefix_label(actions: u32) -> String {
    if actions == 1 {
        "Open the first action".to_owned()
    } else {
        format!("Open the first {} actions", format::data(actions))
    }
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

/// The module panel's label for a step of `kind`, in the design's words:
/// "Explain", "Do this", "Predict", "Watch", and "Done" on the done screen.
pub(crate) fn module_step_kind(kind: ModuleStepKind) -> String {
    match kind {
        ModuleStepKind::Explain => "Explain",
        ModuleStepKind::Instruct => "Do this",
        ModuleStepKind::Predict => "Predict",
        ModuleStepKind::Observe => "Watch",
        ModuleStepKind::Done => "Done",
    }
    .to_owned()
}

/// The module panel's progress: "Step 3 of 6" on step index 2 of 6, and
/// "6 of 6" on the done screen, where `step` is the number of steps.
pub(crate) fn module_progress(step: u32, steps: u32) -> String {
    if step >= steps {
        format!("{} of {}", format::data(steps), format::data(steps))
    } else {
        format!(
            "Step {} of {}",
            format::data(step.saturating_add(1)),
            format::data(steps)
        )
    }
}

/// The picker's line while a module runs: its number and title, then the
/// step, "Running 04 · Tracking LRU · step 3 of 6", or on the done screen
/// "Running 04 · Tracking LRU · complete". `step` is the step's index, and
/// the number of steps on the done screen.
pub(crate) fn module_running(number: &str, title: &str, step: u32, steps: u32) -> String {
    let at = if step >= steps {
        "complete".to_owned()
    } else {
        format!(
            "step {} of {}",
            format::data(step.saturating_add(1)),
            format::data(steps)
        )
    };
    format!("Running {number} \u{b7} {title} \u{b7} {at}")
}

/// The picker's line while no module runs: "9 modules · none running".
pub(crate) fn modules_idle(modules: u32) -> String {
    format!(
        "{} {} \u{b7} none running",
        format::data(modules),
        format::plural(modules, "module", "modules")
    )
}

/// The done screen's text.
pub(crate) fn module_complete() -> String {
    "Module complete. The workbench stays as you left it; keep experimenting or pick another \
     module."
        .to_owned()
}

/// The head of a prediction's feedback: "Correct.", "Not quite." or
/// "Shown.".
pub(crate) fn feedback_head(tone: FeedbackTone) -> String {
    match tone {
        FeedbackTone::Correct => "Correct.",
        FeedbackTone::Incorrect => "Not quite.",
        FeedbackTone::Shown => "Shown.",
    }
    .to_owned()
}

/// What a revealed prediction's feedback says before the explanation.
pub(crate) fn answer_shown() -> String {
    "The answer is highlighted. ".to_owned()
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
            .map(|&addr| address(geometry, addr).to_string())
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

/// The placement as the summaries write it: "direct-mapped", "fully
/// associative" or "2-way".
fn placement_name(cache: &CacheConfig) -> String {
    match cache.placement() {
        Placement::DirectMapped => "direct-mapped".to_owned(),
        Placement::FullyAssociative => "fully associative".to_owned(),
        Placement::SetAssociative => format!("{}-way", format::setting(cache.associativity)),
    }
}

/// The replacement policy as the summaries and options name it.
fn replacement_name(replacement: Replacement) -> &'static str {
    match replacement {
        Replacement::Lru => "LRU",
        Replacement::Fifo => "FIFO",
        Replacement::Random => "Random",
    }
}

/// An address or a line address, padded to the address width. A `Display`
/// adapter, like [`format::Hex`] itself, so a per-event line can interpolate
/// it without allocating a `String` first.
fn address(geometry: &Geometry, value: u32) -> format::Hex {
    format::Hex {
        value: u64::from(value),
        digits: usize::from(geometry.address_bits()).div_ceil(4),
    }
}

/// A tag, padded to the tag width. A `Display` adapter, for the same reason
/// as [`address`].
fn tag(geometry: &Geometry, tag: Tag) -> format::Hex {
    format::Hex {
        value: u64::from(tag.get()),
        digits: usize::from(geometry.tag_bits()).div_ceil(4).max(2),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::{CacheConfig, Config, Preset, WordBytes};
    use crate::limits::MAX_LINES;

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

    fn line(value: u32) -> LineAddr {
        LineAddr::new_unchecked(value)
    }

    fn tag_of(value: u32) -> Tag {
        Tag::new_unchecked(value)
    }

    fn set(index: u32) -> SetIndex {
        SetIndex::new_unchecked(index)
    }

    fn way(index: u32) -> WayIndex {
        WayIndex::new_unchecked(index)
    }

    fn word(index: u32) -> WordIndex {
        WordIndex::new_unchecked(index)
    }

    fn value(value: u64) -> Word {
        Word::new_unchecked(value)
    }

    fn choice(index: u32, reason: VictimReason) -> VictimChoice {
        VictimChoice {
            way: way(index),
            reason,
        }
    }

    #[test]
    fn the_module_panel_s_texts() {
        let labels = [
            (ModuleStepKind::Explain, "Explain"),
            (ModuleStepKind::Instruct, "Do this"),
            (ModuleStepKind::Predict, "Predict"),
            (ModuleStepKind::Observe, "Watch"),
            (ModuleStepKind::Done, "Done"),
        ];
        for (kind, label) in labels {
            assert_eq!(module_step_kind(kind), label);
        }
        assert_eq!(module_progress(2, 6), "Step 3 of 6");
        assert_eq!(module_progress(6, 6), "6 of 6");
        assert_eq!(
            module_running("04", "Tracking LRU", 2, 6),
            "Running 04 \u{b7} Tracking LRU \u{b7} step 3 of 6"
        );
        assert_eq!(
            module_running("04", "Tracking LRU", 6, 6),
            "Running 04 \u{b7} Tracking LRU \u{b7} complete"
        );
        assert_eq!(modules_idle(9), "9 modules \u{b7} none running");
        assert_eq!(modules_idle(1), "1 module \u{b7} none running");
        assert_eq!(feedback_head(FeedbackTone::Correct), "Correct.");
        assert_eq!(feedback_head(FeedbackTone::Incorrect), "Not quite.");
        assert_eq!(feedback_head(FeedbackTone::Shown), "Shown.");
        assert_eq!(answer_shown(), "The answer is highlighted. ");
    }

    #[test]
    fn decode_names_the_fields_of_the_address() {
        let two_way = preset(Preset::TwoWayLru);
        let at = addr(0x2A4);
        assert_eq!(
            decode(&two_way, AccessKind::Read, at, two_way.locate(at)),
            "Load 0x2A4: tag 0x0A, set 2, offset 4 (word 1)."
        );
        let direct = preset(Preset::DirectMapped8);
        let at = addr(0x0A4);
        assert_eq!(
            decode(&direct, AccessKind::Write, at, direct.locate(at)),
            "Store 0x0A4: tag 0x01, set 2, offset 4 (word 1)."
        );
    }

    #[test]
    fn decode_pads_to_the_configured_widths() {
        let config = Config {
            address_bits: 16,
            word_bytes: WordBytes::Eight,
            ..Preset::DirectMapped8.config()
        };
        let wide = geometry(&config);
        let at = addr(0x0A8);
        assert_eq!(
            decode(&wide, AccessKind::Read, at, wide.locate(at)),
            "Load 0x00A8: tag 0x001, set 2, offset 8 (word 1)."
        );
    }

    #[test]
    fn a_fully_associative_decode_has_no_set() {
        for geometry in [ways(8), one_line()] {
            let at = addr(0x0A4);
            assert_eq!(
                decode(&geometry, AccessKind::Read, at, geometry.locate(at)),
                "Load 0x0A4: tag 0x0A, offset 4 (word 1). Fully associative: one set, no index \
                 bits."
            );
        }
    }

    #[test]
    fn a_hit_names_the_way_and_the_tag() {
        assert_eq!(
            hit(&preset(Preset::DirectMapped8), set(2), way(0), tag_of(1)),
            "Set 2, way 0: valid, and tag 0x01 matches. Hit."
        );
    }

    #[test]
    fn a_miss_in_a_one_way_set_names_what_the_way_held() {
        let direct = preset(Preset::DirectMapped8);
        assert_eq!(
            miss(&direct, set(2), tag_of(3), Some(tag_of(1))),
            "Set 2 holds tag 0x01, not 0x03. Miss."
        );
        assert_eq!(
            miss(&direct, set(2), tag_of(1), None),
            "Set 2 is invalid. Miss."
        );
        assert_eq!(
            miss(&one_line(), set(0), tag_of(0x1A), Some(tag_of(0x0A))),
            "Set 0 holds tag 0x0A, not 0x1A. Miss."
        );
    }

    #[test]
    fn a_miss_in_a_set_of_several_ways_names_only_the_tag() {
        let two_way = preset(Preset::TwoWayLru);
        for held in [None, Some(tag_of(6))] {
            assert_eq!(
                miss(&two_way, set(2), tag_of(0x0A), held),
                "Set 2: no valid way holds tag 0x0A. Miss."
            );
        }
    }

    #[test]
    fn a_victim_line_gives_the_reason_for_the_way() {
        let two_way = preset(Preset::TwoWayLru);
        let lines = [
            (
                choice(0, VictimReason::Empty),
                "Way 0 chosen: it is invalid (empty).",
            ),
            (
                choice(1, VictimReason::LeastRecentlyUsed { rank: 1 }),
                "Way 1 chosen: least recently used (rank 1).",
            ),
            (
                choice(0, VictimReason::FirstIn { rank: 1 }),
                "Way 0 chosen: filled earliest (FIFO).",
            ),
            (choice(1, VictimReason::Random), "Way 1 chosen at random."),
        ];
        for (chosen, line) in lines {
            assert_eq!(victim(&two_way, set(0), chosen), line);
        }
    }

    #[test]
    fn a_one_way_set_names_its_only_way() {
        let direct = preset(Preset::DirectMapped8);
        assert_eq!(
            victim(&direct, set(2), choice(0, VictimReason::Empty)),
            "Set 2 has one way; the line goes there."
        );
        assert_eq!(
            victim(&direct, set(2), choice(0, VictimReason::OnlyWay)),
            "Direct-mapped: way 0 is the only place for this line, so its current line must go."
        );
        // A one-line cache is fully associative, not direct-mapped.
        assert_eq!(
            victim(&one_line(), set(0), choice(0, VictimReason::Empty)),
            "Set 0 has one way; the line goes there."
        );
        assert_eq!(
            victim(&one_line(), set(0), choice(0, VictimReason::OnlyWay)),
            "Way 0 is the only place for this line, so its current line must go."
        );
    }

    #[test]
    fn the_allocation_lines_name_the_old_and_the_new_line() {
        let two_way = preset(Preset::TwoWayLru);
        assert_eq!(
            write_back(&two_way, way(1), line(0x1A0)),
            "Way 1 is dirty: write line 0x1A0 back to memory first."
        );
        assert_eq!(
            evict(&two_way, way(1), line(0x1A0), tag_of(6)),
            "Evict line 0x1A0 (tag 0x06) from way 1."
        );
        assert_eq!(
            fill(&two_way, way(1), line(0x2A0)),
            "Fill way 1 with line 0x2A0\u{2013}0x2AF from memory."
        );
    }

    #[test]
    fn a_stall_names_its_length_and_what_it_waits_for() {
        let two_way = preset(Preset::TwoWayLru);
        assert_eq!(
            stall(&two_way, 20, StallFor::Fill { line: line(0x2A0) }),
            "The CPU stalls for 20 cycles while memory supplies line 0x2A0."
        );
        assert_eq!(
            stall(
                &two_way,
                20,
                StallFor::WriteBackThenFill {
                    victim: line(0x1A0),
                    line: line(0x2A0)
                }
            ),
            "The CPU stalls for 40 cycles: 20 to write line 0x1A0 back, then 20 to fetch line \
             0x2A0."
        );
        assert_eq!(
            stall(&two_way, 20, StallFor::WriteAround),
            "The CPU stalls for 20 cycles while the store goes to memory."
        );
    }

    #[test]
    fn a_stall_of_one_cycle_is_singular_and_long_ones_are_grouped() {
        let direct = preset(Preset::DirectMapped8);
        assert_eq!(
            stall(&direct, 1, StallFor::Fill { line: line(0x0A0) }),
            "The CPU stalls for 1 cycle while memory supplies line 0x0A0."
        );
        assert_eq!(
            stall(
                &direct,
                1,
                StallFor::WriteBackThenFill {
                    victim: line(0x0A0),
                    line: line(0x1A0)
                }
            ),
            "The CPU stalls for 2 cycles: 1 to write line 0x0A0 back, then 1 to fetch line \
             0x1A0."
        );
        assert_eq!(
            stall(
                &direct,
                1_500,
                StallFor::WriteBackThenFill {
                    victim: line(0x0A0),
                    line: line(0x1A0)
                }
            ),
            "The CPU stalls for 3,000 cycles: 1,500 to write line 0x0A0 back, then 1,500 to \
             fetch line 0x1A0."
        );
        assert_eq!(
            stall(&direct, 1, StallFor::WriteAround),
            "The CPU stalls for 1 cycle while the store goes to memory."
        );
    }

    #[test]
    fn a_stall_waits_for_one_transfer_or_two() {
        let fill = StallFor::Fill { line: line(0x2A0) };
        let both = StallFor::WriteBackThenFill {
            victim: line(0x1A0),
            line: line(0x2A0),
        };
        assert_eq!(fill.transfers(), 1);
        assert_eq!(StallFor::WriteAround.transfers(), 1);
        assert_eq!(both.transfers(), 2);
    }

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
    fn a_rank_line_lists_every_other_way_in_way_order() {
        assert_eq!(
            rank(Replacement::Lru, set(2), way(1), &[1, 0]),
            "LRU ranks in set 2: way 1 becomes 0 (most recent); way 0 is 1."
        );
        assert_eq!(
            rank(Replacement::Fifo, set(0), way(0), &[0, 1]),
            "FIFO order in set 0: way 0 becomes 0 (newest); way 1 is 1."
        );
        assert_eq!(
            rank(Replacement::Lru, set(3), way(2), &[1, 3, 0, 2]),
            "LRU ranks in set 3: way 2 becomes 0 (most recent); way 0 is 1, way 1 is 3, way 3 \
             is 2."
        );
    }

    #[test]
    fn a_rank_line_lists_every_other_way_of_a_64_way_set() {
        let ranks: Vec<u32> = (0..64).map(|index| (index + 1) % 64).collect();
        let others = (1..64_u32)
            .map(|other| format!("way {other} is {}", (other + 1) % 64))
            .collect::<Vec<String>>()
            .join(", ");
        let expected = format!("LRU ranks in set 5: way 0 becomes 1 (most recent); {others}.");

        assert_eq!(rank(Replacement::Lru, set(5), way(0), &ranks), expected);
    }

    #[test]
    fn a_load_names_the_word_the_value_and_the_register() {
        assert_eq!(
            load(word(1), way(1), 68, Register::R3, false),
            "Read word 1 of way 1 (68) into r3."
        );
        assert_eq!(
            load(word(0), way(0), 12_345, Register::R0, false),
            "Read word 0 of way 0 (12,345); r0 always reads 0, so the value is dropped."
        );
    }

    #[test]
    fn a_load_issued_beside_a_program_says_the_program_register_changed() {
        assert_eq!(
            load(word(1), way(0), 53, Register::R1, true),
            "Read word 1 of way 0 (53) into r1. A program is loaded, so its r1 now holds 53."
        );
        assert_eq!(
            load(word(1), way(0), 12_345, Register::R1, true),
            "Read word 1 of way 0 (12,345) into r1. A program is loaded, so its r1 now holds \
             12,345."
        );
    }

    #[test]
    fn a_truncated_store_names_the_register_and_the_bits_kept() {
        assert_eq!(
            truncated(Register::R2, 300, 1, value(44)),
            "r2 = 300 does not fit a 1-byte word, so its low 8 bits, 44, are stored. "
        );
        assert_eq!(
            truncated(Register::R7, u64::MAX, 4, value(u64::from(u32::MAX))),
            "r7 = 18,446,744,073,709,551,615 does not fit a 4-byte word, so its low 32 bits, \
             4,294,967,295, are stored. "
        );
        assert_eq!(
            truncated(Register::R1, 70_000, 8, value(70_000)),
            "r1 = 70,000 does not fit an 8-byte word, so its low 64 bits, 70,000, are stored. "
        );
    }

    #[test]
    fn alu_lines_name_the_register_and_the_value() {
        assert_eq!(
            add(Register::R2, 176),
            "add: r2 \u{2190} 176. No memory access."
        );
        assert_eq!(
            add(Register::R1, u64::MAX),
            "add: r1 \u{2190} 18,446,744,073,709,551,615. No memory access."
        );
        assert_eq!(
            add(Register::R0, 5),
            "add: r0 always reads 0, so the result 5 is dropped."
        );
        assert_eq!(
            bnez(Register::R1, 3, Some("loop")),
            "bnez: r1 is 3, branch taken to loop."
        );
        assert_eq!(
            bnez(Register::R1, 0, None),
            "bnez: r1 is 0, branch not taken."
        );
    }

    #[test]
    fn a_fault_names_the_line_the_register_and_its_value() {
        let direct = preset(Preset::DirectMapped8);
        assert_eq!(
            fault_out_of_range(&direct, 4, Register::R2, 4_096),
            "Line 4: r2 holds 4096, which does not fit a 12-bit address. The program stopped."
        );
        assert_eq!(
            fault_out_of_range(&direct, 12, Register::R3, u64::MAX),
            "Line 12: r3 holds 18,446,744,073,709,551,615, which does not fit a 12-bit address. \
             The program stopped."
        );
        assert_eq!(
            fault_misaligned(&direct, 4, Register::R2, addr(0x0A6)),
            "Line 4: r2 holds 166 (0x0A6), which is not aligned to a 4-byte word. The program \
             stopped."
        );
        let eight_bits = geometry(&Config {
            address_bits: 8,
            word_bytes: WordBytes::Eight,
            ..Preset::DirectMapped8.config()
        });
        assert_eq!(
            fault_out_of_range(&eight_bits, 1, Register::R1, 256),
            "Line 1: r1 holds 256, which does not fit an 8-bit address. The program stopped."
        );
        assert_eq!(
            fault_misaligned(&eight_bits, 1, Register::R1, addr(0xA4)),
            "Line 1: r1 holds 164 (0xA4), which is not aligned to an 8-byte word. The program \
             stopped."
        );
    }

    #[test]
    fn the_budget_line_states_the_budget() {
        assert_eq!(
            out_of_budget(),
            format!(
                "The program used its budget of {} instructions and stopped. Is a loop missing \
                 its exit?",
                format::setting(INSTRUCTION_BUDGET)
            )
        );
    }

    #[test]
    fn a_program_load_counts_instructions_and_a_trace_accesses() {
        assert_eq!(
            program_loaded(ProgramFormat::Asm, 5),
            "Loaded a program of 5 instructions. Registers and the pc are reset; the cache and \
             memory keep their contents."
        );
        assert_eq!(
            program_loaded(ProgramFormat::Asm, 1),
            "Loaded a program of 1 instruction. Registers and the pc are reset; the cache and \
             memory keep their contents."
        );
        assert_eq!(
            program_loaded(ProgramFormat::Trace, 3),
            "Loaded a trace of 3 accesses. Registers and the pc are reset; the cache and memory \
             keep their contents."
        );
        assert_eq!(
            program_summary(ProgramFormat::Asm, 5, Some("ld r1, [0x0A4]")),
            "5 instructions \u{b7} next: ld r1, [0x0A4]"
        );
        assert_eq!(
            program_summary(ProgramFormat::Trace, 1, Some("R 0x0A4")),
            "1 access \u{b7} next: R 0x0A4"
        );
        assert_eq!(
            program_summary(ProgramFormat::Asm, 1, None),
            "1 instruction \u{b7} next: end"
        );
    }

    #[test]
    fn a_parse_error_names_its_line_and_column() {
        assert_eq!(
            parse_error(3, 5, "unknown instruction `lw`; use ld, st, add or bnez."),
            "Line 3, column 5: unknown instruction `lw`; use ld, st, add or bnez."
        );
        // A place is named, not counted, so it is never grouped.
        assert_eq!(
            parse_error(1, 16_385, "the program is larger than 16,384 bytes."),
            "Line 1, column 16385: the program is larger than 16,384 bytes."
        );
    }

    #[test]
    fn parse_examples_follow_the_address_width_and_the_word() {
        let direct = preset(Preset::DirectMapped8);
        let operands = ParseProblem::Operands {
            mnemonic: Mnemonic::Ld,
        };
        assert_eq!(
            parse_problem(&direct, operands),
            "`ld` needs a register and an address, like `ld r1, [0x0A4]`."
        );
        let wide = geometry(&Config {
            address_bits: 16,
            word_bytes: WordBytes::Eight,
            ..Preset::DirectMapped8.config()
        });
        assert_eq!(
            parse_problem(&wide, operands),
            "`ld` needs a register and an address, like `ld r1, [0x00A0]`."
        );
        assert_eq!(
            parse_problem(
                &wide,
                ParseProblem::DecimalAddress {
                    text: "164",
                    value: 164
                }
            ),
            "addresses are hex: write 0x00A4, not 164."
        );
        let narrow = geometry(&Config {
            address_bits: 8,
            ..Preset::DirectMapped8.config()
        });
        assert_eq!(
            parse_problem(
                &narrow,
                ParseProblem::TraceOperands {
                    kind: AccessKind::Write
                }
            ),
            "`W` needs an address and a value, like `W 0xA4 7`."
        );
    }

    #[test]
    fn a_trace_value_too_wide_gives_the_word_s_unsigned_range() {
        let too_wide = |word_bytes: WordBytes, value: u64| {
            let geometry = geometry(&Config {
                word_bytes,
                ..Preset::DirectMapped8.config()
            });
            parse_problem(&geometry, ParseProblem::ValueTooWide { value })
        };
        assert_eq!(
            too_wide(WordBytes::One, 300),
            "300 does not fit a 1-byte word (0\u{2013}255)."
        );
        assert_eq!(
            too_wide(WordBytes::Two, 70_000),
            "70,000 does not fit a 2-byte word (0\u{2013}65,535)."
        );
        assert_eq!(
            too_wide(WordBytes::Four, 1 << 32),
            "4,294,967,296 does not fit a 4-byte word (0\u{2013}4,294,967,295)."
        );
    }

    #[test]
    fn a_write_back_store_names_the_dirty_bit_and_memory() {
        let direct = preset(Preset::DirectMapped8);
        let lines = [
            (
                Dirtiness::Clean,
                true,
                "Write 7 into word 1 of way 0 and set its dirty bit. Memory at 0x0A4 is now \
                 stale.",
            ),
            (
                Dirtiness::Clean,
                false,
                "Write 7 into word 1 of way 0 and set its dirty bit. Memory at 0x0A4 already \
                 holds 7.",
            ),
            (
                Dirtiness::Dirty,
                true,
                "Write 7 into word 1 of way 0; the line was already dirty. Memory at 0x0A4 is \
                 stale.",
            ),
            (
                Dirtiness::Dirty,
                false,
                "Write 7 into word 1 of way 0; the line was already dirty. Memory at 0x0A4 \
                 already holds 7.",
            ),
        ];
        for (before, stale, line) in lines {
            let note = WriteNote::Back { before, stale };
            assert_eq!(
                write(&direct, addr(0x0A4), word(1), way(0), value(7), note),
                line
            );
        }
    }

    #[test]
    fn a_write_through_store_writes_the_cache_then_memory() {
        let direct = preset(Preset::DirectMapped8);
        assert_eq!(
            write(
                &direct,
                addr(0x0A4),
                word(1),
                way(0),
                value(7),
                WriteNote::Through
            ),
            "Write 7 into word 1 of way 0."
        );
        assert_eq!(
            write_through(&direct, addr(0x0A4), value(7)),
            "Write-through: 7 also goes to memory at 0x0A4."
        );
    }

    #[test]
    fn a_write_around_leaves_the_cache_alone() {
        assert_eq!(
            write_around(&preset(Preset::DirectMapped8), addr(0x0A4), value(70_000)),
            "Write-no-allocate: 70,000 goes straight to memory at 0x0A4. The cache is unchanged."
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
    fn an_internal_error_names_the_invariant() {
        let violation = InvariantViolation::DuplicateTag { set: set(2) };
        assert_eq!(
            internal_error(&violation),
            "Internal error: invariant I3 (no two valid ways in set 2 hold the same tag) no \
             longer holds. The run stopped here; please report it with a share link."
        );
    }

    #[test]
    fn a_new_run_names_its_configuration_and_the_program_it_keeps() {
        let two_way = Preset::TwoWayLru.config();
        assert_eq!(
            new_run(&two_way, None),
            "New run: 128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU. The cache is empty and \
             memory, registers and counters are reset."
        );
        assert_eq!(
            new_run(
                &Preset::DirectMapped8.config(),
                Some((ProgramFormat::Asm, 3))
            ),
            "New run: 128 B \u{b7} 16 B lines \u{b7} direct-mapped. The cache is empty and \
             memory, registers and counters are reset. The program is kept and restarts at line \
             3."
        );
        // A trace is named as a trace.
        assert_eq!(
            new_run(&two_way, Some((ProgramFormat::Trace, 1))),
            "New run: 128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU. The cache is empty and \
             memory, registers and counters are reset. The trace is kept and restarts at line 1."
        );
        // A configuration that needs Advanced names its policies, whatever
        // mode the student is in, and the presentation flag changes nothing.
        let timed = Config {
            timing: Some(crate::config::Timing {
                hit_cycles: 1,
                miss_cycles: 20,
            }),
            ..two_way.clone()
        };
        let line = "New run: 128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU \u{b7} write-back \
                    \u{b7} allocate. The cache is empty and memory, registers and counters are \
                    reset.";
        assert_eq!(new_run(&timed, None), line);
        let advanced = Config {
            advanced: true,
            ..timed
        };
        assert_eq!(new_run(&advanced, None), line);
        assert_eq!(
            new_run(
                &Config {
                    advanced: true,
                    ..two_way.clone()
                },
                None
            ),
            new_run(&two_way, None)
        );
    }

    #[test]
    fn a_program_unloaded_by_a_new_run_quotes_its_first_error() {
        let first = ParseError {
            line: 3,
            column: 8,
            length: 7,
            message: "0x0A4 is not aligned to an 8-byte word; use 0x0A0 or 0x0A8.".to_owned(),
            text: String::new(),
        };
        assert_eq!(
            program_unloaded(ProgramFormat::Asm, &first),
            "The program no longer fits this configuration (line 3: 0x0A4 is not aligned to an \
             8-byte word; use 0x0A0 or 0x0A8); it was unloaded."
        );
        assert!(program_unloaded(ProgramFormat::Trace, &first).starts_with("The trace no longer"));
    }

    #[test]
    fn the_session_limit_line_and_the_end_of_a_program_say_why_nothing_runs() {
        assert_eq!(
            session_limit(),
            format!(
                "The session reached its limit of {} steps, so the program stopped here.",
                format::setting(SESSION_EVENT_LIMIT)
            )
        );
        assert_eq!(end_of_program(), "End of program.");
    }

    #[test]
    fn the_reset_question_counts_the_steps_kept_and_dropped() {
        assert_eq!(
            needs_reset(14, 0),
            "Configuration is fixed for a run. Applying starts a new, empty run; this one has 14 \
             steps and stays reachable with Undo."
        );
        assert!(needs_reset(1, 0).contains("this one has 1 step and stays"));
        assert_eq!(
            needs_reset(3, 6),
            "Configuration is fixed for a run. Applying starts a new, empty run here and drops \
             the 6 undone steps after this point; this run's 3 steps stay reachable with Undo."
        );
        assert!(needs_reset(1, 1).ends_with(
            "drops the 1 undone step after this point; this run's 1 step stays reachable with \
             Undo."
        ));
        // Counts are data, grouped from 10,000.
        assert!(needs_reset(12_000, 0).contains("this one has 12,000 steps"));
    }

    fn config(size: u32, ways: u32, replacement: Replacement) -> Config {
        Config {
            address_bits: ADDRESS_BITS,
            cache: CacheConfig {
                size_bytes: size,
                associativity: ways,
                replacement,
                ..Preset::DirectMapped8.config().cache
            },
            ..Preset::DirectMapped8.config()
        }
    }

    #[test]
    fn summaries_name_the_placement_policy_and_sizes() {
        let two_way = config(128, 2, Replacement::Lru);
        assert_eq!(
            config_summary(&two_way, false),
            "128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU"
        );
        assert_eq!(
            config_summary(&two_way, true),
            "128 B \u{b7} 16 B lines \u{b7} 2-way \u{b7} LRU \u{b7} write-back \u{b7} allocate"
        );
        assert_eq!(short_summary(&two_way), "2-way LRU \u{b7} 128 B");
        let direct = config(128, 1, Replacement::Fifo);
        assert_eq!(
            config_summary(&direct, false),
            "128 B \u{b7} 16 B lines \u{b7} direct-mapped"
        );
        assert_eq!(short_summary(&direct), "Direct-mapped \u{b7} 128 B");
        let full = config(2_048, 128, Replacement::Random);
        assert_eq!(
            config_summary(&full, false),
            "2,048 B \u{b7} 16 B lines \u{b7} fully associative \u{b7} Random"
        );
        assert_eq!(
            short_summary(&full),
            "Fully associative Random \u{b7} 2,048 B"
        );
    }

    #[test]
    fn a_repair_label_names_the_repair_of_the_first_rule_broken() {
        let broken = config(128, 3, Replacement::Lru);
        let first = broken.validate().unwrap_err();
        let repaired = config(128, 2, Replacement::Lru);
        assert_eq!(
            repair_label(&first, &repaired, &geometry(&repaired)),
            "Open with 2-way instead"
        );
        // Twice as many lines as the limit, repaired to the limit, which
        // holds whatever the limit is tuned to.
        let size = MAX_LINES * 16;
        let too_many = ConfigError::TooManyLines {
            lines: 2 * MAX_LINES,
            size: 2 * size,
            line: 16,
        };
        let smaller = config(size, 1, Replacement::Lru);
        assert_eq!(
            repair_label(&too_many, &smaller, &geometry(&smaller)),
            format!(
                "Open as {} {} B cache instead",
                format::a_or_an(size),
                format::setting(size)
            )
        );
    }

    #[test]
    fn option_labels_are_the_designs() {
        assert_eq!(bytes_option(64), "64 B");
        assert_eq!(bytes_option(1_024), "1,024 B");
        assert_eq!(ways_option(1), "Direct");
        assert_eq!(ways_option(2), "2-way");
        assert_eq!(ways_option(16), "16-way");
        assert_eq!(full_option(), "Full");
        assert_eq!(replacement_option(Replacement::Fifo), "FIFO");
        assert_eq!(address_option(12), "12-bit");
        assert_eq!(word_option(WordBytes::Eight), "8 B");
        assert_eq!(write_option(WritePolicy::WriteThrough), "Write-through");
        assert_eq!(
            allocate_option(AllocatePolicy::WriteNoAllocate),
            "No allocate"
        );
        assert_eq!(same_as_full(8), "Same as Full for an 8-line cache.");
        assert_eq!(same_as_full(16), "Same as Full for a 16-line cache.");
        assert_eq!(same_as_full(1), "Same as Full for a 1-line cache.");
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
