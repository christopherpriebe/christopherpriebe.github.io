// Builds an instrument's facade with wasm-pack, then optimises it with the pinned binaryen.
// Usage: node build-wasm.mjs <instrument>. Run through `npm run build:wasm` in its web/, which
// puts _instruments/node_modules/.bin, where both tools are, first on PATH.
import { execFileSync } from 'node:child_process';
import { homedir } from 'node:os';
import { relative, resolve } from 'node:path';
import { loadInstrument, workspace } from './instruments.mjs';

const { web, crates, facade, outName } = loadInstrument(process.argv[2]);
const cargoHome = process.env.CARGO_HOME ?? resolve(homedir(), '.cargo');
// Run in the workspace, so that rust-toolchain.toml picks the toolchain, as it does for cargo.
const rustc = (...args) => execFileSync('rustc', args, { cwd: workspace, encoding: 'utf8' });
const sysroot = rustc('--print', 'sysroot').trim();
const commitHash = /^commit-hash: (\w+)$/m.exec(rustc('-vV'))?.[1];
if (!commitHash) throw new Error('build-wasm: no commit-hash line in `rustc -vV`');
const env = {
  ...process.env,
  // Absolute paths end up in panic locations; remap them so the committed wasm is the same on every machine.
  // The sources of the workspace's own crates are already relative to the workspace root, so the
  // workspace remap is a safeguard for any absolute path under it.
  // The prebuilt std carries /rustc/<commit>/ paths, but when the rust-src component is installed
  // rustc swaps them for the local copy under the sysroot, so that is remapped back too. The build
  // is then the same with and without rust-src.
  RUSTFLAGS: [
    `--remap-path-prefix=${cargoHome}=/cargo`,
    `--remap-path-prefix=${workspace}=/src`,
    `--remap-path-prefix=${sysroot}/lib/rustlib/src/rust=/rustc/${commitHash}`,
  ].join(' '),
};
const run = (cmd, args) => execFileSync(cmd, args, { cwd: web, env, stdio: 'inherit' });

// --out-dir is relative to the crate.
const crate = resolve(crates, facade);
run('wasm-pack', ['build', relative(web, crate), '--target', 'web', '--release',
                  '--out-dir', relative(crate, resolve(web, 'pkg')), '--out-name', outName, '--', '--locked']);
// --converge repeats -Oz while the module keeps shrinking, which doubles the time wasm-opt takes.
// --low-memory-unused lets wasm-opt fold a constant below 1,024 that is added to an address into the
// load's or store's offset. The two differ only when the addition wraps past 2^32, and no address
// the Rust code computes does.
// --reorder-functions-by-name sorts the functions by their mangled names, which puts the
// instantiations of one generic function side by side, where gzip finds their common bytes. It only
// moves functions; it runs before --strip-debug removes the names.
run('wasm-opt', [`pkg/${outName}_bg.wasm`, '-Oz', '--converge', '--low-memory-unused',
                 '--reorder-functions-by-name', '--strip-debug', '--strip-producers',
                 '-o', `pkg/${outName}_bg.wasm`]);
