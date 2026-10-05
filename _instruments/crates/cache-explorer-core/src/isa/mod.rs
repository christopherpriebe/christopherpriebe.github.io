//! Programs: the tiny assembly language and the trace format, parsed into a
//! [`Program`] under a run's geometry, or into the errors the program drawer
//! lists.
//!
//! The assembly language has four instructions: `ld rD, [addr|rS]`,
//! `st [addr|rS], rS|#imm`, `add rD, rS, rT|#imm` and `bnez rS, label`, with
//! labels and `;` comments. A trace has one access per line: `R addr`, which
//! loads into `r1`, and `W addr value`, which stores. Both are parsed line by
//! line; a line reports at most its first problem, and every problem is an
//! error with the line, column and length the drawer underlines. A literal
//! address must be hex, fit the address width and be word-aligned; a value
//! must fit the word. Both are checked here, so a program that parses never
//! faults on a literal; only an address held in a register can fault, when
//! it executes.
//!
//! The limits of `limits.rs` bound the work: a source of more than
//! [`MAX_PROGRAM_BYTES`] is refused before it is read, only the first
//! [`MAX_PROGRAM_LINES`] lines are read, and a result lists at most
//! [`MAX_PARSE_ERRORS`] errors.

mod asm;
mod diag;
mod space;
mod trace;

use std::sync::Arc;

use instrument_asm::lexer::{self, NumberWord, RegisterWord, Syntax, Token, TokenKind};
use serde::Serialize;

use crate::explain::{self, ParseProblem};
use crate::geometry::{AccessError, Geometry};
use crate::limits::{MAX_PARSE_ERRORS, MAX_PROGRAM_BYTES, MAX_PROGRAM_LINES};
use crate::op::{OpError, ProgramFormat};
use crate::units::{Addr, Register};
use diag::{chars, place_of_byte};

pub(crate) use asm::Mnemonic;
pub use diag::ParseError;
pub(crate) use space::AddressSpace;

/// The marks of the tiny language and of a trace: `;` starts a comment and
/// `[` and `]` bracket a memory operand.
const SYNTAX: Syntax = Syntax {
    comment: ';',
    open: '[',
    close: ']',
};

/// A program or trace that parsed under a run's geometry: its source, kept
/// so a new run can parse it again under its own configuration, and its
/// instructions in order. A trace's `R` and `W` lines are the loads and
/// stores they stand for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Program {
    /// Whether the source is assembly or a trace.
    format: ProgramFormat,
    /// The source as the student wrote it.
    source: String,
    /// The instructions, in order; never empty.
    instructions: Vec<Placed>,
}

/// An instruction and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Placed {
    /// The instruction.
    pub(crate) instruction: Instruction,
    /// Its source line, from 1.
    pub(crate) line: u32,
    /// Its source text without its label and comment, with each run of
    /// spaces and tabs made one space: `ld r1, [0x0A4]`, `W 0x0A4 7`. Shared
    /// with the label of the log entry the instruction makes.
    pub(crate) text: Arc<str>,
}

/// One instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Instruction {
    /// `ld rD, [addr|rS]`: the word at the address into `into`.
    Load {
        /// The register the word goes into.
        into: Register,
        /// Where the word is.
        from: Address,
    },
    /// `st [addr|rS], rS|#imm`: a register's value, or an immediate, into
    /// the word at the address.
    Store {
        /// Where the word goes.
        to: Address,
        /// What is stored.
        value: Operand,
    },
    /// `add rD, rS, rT|#imm`: the sum of a register and a register or an
    /// immediate into `into`, modulo 2^64.
    Add {
        /// The register the sum goes into.
        into: Register,
        /// The first addend.
        left: Register,
        /// The second addend.
        right: Operand,
    },
    /// `bnez rS, label`: branch to the label if the register is not 0.
    BranchIfNotZero {
        /// The register tested.
        test: Register,
        /// The index of the instruction the label marks; the program's
        /// length when the label follows the last instruction, where a
        /// branch finishes the program.
        target: u32,
        /// The label, as written.
        label: String,
    },
}

