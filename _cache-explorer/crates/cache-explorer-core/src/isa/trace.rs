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

use crate::explain::{self, ParseProblem};
use crate::geometry::Geometry;
use crate::op::AccessKind;
use crate::units::Register;

use super::lexer::{self, NumberWord, Token};
use super::{
    literal_address, too_long, Address, Instruction, Operand, ParseError, Placed, Problem,
};

/// Reads the trace in `lines`, each with its number: the accesses, and the
/// errors.
pub(super) fn parse(geometry: &Geometry, lines: &[(u32, &str)]) -> (Vec<Placed>, Vec<ParseError>) {
    let mut placed = Vec::new();
    let mut errors = Vec::new();
    for &(number, line) in lines {
        let tokens = lexer::tokens(line);
        match access(geometry, &tokens) {
            Ok(Some(instruction)) => {
                let (start, end) = match (tokens.first(), tokens.last()) {
                    (Some(first), Some(last)) => (first.start, last.end),
                    _ => (0, 0),
                };
                placed.push(Placed {
                    instruction,
                    line: number,
                    text: lexer::collapse(line.get(start..end).unwrap_or_default()),
                });
            }
            Ok(None) => {}
            Err(problem) => errors.push(problem.on(number)),
        }
    }
    (placed, errors)
}

/// The access a line's `tokens` stand for; `None` for a line with none.
fn access(geometry: &Geometry, tokens: &[Token<'_>]) -> Result<Option<Instruction>, Problem> {
    let [first, operands @ ..] = tokens else {
        return Ok(None);
    };
    let kind = if first.is("R") {
        AccessKind::Read
    } else if first.is("W") {
        AccessKind::Write
    } else {
        let message =
            explain::parse_problem(geometry, ParseProblem::UnknownAccess { name: first.text });
        return Err(Problem::at(first, message));
    };
    let shape = |token: &Token<'_>| {
        let message = explain::parse_problem(geometry, ParseProblem::TraceOperands { kind });
        Problem::at(token, message)
    };
    let [at, rest @ ..] = operands else {
        return Err(shape(first));
    };
    let addr = match literal_address(geometry, at) {
        Some(addr) => addr?,
        None => return Err(shape(at)),
    };
    let from = Address::Literal(addr);
    match (kind, rest) {
        (AccessKind::Read, []) => Ok(Some(Instruction::Load {
            into: Register::R1,
            from,
        })),
        (AccessKind::Write, []) => {
            let message =
                explain::parse_problem(geometry, ParseProblem::StoreWithoutValue { addr });
            Err(Problem::at(first, message))
        }
        // The value comes first: a line reports its first problem, so a word
        // after the value is extra only once the value itself is fine.
        (AccessKind::Write, [value, after @ ..]) => {
            let written = match value.number() {
                NumberWord::Hex(Some(written)) | NumberWord::Decimal(Some(written)) => written,
                NumberWord::Hex(None) | NumberWord::Decimal(None) => {
                    return Err(Problem::at(value, too_long(geometry, value)));
                }
                NumberWord::Other => return Err(shape(value)),
            };
            let word = geometry.fit_word(written);
            if word.get() != written {
                let message =
                    explain::parse_problem(geometry, ParseProblem::ValueTooWide { value: written });
                return Err(Problem::at(value, message));
            }
            if let Some(extra) = after.first() {
                return Err(shape(extra));
            }
            Ok(Some(Instruction::Store {
                to: from,
                value: Operand::Immediate(word.get()),
            }))
        }
        (AccessKind::Read, [extra, ..]) => Err(shape(extra)),
    }
}
