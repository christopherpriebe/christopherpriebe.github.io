//! What the student asks the machine to do, and why the machine may refuse.
//!
//! An [`Op`] is one item of the session's log. The session's state is a fold
//! over the log, so an op is only ever appended once it has been checked
//! against the machine it applies to: a refused op returns an [`OpError`]
//! and leaves no trace, and whatever goes wrong only while an accepted op
//! executes is recorded as an `Error` event instead.

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::config::{Config, ConfigError};
use crate::de::deserialize_unit_enum;
use crate::engine::EditRefusal;
use crate::format;
use crate::geometry::{highest_word, AccessError};
use crate::isa::ParseError;
use crate::limits::{MAX_RUN_STEPS, MAX_SHARE_JSON_BYTES, MAX_SHARE_OPS, SESSION_EVENT_LIMIT};
use crate::units::{Addr, SetIndex, WayIndex, Word};

/// One item in the session log. The session's state is a fold over these.
///
/// Serialized with its kind under `op`: `{"op":"config","config":{…}}`,
/// `{"op":"issue","kind":"write","addr":164,"value":7}`,
/// `{"op":"program","source":"…","format":"asm"}`, `{"op":"step"}`,
/// `{"op":"stepAccess"}`, `{"op":"run","n":20}` and
/// `{"op":"edit","target":{…},"patch":{…}}`. It is read through a flat form
/// with every field optional, which then requires the fields of its kind and
/// refuses any other: that keeps serde's line and column in the errors of a
/// share document and avoids the code serde generates to buffer an
/// internally tagged enum.
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
    /// Change a cache way or a memory word. It is planned again against the
    /// machine it applies to, and applied only if that plan succeeds.
    Edit {
        /// The way or the memory word.
        target: EditTarget,
        /// What changes.
        patch: EditPatch,
    },
}

/// What an edit changes: one way of the cache, or one word of memory.
///
/// Serialized with its kind under `kind`: `{"kind":"way","set":2,"way":0}`
/// or `{"kind":"memory","addr":176}`. Its numbers are unchecked until the
/// edit is planned against the configuration in force. Read through a flat
/// form, as an [`Op`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    try_from = "RawEditTarget"
)]
pub enum EditTarget {
    /// Way `way` of set `set`.
    Way {
        /// The set.
        set: SetIndex,
        /// The way within the set.
        way: WayIndex,
    },
    /// The memory word at `addr`, which must be a multiple of the word size.
    Memory {
        /// The word's address.
        addr: Addr,
    },
}

/// The change an edit makes, of the target's kind.
///
/// Serialized with its kind under `kind`: a way's patch with the fields it
/// gives beside it, `{"kind":"way","line":192,"dirty":true}`, or a memory
/// word's new value, `{"kind":"memory","value":12}`. Read through a flat
/// form, as an [`Op`] is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    try_from = "RawEditPatch"
)]
pub enum EditPatch {
    /// The fields of a way that change.
    Way(WayPatch),
    /// A memory word's new value.
    Memory {
        /// The value, which must fit the word.
        value: Word,
    },
}

/// The fields of a way an edit changes; a field left out keeps its value,
/// or takes the one the edit implies. A way is given the line it holds by
/// an address, from which the tag follows: a bare tag has no index bits, so
/// it could not be checked against the way's set.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WayPatch {
    /// Whether the way holds a line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid: Option<bool>,
    /// Any byte address inside the line the way should hold. Its index bits
    /// must equal the way's set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<Addr>,
    /// The whole line, one value per word.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<Word>>,
    /// Whether the line is dirty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty: Option<bool>,
    /// The way's place in its set's replacement order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<u32>,
}

/// Whether an access reads or writes memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum AccessKind {
    /// A load: a word from memory into a register.
    Read,
    /// A store: a word into memory.
    Write,
}

deserialize_unit_enum!(AccessKind {
    "read" => Read,
    "write" => Write,
});

/// How a program's source is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(rename_all = "camelCase")]
pub enum ProgramFormat {
    /// The assembly language: `ld`, `st`, `add` and `bnez`, with labels.
    Asm,
    /// A trace: one `R addr` or `W addr value` access per line.
    Trace,
}

