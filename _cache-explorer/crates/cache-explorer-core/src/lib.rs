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

#![forbid(unsafe_code)]

pub mod limits;

mod cache;
mod config;
mod cpu;
mod engine;
mod event;
mod explain;
mod format;
mod geometry;
#[cfg(feature = "golden")]
pub mod golden;
mod input;
mod isa;
mod machine;
mod memory;
mod op;
mod prng;
mod replacement;
mod session;
mod snapshot;
mod stats;
mod units;

pub use config::{
    presets, validate_config, AllocatePolicy, CacheConfig, Config, ConfigCheck, ConfigControls,
    ConfigError, ConfigErrorView, ConfigField, ConfigGroup, ConfigGroupKind, ConfigNumberField,
    ConfigOption, MemoryInit, Preset, Repair, Replacement, Timing, TimingControl, WordBytes,
    WordBytesError, WritePolicy,
};
pub use event::{Cause, ChangeRef, Counter, EventKind, MicroEvent};
pub use geometry::{
    AccessError, AddressField, BitRange, BitRanges, Decoded, FieldKind, Geometry, Placement,
};
pub use input::{AddressInput, ConfigNumberInput, IssueDraft, WordInput};
pub use isa::{ParseError, ParseResult};
pub use machine::{InvariantViolation, Machine};
pub use memory::StaleInfo;
pub use op::{AccessKind, Op, OpError, ProgramFormat, ProgramSource};
pub use session::{
    Boundary, ConfigChange, EntryKind, EntryOutcome, EntryView, EventRow, Position, Replay,
    ReplayError, Session, StepOutcome, Tense, Timeline,
};
pub use snapshot::{
    BusView, CacheView, ControlAction, Controls, CpuStatus, CpuView, HitMarker, Labels,
    MemoryLineView, MemoryView, MemoryWordView, ProgramLine, ProgramView, RankCell, RankHeader,
    RankRole, RegisterView, RequestStatus, RequestView, SetView, Stale, State, Transfer,
    WayCompare, WayLoc, WayView,
};
pub use stats::{Counters, Figure, Outcome, StatKey, StatRow, Stats, Traffic};
pub use units::{
    Addr, Cycle, EventPos, LineAddr, OpIndex, Register, SetIndex, Tag, WayIndex, Word, WordIndex,
};