/// The address a load or store reads or writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Address {
    /// A literal address, checked against the geometry when the program was
    /// parsed.
    Literal(Addr),
    /// An address held in a register, checked when the instruction executes.
    Register(Register),
}

/// A value an instruction reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operand {
    /// A register's value.
    Register(Register),
    /// An immediate. A store's is already the word it stores, a negative
    /// one as its two's complement in the word's width; an add's is a
    /// 64-bit pattern.
    Immediate(u64),
}

/// The result of parsing a program or trace, as the program drawer shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ParseResult {
    /// The source is a program.
    Ok {
        /// How many instructions it has; for a trace, how many accesses.
        instructions: u32,
        /// The drawer's line: `5 instructions · next: ld r1, [0x0A4]`.
        summary: String,
    },
    /// The source has errors.
    Invalid {
        /// The errors in source order, at most [`MAX_PARSE_ERRORS`]; never
        /// empty.
        errors: Vec<ParseError>,
        /// The drawer's line: `2 errors · first on line 3`, or
        /// `1 error on line 3`. It counts every error, not only the listed.
        summary: String,
    },
}

/// Why a source is not a program: every error it has, in source order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParseFailure {
    /// Whether the source was read as assembly or as a trace.
    format: ProgramFormat,
    /// The first error.
    first: ParseError,
    /// The others, which the drawer lists up to its limit.
    rest: Vec<ParseError>,
}

/// A problem on a line: where it is and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Problem {
    /// Its first column.
    column: u32,
    /// Its length in characters.
    length: u32,
    /// Its message.
    message: String,
}

impl Geometry {
    /// Parses `source` as a program or a trace whose literal addresses and
    /// values must fit this geometry: the drawer's summary, or the errors,
    /// at most [`MAX_PARSE_ERRORS`], in source order.
    pub fn parse_program(&self, source: &str, format: ProgramFormat) -> ParseResult {
        match Program::parse(self, source, format) {
            Ok(program) => ParseResult::Ok {
                instructions: program.len(),
                summary: program.summary(0),
            },
            Err(failure) => ParseResult::Invalid {
                summary: failure.summary(),
                errors: failure.listed(),
            },
        }
    }
}

impl Program {
    /// Parses `source` as `format` under `space`.
    ///
    /// # Errors
    ///
    /// Every error the source has, in source order: each line's first
    /// problem, and each branch to a label never defined; or, for a source
    /// with neither errors nor instructions, that it is empty. A source
    /// larger than [`MAX_PROGRAM_BYTES`] has that error alone.
    pub(crate) fn parse(
        space: &impl AddressSpace,
        source: &str,
        format: ProgramFormat,
    ) -> Result<Program, ParseFailure> {
        let fail = |errors: Vec<ParseError>| ParseFailure::new(format, errors, space);
        if u64::try_from(source.len()).unwrap_or(u64::MAX) > u64::from(MAX_PROGRAM_BYTES) {
            let (line, column) = place_of_byte(source, MAX_PROGRAM_BYTES);
            let message = explain::parse_problem(space, ParseProblem::TooLarge { format });
            return Err(fail(vec![error(line, column, 1, message)]));
        }
        let mut lines = (1_u32..).zip(source.lines());
        let read: Vec<(u32, &str)> = lines
            .by_ref()
            .take(usize::try_from(MAX_PROGRAM_LINES).unwrap_or(usize::MAX))
            .collect();
        let (instructions, mut errors) = match format {
            ProgramFormat::Asm => asm::parse(space, &read),
            ProgramFormat::Trace => trace::parse(space, &read),
        };
        if let Some((line, text)) = lines.next() {
            let message = explain::parse_problem(space, ParseProblem::TooManyLines { format });
            errors.push(error(line, 1, chars(text), message));
        }
        if !errors.is_empty() || instructions.is_empty() {
            return Err(fail(errors));
        }
        Ok(Program {
            format,
            source: source.to_owned(),
            instructions,
        })
    }

