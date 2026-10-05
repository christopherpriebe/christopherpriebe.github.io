#!/bin/sh
# One-command setup and first build for the Interactive Cache Explorer.
#
# Run it from anywhere; it works inside _instruments/cache-explorer/. Every step is safe to
# rerun. It checks two machine-wide prerequisites and installs neither:
#   1. rustup's proxies first on PATH, so rust-toolchain.toml takes effect;
#   2. the Node version in web/.nvmrc.
# It then installs the pinned Rust toolchain (through rustup), the pinned
# JavaScript tools (into web/node_modules) and runs the first build.

set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

fail() {
  printf 'setup: %s\n' "$1" >&2
  exit 1
}

# 1. rustup's proxies, not another Rust, must be the cargo and rustc on PATH.
command -v rustup >/dev/null 2>&1 ||
  fail "rustup is not installed. Install it (for example 'brew install rustup'), then run this again."
case $(rustc --print sysroot 2>/dev/null || true) in
  *rustup*) ;;
  *) fail "the rustc on PATH is not rustup's, so rust-toolchain.toml would be ignored. Put rustup's proxies first on PATH (on Homebrew: export PATH=\"\$(brew --prefix rustup)/bin:\$PATH\", in your shell profile) and remove any other Rust, then run this again." ;;
esac

# 2. The pinned toolchain: rustfmt, clippy and the wasm32 target come from the toolchain file.
rustup toolchain install --no-self-update

# 3. The exact Node version of web/.nvmrc.
want="v$(cat web/.nvmrc)"
command -v node >/dev/null 2>&1 ||
  fail "node is not on PATH. Install Node ${want#v} (for example 'fnm install ${want#v}') and use it in this folder, then run this again."
have=$(node -v)
[ "$have" = "$want" ] ||
  fail "node is $have, but this project needs $want (web/.nvmrc). Switch to it (for example 'fnm use' inside _instruments/cache-explorer/web), then run this again."

# 4. The pinned JavaScript tools, wasm-pack and wasm-opt among them, all inside web/node_modules.
npm ci --prefix web

# 5. The first build, which also shows that the toolchain works.
npm --prefix web run build

# 6. What was built.
bin="$root/web/node_modules/.bin"
cargo --version
"$bin/wasm-pack" --version
"$bin/wasm-opt" --version
wasm="$root/../../assets/cache-explorer/cache_explorer_bg.wasm"
printf 'wasm: %s bytes, %s bytes gzipped\n' "$(wc -c <"$wasm" | tr -d ' ')" "$(gzip -9 -c "$wasm" | wc -c | tr -d ' ')"
