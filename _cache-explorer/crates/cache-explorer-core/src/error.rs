//! The error of a value from JavaScript that is not the argument it was
//! passed as.
//!
//! Every other error belongs to an area of the model and is that area's enum,
//! whose `Display` is the message: [`ConfigError`](crate::ConfigError),
//! [`OpError`](crate::OpError), [`LoadError`](crate::LoadError) and the rest.
//! This one belongs to the boundary: the WebAssembly facade reads each
//! argument before the core sees it, and when the value does not have the
//! argument's shape, it throws this error's message, so the facade formats
//! nothing itself.

use std::fmt;

use thiserror::Error;

/// A value from JavaScript that could not be read as the argument it was
/// passed as.
///
/// `Display` is the message thrown, the argument's type then the
/// deserializer's detail, passed through as it is worded: ``invalid Op:
/// unknown variant `stpe`, expected one of `config`, `issue`, …``.
///
/// The page never shows it: every argument the page sends was built by the
/// core, so the message reaches only developers and tests.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("invalid {kind}{}", detail_suffix(detail))]
pub struct ArgumentError {
    /// The argument's type.
    pub kind: ArgKind,
    /// Why the value is not one, as the deserializer words it; empty when it
    /// gave no reason.
    pub detail: String,
}

impl ArgumentError {
    /// The error of a value that is not a `kind`, for the reason `detail`.
    pub fn new(kind: ArgKind, detail: String) -> Self {
        ArgumentError { kind, detail }
    }
}

/// The type an argument from JavaScript is read as. `Display` is the type's
/// name in the generated TypeScript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgKind {
    /// An `AccessKind`: `"read"` or `"write"`.
    AccessKind,
    /// An `Addr`: a byte address.
    Addr,
    /// A `Config`.
    Config,
    /// A `ConfigNumberField`: `"hitCycles"` or `"missCycles"`.
    ConfigNumberField,
    /// An `EditPatch`.
    EditPatch,
    /// An `EditTarget`.
    EditTarget,
    /// An `EventPos`: a position of the event cursor.
    EventPos,
    /// A `LineAddr[]`: addresses of memory lines.
    LineAddrs,
    /// An `Op`.
    Op,
    /// A `Predicate`.
    Predicate,
    /// A `ProgramFormat`: `"asm"` or `"trace"`.
    ProgramFormat,
    /// A `ProgramSource`: a program's text and its format.
    ProgramSource,
    /// A `SetIndex`: a set of the cache.
    SetIndex,
    /// A `WayForm`: the way editor's form.
    WayForm,
    /// A `WayIndex`: a way of a set.
    WayIndex,
}

impl fmt::Display for ArgKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ArgKind::AccessKind => "AccessKind",
            ArgKind::Addr => "Addr",
            ArgKind::Config => "Config",
            ArgKind::ConfigNumberField => "ConfigNumberField",
            ArgKind::EditPatch => "EditPatch",
            ArgKind::EditTarget => "EditTarget",
            ArgKind::EventPos => "EventPos",
            ArgKind::LineAddrs => "LineAddr[]",
            ArgKind::Op => "Op",
            ArgKind::Predicate => "Predicate",
            ArgKind::ProgramFormat => "ProgramFormat",
            ArgKind::ProgramSource => "ProgramSource",
            ArgKind::SetIndex => "SetIndex",
            ArgKind::WayForm => "WayForm",
            ArgKind::WayIndex => "WayIndex",
        })
    }
}

/// `detail` after a colon, or nothing when it is empty.
fn detail_suffix(detail: &str) -> String {
    if detail.is_empty() {
        String::new()
    } else {
        format!(": {detail}")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    #[test]
    fn the_message_names_the_type_then_passes_the_detail_through() {
        let error = ArgumentError::new(
            ArgKind::Op,
            "unknown variant `stpe`, expected one of `config`, `issue`".to_owned(),
        );
        assert_eq!(
            error.to_string(),
            "invalid Op: unknown variant `stpe`, expected one of `config`, `issue`"
        );
    }

    #[test]
    fn a_missing_detail_leaves_the_type_alone() {
        let error = ArgumentError::new(ArgKind::Config, String::new());
        assert_eq!(error.to_string(), "invalid Config");
    }

    #[test]
    fn every_kind_is_named_as_its_typescript_type() {
        let names = [
            (ArgKind::AccessKind, "AccessKind"),
            (ArgKind::Addr, "Addr"),
            (ArgKind::Config, "Config"),
            (ArgKind::ConfigNumberField, "ConfigNumberField"),
            (ArgKind::EditPatch, "EditPatch"),
            (ArgKind::EditTarget, "EditTarget"),
            (ArgKind::EventPos, "EventPos"),
            (ArgKind::LineAddrs, "LineAddr[]"),
            (ArgKind::Op, "Op"),
            (ArgKind::Predicate, "Predicate"),
            (ArgKind::ProgramFormat, "ProgramFormat"),
            (ArgKind::ProgramSource, "ProgramSource"),
            (ArgKind::SetIndex, "SetIndex"),
            (ArgKind::WayForm, "WayForm"),
            (ArgKind::WayIndex, "WayIndex"),
        ];
        for (kind, name) in names {
            assert_eq!(kind.to_string(), name);
        }
    }
}