    /// Whether the source is assembly or a trace.
    pub(crate) fn format(&self) -> ProgramFormat {
        self.format
    }

    /// The source as the student wrote it.
    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    /// How many instructions the program has.
    pub(crate) fn len(&self) -> u32 {
        u32::try_from(self.instructions.len()).unwrap_or(u32::MAX)
    }

    /// The instruction at index `index`, if the program has one there.
    pub(crate) fn get(&self, index: u32) -> Option<&Placed> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.instructions.get(index))
    }

    /// The drawer's line with the pc at `pc`: how many instructions, and the
    /// next one, `end` past the last.
    pub(crate) fn summary(&self, pc: u32) -> String {
        let next = self.get(pc).map(|placed| &*placed.text);
        explain::program_summary(self.format, self.len(), next)
    }
}

impl ParseFailure {
    /// The failure of a source read as `format` with `errors`, sorted into
    /// source order here; with no errors, the source is empty.
    fn new(format: ProgramFormat, mut errors: Vec<ParseError>, space: &impl AddressSpace) -> Self {
        errors.sort_by_key(|error| (error.line, error.column));
        let mut errors = errors.into_iter();
        let first = errors.next().unwrap_or_else(|| {
            let message = explain::parse_problem(space, ParseProblem::Empty { format });
            error(1, 1, 0, message)
        });
        Self {
            format,
            first,
            rest: errors.collect(),
        }
    }

    /// Whether the source was read as assembly or as a trace.
    pub(crate) fn format(&self) -> ProgramFormat {
        self.format
    }

    /// The first error in source order.
    pub(crate) fn first(&self) -> &ParseError {
        &self.first
    }

    /// The drawer's line for these errors: how many, and where the first is.
    pub(crate) fn summary(&self) -> String {
        let line = self.first.line;
        match self.rest.len() {
            0 => format!("1 error on line {line}"),
            more => format!("{} errors · first on line {line}", more.saturating_add(1)),
        }
    }

    /// The errors the drawer lists: the first [`MAX_PARSE_ERRORS`].
    pub(crate) fn listed(self) -> Vec<ParseError> {
        std::iter::once(self.first)
            .chain(self.rest)
            .take(usize::try_from(MAX_PARSE_ERRORS).unwrap_or(usize::MAX))
            .collect()
    }

    /// The error that refuses a `Program` op with this source, which counts
    /// every error and quotes the first.
    pub(crate) fn into_op_error(self) -> OpError {
        OpError::Program {
            format: self.format,
            errors: u32::try_from(self.rest.len())
                .unwrap_or(u32::MAX)
                .saturating_add(1),
            first: self.first,
        }
    }
}

impl Problem {
    /// A problem at `token`.
    fn at(token: &Token<'_>, message: String) -> Self {
        Self {
            column: token.column,
            length: token.length,
            message,
        }
    }

    /// The error this problem is on line `line`.
    fn on(self, line: u32) -> ParseError {
        error(line, self.column, self.length, self.message)
    }
}

/// An error on line `line` at column `column`, `length` characters long.
fn error(line: u32, column: u32, length: u32, message: String) -> ParseError {
    ParseError {
        text: explain::parse_error(line, column, &message),
        line,
        column,
        length,
        message,
    }
}

