//! What the instruments of this workspace share about a session: the runs of
//! styled text a message reaches the page in, the reader of enums of unit
//! variants, the positions a log is read by, the share-string codec and the
//! room a saved file has left.
//!
//! The crate holds no sentence a student reads; the validation messages of
//! the markup checker are for module authors and are allowed. It depends on
//! no JavaScript or operating-system crate, reads no clock and does no I/O.
//! The optional `ts` feature, enabled only through an instrument's own,
//! derives TypeScript declarations for the types that cross the boundary.

#![forbid(unsafe_code)]

pub mod codec;
pub mod de;
pub mod saved;
pub mod text;
pub mod units;
