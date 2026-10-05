//! The two positions a session's log is read by: a place among its
//! micro-events and an index among its ops.
//!
//! Each serializes as its bare number and has a public `get`. The raw
//! constructor, `new_unchecked`, is public but hidden from the
//! documentation: a session wraps only a number it has checked against its
//! log, and a value deserialized from JavaScript or JSON is unchecked until
//! it is validated against the log in force.

use serde::{Deserialize, Serialize};

/// A position in the session's log of micro-events: 0 is the state before the
/// first event, and k the state after the k-th event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct EventPos(u32);

/// An index into the session's ops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "ts", derive(tsify::Tsify))]
#[serde(transparent)]
pub struct OpIndex(u32);

impl EventPos {
    /// Wraps a position the session has checked against its log.
    #[doc(hidden)]
    pub const fn new_unchecked(value: u32) -> Self {
        Self(value)
    }

    /// The position as a number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl OpIndex {
    /// Wraps an op index the session has checked against its log.
    #[doc(hidden)]
    pub const fn new_unchecked(value: u32) -> Self {
        Self(value)
    }

    /// The op index as a number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::disallowed_macros)] // assertions are how tests fail

    use super::*;

    #[test]
    fn get_returns_the_wrapped_number() {
        assert_eq!(EventPos::new_unchecked(7).get(), 7);
        assert_eq!(OpIndex::new_unchecked(8).get(), 8);
    }

    #[test]
    fn positions_order_by_their_numbers() {
        assert!(EventPos::new_unchecked(2) < EventPos::new_unchecked(10));
        assert!(OpIndex::new_unchecked(0) < OpIndex::new_unchecked(1));
    }

    #[test]
    fn positions_serialize_as_bare_numbers() {
        let json = serde_json::to_string(&(EventPos::new_unchecked(9), OpIndex::new_unchecked(4)))
            .unwrap();
        assert_eq!(json, "[9,4]");
    }

    #[test]
    fn an_event_position_deserializes_from_a_bare_number() {
        assert_eq!(
            serde_json::from_str::<EventPos>("9").unwrap(),
            EventPos::new_unchecked(9)
        );
        assert!(serde_json::from_str::<EventPos>("-1").is_err());
        assert!(serde_json::from_str::<EventPos>("4294967296").is_err());
    }
}
