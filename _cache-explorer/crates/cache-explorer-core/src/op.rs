//! What the student asks the machine to do, and why the machine may refuse.
//!
//! An [`Op`] is one item of the session's log. The session's state is a fold
//! over the log, so an op is only ever appended once it has been checked
//! against the machine it applies to: a refused op returns an [`OpError`]
//! and leaves no trace, and whatever goes wrong only while an accepted op
//! executes is recorded as an `Error` event instead.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::{Config, ConfigError};
use crate::format;
use crate::geometry::{highest_word, AccessError};
use crate::isa::ParseError;
use crate::limits::{MAX_RUN_STEPS, SESSION_EVENT_LIMIT};
use crate::units::{Addr, Word};

/// One item in the session log. The session's state is a fold over these.
///
/// Serialized with its kind under `op`: `{"op":"config","config":{…}}`,
/// `{"op":"issue","kind":"write","addr":164,"value":7}`,
/// `{"op":"program","source":"…","format":"asm"}`, `{"op":"step"}`,
/// `{"op":"stepAccess"}` and `{"op":"run","n":20}`. It is read through a flat
/// form with every field optional, which then requires the fields of its
/// kind and refuses any other: that keeps serde's line and column in the
/// errors of a share document and avoids the code serde generates to buffer
/// an internally tagged enum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    try_from = "RawOp"
)]
pub enum Op {
    /// Start a new run with this configuration. The loaded program is kept
    /// and restarts at its first instruction.
    Config {
        /// The new run's configuration.
        config: Config,
    },
    /// A load or store issued by hand.
    Issue {
        /// Whether it loads or stores.
        kind: AccessKind,
        /// The word's address.
        addr: Addr,
        /// The value a store writes; a load has none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<Word>,
    },
    /// Load a program or trace, replacing the current one.
    Program {
        /// The source as the student wrote it.
        source: String,
        /// Whether the source is assembly or a trace.
        format: ProgramFormat,
    },
    /// Execute one instruction.
    Step,
    /// Execute instructions until one memory access completes.
    StepAccess,
    /// Execute up to `n` instructions, from 1 to
    /// [`MAX_RUN_STEPS`](crate::limits::MAX_RUN_STEPS).
    Run {
        /// How many instructions at most.
        n: u32,
    },
}

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

/// The text of the program drawer and its format, which Space and
/// Shift+Space pass so the session can tell whether the drawer holds a
/// program other than the one loaded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProgramSource {
    /// The source as the student wrote it.
    pub source: String,
    /// Whether the source is assembly or a trace.
    pub format: ProgramFormat,
}

/// Why an op is refused. A refused op changes nothing. `Display` is the
/// message the student reads.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum OpError {
    /// A `Config` op's configuration is invalid.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// An issued access's address does not fit the address width, or is
    /// not a multiple of the word size.
    #[error(transparent)]
    Access(#[from] AccessError),
    /// An issued store has no value to write.
    #[error("A store needs a value to write.")]
    MissingValue,
    /// An issued load has a value, which only a store takes.
    #[error("A load takes no value; only a store writes one.")]
    UnexpectedValue,
    /// An issued store's value does not fit the word: `300 does not fit a
    /// 1-byte word (0–255).` Its range is unsigned, since the value is a
    /// `Word`, which has no sign.
    #[error("{}", value_too_wide(*.value, *.word_bytes))]
    ValueTooWide {
        /// The store's value.
        value: Word,
        /// The word size, in bytes.
        word_bytes: u32,
    },
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
    /// The program stopped before its end, on a fault, at the end of its
    /// budget or at the session's limit, at the instruction on line `line`.
    #[error(
        "The program stopped at line {}; load it again to restart.",
        format::position(*.line)
    )]
    Stopped {
        /// The source line of the instruction it stopped at.
        line: u32,
    },
    /// A `Run` op asks for no instructions, or for more than
    /// [`MAX_RUN_STEPS`].
    #[error(
        "Run between 1 and {} instructions at a time.",
        format::setting(MAX_RUN_STEPS)
    )]
    RunSteps,
    /// The log up to where the op would be appended leaves no room for one
    /// more entry under [`SESSION_EVENT_LIMIT`].
    #[error(
        "This session has reached its limit of {} steps. Undo to make room, or save it and \
         start fresh.",
        format::setting(SESSION_EVENT_LIMIT)
    )]
    SessionFull,
}

