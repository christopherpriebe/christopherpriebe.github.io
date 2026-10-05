//! What the instruments of this workspace share about assembly: the lexer of
//! a line of source, the record of an error in it with the arithmetic of
//! places in the source, and the toolkit that reads a source into a program
//! for any instruction set that implements [`set::InstructionSet`].
//!
//! The crate holds no sentence a student reads, depends on no JavaScript or
//! operating-system crate, reads no clock and does no I/O. The optional `ts`
//! feature, enabled only through an instrument's own, derives TypeScript
//! declarations for the types that cross the boundary.

#![forbid(unsafe_code)]

pub mod diag;
pub mod lexer;
pub mod program;
pub mod set;
