#!/bin/sh
# Fails if the default build of a pure crate depends on a JavaScript or
# operating-system crate. Only an optional "ts" feature, which a facade
# enables, may bring in tsify, wasm-bindgen and what they pull; the default
# build, which every test of the crate uses, must have none of them.
#
# `cargo tree -p <crate>` resolves the crate alone, so a facade's "ts" feature
# is not unified into it. `--target all` keeps dependencies that apply only on
# some platforms, such as a crate's wasm32-only ones; without it cargo tree
# shows only the host's, and the crate also builds for wasm32.
#
# Usage: sh scripts/check-deps.sh [<crate>...]
# With no crate it checks every crate of crates/ whose name does not end in -wasm.
set -eu
cd "$(dirname "$0")/.."

if [ "$#" -eq 0 ]; then
  for dir in crates/*/; do
    name=$(basename "$dir")
    case "$name" in *-wasm) ;; *) set -- "$@" "$name" ;; esac
  done
fi
for crate in "$@"; do
  tree=$(cargo tree -p "$crate" -e normal --target all --prefix none --locked)
  if printf '%s\n' "$tree" | grep -Eq '^(wasm-bindgen|js-sys|serde-wasm-bindgen|tsify|getrandom|web-sys) '; then
    echo "$crate pulls a JS or OS dependency in its default build" >&2; exit 1
  fi
done