deserialize_unit_enum!(ProgramFormat {
    "asm" => Asm,
    "trace" => Trace,
});

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
    /// A guided module's prediction is unanswered, and the session refuses
    /// every op until it is answered, so nothing can reveal the answer
    /// first.
    #[error("Answer the prediction in the module panel first.")]
    Gated,
    /// The log up to where the op would be appended leaves no room for one
    /// more entry under [`SESSION_EVENT_LIMIT`].
    #[error(
        "This session has reached its limit of {} steps. Undo to make room, or save it and \
         start fresh.",
        format::setting(SESSION_EVENT_LIMIT)
    )]
    SessionFull,
    /// The log up to where the op would be appended already holds
    /// [`MAX_SHARE_OPS`] ops, as many as a link can hold, so that every log
    /// a session keeps can be shared. The message states the limit rather
    /// than the log's length, since a call that appends several ops at once
    /// is refused whole with room left for fewer.
    #[error(
        "A link can hold at most {} actions, and this would pass that. Undo to make room, or \
         save the run and start fresh.",
        format::setting(MAX_SHARE_OPS)
    )]
    LinkFull,
    /// The op would take the saved file of the log up to where it would be
    /// appended past [`MAX_SHARE_JSON_BYTES`], the most a file or a link
    /// can hold, so that every log a session keeps can be saved and opened
    /// again, wherever its cursor is and in either mode.
    #[error(
        "A saved file can hold at most {} bytes, and this would pass that. Undo to make room, \
         or save the run and start fresh.",
        format::setting(MAX_SHARE_JSON_BYTES)
    )]
    FileFull,
    /// The edit's plan refuses it; the message is the refusal's title and
    /// text: `Refused. 0x120 has index 2, so it can only live in set 2, …`.
    #[error("{}. {}", .0.title, .0.text)]
    Edit(Box<EditRefusal>),
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
    #[serde(default)]
    config: Field<Config>,
    /// An `Issue` op's kind of access.
    #[serde(default)]
    kind: Field<AccessKind>,
    /// An `Issue` op's address.
    #[serde(default)]
    addr: Field<Addr>,
    /// An `Issue` op's value.
    #[serde(default)]
    value: Field<Word>,
    /// A `Program` op's source.
    #[serde(default)]
    source: Field<String>,
    /// A `Program` op's format.
    #[serde(default)]
    format: Field<ProgramFormat>,
    /// A `Run` op's count.
    #[serde(default)]
    n: Field<u32>,
    /// An `Edit` op's target.
    #[serde(default)]
    target: Field<EditTarget>,
    /// An `Edit` op's patch.
    #[serde(default)]
    patch: Field<EditPatch>,
}

/// An edit's target as read from JSON: its kind, and every field either kind
/// has, each optional.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawEditTarget {
    /// The target's kind.
    kind: EditKind,
    /// A way's set.
    #[serde(default)]
    set: Field<SetIndex>,
    /// A way's number within its set.
    #[serde(default)]
    way: Field<WayIndex>,
    /// A memory word's address.
    #[serde(default)]
    addr: Field<Addr>,
}

/// An edit's patch as read from JSON: its kind, and every field either kind
/// has, each optional.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawEditPatch {
    /// The patch's kind.
    kind: EditKind,
    /// A way patch's valid bit.
    #[serde(default)]
    valid: Field<bool>,
    /// A way patch's line.
    #[serde(default)]
    line: Field<Addr>,
    /// A way patch's data.
    #[serde(default)]
    data: Field<Vec<Word>>,
    /// A way patch's dirty bit.
    #[serde(default)]
    dirty: Field<bool>,
    /// A way patch's rank.
    #[serde(default)]
    rank: Field<u32>,
    /// A memory patch's value.
    #[serde(default)]
    value: Field<Word>,
}

/// The kinds of edit target and patch, as their `kind` field names them.
#[derive(Clone, Copy)]
enum EditKind {
    /// `way`.
    Way,
    /// `memory`.
    Memory,
}

deserialize_unit_enum!(EditKind {
    "way" => Way,
    "memory" => Memory,
});

impl EditKind {
    /// The name the `kind` field gives it.
    fn name(self) -> &'static str {
        match self {
            EditKind::Way => "way",
            EditKind::Memory => "memory",
        }
    }
}

/// A field of a flat op as the JSON gives it: absent, `null`, or a value.
/// A `null` is still a field, so a kind that does not have the field
/// refuses it; a kind that requires the field finds it missing, and an
/// issue's optional `value` reads it as no value, as serde reads a `null`
/// option.
#[derive(Default)]
pub(crate) enum Field<T> {
    /// The field is not there.
    #[default]
    Absent,
    /// The field is there, as `null`.
    Null,
    /// The field is there, with a value.
    Value(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<T>::deserialize(deserializer).map(|value| value.map_or(Field::Null, Field::Value))
    }
}

