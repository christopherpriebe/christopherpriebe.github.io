//! A source read under an instruction set: the program it makes, or every
//! error it has.

use std::collections::BTreeMap;
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;

use crate::diag::{chars, place_of_byte, ParseError};
use crate::lexer::{self, Token, TokenKind};
use crate::set::{Context, Encode, InstructionSet, Limits, Problem, Reason};

/// A source that was read under an instruction set: the source, kept so a
/// new run can read it again under its own space, and its instructions in
/// order.
pub struct Program<I: InstructionSet> {
    /// The format the source was read as.
    format: I::Format,
    /// The source as the student wrote it.
    source: String,
    /// The instructions, in order; never empty.
    instructions: Vec<Placed<I::Instruction>>,
}

/// An instruction and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed<T> {
    /// The instruction.
    pub instruction: T,
    /// Its source line, from 1.
    pub line: u32,
    /// Its source text without its label and comment, with each run of
    /// spaces and tabs made one space. The instructions of one line share
    /// it, and so does the log entry an instruction makes.
    pub text: Arc<str>,
}

/// Why a source is not a program: every error it has, in source order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseFailure<F> {
    /// The format the source was read as.
    format: F,
    /// The first error.
    first: ParseError,
    /// The others.
    rest: Vec<ParseError>,
}

/// A branch whose label is resolved once every line is read.
struct Pending<'s> {
    /// The instruction's index in the program.
    index: usize,
    /// The label's token, for an error if it is never defined.
    label: Token<'s>,
    /// Its line.
    line: u32,
}

/// What has been read of a source so far.
struct Reading<'a, 's, I: InstructionSet> {
    /// What each line is read under.
    context: Context<'a, I>,
    /// The limits of the format.
    limits: Limits,
    /// The instructions, with the branches still to resolve.
    instructions: Vec<Placed<I::Instruction>>,
    /// Each label, with the index of the instruction it marks and its line.
    labels: BTreeMap<&'s str, (u32, u32)>,
    /// The branches to resolve.
    branches: Vec<Pending<'s>>,
    /// The errors so far.
    errors: Vec<ParseError>,
    /// Whether the instruction limit has been reported.
    too_many: bool,
}

// Written by hand, so that the marker type of an instruction set need not
// implement them: only what the program holds does.
impl<I: InstructionSet> Clone for Program<I> {
    fn clone(&self) -> Self {
        Self {
            format: self.format,
            source: self.source.clone(),
            instructions: self.instructions.clone(),
        }
    }
}

impl<I: InstructionSet> Debug for Program<I> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Program")
            .field("format", &self.format)
            .field("source", &self.source)
            .field("instructions", &self.instructions)
            .finish()
    }
}

impl<I: InstructionSet> PartialEq for Program<I> {
    fn eq(&self, other: &Self) -> bool {
        self.format == other.format
            && self.source == other.source
            && self.instructions == other.instructions
    }
}

impl<I: InstructionSet> Eq for Program<I> {}

impl<I: InstructionSet> Program<I> {
    /// Reads `source` as `format` under `space`.
    ///
    /// # Errors
    ///
    /// Every error the source has, in source order: each line's first
    /// problem, and each branch to a label never defined; or, for a source
    /// with neither errors nor instructions, that it is empty. A source
    /// larger than the format's limit has that error alone.
    pub fn parse(
        space: &I::Space,
        source: &str,
        format: I::Format,
    ) -> Result<Self, ParseFailure<I::Format>> {
        let limits = I::limits(format);
        let fail = |errors: Vec<ParseError>| ParseFailure::new::<I>(format, errors, space);
        if u64::try_from(source.len()).unwrap_or(u64::MAX) > u64::from(limits.program_bytes) {
            let (line, column) = place_of_byte(source, limits.program_bytes);
            let message = I::word(Reason::TooLarge, space, format);
            return Err(fail(vec![error::<I>(line, column, 1, message)]));
        }
        let mut lines = (1_u32..).zip(source.lines());
        let read: Vec<(u32, &str)> = lines
            .by_ref()
            .take(usize::try_from(limits.program_lines).unwrap_or(usize::MAX))
            .collect();
        let (instructions, mut errors) = Reading::<I>::read(space, format, &read);
        if let Some((line, text)) = lines.next() {
            let message = I::word(Reason::TooManyLines, space, format);
            errors.push(error::<I>(line, 1, chars(text), message));
        }
        if !errors.is_empty() || instructions.is_empty() {
            return Err(fail(errors));
        }
        Ok(Self {
            format,
            source: source.to_owned(),
            instructions,
        })
    }

    /// The format the source was read as.
    #[must_use]
    pub fn format(&self) -> I::Format {
        self.format
    }

    /// The source as the student wrote it.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// How many instructions the program has.
    #[must_use]
    pub fn len(&self) -> u32 {
        u32::try_from(self.instructions.len()).unwrap_or(u32::MAX)
    }

    /// Whether the program has no instructions, which no parsed program has.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    /// The instruction at index `index`, if the program has one there.
    #[must_use]
    pub fn get(&self, index: u32) -> Option<&Placed<I::Instruction>> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.instructions.get(index))
    }
}

impl<I: Encode> Program<I> {
    /// The program's instructions as machine words, in order.
    pub fn words(&self) -> impl Iterator<Item = I::Word> + '_ {
        self.instructions
            .iter()
            .map(|placed| I::encode(&placed.instruction))
    }
}

