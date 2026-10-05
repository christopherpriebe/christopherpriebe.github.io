//! The trace parser: one access per line.
//!
//! ```text
//! line = ws [ ( "R" | "r" ) ws1 hex  |  ( "W" | "w" ) ws1 hex ws1 ( hex | dec ) ] ws [ comment ] EOL ;
//! ```
//!
//! `R a` executes as `ld r1, [a]` and `W a v` as `st [a], #v`, and the line's
//! text is what the CPU box and the log show for it. The address follows the
//! assembler's rules; the value is required, has no sign and must fit the
//! word as an unsigned number.

use instrument_asm::lexer::{NumberWord, Token};
use instrument_asm::set::{Problem, Read};

use crate::explain::{self, ParseProblem};
use crate::op::AccessKind;
use crate::units::Register;

use super::{literal_address, too_long, Address, AddressSpace, Bounds, Instruction, Operand};

/// Reads the access of a line whose first token is `first` and whose others
/// are `operands`.
pub(super) fn line<'s>(
    space: Bounds,
    first: &Token<'s>,
    operands: &[Token<'s>],
) -> Result<Read<'s, Instruction>, Problem> {
    access(&space, first, operands).map(Read::one)
}

/// The access a line stands for.
fn access(
    space: &impl AddressSpace,
    first: &Token<'_>,
    operands: &[Token<'_>],
) -> Result<Instruction, Problem> {
    let kind = if first.is("R") {
        AccessKind::Read
    } else if first.is("W") {
        AccessKind::Write
    } else {
        let message =
            explain::parse_problem(space, ParseProblem::UnknownAccess { name: first.text });
        return Err(Problem::at(first, message));
    };
    let shape = |token: &Token<'_>| {
        let message = explain::parse_problem(space, ParseProblem::TraceOperands { kind });
        Problem::at(token, message)
    };
    let [at, rest @ ..] = operands else {
        return Err(shape(first));
    };
    let addr = match literal_address(space, at) {
        Some(addr) => addr?,
        None => return Err(shape(at)),
    };
    let from = Address::Literal(addr);
    match (kind, rest) {
        (AccessKind::Read, []) => Ok(Instruction::Load {
            into: Register::R1,
            from,
        }),
        (AccessKind::Write, []) => {
            let message = explain::parse_problem(space, ParseProblem::StoreWithoutValue { addr });
            Err(Problem::at(first, message))
        }
        // The value comes first: a line reports its first problem, so a word
        // after the value is extra only once the value itself is fine.
        (AccessKind::Write, [value, after @ ..]) => {
            let written = match value.number() {
                NumberWord::Hex(Some(written)) | NumberWord::Decimal(Some(written)) => written,
                NumberWord::Hex(None) | NumberWord::Decimal(None) => {
                    return Err(Problem::at(value, too_long(space, value)));
                }
                NumberWord::Other => return Err(shape(value)),
            };
            let word = space.fit_word(written);
            if word.get() != written {
                let message =
                    explain::parse_problem(space, ParseProblem::ValueTooWide { value: written });
                return Err(Problem::at(value, message));
            }
            if let Some(extra) = after.first() {
                return Err(shape(extra));
            }
            Ok(Instruction::Store {
                to: from,
                value: Operand::Immediate(word.get()),
            })
        }
        (AccessKind::Read, [extra, ..]) => Err(shape(extra)),
    }
}
