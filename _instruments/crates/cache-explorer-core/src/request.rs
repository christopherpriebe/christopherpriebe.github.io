//! A load or store as the engine executes it, and who made it.
//!
//! The machine holds the request of the access in flight; the state views
//! and the timeline read who made it.

use crate::op::AccessKind;
use crate::units::{Addr, Register};

/// A load or store the engine executes. Its address has been checked: it
/// fits the address width and is a multiple of the word size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    /// A load of the word at `addr` into register `into`.
    Load {
        /// The word's address.
        addr: Addr,
        /// The register the word goes into.
        into: Register,
        /// Who made the request.
        origin: Origin,
    },
    /// A store of `value` to the word at `addr`. Only the value's low
    /// `8 × word_bytes` bits are stored.
    Store {
        /// The word's address.
        addr: Addr,
        /// The value, as wide as a register.
        value: u64,
        /// The register the value came from, if it came from one: the store's
        /// result line names it when the word cannot hold the value.
        from: Option<Register>,
        /// Who made the request.
        origin: Origin,
    },
}

/// Who made a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The student, by hand. The access touches neither the pc nor the
    /// budget; a load into a register the loaded program may be using says
    /// so.
    Hand,
    /// The program's instruction at the pc, which retires at the access's
    /// last event.
    Program,
}

impl Request {
    /// The address the request reads or writes.
    pub(crate) fn addr(self) -> Addr {
        match self {
            Request::Load { addr, .. } | Request::Store { addr, .. } => addr,
        }
    }

    /// Who made the request.
    pub(crate) fn origin(self) -> Origin {
        match self {
            Request::Load { origin, .. } | Request::Store { origin, .. } => origin,
        }
    }

    /// Whether the request is a load or a store.
    pub(crate) fn kind(self) -> AccessKind {
        match self {
            Request::Load { .. } => AccessKind::Read,
            Request::Store { .. } => AccessKind::Write,
        }
    }
}