/// An op as read from JSON: its kind, and every field any kind has, each
/// optional. [`Op`]'s `TryFrom` then requires the fields of the kind and
/// refuses the others.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawOp {
    /// The op's kind.
    op: OpName,
    /// A `Config` op's configuration.
    config: Option<Config>,
    /// An `Issue` op's kind of access.
    kind: Option<AccessKind>,
    /// An `Issue` op's address.
    addr: Option<Addr>,
    /// An `Issue` op's value.
    value: Option<Word>,
    /// A `Program` op's source.
    source: Option<String>,
    /// A `Program` op's format.
    format: Option<ProgramFormat>,
    /// A `Run` op's count.
    n: Option<u32>,
}

/// The kinds of op, as the `op` field names them.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum OpName {
    /// `config`.
    Config,
    /// `issue`.
    Issue,
    /// `program`.
    Program,
    /// `step`.
    Step,
    /// `stepAccess`.
    StepAccess,
    /// `run`.
    Run,
}

/// Why a flat op does not have the fields of its kind. Lower case and
/// unterminated, like serde's own messages, since it reaches a reader only
/// through serde, which adds the line and column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
enum ShapeError {
    /// A field the kind requires is absent.
    #[error("missing field `{field}` for op `{op}`")]
    Missing {
        /// The op's kind.
        op: &'static str,
        /// The absent field.
        field: &'static str,
    },
    /// A field the kind does not have is present.
    #[error("unexpected field `{field}` for op `{op}`")]
    Unexpected {
        /// The op's kind.
        op: &'static str,
        /// The field that does not belong.
        field: &'static str,
    },
}

impl OpName {
    /// The name the `op` field gives the kind.
    fn name(self) -> &'static str {
        match self {
            OpName::Config => "config",
            OpName::Issue => "issue",
            OpName::Program => "program",
            OpName::Step => "step",
            OpName::StepAccess => "stepAccess",
            OpName::Run => "run",
        }
    }
}

impl TryFrom<RawOp> for Op {
    type Error = ShapeError;

    fn try_from(raw: RawOp) -> Result<Self, ShapeError> {
        let name = raw.op;
        let present = [
            ("config", raw.config.is_some()),
            ("kind", raw.kind.is_some()),
            ("addr", raw.addr.is_some()),
            ("value", raw.value.is_some()),
            ("source", raw.source.is_some()),
            ("format", raw.format.is_some()),
            ("n", raw.n.is_some()),
        ];
        let allowed: &[&str] = match name {
            OpName::Config => &["config"],
            OpName::Issue => &["kind", "addr", "value"],
            OpName::Program => &["source", "format"],
            OpName::Step | OpName::StepAccess => &[],
            OpName::Run => &["n"],
        };
        if let Some((field, _)) = present
            .iter()
            .find(|(field, present)| *present && !allowed.contains(field))
        {
            return Err(ShapeError::Unexpected {
                op: name.name(),
                field,
            });
        }
        let missing = |field| ShapeError::Missing {
            op: name.name(),
            field,
        };
        Ok(match name {
            OpName::Config => Op::Config {
                config: raw.config.ok_or_else(|| missing("config"))?,
            },
            OpName::Issue => Op::Issue {
                kind: raw.kind.ok_or_else(|| missing("kind"))?,
                addr: raw.addr.ok_or_else(|| missing("addr"))?,
                value: raw.value,
            },
            OpName::Program => Op::Program {
                source: raw.source.ok_or_else(|| missing("source"))?,
                format: raw.format.ok_or_else(|| missing("format"))?,
            },
            OpName::Step => Op::Step,
            OpName::StepAccess => Op::StepAccess,
            OpName::Run => Op::Run {
                n: raw.n.ok_or_else(|| missing("n"))?,
            },
        })
    }
}