impl<T> Field<T> {
    /// Whether the field is there, `null` included.
    pub(crate) fn is_present(&self) -> bool {
        !matches!(self, Field::Absent)
    }

    /// The field's value; none if it is absent or `null`.
    pub(crate) fn value(self) -> Option<T> {
        match self {
            Field::Value(value) => Some(value),
            Field::Absent | Field::Null => None,
        }
    }
}

/// The kinds of op, as the `op` field names them.
#[derive(Clone, Copy)]
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
    /// `edit`.
    Edit,
}

deserialize_unit_enum!(OpName {
    "config" => Config,
    "issue" => Issue,
    "program" => Program,
    "step" => Step,
    "stepAccess" => StepAccess,
    "run" => Run,
    "edit" => Edit,
});

/// Why a flat op, edit target or edit patch does not have the fields of its
/// kind. Lower case and unterminated, like serde's own messages, since it
/// reaches a reader only through serde, which adds the line and column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
enum ShapeError {
    /// A field the kind requires is absent.
    #[error("missing field `{field}` for {form} `{kind}`")]
    Missing {
        /// What has the kind: "op", "target" or "patch".
        form: &'static str,
        /// The kind.
        kind: &'static str,
        /// The absent field.
        field: &'static str,
    },
    /// A field the kind does not have is present.
    #[error("unexpected field `{field}` for {form} `{kind}`")]
    Unexpected {
        /// What has the kind: "op", "target" or "patch".
        form: &'static str,
        /// The kind.
        kind: &'static str,
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
            OpName::Edit => "edit",
        }
    }
}

