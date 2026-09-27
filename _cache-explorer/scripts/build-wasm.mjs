// Builds the facade with wasm-pack, then optimises it with the pinned binaryen.
// Run through `npm run build:wasm` in web/, which puts web/node_modules/.bin,
// where both tools are, first on PATH.
import { execFileSync } from 'node:child_process';
import { homedir } from 'node:os';
import { resolve } from 'node:path';

const workspace = resolve(import.meta.dirname, '..'); // _cache-explorer
const web = resolve(workspace, 'web');
const cargoHome = process.env.CARGO_HOME ?? resolve(homedir(), '.cargo');
const env = {
  ...process.env,
  // Absolute paths end up in panic locations; remap them so the committed wasm is the same on every machine.
  RUSTFLAGS: `--remap-path-prefix=${cargoHome}=/cargo --remap-path-prefix=${workspace}=/src`,
};
const run = (cmd, args) => execFileSync(cmd, args, { cwd: web, env, stdio: 'inherit' });

// --out-dir is relative to the crate, so ../../web/pkg is _cache-explorer/web/pkg.
run('wasm-pack', ['build', '../crates/cache-explorer-wasm', '--target', 'web', '--release',
                  '--out-dir', '../../web/pkg', '--out-name', 'cache_explorer', '--', '--locked']);
run('wasm-opt', ['pkg/cache_explorer_bg.wasm', '-Oz', '--strip-debug', '--strip-producers',
                 '-o', 'pkg/cache_explorer_bg.wasm']);
