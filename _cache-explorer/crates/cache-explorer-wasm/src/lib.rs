//! The WebAssembly facade of the Interactive Cache Explorer.
//!
//! This crate marshals values between JavaScript and `cache_explorer_core` and
//! does nothing else: each export is a call into the core plus the conversion
//! of its arguments and result, and every `Err` becomes a `JsError` carrying
//! the core's message. The machine model, its wording and its number
//! formatting all live in the core.

#![forbid(unsafe_code)]