/// `token` read as a literal address: hex, within the address width, and a
/// multiple of the word size. `None` if the token is not a number at all,
/// which each parser reports in its own words.
fn literal_address(space: &impl AddressSpace, token: &Token<'_>) -> Option<Result<Addr, Problem>> {
    let problem = |message: String| Some(Err(Problem::at(token, message)));
    match token.number() {
        NumberWord::Hex(Some(value)) => {
            let Some(addr) = u32::try_from(value)
                .ok()
                .and_then(|value| space.addr(value))
            else {
                let error = AccessError::OutOfRange {
                    addr: value,
                    address_bits: space.address_bits(),
                };
                return problem(error.to_string());
            };
            if addr.get() % space.word_bytes() != 0 {
                let error = AccessError::Misaligned {
                    addr,
                    address_bits: space.address_bits(),
                    word_bytes: space.word_bytes(),
                };
                return problem(error.to_string());
            }
            Some(Ok(addr))
        }
        NumberWord::Decimal(Some(value)) => problem(explain::parse_problem(
            space,
            ParseProblem::DecimalAddress {
                text: token.text,
                value,
            },
        )),
        NumberWord::Hex(None) | NumberWord::Decimal(None) => problem(too_long(space, token)),
        NumberWord::Other => None,
    }
}

/// The message for a number in `token` that does not fit 64 bits.
fn too_long(space: &impl AddressSpace, token: &Token<'_>) -> String {
    explain::parse_problem(space, ParseProblem::TooLong { text: token.text })
}

