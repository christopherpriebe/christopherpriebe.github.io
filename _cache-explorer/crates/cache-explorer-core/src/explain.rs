//! The wording of the log: every line built from machine state is written
//! here, so the tool's voice has one source. Error messages are the
//! exception; each is the `Display` of its error type, next to that type.

use crate::machine::InvariantViolation;

/// The line of the `Error` event that ends an op when an invariant no longer
/// holds.
pub(crate) fn internal_error(violation: &InvariantViolation) -> String {
    format!(
        "Internal error: {violation} no longer holds. The run stopped here; please report it \
         with a share link."
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;
    use crate::units::SetIndex;

    #[test]
    fn an_internal_error_names_the_invariant() {
        let violation = InvariantViolation::DuplicateTag {
            set: SetIndex::new_unchecked(2),
        };
        assert_eq!(
            internal_error(&violation),
            "Internal error: invariant I3 (no two valid ways in set 2 hold the same tag) no \
             longer holds. The run stopped here; please report it with a share link."
        );
    }
}
