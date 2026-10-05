#!/bin/sh
# One-command setup and first build for the instruments.
#
# Run it from anywhere; it works inside _instruments/. Every step is safe to
# rerun. It checks two machine-wide prerequisites and installs neither:
#   1. rustup's proxies first on PATH, so rust-toolchain.toml takes effect;
#   2. the Node version in _instruments/.nvmrc.
# It then installs the pinned Rust toolchain (through rustup), the pinned
# JavaScript tools (into _instruments/node_modules, the npm workspace's) and builds every instrument.

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
want="v$(cat .nvmrc)"
command -v node >/dev/null 2>&1 ||
  fail "node is not on PATH. Install Node ${want#v} (for example 'fnm install ${want#v}') and use it in this folder, then run this again."
have=$(node -v)
[ "$have" = "$want" ] ||
  fail "node is $have, but this project needs $want (_instruments/.nvmrc). Switch to it (for example 'fnm use' inside _instruments), then run this again."

# 4. The pinned JavaScript tools, wasm-pack and wasm-opt among them, all inside _instruments/node_modules.
npm ci

# 5. The first build of every instrument, which also shows that the toolchain works.
npm run build

# 6. What was built: the tool versions, and each instrument's wasm against its budget.
bin="$root/node_modules/.bin"
cargo --version
"$bin/wasm-pack" --version
"$bin/wasm-opt" --version
npm run size
