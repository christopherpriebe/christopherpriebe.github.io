//! The assembler: `ld`, `st`, `add` and `bnez`, labels and comments.
//!
//! ```text
//! line        = ws [ label ws ":" ] ws [ instruction ] ws [ comment ] EOL ;
//! instruction = "ld"   ws1 reg ws "," ws mem
//!             | "st"   ws1 mem ws "," ws ( reg | imm )
//!             | "add"  ws1 reg ws "," ws reg ws "," ws ( reg | imm )
//!             | "bnez" ws1 reg ws "," ws label ;
//! mem         = "[" ws ( hex | reg ) ws "]" ;
//! imm         = "#" [ "-" ] ( hex | dec ) ;
//! ```
//!
//! Mnemonics and registers are read in either case; labels are
//! case-sensitive, at most [`MAX_LABEL_CHARS`] characters, not a register or
//! a mnemonic, and defined once. A branch may name a label defined further
//! down, so labels are resolved once every line is read, and a label after
//! the last instruction marks the program's end.
//!
//! A line reports its first problem only. A register-shaped word that names
//! no register (`r8`), an address written in decimal, a literal that does not
//! fit, and a bad label each have their own message; any other misshapen
//! operand list gets the instruction's own example.

use std::collections::BTreeMap;

use crate::explain::{self, ParseProblem};
use crate::geometry::Geometry;
use crate::input::fit_word;
use crate::limits::{MAX_LABEL_CHARS, MAX_PROGRAM_INSTRUCTIONS};
use crate::units::Register;

use super::lexer::{self, NumberWord, RegisterWord, Token, TokenKind};
use super::{
    error, literal_address, too_long, Address, Instruction, Operand, ParseError, Placed, Problem,
};

/// Bytes in a register, the width of an `add` immediate.
const REGISTER_BYTES: u32 = 8;

/// An instruction's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mnemonic {
    /// `ld`.
    Ld,
    /// `st`.
    St,
    /// `add`.
    Add,
    /// `bnez`.
    Bnez,
}

impl Mnemonic {
    /// Every mnemonic.
    const ALL: [Mnemonic; 4] = [Mnemonic::Ld, Mnemonic::St, Mnemonic::Add, Mnemonic::Bnez];

    /// The mnemonic as the assembler reads it.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Mnemonic::Ld => "ld",
            Mnemonic::St => "st",
            Mnemonic::Add => "add",
            Mnemonic::Bnez => "bnez",
        }
    }

    /// The mnemonic `token` spells, in any case.
    fn read(token: &Token<'_>) -> Option<Mnemonic> {
        Mnemonic::ALL
            .into_iter()
            .find(|mnemonic| token.is(mnemonic.name()))
    }
}

/// A branch whose label is resolved once every line is read.
struct Branch<'s> {
    /// The instruction's index.
    index: usize,
    /// The label token, for an error if it is never defined.
    label: Token<'s>,
    /// Its line.
    line: u32,
}

/// What the assembler has read so far.
struct Assembler<'g, 's> {
    /// The run's geometry, which literals must fit.
    geometry: &'g Geometry,
    /// The instructions, with the branches' targets still to resolve.
    instructions: Vec<Placed>,
    /// Each label, with the index of the instruction it marks and its line.
    labels: BTreeMap<&'s str, (u32, u32)>,
    /// The branches to resolve.
    branches: Vec<Branch<'s>>,
    /// The errors so far.
    errors: Vec<ParseError>,
    /// Whether the instruction limit has been reported.
    too_many: bool,
}

/// Assembles `lines`, each with its number: the instructions, and the
/// errors.
pub(super) fn parse(geometry: &Geometry, lines: &[(u32, &str)]) -> (Vec<Placed>, Vec<ParseError>) {
    let mut assembler = Assembler {
        geometry,
        instructions: Vec::new(),
        labels: BTreeMap::new(),
        branches: Vec::new(),
        errors: Vec::new(),
        too_many: false,
    };
    for &(number, line) in lines {
        if let Err(problem) = assembler.line(number, line) {
            assembler.errors.push(problem.on(number));
        }
    }
    assembler.resolve();
    (assembler.instructions, assembler.errors)
}

