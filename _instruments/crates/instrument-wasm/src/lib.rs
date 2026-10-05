//! What the WebAssembly facades of this workspace share.
//!
//! A facade marshals values between JavaScript and an instrument's core and
//! does nothing else. The conversions that are the same for every instrument
//! live here: reading an argument as the type it was passed as, with an error
//! of the instrument's own wording, and serializing a list as the type the
//! generated declarations give it.
//!
//! This crate is on the facade side. No pure crate depends on it, so nothing
//! of `wasm-bindgen` reaches a pure crate's default build.

#![forbid(unsafe_code)]

use serde::de::DeserializeOwned;
use serde::Serialize;
use tsify::{Ts, Tsify};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// `value` read as the `T` it was passed as.
///
/// `invalid` builds the error of a value that is not a `T`, from
/// serde-wasm-bindgen's reason, which is empty when it gave none.
///
/// # Errors
///
/// A `JsError` whose message is the `Display` of `invalid`'s error, when
/// `value` does not have `T`'s shape.
#[inline]
pub fn read<T, E>(value: JsValue, invalid: impl FnOnce(String) -> E) -> Result<T, JsError>
where
    T: DeserializeOwned,
    E: std::error::Error,
{
    serde_wasm_bindgen::from_value(value).map_err(|error| {
        let detail = JsValue::from(error)
            .dyn_into::<js_sys::Error>()
            .map(|error| String::from(error.message()))
            .unwrap_or_default();
        JsError::new(&invalid(detail).to_string())
    })
}

/// `value` read as the `T` it was passed as, for an argument typed by tsify.
///
/// # Errors
///
/// As [`read`].
#[inline]
pub fn arg<T, E>(value: &Ts<T>, invalid: impl FnOnce(String) -> E) -> Result<T, JsError>
where
    T: DeserializeOwned + Tsify,
    E: std::error::Error,
{
    read(value.js_value(), invalid)
}

/// Serializes `items` to the JavaScript value a `T` holding them would give,
/// without a copy of them.
///
/// # Caller's obligation
///
/// `T` must serialize as `items` does. Nothing checks it: the value is typed
/// as a `T` whatever it holds, so a `T` that differs gives JavaScript a value
/// whose declared type is wrong.
///
/// # Errors
///
/// A `JsError` carrying serde-wasm-bindgen's message, when `items` do not
/// convert.
#[inline]
pub fn serialize_as_unchecked<T, S>(items: &S) -> Result<Ts<T>, JsError>
where
    T: Tsify,
    S: Serialize + ?Sized,
{
    // Configured as `Tsify::into_js` configures it for a `T`.
    let config = T::SERIALIZATION_CONFIG;
    let serializer = serde_wasm_bindgen::Serializer::new()
        .serialize_missing_as_null(config.missing_as_null)
        .serialize_maps_as_objects(config.hashmap_as_object)
        .serialize_large_number_types_as_bigints(config.large_number_types_as_bigints);
    Ok(Ts::new_unchecked(items.serialize(&serializer)?))
}
