//! What an instruction set tells the toolkit: its syntax and mnemonics, its
//! registers, how to read one line, how to resolve a branch, and the
//! sentences it uses for what the toolkit itself refuses.
//!
//! The toolkit owns what every assembler does: splitting a source into
//! lines, the limits that bound the work, labels and their forward
//! references, the errors that carry a place, and the program that results.
//! An instruction set owns its grammar: which words are instructions, what
//! operands each takes, what a literal must satisfy and how a branch to a
//! label is represented. A line of an instruction set can stand for more than
//! one instruction, as a pseudo-instruction does, and an instruction set can
//! have a second format that has no labels, as a trace has none.
//!
//! The toolkit holds no sentence a student reads. Whatever it refuses, it
//! hands the instruction set a [`Reason`] and puts the sentence it gets back
//! into the error.

use std::fmt::Debug;

use crate::lexer::{RegisterWord, Syntax, Token};

/// The registers of an instruction set.
pub trait RegisterFile {
    /// One register.
    type Register: Copy + Debug + Eq;

    /// How many registers the file has.
    const COUNT: u32;

    /// What `token` reads as where a register may stand. A word shaped like
    /// a register's name that names none is [`RegisterWord::NotARegister`],
    /// so the instruction set can say so, rather than call it a label.
    fn read(token: &Token<'_>) -> RegisterWord<Self::Register>;

    /// The number of `register` in the file, from 0: the number an encoding
    /// holds.
    fn number(register: Self::Register) -> u32;
}

/// The limits that bound the work of reading a source, so a pasted or shared
/// program is read in bounded time and memory however it is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The largest source, in bytes. A larger one is refused before it is
    /// read.
    pub program_bytes: u32,
    /// The most lines read; a source with more is an error at the first
    /// line past it.
    pub program_lines: u32,
    /// The most instructions in a program, or `None` where only the lines
    /// bound it.
    pub instructions: Option<u32>,
    /// The longest label, in characters.
    pub label_chars: u32,
}

/// Why the toolkit refuses a source or a line, for the instruction set to put
/// into words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason<'a> {
    /// A source of more than [`Limits::program_bytes`].
    TooLarge,
    /// A source of more than [`Limits::program_lines`] lines.
    TooManyLines,
    /// A program of more than [`Limits::instructions`] instructions.
    TooManyInstructions,
    /// A source without a single instruction.
    Empty,
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
    /// A label longer than [`Limits::label_chars`].
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
}

/// A problem on a line: where it is and what it is. The toolkit adds the
/// line and the sentence that gives the place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// Its first column, from 1.
    pub column: u32,
    /// Its length in characters.
    pub length: u32,
    /// Its message.
    pub message: String,
}

/// One line read: its instructions, and the branch among them to resolve
/// once every label is known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read<'s, T> {
    /// The instructions the line stands for, in order; never empty. A
    /// pseudo-instruction stands for several.
    pub instructions: Vec<T>,
    /// The branch among them, if the line has one.
    pub branch: Option<Branch<'s>>,
}

/// A branch to a label, which is resolved when the whole source is read, so
/// the label may be defined further down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Branch<'s> {
    /// Which of the line's instructions branches, from 0.
    pub index: usize,
    /// The label's token, where an error goes if it is never defined.
    pub label: Token<'s>,
}

/// What a line is read under: the space its literals must fit, the format
/// it is written in, and the checks every label goes through.
pub struct Context<'a, I: InstructionSet> {
    /// The space the literals must fit.
    space: &'a I::Space,
    /// The format of the source.
    format: I::Format,
}

/// An instruction set: the grammar of an assembly language, for the toolkit
/// to read a source with.
pub trait InstructionSet: Sized {
    /// One instruction, with its operands, as the machine executes or draws
    /// it.
    type Instruction: Clone + Debug + Eq;

    /// The formats a source is written in: an assembly language, or a trace
    /// of accesses.
    type Format: Copy + Debug + Eq;

    /// What literals are checked against: the width of an address, the size
    /// of a word, the range of an immediate. It is the machine's, not the
    /// language's, so one language can be read for several machines.
    type Space: ?Sized;

    /// The registers of the instruction set.
    type Registers: RegisterFile;

    /// Where a comment starts and what brackets a memory operand.
    const SYNTAX: Syntax;

