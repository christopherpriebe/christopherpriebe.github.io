//! The wording of the program drawer: a program or trace's summary, the
//! messages of one that does not parse, and the lines a run's Setup and a
//! new run's Error print around loading, unloading and resetting.

use crate::config::Config;
use crate::format;
use crate::geometry::{highest_word, Geometry};
use crate::isa::{Mnemonic, ParseError};
use crate::limits::{
    MAX_LABEL_CHARS, MAX_PROGRAM_BYTES, MAX_PROGRAM_INSTRUCTIONS, MAX_PROGRAM_LINES,
    SESSION_EVENT_LIMIT,
};
use crate::op::{AccessKind, ProgramFormat};
use crate::units::Addr;

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
                super::source_noun(format),
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
            super::address(geometry, addr.get())
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
            super::source_noun(format),
            format::setting(MAX_PROGRAM_BYTES)
        ),
        ParseProblem::TooManyLines { format } => format!(
            "the {} has more than {} lines.",
            super::source_noun(format),
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
    super::address(geometry, DESIGN_EXAMPLE - DESIGN_EXAMPLE % word_bytes).to_string()
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

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::{Config, Preset, WordBytes};

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
}
