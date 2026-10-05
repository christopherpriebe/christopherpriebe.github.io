//! The assembler: `ld`, `st`, `add` and `bnez`, labels and comments.
//!
//! ```text
//! line        = ws [ label ws ":" ] ws [ instruction ] ws [ comment ] EOL ;
//! instruction = "ld"   ws1 reg ws "," ws mem
//!             | "st"   ws mem ws "," ws ( reg | imm )
//!             | "add"  ws1 reg ws "," ws reg ws "," ws ( reg | imm )
//!             | "bnez" ws1 reg ws "," ws label ;
//! mem         = "[" ws ( hex | reg ) ws "]" ;
//! imm         = "#" [ "-" ] ( hex | dec ) ;
//! ```
//!
//! `ws` is any run of spaces and tabs, and `ws1` one that is not empty. A
//! word runs to the next space, tab, `,`, `[`, `]` or `:`, so a mnemonic
//! needs a space or a tab before a register (`ldr1` is one word, an unknown
//! instruction) but none before `[` (`st[0x0A4], #7` is a store).
//!
//! Mnemonics and registers are read in either case; labels are
//! case-sensitive, at most
//! [`MAX_LABEL_CHARS`](crate::limits::MAX_LABEL_CHARS) characters, not a
//! register or a mnemonic, and defined once. A branch may name a label defined further
//! down, so labels are resolved once every line is read, and a label after
//! the last instruction marks the program's end.
//!
//! A line reports its first problem only. A register-shaped word that names
//! no register (`r8`), an address written in decimal, a literal that does not
//! fit, and a bad label each have their own message; any other misshapen
//! operand list gets the instruction's own example.

use instrument_asm::lexer::{self, NumberWord, RegisterWord, Token, TokenKind};
use instrument_asm::set::{Context, Problem, Read};

use crate::explain::ParseProblem;
use crate::input::fit_word;
use crate::units::Register;

use super::tiny::register;
use super::{
    literal_address, problem, too_long, Address, AddressSpace, Bounds, Instruction, Operand, Tiny,
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

/// Reads the instruction of a line whose first token is `first`, after its
/// label if it has one, and whose others are `tokens`.
pub(super) fn line<'s>(
    context: &Context<'_, Tiny>,
    first: &Token<'s>,
    tokens: &[Token<'s>],
) -> Result<Read<'s, Instruction>, Problem> {
    let Some(mnemonic) = Mnemonic::read(first) else {
        let unknown = ParseProblem::UnknownInstruction { name: first.text };
        return Err(problem(context.space(), first, unknown));
    };
    let reader = Operands {
        context,
        mnemonic,
        at: *first,
        tokens,
    };
    Ok(match reader.read()? {
        (instruction, Some(label)) => Read::branching(instruction, label),
        (instruction, None) => Read::one(instruction),
    })
}

/// The operands of one instruction, read in order.
struct Operands<'a, 's> {
    /// What the line is read under: the address space, and the label checks.
    context: &'a Context<'a, Tiny>,
    /// The instruction.
    mnemonic: Mnemonic,
    /// The mnemonic's token, where an error goes when an operand is missing.
    at: Token<'s>,
    /// The tokens after the mnemonic, yet to be read.
    tokens: &'a [Token<'s>],
}

impl<'a, 's> Operands<'a, 's> {
    /// The address space the literals must fit.
    fn space(&self) -> &'a Bounds {
        self.context.space()
    }

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
                let value = self.operand(self.space().word_bytes())?;
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
                self.context.label(&label)?;
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
        match register(&token) {
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
        let address = match register(&inner) {
            RegisterWord::Register(register) => Address::Register(register),
            RegisterWord::NotARegister => return Err(self.not_a_register(&inner)),
            RegisterWord::Other => match literal_address(self.space(), &inner) {
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
        match register(&token) {
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
                return Err(Problem::at(&token, too_long(self.space(), &token)));
            }
            NumberWord::Other => return Err(self.shape(Some(&token))),
        };
        fit_word(negative, magnitude, bytes)
            .map(|word| Operand::Immediate(word.get()))
            .map_err(|error| Problem::at(&token, error.to_string()))
    }

    /// The instruction's own example, at `token`, or at the mnemonic when an
    /// operand is missing.
    fn shape(&self, token: Option<&Token<'_>>) -> Problem {
        problem(
            self.space(),
            token.unwrap_or(&self.at),
            ParseProblem::Operands {
                mnemonic: self.mnemonic,
            },
        )
    }

    /// The message for a register-shaped word that names no register.
    fn not_a_register(&self, token: &Token<'_>) -> Problem {
        problem(
            self.space(),
            token,
            ParseProblem::NotARegister { name: token.text },
        )
    }
}