    /// Every mnemonic, which a label may not be. A pseudo-instruction's name
    /// is one too.
    const MNEMONICS: &'static [&'static str];

    /// The limits on a source of `format`.
    fn limits(format: Self::Format) -> Limits;

    /// Whether a source of `format` has labels. A line may then start with a
    /// label and a colon.
    fn has_labels(format: Self::Format) -> bool;

    /// Reads the `tokens` of a line, after its label, if it has one: the
    /// instructions they stand for, or the line's first problem. `tokens` is
    /// never empty.
    ///
    /// # Errors
    ///
    /// The first problem on the line, as the program drawer underlines it.
    fn read_line<'s>(
        context: &Context<'_, Self>,
        tokens: &[Token<'s>],
    ) -> Result<Read<'s, Self::Instruction>, Problem>;

    /// Points the branch `instruction`, which is the `at`th of the program,
    /// to the instruction `target` that its label marks, which is the
    /// program's length when the label follows the last instruction. An
    /// instruction set whose branches hold offsets works it out here, and
    /// refuses one out of range.
    ///
    /// # Errors
    ///
    /// The message for a branch that cannot reach its label.
    fn resolve(instruction: &mut Self::Instruction, at: u32, target: u32) -> Result<(), String>;

    /// The sentence for `reason`, under `space`.
    fn word(reason: Reason<'_>, space: &Self::Space, format: Self::Format) -> String;

    /// The `message` of an error with its place, on `line` at `column`.
    fn locate(line: u32, column: u32, message: &str) -> String;
}

/// An instruction set whose instructions have a binary form, as RISC-V's do
/// in 32 bits. An instruction set that is only drawn and executed, as the
/// tiny one is, has none.
pub trait Encode: InstructionSet {
    /// The machine word an instruction is encoded in.
    type Word: Copy + Debug + Eq;

    /// The encoding of `instruction`, whose branches are already resolved.
    fn encode(instruction: &Self::Instruction) -> Self::Word;
}

impl Problem {
    /// A problem at `token`, with `message`.
    #[must_use]
    pub fn at(token: &Token<'_>, message: String) -> Self {
        Self {
            column: token.column,
            length: token.length,
            message,
        }
    }
}

impl<'s, T> Read<'s, T> {
    /// A line of one instruction.
    #[must_use]
    pub fn one(instruction: T) -> Self {
        Self {
            instructions: vec![instruction],
            branch: None,
        }
    }

    /// A line of one instruction, a branch to the label in `label`.
    #[must_use]
    pub fn branching(instruction: T, label: Token<'s>) -> Self {
        Self {
            instructions: vec![instruction],
            branch: Some(Branch { index: 0, label }),
        }
    }
}

impl<'a, I: InstructionSet> Context<'a, I> {
    /// The context of a source of `format`, read under `space`.
    pub(crate) fn new(space: &'a I::Space, format: I::Format) -> Self {
        Self { space, format }
    }

    /// The space the literals must fit.
    #[must_use]
    pub fn space(&self) -> &'a I::Space {
        self.space
    }

    /// The format of the source.
    #[must_use]
    pub fn format(&self) -> I::Format {
        self.format
    }

    /// The problem `reason` makes at `token`.
    #[must_use]
    pub fn problem(&self, token: &Token<'_>, reason: Reason<'_>) -> Problem {
        Problem::at(token, I::word(reason, self.space, self.format))
    }

    /// Checks that `token` can be a label, where one is defined or named: it
    /// is spelled like one, is neither a register nor a mnemonic, and is not
    /// too long.
    ///
    /// # Errors
    ///
    /// The problem at `token`.
    pub fn label(&self, token: &Token<'_>) -> Result<(), Problem> {
        if !token.is_label() {
            return Err(self.problem(token, Reason::NotALabel { text: token.text }));
        }
        let register = matches!(
            <I::Registers as RegisterFile>::read(token),
            RegisterWord::Register(_)
        );
        if register || I::MNEMONICS.iter().any(|name| token.is(name)) {
            return Err(self.problem(
                token,
                Reason::ReservedLabel {
                    name: token.text,
                    register,
                },
            ));
        }
        if token.length > I::limits(self.format).label_chars {
            return Err(self.problem(
                token,
                Reason::LabelTooLong {
                    chars: token.length,
                },
            ));
        }
        Ok(())
    }
}
