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
//! cross the boundary.

#![forbid(unsafe_code)]

pub mod limits;

mod cache;
mod config;
mod format;
mod geometry;
mod input;
mod machine;
mod prng;
mod replacement;
mod units;

pub use config::{
    presets, AllocatePolicy, CacheConfig, Config, ConfigError, ConfigNumberField, MemoryInit,
    Preset, Repair, Replacement, Timing, WordBytes, WordBytesError, WritePolicy,
};
pub use geometry::{
    AccessError, AddressField, BitRange, BitRanges, Decoded, FieldKind, Geometry, Placement,
};
pub use input::{AddressInput, WordInput};
pub use machine::{InvariantViolation, Machine};
pub use units::{
    Addr, Cycle, EventPos, LineAddr, OpIndex, Register, SetIndex, Tag, WayIndex, Word, WordIndex,
};
