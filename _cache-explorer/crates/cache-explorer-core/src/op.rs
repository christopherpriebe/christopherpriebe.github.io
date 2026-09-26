//! What the student asks the machine to do, and why the machine may refuse.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::format;
use crate::isa::ParseError;

/// Whether an access reads or writes memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum AccessKind {
    /// A load: a word from memory into a register.
    Read,
    /// A store: a word into memory.
    Write,
}

/// How a program's source is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum ProgramFormat {
    /// The assembly language: `ld`, `st`, `add` and `bnez`, with labels.
    Asm,
    /// A trace: one `R addr` or `W addr value` access per line.
    Trace,
}

/// Why an op is refused. A refused op changes nothing. `Display` is the
/// message the student reads.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum OpError {
    /// The program or trace does not parse under the run's configuration.
    /// The message counts its errors and quotes the first: `The program has
    /// 2 errors; the first is on line 3: …`.
    #[error("{}", unparsed(*.format, *.errors, .first))]
    Program {
        /// Whether it was read as assembly or as a trace.
        format: ProgramFormat,
        /// How many errors it has; at least 1.
        errors: u32,
        /// The first of them.
        first: ParseError,
    },
    /// There is nothing to execute: no program is loaded.
    #[error("No program is loaded. Load a program or trace, or issue an access.")]
    NoProgram,
    /// The program has executed its last instruction.
    #[error("The program has finished.")]
    Finished,
    /// The program stopped before its end, on a fault or at the end of its
    /// budget, at the instruction on line `line`.
    #[error(
        "The program stopped at line {}; load it again to restart.",
        format::position(*.line)
    )]
    Stopped {
        /// The source line of the instruction it stopped at.
        line: u32,
    },
}

/// The message of a source with `errors` errors, the first being `first`.
fn unparsed(format: ProgramFormat, errors: u32, first: &ParseError) -> String {
    let source = match format {
        ProgramFormat::Asm => "program",
        ProgramFormat::Trace => "trace",
    };
    let line = format::position(first.line);
    if errors == 1 {
        format!(
            "The {source} has an error on line {line}: {}",
            first.message
        )
    } else {
        format!(
            "The {source} has {} errors; the first is on line {line}: {}",
            format::data(errors),
            first.message
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    #[test]
    fn access_kinds_serialize_by_lower_case_name() {
        for (kind, name) in [
            (AccessKind::Read, "\"read\""),
            (AccessKind::Write, "\"write\""),
        ] {
            assert_eq!(serde_json::to_string(&kind).unwrap(), name);
            assert_eq!(serde_json::from_str::<AccessKind>(name).unwrap(), kind);
        }
        assert!(serde_json::from_str::<AccessKind>("\"Read\"").is_err());
    }

    #[test]
    fn program_formats_serialize_by_lower_case_name() {
        for (format, name) in [
            (ProgramFormat::Asm, "\"asm\""),
            (ProgramFormat::Trace, "\"trace\""),
        ] {
            assert_eq!(serde_json::to_string(&format).unwrap(), name);
            assert_eq!(serde_json::from_str::<ProgramFormat>(name).unwrap(), format);
        }
        assert!(serde_json::from_str::<ProgramFormat>("\"program\"").is_err());
    }

    #[test]
    fn a_program_that_does_not_parse_is_refused_with_its_first_error() {
        let first = ParseError {
            line: 3,
            column: 5,
            length: 2,
            message: "unknown instruction `lw`; use ld, st, add or bnez.".to_owned(),
            text: String::new(),
        };
        let refused = |format, errors| {
            OpError::Program {
                format,
                errors,
                first: first.clone(),
            }
            .to_string()
        };
        assert_eq!(
            refused(ProgramFormat::Asm, 2),
            "The program has 2 errors; the first is on line 3: unknown instruction `lw`; use \
             ld, st, add or bnez."
        );
        assert_eq!(
            refused(ProgramFormat::Asm, 1),
            "The program has an error on line 3: unknown instruction `lw`; use ld, st, add or \
             bnez."
        );
        assert!(refused(ProgramFormat::Trace, 12_000).starts_with("The trace has 12,000 errors;"));
    }

    #[test]
    fn a_step_with_nothing_to_execute_says_why() {
        assert_eq!(
            OpError::NoProgram.to_string(),
            "No program is loaded. Load a program or trace, or issue an access."
        );
        assert_eq!(OpError::Finished.to_string(), "The program has finished.");
        assert_eq!(
            OpError::Stopped { line: 4 }.to_string(),
            "The program stopped at line 4; load it again to restart."
        );
    }
}
