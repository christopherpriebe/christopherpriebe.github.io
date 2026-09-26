//! Text written in runs of plain, code and emphasised words, as the page
//! renders the core's structured messages: code in mono, the way the
//! malformed-link message shows the offending field.

mod text;

pub use text::{TextSegment, TextStyle};

pub(crate) use text::{code_spans, Segments};
