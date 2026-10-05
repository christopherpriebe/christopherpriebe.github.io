//! The tiny language and the trace format as an instruction set for the
//! shared assembler toolkit: what is a mnemonic, a register and a label,
//! which limits bound a source, how each format's line is read, and the
//! wording of everything the toolkit refuses.

use instrument_asm::lexer::{self, RegisterWord, Syntax, Token, TokenKind};
use instrument_asm::set::{Context, InstructionSet, Limits, Problem, Read, Reason, RegisterFile};

use crate::explain::{self, ParseProblem};
use crate::limits::{
    MAX_LABEL_CHARS, MAX_PROGRAM_BYTES, MAX_PROGRAM_INSTRUCTIONS, MAX_PROGRAM_LINES,
};
use crate::op::ProgramFormat;
use crate::units::Register;

use super::{asm, trace, Bounds, Instruction, Mnemonic};

/// The tiny assembly language and the trace format. No value has this type;
/// it names the instruction set.
pub(crate) enum Tiny {}

impl RegisterFile for Tiny {
    type Register = Register;

    fn read(token: &Token<'_>) -> RegisterWord<Register> {
        register(token)
    }
}

impl InstructionSet for Tiny {
    type Instruction = Instruction;
    type Format = ProgramFormat;
    type Space = Bounds;
    type Registers = Self;

    /// `;` starts a comment and `[` and `]` bracket a memory operand.
    const SYNTAX: Syntax = Syntax {
        comment: ';',
        open: '[',
        close: ']',
    };

    const MNEMONICS: &'static [&'static str] = &[
        Mnemonic::Ld.name(),
        Mnemonic::St.name(),
        Mnemonic::Add.name(),
        Mnemonic::Bnez.name(),
    ];

    fn limits(format: ProgramFormat) -> Limits {
        Limits {
            program_bytes: MAX_PROGRAM_BYTES,
            program_lines: MAX_PROGRAM_LINES,
            // A trace is bounded by its lines alone.
            instructions: match format {
                ProgramFormat::Asm => Some(MAX_PROGRAM_INSTRUCTIONS),
                ProgramFormat::Trace => None,
            },
            label_chars: MAX_LABEL_CHARS,
        }
    }

    fn has_labels(format: ProgramFormat) -> bool {
        format == ProgramFormat::Asm
    }

    fn read_line<'s>(
        context: &Context<'_, Self>,
        first: &Token<'s>,
        operands: &[Token<'s>],
    ) -> Result<Read<'s, Instruction>, Problem> {
        match context.format() {
            ProgramFormat::Asm => asm::line(context, first, operands),
            ProgramFormat::Trace => trace::line(*context.space(), first, operands),
        }
    }

    fn resolve(instruction: &mut Instruction, _at: u32, target: u32) -> Result<(), String> {
        if let Instruction::BranchIfNotZero { target: index, .. } = instruction {
            *index = target;
        }
        Ok(())
    }

    fn word(reason: Reason<'_>, space: &Bounds, format: ProgramFormat) -> String {
        let problem = match reason {
            Reason::TooLarge => ParseProblem::TooLarge { format },
            Reason::TooManyLines => ParseProblem::TooManyLines { format },
            Reason::TooManyInstructions => ParseProblem::TooManyInstructions,
            Reason::Empty => ParseProblem::Empty { format },
            Reason::NotALabel { text } => ParseProblem::NotALabel { text },
            Reason::ReservedLabel { name, register } => {
                ParseProblem::ReservedLabel { name, register }
            }
            Reason::LabelTooLong { chars } => ParseProblem::LabelTooLong { chars },
            Reason::DuplicateLabel { name, line } => ParseProblem::DuplicateLabel { name, line },
            Reason::UndefinedLabel { name } => ParseProblem::UndefinedLabel { name },
        };
        explain::parse_problem(space, problem)
    }

    fn locate(line: u32, column: u32, message: &str) -> String {
        explain::parse_error(line, column, message)
    }
}

/// What `token` reads as where a register may stand: `r0` to `r7`, in
/// either case. `r` and digits that name no register, such as `r8` or `r01`,
/// are shaped like one.
pub(super) fn register(token: &Token<'_>) -> RegisterWord<Register> {
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
    #![expect(clippy::disallowed_macros, reason = "assertions are how tests fail")]

    use instrument_asm::lexer::tokens;

    use super::*;

    /// What the first token of `text` reads as where a register may stand.
    fn read(text: &str) -> RegisterWord<Register> {
        register(tokens(text, Tiny::SYNTAX).first().unwrap())
    }

    #[test]
    fn registers_are_r0_to_r7_in_either_case() {
        assert_eq!(read("r0"), RegisterWord::Register(Register::R0));
        assert_eq!(read("R7"), RegisterWord::Register(Register::R7));
        for text in ["r8", "R9", "r10", "r01", "r99999999999999999999999"] {
            assert_eq!(read(text), RegisterWord::NotARegister, "{text}");
        }
        for text in ["r", "rx", "x1", "0x0A4", "#1", "loop"] {
            assert_eq!(read(text), RegisterWord::Other, "{text}");
        }
    }

    #[test]
    fn only_assembly_has_labels_and_an_instruction_limit() {
        assert!(Tiny::has_labels(ProgramFormat::Asm));
        assert!(!Tiny::has_labels(ProgramFormat::Trace));
        assert_eq!(
            Tiny::limits(ProgramFormat::Asm).instructions,
            Some(MAX_PROGRAM_INSTRUCTIONS)
        );
        assert_eq!(Tiny::limits(ProgramFormat::Trace).instructions, None);
    }
}