/// What `token` reads as where a register may stand: `r0` to `r7`, in
/// either case. `r` and digits that name no register, such as `r8` or `r01`,
/// are shaped like one.
fn register(token: &Token<'_>) -> RegisterWord<Register> {
    let digits = match token.text.strip_prefix(['r', 'R']) {
        Some(digits) if token.kind == TokenKind::Word && lexer::is_digits(digits) => digits,
        _ => return RegisterWord::Other,
    };
    match digits.parse::<u32>().ok().and_then(Register::numbered) {
        Some(register) if digits.len() == 1 => RegisterWord::Register(register),
        _ => RegisterWord::NotARegister,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::config::{Config, Preset, WordBytes};

    /// The tests' address width: 12 bits, so the messages below carry 12-bit
    /// addresses; set here rather than taken from `Config::default()`.
    const ADDRESS_BITS: u8 = 12;

    fn geometry() -> Geometry {
        Config {
            address_bits: ADDRESS_BITS,
            ..Preset::DirectMapped8.config()
        }
        .validate()
        .unwrap()
    }

    fn placed(source: &str, format: ProgramFormat) -> Vec<Placed> {
        Program::parse(&geometry(), source, format)
            .unwrap()
            .instructions
    }

    fn addr(value: u32) -> Address {
        Address::Literal(Addr::new_unchecked(value))
    }

    #[test]
    fn a_program_parses_into_its_instructions() {
        let source = "start:\n  ld  r1, [0x0A4]  ; load\n  st [r2], #-3\n  add r3, r3, #-1\n  \
                      bnez r3, start\n  st [0x0A8], r3\nend:";
        let placed = placed(source, ProgramFormat::Asm);
        let read: Vec<(Instruction, u32, &str)> = placed
            .iter()
            .map(|placed| (placed.instruction.clone(), placed.line, &*placed.text))
            .collect();
        assert_eq!(
            read,
            [
                (
                    Instruction::Load {
                        into: Register::R1,
                        from: addr(0x0A4),
                    },
                    2,
                    "ld r1, [0x0A4]",
                ),
                (
                    Instruction::Store {
                        to: Address::Register(Register::R2),
                        // −3 in a 4-byte word.
                        value: Operand::Immediate(0xFFFF_FFFD),
                    },
                    3,
                    "st [r2], #-3",
                ),
                (
                    Instruction::Add {
                        into: Register::R3,
                        left: Register::R3,
                        right: Operand::Immediate(u64::MAX),
                    },
                    4,
                    "add r3, r3, #-1",
                ),
                (
                    Instruction::BranchIfNotZero {
                        test: Register::R3,
                        target: 0,
                        label: "start".to_owned(),
                    },
                    5,
                    "bnez r3, start",
                ),
                (
                    Instruction::Store {
                        to: addr(0x0A8),
                        value: Operand::Register(Register::R3),
                    },
                    6,
                    "st [0x0A8], r3",
                ),
            ]
        );
    }

    #[test]
    fn a_trace_parses_into_loads_into_r1_and_stores_of_values() {
        let placed = placed(
            "R 0x0A4\n\nw 0x0a8 7 ; store\nr  0X1A4",
            ProgramFormat::Trace,
        );
        let read: Vec<(Instruction, u32, &str)> = placed
            .iter()
            .map(|placed| (placed.instruction.clone(), placed.line, &*placed.text))
            .collect();
        assert_eq!(
            read,
            [
                (
                    Instruction::Load {
                        into: Register::R1,
                        from: addr(0x0A4),
                    },
                    1,
                    "R 0x0A4",
                ),
                (
                    Instruction::Store {
                        to: addr(0x0A8),
                        value: Operand::Immediate(7),
                    },
                    3,
                    "w 0x0a8 7",
                ),
                (
                    Instruction::Load {
                        into: Register::R1,
                        from: addr(0x1A4),
                    },
                    4,
                    "r 0X1A4",
                ),
            ]
        );
    }

    #[test]
    fn a_program_keeps_its_source_and_names_its_next_instruction() {
        let source = "ld r1, [0x0A4]\nld r2, [0x0A8]\n";
        let program = Program::parse(&geometry(), source, ProgramFormat::Asm).unwrap();
        assert_eq!(program.source(), source);
        assert_eq!(program.format(), ProgramFormat::Asm);
        assert_eq!(program.len(), 2);
        assert_eq!(
            program.summary(0),
            "2 instructions \u{b7} next: ld r1, [0x0A4]"
        );
        assert_eq!(
            program.summary(1),
            "2 instructions \u{b7} next: ld r2, [0x0A8]"
        );
        assert_eq!(program.summary(2), "2 instructions \u{b7} next: end");
        assert!(program.get(2).is_none());
    }

    #[test]
    fn a_store_immediate_is_fitted_to_the_word_size() {
        let geometry = Config {
            address_bits: ADDRESS_BITS,
            word_bytes: WordBytes::One,
            ..Preset::DirectMapped8.config()
        }
        .validate()
        .unwrap();
        let program = Program::parse(&geometry, "st [0x0A4], #-128", ProgramFormat::Asm).unwrap();
        assert_eq!(
            program.get(0).map(|placed| &placed.instruction),
            Some(&Instruction::Store {
                to: addr(0x0A4),
                value: Operand::Immediate(0x80),
            })
        );
    }

    #[test]
    fn a_failure_counts_every_error_and_lists_the_first_ones() {
        let source = "lw r1\n".repeat(usize::try_from(MAX_PARSE_ERRORS).unwrap() + 3);
        let failure = Program::parse(&geometry(), &source, ProgramFormat::Asm).unwrap_err();
        assert_eq!(
            failure.clone().into_op_error(),
            OpError::Program {
                format: ProgramFormat::Asm,
                errors: MAX_PARSE_ERRORS + 3,
                first: failure.first.clone(),
            }
        );
        let listed = failure.listed();
        assert_eq!(listed.len(), usize::try_from(MAX_PARSE_ERRORS).unwrap());
        let lines: Vec<u32> = listed.iter().map(|error| error.line).collect();
        assert_eq!(lines, (1..=MAX_PARSE_ERRORS).collect::<Vec<u32>>());
    }

    #[test]
    fn registers_are_r0_to_r7_in_either_case() {
        let read = |text: &str| register(lexer::tokens(text, SYNTAX).first().unwrap());
        assert_eq!(read("r0"), RegisterWord::Register(Register::R0));
        assert_eq!(read("R7"), RegisterWord::Register(Register::R7));
        for text in ["r8", "R9", "r10", "r01", "r99999999999999999999999"] {
            assert_eq!(read(text), RegisterWord::NotARegister, "{text}");
        }
        for text in ["r", "rx", "x1", "0x0A4", "#1", "loop"] {
            assert_eq!(read(text), RegisterWord::Other, "{text}");
        }
    }
}