impl<'s> Assembler<'_, 's> {
    /// Reads line `number`, `line`.
    fn line(&mut self, number: u32, line: &'s str) -> Result<(), Problem> {
        let tokens = lexer::tokens(line);
        let mut rest = tokens.as_slice();
        if let [label, colon, after @ ..] = rest {
            if colon.kind == TokenKind::Colon {
                self.define(number, label)?;
                rest = after;
            }
        }
        let [first, operands @ ..] = rest else {
            return Ok(());
        };
        let Some(mnemonic) = Mnemonic::read(first) else {
            return Err(self.problem(first, ParseProblem::UnknownInstruction { name: first.text }));
        };
        let reader = Operands {
            assembler: self,
            mnemonic,
            at: *first,
            tokens: operands,
        };
        let (instruction, branch) = reader.read()?;
        if self.instructions.len()
            >= usize::try_from(MAX_PROGRAM_INSTRUCTIONS).unwrap_or(usize::MAX)
        {
            if self.too_many {
                return Ok(());
            }
            self.too_many = true;
            return Err(self.problem(first, ParseProblem::TooManyInstructions));
        }
        if let Some(label) = branch {
            self.branches.push(Branch {
                index: self.instructions.len(),
                label,
                line: number,
            });
        }
        let end = operands.last().unwrap_or(first).end;
        self.instructions.push(Placed {
            instruction,
            line: number,
            text: lexer::collapse(line.get(first.start..end).unwrap_or_default()),
        });
        Ok(())
    }

    /// Defines `label` on line `number`, marking the next instruction.
    fn define(&mut self, number: u32, label: &Token<'s>) -> Result<(), Problem> {
        self.label(label)?;
        if let Some(&(_, line)) = self.labels.get(label.text) {
            return Err(self.problem(
                label,
                ParseProblem::DuplicateLabel {
                    name: label.text,
                    line,
                },
            ));
        }
        let index = u32::try_from(self.instructions.len()).unwrap_or(u32::MAX);
        self.labels.insert(label.text, (index, number));
        Ok(())
    }

    /// Checks that `token` can be a label, where one is defined or named.
    fn label(&self, token: &Token<'s>) -> Result<(), Problem> {
        if !token.is_label() {
            return Err(self.problem(token, ParseProblem::NotALabel { text: token.text }));
        }
        let register = matches!(token.register(), RegisterWord::Register(_));
        if register || Mnemonic::read(token).is_some() {
            return Err(self.problem(
                token,
                ParseProblem::ReservedLabel {
                    name: token.text,
                    register,
                },
            ));
        }
        if token.length > MAX_LABEL_CHARS {
            return Err(self.problem(
                token,
                ParseProblem::LabelTooLong {
                    chars: token.length,
                },
            ));
        }
        Ok(())
    }

    /// Resolves every branch to the instruction its label marks, or reports
    /// the label as not defined.
    fn resolve(&mut self) {
        for branch in &self.branches {
            let Some(&(index, _)) = self.labels.get(branch.label.text) else {
                let message = explain::parse_problem(
                    self.geometry,
                    ParseProblem::UndefinedLabel {
                        name: branch.label.text,
                    },
                );
                self.errors.push(error(
                    branch.line,
                    branch.label.column,
                    branch.label.length,
                    message,
                ));
                continue;
            };
            let placed = self.instructions.get_mut(branch.index);
            if let Some(Instruction::BranchIfNotZero { target, .. }) =
                placed.map(|placed| &mut placed.instruction)
            {
                *target = index;
            }
        }
    }

    /// `problem` at `token`.
    fn problem(&self, token: &Token<'_>, problem: ParseProblem<'_>) -> Problem {
        Problem::at(token, explain::parse_problem(self.geometry, problem))
    }
}

/// The operands of one instruction, read in order.
struct Operands<'a, 'g, 's> {
    /// The assembler, for the geometry and the messages.
    assembler: &'a Assembler<'g, 's>,
    /// The instruction.
    mnemonic: Mnemonic,
    /// The mnemonic's token, where an error goes when an operand is missing.
    at: Token<'s>,
    /// The tokens after the mnemonic, yet to be read.
    tokens: &'a [Token<'s>],
}

