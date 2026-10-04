# Interactive Cache Explorer

A teaching tool for one L1 cache. You are the CPU: issue loads and stores by hand, or run a program, and watch each access resolve, step by step. It is published at `/cache-explorer/`.

The cache logic is a Rust crate compiled to WebAssembly, and the page is plain TypeScript on top of it, with no UI framework. The page holds no cache logic: everything it shows, wording included, comes from the core.

```text
crates/cache-explorer-core   the cache, the access engine, sessions, modules, share links (no JS or OS dependencies)
crates/cache-explorer-wasm   the wasm-bindgen facade over the core, and its TypeScript declarations
web/                         the page: Vite, TypeScript, Vitest
modules/                     the guided modules (JSON), and their index
scripts/                     setup, build and check scripts
docs/                        AUTHORING.md, and cache_explorer.d.ts, the committed TypeScript surface
```

This folder is excluded from Jekyll (`_cache-explorer/` in `exclude:` of `_config.yml`), so none of it is published as it stands. What the site serves is the built output in `assets/cache-explorer/`.

## Setup

Two things have to be true first, because both are choices about your whole machine, so setup checks them and does not change them:

1. **rustup's proxies come first on `PATH`.** `rust-toolchain.toml` pins the Rust version, and only rustup's `cargo` and `rustc` read it. Another Rust ahead of it (Homebrew's `rust`, for instance) ignores the file and has no wasm32 standard library. With Homebrew's keg-only rustup, put `$(brew --prefix rustup)/bin` first on `PATH` in your shell profile and remove the other Rust.
2. **Node is the version in `web/.nvmrc`** (24.21.0). `package.json` pins it exactly and `npm ci` refuses any other. A version manager such as fnm or nvm that reads `.nvmrc` does this for you.

Then, from the repository root:

```sh
_cache-explorer/scripts/setup.sh
```

It installs the pinned Rust toolchain through rustup, the pinned JavaScript tools into `web/node_modules` (wasm-pack and wasm-opt among them, so nothing else is installed globally), and runs the first build. Each step is safe to rerun. The first build also has wasm-pack fetch the matching `wasm-bindgen` command-line tool and compile it, which takes about a minute, once.

## Building

From `_cache-explorer/web/`:

```sh
npm run dev          # the development server; the page is at /, scenarios at /dev/scenarios.html
npm run build        # the facade to wasm and optimised, a type check, then Vite
npm run build:wasm   # only the first part
```

`npm run build` writes `assets/cache-explorer/`: `cache-explorer.js`, `cache-explorer.css`, `cache_explorer_bg.wasm` and `modules/`. The build is meant to give the same bytes on any machine: the Rust version, the dependencies and the tools are all pinned, and `scripts/build-wasm.mjs` maps local paths out of the wasm.

`web/pkg/` is the wasm-pack output, which the build reads and `tsc` checks against. It is not committed.

## Testing

From `_cache-explorer/`, with the toolchain from setup:

```sh
cargo fmt --all --check
sh scripts/check-core-deps.sh
cargo clippy -p cache-explorer-core --all-targets --locked -- -D warnings
cargo clippy -p cache-explorer-wasm --target wasm32-unknown-unknown --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

and from `_cache-explorer/web/`, after `npm run build:wasm`:

```sh
npm run check            # types, lint, format, the checks in scripts/, and Vitest
npm run test:glue        # the generated wasm glue, on the release build
npm run test:wasm:node   # the facade's tests, under Node
npm run test:wasm        # the same tests in headless Chrome (needs a ChromeDriver that matches Chrome)
npm run size             # the wasm's gzipped size against its budget
```

`.github/workflows/cache-explorer.yml` runs all of this on pushes that touch this folder or `assets/cache-explorer/` and on every pull request (it skips the work when neither is touched), and its `status` job is the one to require on `master`.

The core's tests include every guided module: each file in `modules/` is parsed and then run to its end through its own Show me path. See [docs/AUTHORING.md](docs/AUTHORING.md) for the module format and how to add one.

### Declarations

The core's types reach TypeScript through generated declarations. `docs/cache_explorer.d.ts` is a committed copy of what the build writes to `web/pkg/cache_explorer.d.ts`, and CI fails if they differ. When you change a type in the core or the facade, copy the new file over after building, from `_cache-explorer/`:

```sh
cp web/pkg/cache_explorer.d.ts docs/cache_explorer.d.ts
```

## Embedding in a Jekyll page

The page is `_pages/cache-explorer.md`, which uses the `tool` layout. To put the tool on another page, include a mount point and the script, giving the mount point the path of the module index:

```liquid
<div id="cache-explorer" data-modules="{{ '/assets/cache-explorer/modules/index.json' | relative_url }}"></div>
<script type="module" src="{{ '/assets/cache-explorer/cache-explorer.js' | relative_url }}?v={{ site.time | date: '%s' }}"></script>
```

- `relative_url` keeps both paths right under a `baseurl`. The script resolves each module's file relative to the index's address.
- The script adds its own stylesheet, so the page links none.
- The tool reads these custom properties from the page, all named `--site-*`, and falls back to its own values for any that are missing: the fonts, colours and spacing of the site. Its own properties are all `--cache-explorer-*`.
- The page needs JavaScript and WebAssembly. The `tool` layout shows a `<noscript>` line.

## Committed assets

`assets/cache-explorer/` is committed, so the site builds with `jekyll build` and the deploy workflow without Rust or Node. The rule that goes with it: **any change under `_cache-explorer/` that changes the built output, which includes every module and every change to the core or the page, is committed together with a fresh build.** CI rebuilds from source and fails if the result differs from what is committed.

So the working order is: change the source, run `npm run build` in `web/`, check `git status` shows the changed assets, and commit them with the change. Do not edit anything in `assets/cache-explorer/` by hand.

## The design

The look of the tool was drawn in a separate design system. That export is kept outside the repository, and nothing here depends on it: the tokens, markup and styles the page uses are in `web/src/styles/`.