/// Refuses the first field that is `present` but not `allowed` for the kind
/// `kind` of `form`.
fn only_allowed(
    form: &'static str,
    kind: &'static str,
    present: &[(&'static str, bool)],
    allowed: &[&str],
) -> Result<(), ShapeError> {
    match present
        .iter()
        .find(|(field, present)| *present && !allowed.contains(field))
    {
        Some(&(field, _)) => Err(ShapeError::Unexpected { form, kind, field }),
        None => Ok(()),
    }
}

impl TryFrom<RawOp> for Op {
    type Error = ShapeError;

    fn try_from(raw: RawOp) -> Result<Self, ShapeError> {
        let name = raw.op;
        let present = [
            ("config", raw.config.is_present()),
            ("kind", raw.kind.is_present()),
            ("addr", raw.addr.is_present()),
            ("value", raw.value.is_present()),
            ("source", raw.source.is_present()),
            ("format", raw.format.is_present()),
            ("n", raw.n.is_present()),
            ("target", raw.target.is_present()),
            ("patch", raw.patch.is_present()),
        ];
        let allowed: &[&str] = match name {
            OpName::Config => &["config"],
            OpName::Issue => &["kind", "addr", "value"],
            OpName::Program => &["source", "format"],
            OpName::Step | OpName::StepAccess => &[],
            OpName::Run => &["n"],
            OpName::Edit => &["target", "patch"],
        };
        only_allowed("op", name.name(), &present, allowed)?;
        let missing = |field| ShapeError::Missing {
            form: "op",
            kind: name.name(),
            field,
        };
        Ok(match name {
            OpName::Config => Op::Config {
                config: raw.config.value().ok_or_else(|| missing("config"))?,
            },
            OpName::Issue => Op::Issue {
                kind: raw.kind.value().ok_or_else(|| missing("kind"))?,
                addr: raw.addr.value().ok_or_else(|| missing("addr"))?,
                value: raw.value.value(),
            },
            OpName::Program => Op::Program {
                source: raw.source.value().ok_or_else(|| missing("source"))?,
                format: raw.format.value().ok_or_else(|| missing("format"))?,
            },
            OpName::Step => Op::Step,
            OpName::StepAccess => Op::StepAccess,
            OpName::Run => Op::Run {
                n: raw.n.value().ok_or_else(|| missing("n"))?,
            },
            OpName::Edit => Op::Edit {
                target: raw.target.value().ok_or_else(|| missing("target"))?,
                patch: raw.patch.value().ok_or_else(|| missing("patch"))?,
            },
        })
    }
}

impl TryFrom<RawEditTarget> for EditTarget {
    type Error = ShapeError;

    fn try_from(raw: RawEditTarget) -> Result<Self, ShapeError> {
        let kind = raw.kind;
        let present = [
            ("set", raw.set.is_present()),
            ("way", raw.way.is_present()),
            ("addr", raw.addr.is_present()),
        ];
        let allowed: &[&str] = match kind {
            EditKind::Way => &["set", "way"],
            EditKind::Memory => &["addr"],
        };
        only_allowed("target", kind.name(), &present, allowed)?;
        let missing = |field| ShapeError::Missing {
            form: "target",
            kind: kind.name(),
            field,
        };
        Ok(match kind {
            EditKind::Way => EditTarget::Way {
                set: raw.set.value().ok_or_else(|| missing("set"))?,
                way: raw.way.value().ok_or_else(|| missing("way"))?,
            },
            EditKind::Memory => EditTarget::Memory {
                addr: raw.addr.value().ok_or_else(|| missing("addr"))?,
            },
        })
    }
}

impl TryFrom<RawEditPatch> for EditPatch {
    type Error = ShapeError;

    // A way patch's fields are all optional, and a `null` one is left out, as
    // serde reads a `null` option; a memory patch requires its value.
    fn try_from(raw: RawEditPatch) -> Result<Self, ShapeError> {
        let kind = raw.kind;
        let present = [
            ("valid", raw.valid.is_present()),
            ("line", raw.line.is_present()),
            ("data", raw.data.is_present()),
            ("dirty", raw.dirty.is_present()),
            ("rank", raw.rank.is_present()),
            ("value", raw.value.is_present()),
        ];
        let allowed: &[&str] = match kind {
            EditKind::Way => &["valid", "line", "data", "dirty", "rank"],
            EditKind::Memory => &["value"],
        };
        only_allowed("patch", kind.name(), &present, allowed)?;
        Ok(match kind {
            EditKind::Way => EditPatch::Way(WayPatch {
                valid: raw.valid.value(),
                line: raw.line.value(),
                data: raw.data.value(),
                dirty: raw.dirty.value(),
                rank: raw.rank.value(),
            }),
            EditKind::Memory => EditPatch::Memory {
                value: raw.value.value().ok_or(ShapeError::Missing {
                    form: "patch",
                    kind: kind.name(),
                    field: "value",
                })?,
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
    fn a_null_field_is_still_a_field() {
        let read = |json: &str| serde_json::from_str::<Op>(json).map_err(|error| error.to_string());
        let refused = |json: &str, message: &str| {
            let error = read(json).unwrap_err();
            assert!(error.starts_with(message), "{json}: {error}");
        };
        // A kind that does not have the field refuses it, null or not.
        refused(
            "{\"op\":\"step\",\"n\":null}",
            "unexpected field `n` for op `step`",
        );
        refused(
            "{\"op\":\"stepAccess\",\"config\":null}",
            "unexpected field `config` for op `stepAccess`",
        );
        refused(
            "{\"op\":\"run\",\"n\":3,\"value\":null}",
            "unexpected field `value` for op `run`",
        );
        refused(
            "{\"op\":\"issue\",\"kind\":\"read\",\"addr\":164,\"source\":null}",
            "unexpected field `source` for op `issue`",
        );
        // A kind that requires it finds it missing.
        refused(
            "{\"op\":\"run\",\"n\":null}",
            "missing field `n` for op `run`",
        );
        refused(
            "{\"op\":\"program\",\"source\":\"\",\"format\":null}",
            "missing field `format` for op `program`",
        );
        // An issue's value is optional, so null is no value.
        assert_eq!(
            read("{\"op\":\"issue\",\"kind\":\"write\",\"addr\":164,\"value\":null}"),
            Ok(Op::Issue {
                kind: AccessKind::Write,
                addr: Addr::new_unchecked(164),
                value: None,
            })
        );
    }

    #[test]
    fn edits_serialize_with_their_kinds() {
        let way = EditTarget::Way {
            set: SetIndex::new_unchecked(2),
            way: WayIndex::new_unchecked(0),
        };
        let memory = EditTarget::Memory {
            addr: Addr::new_unchecked(176),
        };
        let patch = EditPatch::Way(WayPatch {
            line: Some(Addr::new_unchecked(192)),
            data: Some(vec![Word::new_unchecked(u64::MAX)]),
            dirty: Some(true),
            ..WayPatch::default()
        });
        let shapes = [
            (
                Op::Edit {
                    target: way,
                    patch: patch.clone(),
                },
                "{\"op\":\"edit\",\"target\":{\"kind\":\"way\",\"set\":2,\"way\":0},\
                 \"patch\":{\"kind\":\"way\",\"line\":192,\"data\":[18446744073709551615],\
                 \"dirty\":true}}",
            ),
            (
                Op::Edit {
                    target: memory,
                    patch: EditPatch::Memory {
                        value: Word::new_unchecked(12),
                    },
                },
                "{\"op\":\"edit\",\"target\":{\"kind\":\"memory\",\"addr\":176},\
                 \"patch\":{\"kind\":\"memory\",\"value\":12}}",
            ),
            (
                Op::Edit {
                    target: way,
                    patch: EditPatch::Way(WayPatch::default()),
                },
                "{\"op\":\"edit\",\"target\":{\"kind\":\"way\",\"set\":2,\"way\":0},\
                 \"patch\":{\"kind\":\"way\"}}",
            ),
        ];
        for (op, json) in shapes {
            assert_eq!(serde_json::to_string(&op).unwrap(), json);
            assert_eq!(serde_json::from_str::<Op>(json).unwrap(), op);
        }
        // A way patch alone reads as its fields, and refuses others.
        let alone = "{\"valid\":false}";
        assert_eq!(
            serde_json::from_str::<WayPatch>(alone).unwrap(),
            WayPatch {
                valid: Some(false),
                ..WayPatch::default()
            }
        );
        assert!(serde_json::from_str::<WayPatch>("{\"tag\":1}").is_err());
    }

    #[test]
    fn an_edit_is_read_with_the_fields_of_its_kinds_only() {
        let target = |json: &str| {
            serde_json::from_str::<EditTarget>(json).map_err(|error| error.to_string())
        };
        let patch =
            |json: &str| serde_json::from_str::<EditPatch>(json).map_err(|error| error.to_string());
        let refused = |result: Result<(), String>, message: &str| {
            let error = result.unwrap_err();
            assert!(error.starts_with(message), "{error}");
        };
        refused(
            target("{\"kind\":\"way\",\"set\":2}").map(drop),
            "missing field `way` for target `way`",
        );
        refused(
            target("{\"kind\":\"way\",\"set\":2,\"way\":0,\"addr\":4}").map(drop),
            "unexpected field `addr` for target `way`",
        );
        refused(
            target("{\"kind\":\"memory\",\"set\":null}").map(drop),
            "unexpected field `set` for target `memory`",
        );
        refused(
            target("{\"kind\":\"cell\"}").map(drop),
            "unknown variant `cell`, expected `way` or `memory`",
        );
        refused(
            patch("{\"kind\":\"way\",\"value\":1}").map(drop),
            "unexpected field `value` for patch `way`",
        );
        refused(
            patch("{\"kind\":\"memory\",\"value\":null}").map(drop),
            "missing field `value` for patch `memory`",
        );
        refused(
            patch("{\"kind\":\"memory\",\"value\":1,\"dirty\":false}").map(drop),
            "unexpected field `dirty` for patch `memory`",
        );
        refused(
            patch("{\"kind\":\"way\",\"tag\":1}").map(drop),
            "unknown field `tag`",
        );
        // A way patch's null field is left out, as a null option is.
        assert_eq!(
            patch("{\"kind\":\"way\",\"line\":null,\"rank\":1}"),
            Ok(EditPatch::Way(WayPatch {
                rank: Some(1),
                ..WayPatch::default()
            }))
        );
        refused(
            serde_json::from_str::<Op>(
                "{\"op\":\"edit\",\"target\":{\"kind\":\"memory\",\"addr\":4}}",
            )
            .map(drop)
            .map_err(|error| error.to_string()),
            "missing field `patch` for op `edit`",
        );
        refused(
            serde_json::from_str::<Op>("{\"op\":\"step\",\"patch\":null}")
                .map(drop)
                .map_err(|error| error.to_string()),
            "unexpected field `patch` for op `step`",
        );
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
    fn refusals_of_an_issue_a_run_a_full_session_a_full_link_and_a_full_file_say_why() {
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
        assert_eq!(
            OpError::LinkFull.to_string(),
            format!(
                "A link can hold at most {} actions, and this would pass that. Undo to make \
                 room, or save the run and start fresh.",
                format::setting(MAX_SHARE_OPS)
            )
        );
        assert_eq!(
            OpError::FileFull.to_string(),
            format!(
                "A saved file can hold at most {} bytes, and this would pass that. Undo to make \
                 room, or save the run and start fresh.",
                format::setting(MAX_SHARE_JSON_BYTES)
            )
        );
        let config = OpError::from(ConfigError::AddressBits { got: 4 });
        assert_eq!(
            config.to_string(),
            ConfigError::AddressBits { got: 4 }.to_string()
        );
    }
}