impl<'s> Operands<'_, '_, 's> {
    /// The instruction, with its label token if it is a branch.
    fn read(mut self) -> Result<(Instruction, Option<Token<'s>>), Problem> {
        if self.tokens.is_empty() {
            return Err(self.shape(None));
        }
        let read = match self.mnemonic {
            Mnemonic::Ld => {
                let into = self.register()?;
                self.comma()?;
                let from = self.memory()?;
                (Instruction::Load { into, from }, None)
            }
            Mnemonic::St => {
                let to = self.memory()?;
                self.comma()?;
                let value = self.operand(self.assembler.geometry.word_bytes())?;
                (Instruction::Store { to, value }, None)
            }
            Mnemonic::Add => {
                let into = self.register()?;
                self.comma()?;
                let left = self.register()?;
                self.comma()?;
                let right = self.operand(REGISTER_BYTES)?;
                (Instruction::Add { into, left, right }, None)
            }
            Mnemonic::Bnez => {
                let test = self.register()?;
                self.comma()?;
                let label = self.next()?;
                self.assembler.label(&label)?;
                let branch = Instruction::BranchIfNotZero {
                    test,
                    target: 0,
                    label: label.text.to_owned(),
                };
                (branch, Some(label))
            }
        };
        match self.tokens.first() {
            Some(extra) => Err(self.shape(Some(extra))),
            None => Ok(read),
        }
    }

    /// The next token, or the shape error at the mnemonic if there is none.
    fn next(&mut self) -> Result<Token<'s>, Problem> {
        let Some((&first, rest)) = self.tokens.split_first() else {
            return Err(self.shape(None));
        };
        self.tokens = rest;
        Ok(first)
    }

    /// A `,`.
    fn comma(&mut self) -> Result<(), Problem> {
        let token = self.next()?;
        if token.kind == TokenKind::Comma {
            Ok(())
        } else {
            Err(self.shape(Some(&token)))
        }
    }

    /// A register.
    fn register(&mut self) -> Result<Register, Problem> {
        let token = self.next()?;
        match token.register() {
            RegisterWord::Register(register) => Ok(register),
            RegisterWord::NotARegister => Err(self.not_a_register(&token)),
            RegisterWord::Other => Err(self.shape(Some(&token))),
        }
    }

    /// A memory operand: `[`, a hex address or a register, `]`.
    fn memory(&mut self) -> Result<Address, Problem> {
        let open = self.next()?;
        if open.kind != TokenKind::Open {
            return Err(self.shape(Some(&open)));
        }
        let inner = self.next()?;
        let address = match inner.register() {
            RegisterWord::Register(register) => Address::Register(register),
            RegisterWord::NotARegister => return Err(self.not_a_register(&inner)),
            RegisterWord::Other => match literal_address(self.assembler.geometry, &inner) {
                Some(addr) => Address::Literal(addr?),
                None => return Err(self.shape(Some(&inner))),
            },
        };
        let close = self.next()?;
        if close.kind != TokenKind::Close {
            return Err(self.shape(Some(&close)));
        }
        Ok(address)
    }

    /// A register or an immediate that fits `bytes` bytes, signed or
    /// unsigned; a negative immediate becomes its two's complement in that
    /// width.
    fn operand(&mut self, bytes: u32) -> Result<Operand, Problem> {
        let token = self.next()?;
        match token.register() {
            RegisterWord::Register(register) => return Ok(Operand::Register(register)),
            RegisterWord::NotARegister => return Err(self.not_a_register(&token)),
            RegisterWord::Other => {}
        }
        let Some(written) = token.text.strip_prefix('#') else {
            return Err(self.shape(Some(&token)));
        };
        let (negative, digits) = match written.strip_prefix('-') {
            Some(digits) => (true, digits),
            None => (false, written),
        };
        let magnitude = match lexer::number(digits) {
            NumberWord::Hex(Some(value)) | NumberWord::Decimal(Some(value)) => value,
            NumberWord::Hex(None) | NumberWord::Decimal(None) => {
                return Err(Problem::at(
                    &token,
                    too_long(self.assembler.geometry, &token),
                ));
            }
            NumberWord::Other => return Err(self.shape(Some(&token))),
        };
        fit_word(negative, magnitude, bytes)
            .map(|word| Operand::Immediate(word.get()))
            .map_err(|message| Problem::at(&token, message))
    }

    /// The instruction's own example, at `token`, or at the mnemonic when an
    /// operand is missing.
    fn shape(&self, token: Option<&Token<'_>>) -> Problem {
        self.assembler.problem(
            token.unwrap_or(&self.at),
            ParseProblem::Operands {
                mnemonic: self.mnemonic,
            },
        )
    }

    /// The message for a register-shaped word that names no register.
    fn not_a_register(&self, token: &Token<'_>) -> Problem {
        self.assembler
            .problem(token, ParseProblem::NotARegister { name: token.text })
    }
}