/// The message of a value too wide for a word of `word_bytes` bytes, with
/// the word's unsigned range: `300 does not fit a 1-byte word (0–255).`
pub(crate) fn value_too_wide(value: Word, word_bytes: u32) -> String {
    format!(
        "{} does not fit {} {}-byte word (0\u{2013}{}).",
        format::data(value.get()),
        format::a_or_an(word_bytes),
        format::setting(word_bytes),
        format::setting(highest_word(word_bytes))
    )
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
    use crate::config::Preset;
    use crate::units::Addr;

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
    fn ops_serialize_with_their_kind_under_op() {
        let config = Preset::DirectMapped8.config();
        let config_json = serde_json::to_string(&config).unwrap();
        let shapes = [
            (
                Op::Config {
                    config: config.clone(),
                },
                format!("{{\"op\":\"config\",\"config\":{config_json}}}"),
            ),
            (
                Op::Issue {
                    kind: AccessKind::Write,
                    addr: Addr::new_unchecked(164),
                    value: Some(Word::new_unchecked(7)),
                },
                "{\"op\":\"issue\",\"kind\":\"write\",\"addr\":164,\"value\":7}".to_owned(),
            ),
            (
                Op::Issue {
                    kind: AccessKind::Read,
                    addr: Addr::new_unchecked(164),
                    value: None,
                },
                "{\"op\":\"issue\",\"kind\":\"read\",\"addr\":164}".to_owned(),
            ),
            (
                Op::Program {
                    source: "ld r1, [0x0A4]".to_owned(),
                    format: ProgramFormat::Asm,
                },
                "{\"op\":\"program\",\"source\":\"ld r1, [0x0A4]\",\"format\":\"asm\"}".to_owned(),
            ),
            (Op::Step, "{\"op\":\"step\"}".to_owned()),
            (Op::StepAccess, "{\"op\":\"stepAccess\"}".to_owned()),
            (Op::Run { n: 20 }, "{\"op\":\"run\",\"n\":20}".to_owned()),
        ];
        for (op, json) in shapes {
            assert_eq!(serde_json::to_string(&op).unwrap(), json);
            assert_eq!(serde_json::from_str::<Op>(&json).unwrap(), op);
        }
    }

    #[test]
    fn an_op_is_read_with_the_fields_of_its_kind_only() {
        let read = |json: &str| serde_json::from_str::<Op>(json).map_err(|error| error.to_string());
        assert_eq!(
            read("{\"n\":3,\"op\":\"run\"}"),
            Ok(Op::Run { n: 3 }),
            "fields may come in any order"
        );
        assert_eq!(
            read("{\"op\":\"issue\",\"kind\":\"read\",\"addr\":164,\"value\":null}"),
            Ok(Op::Issue {
                kind: AccessKind::Read,
                addr: Addr::new_unchecked(164),
                value: None,
            })
        );
        let refused = |json: &str, message: &str| {
            let error = read(json).unwrap_err();
            assert!(error.starts_with(message), "{json}: {error}");
        };
        refused(
            "{\"op\":\"stpe\"}",
            "unknown variant `stpe`, expected one of",
        );
        refused("{\"op\":\"run\"}", "missing field `n` for op `run`");
        refused(
            "{\"op\":\"issue\",\"kind\":\"read\"}",
            "missing field `addr` for op `issue`",
        );
        refused(
            "{\"op\":\"step\",\"n\":3}",
            "unexpected field `n` for op `step`",
        );
        refused(
            "{\"op\":\"program\",\"source\":\"\",\"format\":\"asm\",\"addr\":4}",
            "unexpected field `addr` for op `program`",
        );
        refused("{\"op\":\"step\",\"steps\":3}", "unknown field `steps`");
        refused("{\"n\":3}", "missing field `op`");
    }

    #[test]
    fn a_program_source_is_read_with_its_two_fields() {
        let source = ProgramSource {
            source: "R 0x0A4".to_owned(),
            format: ProgramFormat::Trace,
        };
        let json = "{\"source\":\"R 0x0A4\",\"format\":\"trace\"}";
        assert_eq!(serde_json::to_string(&source).unwrap(), json);
        assert_eq!(serde_json::from_str::<ProgramSource>(json).unwrap(), source);
        assert!(serde_json::from_str::<ProgramSource>(
            "{\"source\":\"\",\"format\":\"asm\",\"n\":1}"
        )
        .is_err());
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

    #[test]
    fn refusals_of_an_issue_a_run_and_a_full_session_say_why() {
        assert_eq!(
            OpError::MissingValue.to_string(),
            "A store needs a value to write."
        );
        assert_eq!(
            OpError::UnexpectedValue.to_string(),
            "A load takes no value; only a store writes one."
        );
        assert_eq!(
            OpError::ValueTooWide {
                value: Word::new_unchecked(300),
                word_bytes: 1,
            }
            .to_string(),
            "300 does not fit a 1-byte word (0\u{2013}255)."
        );
        assert_eq!(
            OpError::ValueTooWide {
                value: Word::new_unchecked(u64::from(u32::MAX) + 1),
                word_bytes: 4,
            }
            .to_string(),
            "4,294,967,296 does not fit a 4-byte word (0\u{2013}4,294,967,295)."
        );
        assert_eq!(
            OpError::RunSteps.to_string(),
            format!(
                "Run between 1 and {} instructions at a time.",
                format::setting(MAX_RUN_STEPS)
            )
        );
        assert_eq!(
            OpError::SessionFull.to_string(),
            format!(
                "This session has reached its limit of {} steps. Undo to make room, or save it \
                 and start fresh.",
                format::setting(SESSION_EVENT_LIMIT)
            )
        );
        let config = OpError::from(ConfigError::AddressBits { got: 4 });
        assert_eq!(
            config.to_string(),
            ConfigError::AddressBits { got: 4 }.to_string()
        );
    }
}
