//! What the student asks the machine to do.

use serde::{Deserialize, Serialize};

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
}
