//! The wording of the log: every line built from machine state is written
//! here, so the tool's voice has one source. That includes the CPU's status
//! and the Stats lines: AMAT and the measured cost in timed mode, and the
//! count beside the hit and miss sparkline; and the program drawer's lines:
//! its summary and the messages of a program or trace that does not parse.
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

use crate::cache::Dirtiness;
use crate::config::Replacement;
use crate::format;
use crate::geometry::{highest_word, Geometry, Location, Placement};
use crate::isa::Mnemonic;
use crate::limits::{
    INSTRUCTION_BUDGET, MAX_LABEL_CHARS, MAX_PROGRAM_BYTES, MAX_PROGRAM_INSTRUCTIONS,
    MAX_PROGRAM_LINES,
};
use crate::machine::InvariantViolation;
use crate::op::{AccessKind, ProgramFormat};
use crate::replacement::{VictimChoice, VictimReason};
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
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the CPU status names the cursor's entry")
)]
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
    let offset = format::data(location.offset);
    let word = format::data(location.word.get());
    match geometry.placement() {
        Placement::FullyAssociative => format!(
            "{verb} {addr}: tag {tag}, offset {offset} (word {word}). Fully associative: one \
             set, no index bits."
        ),
        Placement::DirectMapped | Placement::SetAssociative => format!(
            "{verb} {addr}: tag {tag}, set {}, offset {offset} (word {word}).",
            format::data(location.set.get())
        ),
    }
}

/// The `Hit` line: "Set 2, way 0: valid, and tag 0x01 matches. Hit."
pub(crate) fn hit(geometry: &Geometry, set: SetIndex, way: WayIndex, tag: Tag) -> String {
    format!(
        "Set {}, way {}: valid, and tag {} matches. Hit.",
        format::data(set.get()),
        format::data(way.get()),
        self::tag(geometry, tag)
    )
}

/// The `Miss` line. A set of several ways reads "Set 2: no valid way holds
/// tag 0x0A. Miss."; a one-way set names what its way held, `held`, the tag
/// it was compared against: "Set 2 holds tag 0x01, not 0x03. Miss.", or "Set
/// 2 is invalid. Miss." when it held nothing.
pub(crate) fn miss(geometry: &Geometry, set: SetIndex, tag: Tag, held: Option<Tag>) -> String {
    let set = format::data(set.get());
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
    let way = format::data(choice.way.get());
    match choice.reason {
        VictimReason::Empty if geometry.ways() == 1 => format!(
            "Set {} has one way; the line goes there.",
            format::data(set.get())
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
            format::data(rank)
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
            let each = format::setting(miss);
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
        format::data(way.get()),
        address(geometry, line.get())
    )
}

/// The `Evict` line: "Evict line 0x1A0 (tag 0x06) from way 1."
pub(crate) fn evict(geometry: &Geometry, way: WayIndex, line: LineAddr, tag: Tag) -> String {
    format!(
        "Evict line {} (tag {}) from way {}.",
        address(geometry, line.get()),
        self::tag(geometry, tag),
        format::data(way.get())
    )
}

/// The `Fill` line: "Fill way 1 with line 0x2A0–0x2AF from memory."
pub(crate) fn fill(geometry: &Geometry, way: WayIndex, line: LineAddr) -> String {
    format!(
        "Fill way {} with line {} from memory.",
        format::data(way.get()),
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
    let others: Vec<String> = (0_u32..)
        .zip(ranks)
        .filter(|(other, _)| *other != way.get())
        .map(|(other, rank)| format!("way {} is {}", format::data(other), format::data(*rank)))
        .collect();
    let moved = format!(
        "{order} in set {}: way {} becomes {} ({front})",
        format::data(set.get()),
        format::data(way.get()),
        format::data(promoted)
    );
    if others.is_empty() {
        format!("{moved}.")
    } else {
        format!("{moved}; {}.", others.join(", "))
    }
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
    let shown = format::data(value);
    let read = format!(
        "Read word {} of way {} ({shown})",
        format::data(word.get()),
        format::data(way.get())
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
    let value = format::data(value.get());
    let wrote = format!(
        "Write {value} into word {} of way {}",
        format::data(word.get()),
        format::data(way.get())
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
        format::data(value.get()),
        address(geometry, addr.get())
    )
}

/// The `MemWrite` line of a store that missed under write-no-allocate:
/// "Write-no-allocate: 7 goes straight to memory at 0x0A4. The cache is
/// unchanged."
pub(crate) fn write_around(geometry: &Geometry, addr: Addr, value: Word) -> String {
    format!(
        "Write-no-allocate: {} goes straight to memory at {}. The cache is unchanged.",
        format::data(value.get()),
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

/// The program drawer's line: how many instructions, and the next one, or
/// `end` past the last: "5 instructions · next: ld r1, [0x0A4]". A trace
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
    address(geometry, DESIGN_EXAMPLE - DESIGN_EXAMPLE % word_bytes)
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
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the CPU status names the cursor's entry")
)]
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
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the CPU status counts a stall down")
)]
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

/// An address or a line address, padded to the address width.
fn address(geometry: &Geometry, value: u32) -> String {
    format::address(value, geometry.address_bits())
}

/// A tag, padded to the tag width.
fn tag(geometry: &Geometry, tag: Tag) -> String {
    format::tag(tag.get(), geometry.tag_bits())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::{CacheConfig, Config, Preset, WordBytes};

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
}
