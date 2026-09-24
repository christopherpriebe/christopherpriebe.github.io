#!/bin/sh
# Fails if cache-explorer-core's default build depends on a JavaScript or
# operating-system crate. Only the optional "ts" feature, which the facade
# enables, may bring in tsify, wasm-bindgen and what they pull; the default
# build, which every core test uses, must have none of them.
#
# `cargo tree -p cache-explorer-core` resolves the core alone, so the facade's
# "ts" feature is not unified into it.
set -eu
cd "$(dirname "$0")/.."

tree=$(cargo tree -p cache-explorer-core -e normal --prefix none --locked)
if printf '%s\n' "$tree" | grep -Eq '^(wasm-bindgen|js-sys|serde-wasm-bindgen|tsify|getrandom|web-sys) '; then
  echo "cache-explorer-core pulls a JS or OS dependency in its default build" >&2; exit 1
fi
