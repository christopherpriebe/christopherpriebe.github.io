#!/bin/sh
# Prints the size of a release build's wasm, raw and gzipped at level 9,
# against a budget in bytes gzipped. Above the warning line it also emits a
# warning annotation, and above the budget an error annotation and a
# non-zero exit. The annotations go to stderr, so that a step piping the
# report line into the job summary copies that line alone. Such a step must
# set pipefail (in GitHub Actions, `shell: bash`): without it the step's
# status is that of the last command in the pipe, and a build over the
# budget passes.
#
# Usage: sh scripts/wasm-size.sh path/to/x_bg.wasm budget warn
# `node scripts/instruments.mjs size` fills all three in from instrument.json.
set -eu

wasm=${1:?usage: sh scripts/wasm-size.sh path/to/x_bg.wasm budget warn}
budget=${2:?usage: sh scripts/wasm-size.sh path/to/x_bg.wasm budget warn}
warn=${3:?usage: sh scripts/wasm-size.sh path/to/x_bg.wasm budget warn}
if [ ! -f "$wasm" ]; then
  echo "wasm-size: $wasm does not exist; build it first" >&2
  exit 1
fi
name=$(basename "$wasm")
raw=$(wc -c < "$wasm" | tr -d ' ')
gzipped=$(gzip -9 -c "$wasm" | wc -c | tr -d ' ')

# A byte count with thousands separators, as the budget is written.
grouped() {
  awk -v n="$1" 'BEGIN {
    s = ""
    while (length(n) > 3) { s = "," substr(n, length(n) - 2) s; n = substr(n, 1, length(n) - 3) }
    print n s
  }'
}

echo "$name: $(grouped "$raw") bytes, $(grouped "$gzipped") bytes gzip -9 (budget $(grouped "$budget"))"
if [ "$gzipped" -gt "$budget" ]; then
  echo "::error::$name is $(grouped "$gzipped") bytes gzipped, over the budget of $(grouped "$budget")" >&2
  exit 1
fi
if [ "$gzipped" -gt "$warn" ]; then
  echo "::warning::$name is $(grouped "$gzipped") bytes gzipped, within $(grouped $((budget - warn))) of the budget of $(grouped "$budget")" >&2
fi