impl<F: Copy> ParseFailure<F> {
    /// The failure of a source read as `format` with `errors`, sorted into
    /// source order here; with no errors, the source is empty.
    fn new<I: InstructionSet<Format = F>>(
        format: F,
        mut errors: Vec<ParseError>,
        space: &I::Space,
    ) -> Self {
        errors.sort_by_key(|error| (error.line, error.column));
        let mut errors = errors.into_iter();
        let first = errors.next().unwrap_or_else(|| {
            let message = I::word(Reason::Empty, space, format);
            error::<I>(1, 1, 0, message)
        });
        Self {
            format,
            first,
            rest: errors.collect(),
        }
    }

    /// The format the source was read as.
    #[must_use]
    pub fn format(&self) -> F {
        self.format
    }

    /// The first error in source order.
    #[must_use]
    pub fn first(&self) -> &ParseError {
        &self.first
    }

    /// How many errors the source has.
    #[must_use]
    pub fn count(&self) -> u32 {
        u32::try_from(self.rest.len())
            .unwrap_or(u32::MAX)
            .saturating_add(1)
    }

    /// The first `limit` errors, in source order.
    #[must_use]
    pub fn into_listed(self, limit: u32) -> Vec<ParseError> {
        std::iter::once(self.first)
            .chain(self.rest)
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .collect()
    }
}

impl<'a, 's, I: InstructionSet> Reading<'a, 's, I> {
    /// Reads `lines`, each with its number: the instructions, and the errors.
    fn read(
        space: &'a I::Space,
        format: I::Format,
        lines: &[(u32, &'s str)],
    ) -> (Vec<Placed<I::Instruction>>, Vec<ParseError>) {
        let mut reading = Self {
            context: Context::new(space, format),
            limits: I::limits(format),
            instructions: Vec::new(),
            labels: BTreeMap::new(),
            branches: Vec::new(),
            errors: Vec::new(),
            too_many: false,
        };
        for &(number, line) in lines {
            if let Err(problem) = reading.line(number, line) {
                reading.errors.push(on::<I>(problem, number));
            }
        }
        reading.resolve();
        (reading.instructions, reading.errors)
    }

    /// Reads line `number`, `line`.
    fn line(&mut self, number: u32, line: &'s str) -> Result<(), Problem> {
        let tokens = lexer::tokens(line, I::SYNTAX);
        let mut rest = tokens.as_slice();
        if I::has_labels(self.context.format()) {
            if let [label, colon, after @ ..] = rest {
                if colon.kind == TokenKind::Colon {
                    self.define(number, label)?;
                    rest = after;
                }
            }
        }
        let [first, operands @ ..] = rest else {
            return Ok(());
        };
        let last = operands.last().unwrap_or(first);
        let (instructions, branches) = I::read_line(&self.context, first, operands)?.into_parts();
        if let Some(limit) = self.limits.instructions {
            let after = self.instructions.len().saturating_add(instructions.len());
            if after > usize::try_from(limit).unwrap_or(usize::MAX) {
                if self.too_many {
                    return Ok(());
                }
                self.too_many = true;
                return Err(self.context.problem(first, Reason::TooManyInstructions));
            }
        }
        let start = self.instructions.len();
        self.branches
            .extend(branches.into_iter().map(|branch| Pending {
                index: start.saturating_add(branch.index),
                label: branch.label,
                line: number,
            }));
        let text = lexer::collapse(line.get(first.start..last.end).unwrap_or_default());
        self.instructions
            .extend(instructions.into_iter().map(|instruction| Placed {
                instruction,
                line: number,
                text: Arc::clone(&text),
            }));
        Ok(())
    }

    /// Defines `label` on line `number`, marking the next instruction.
    fn define(&mut self, number: u32, label: &Token<'s>) -> Result<(), Problem> {
        self.context.label(label)?;
        if let Some(&(_, line)) = self.labels.get(label.text) {
            return Err(self.context.problem(
                label,
                Reason::DuplicateLabel {
                    name: label.text,
                    line,
                },
            ));
        }
        let index = u32::try_from(self.instructions.len()).unwrap_or(u32::MAX);
        self.labels.insert(label.text, (index, number));
        Ok(())
    }

    /// Resolves every branch to the instruction its label marks, or reports
    /// the label as not defined or out of reach.
    fn resolve(&mut self) {
        for pending in &self.branches {
            let label = &pending.label;
            let resolved = match self.labels.get(label.text) {
                Some(&(target, _)) => {
                    self.instructions
                        .get_mut(pending.index)
                        .map_or(Ok(()), |placed| {
                            let at = u32::try_from(pending.index).unwrap_or(u32::MAX);
                            I::resolve(&mut placed.instruction, at, target)
                        })
                }
                None => Err(self
                    .context
                    .problem(label, Reason::UndefinedLabel { name: label.text })
                    .message),
            };
            if let Err(message) = resolved {
                self.errors.push(error::<I>(
                    pending.line,
                    label.column,
                    label.length,
                    message,
                ));
            }
        }
    }
}

/// An error on line `line` at column `column`, `length` characters long.
fn error<I: InstructionSet>(line: u32, column: u32, length: u32, message: String) -> ParseError {
    ParseError {
        text: I::locate(line, column, &message),
        line,
        column,
        length,
        message,
    }
}

/// `problem` as an error on line `line`.
fn on<I: InstructionSet>(problem: Problem, line: u32) -> ParseError {
    error::<I>(line, problem.column, problem.length, problem.message)
}
