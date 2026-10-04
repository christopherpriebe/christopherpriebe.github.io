//! The model behind the Interactive Cache Explorer, a teaching tool in which the
//! student acts as the CPU: one L1 cache backed by main memory, driven by loads
//! and stores issued by hand, from a tiny assembly language or from a trace.
//!
//! Every decision the tool makes lives in this crate: the machine model, the
//! append-only log of ops and the micro-events each op yields, the session and
//! its cursors, edits, share strings and saved files, and guided modules. So
//! does every sentence built from state. The WebAssembly facade and the
//! browser UI only marshal values and render them.
//!
//! The default build is pure Rust with no JavaScript or operating-system
//! dependencies; it reads no clock and does no I/O, so every behaviour is
//! tested natively with `cargo test`. The optional `ts` feature, enabled only
//! by `cache-explorer-wasm`, derives TypeScript declarations for the types that
//! cross the boundary. The optional `golden` feature, enabled only by tests,
//! adds the `golden` module: the golden fixtures and the canonical text of
//! their events.
//!
//! # Examples
//!
//! A student issues a load, the page reads the counters, and the run is
//! shared as a link that opens to the same log:
//!
//! ```
//! use cache_explorer_core::{AccessKind, Config, EventKind, Op, Session};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let config = Config::default();
//! let addr = config
//!     .validate()?
//!     .addr(0x0A4)
//!     .ok_or("0x0A4 does not fit the address width")?;
//! let mut session = Session::with_config(config)?;
//!
//! let events = session.apply(Op::Issue {
//!     kind: AccessKind::Read,
//!     addr,
//!     value: None,
//! })?;
//! assert_eq!(events.first().map(|event| event.kind), Some(EventKind::Decode));
//! assert_eq!(events.last().map(|event| event.kind), Some(EventKind::Complete));
//!
//! let counters = session.stats().counters;
//! assert_eq!((counters.accesses, counters.misses), (1, 1));
//! assert_eq!(session.cursor(), 1);
//!
//! let link = session.to_share_string()?;
//! let reopened = Session::from_share_string(&link)?;
//! assert_eq!(reopened.ops(), session.ops());
//! assert_eq!(reopened.events(), session.events());
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

pub mod limits;

mod cache;
mod config;
mod cpu;
mod de;
mod engine;
mod error;
mod event;
mod explain;
mod format;
mod geometry;
#[cfg(feature = "golden")]
pub mod golden;
mod info;
mod input;
mod isa;
mod machine;
mod memory;
mod module;
mod op;
mod prng;
mod replacement;
mod request;
mod session;
mod share;
mod snapshot;
mod stats;
mod text;
mod units;

pub use config::{
    presets, validate_config, AllocatePolicy, CacheConfig, Config, ConfigCheck, ConfigControls,
    ConfigError, ConfigErrorView, ConfigField, ConfigGroup, ConfigGroupKind, ConfigNumberField,
    ConfigOption, MemoryInit, Preset, Repair, Replacement, Timing, TimingControl, WordBytes,
    WordBytesError, WritePolicy,
};
pub use engine::{
    Consequence, EditDraft, EditOutcome, EditPlan, EditRefusal, FieldChange, FieldInput,
    MemoryEditDraft, RefusalReason, Suggestion, Tone, WayForm, WordMark,
};
pub use error::{ArgKind, ArgumentError};
pub use event::{Cause, ChangeRef, Counter, EventKind, MicroEvent, MicroEvents};
pub use geometry::{
    AccessError, AddressField, BitRange, BitRanges, Decoded, FieldKind, Geometry, Placement,
};
pub use info::{build_info, BuildInfo};
pub use input::{AddressInput, ConfigNumberInput, IssueDraft, WordInput};
pub use isa::{ParseError, ParseResult};
pub use machine::{InvariantViolation, Machine};
pub use memory::StaleInfo;
pub use module::{
    parse_module_index, AccessMatch, AddrLoc, AllPredicate, AnyPredicate, Callout, CalloutArea,
    CellRef, CmpOp, ConfigMatch, ConfigPredicate, CounterPredicate, EvalContext, EventMatch,
    EventPredicate, Feedback, FeedbackTone, LastAccessPredicate, LinePredicate, MemoryPredicate,
    Module, ModuleAction, ModuleActionKind, ModuleCard, ModuleError, ModuleIndexView,
    ModuleProblem, ModuleRun, ModuleStepKind, ModuleView, NotPredicate, OptionState, OptionView,
    Predicate, RegisterPredicate, Step, WayPredicate,
};
pub use op::{
    AccessKind, EditPatch, EditTarget, Op, OpError, ProgramFormat, ProgramSource, WayPatch,
};
pub use session::{
    Boundary, ConfigChange, EntryKind, EntryOutcome, EntryView, EntryViews, EventRow, Gate,
    Position, Replay, ReplayError, Session, StepOutcome, Tense, Timeline,
};
pub use share::{
    run_file_too_large, trace_file_too_large, EncodingError, LinkTooLong, LoadError, LoadFailure,
    LoadOption, LoadProgress, ShareAt, ShareDoc, ShareLoad, ShareSummary, TooLarge, SCHEMA_VERSION,
};
pub use snapshot::{
    BusView, CacheView, ControlAction, Controls, CpuStatus, CpuView, HitMarker, Labels,
    MemoryLineView, MemoryLines, MemoryView, MemoryWordView, ProgramLine, ProgramView, RankCell,
    RankHeader, RankRole, RegisterView, RequestStatus, RequestView, SetView, Stale, State,
    Transfer, WayCompare, WayLoc, WayView,
};
pub use stats::{Counters, Figure, Outcome, StatKey, StatRow, Stats, Traffic};
pub use text::{Markdown, MarkupError, TextSegment, TextStyle};
pub use units::{
    Addr, Cycle, EventPos, LineAddr, OpIndex, Register, SetIndex, Tag, WayIndex, Word, WordIndex,
};
