// What the shared scripts know about the instruments of this workspace. Each instrument is a folder
// of _instruments/ with an instrument.json (its name, crates, wasm file name, asset folder, size
// budget and signal tokens) and a web/ package that holds its npm scripts.
//
// As a module it exports `instrumentNames` and `loadInstrument`. As a command, from any directory
// (the root package.json's scripts call it):
//   node scripts/instruments.mjs list [--json]   the instrument names
//   node scripts/instruments.mjs run <task> [name]   an npm script of each instrument's web/;
//     the one exception is `test`, which runs test:glue and test:wasm:node, not web/'s own `test`
//   node scripts/instruments.mjs size [name]   the wasm's size against its budget
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';

/** The npm and Cargo workspace, _instruments/. */
export const workspace = resolve(import.meta.dirname, '..');
/** The repository root, where the site's `assets/` and `_pages/` are. */
export const repository = resolve(workspace, '..');

/** The names of the instruments, sorted. */
export function instrumentNames() {
  return readdirSync(workspace, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && existsSync(resolve(workspace, entry.name, 'instrument.json')))
    .map((entry) => entry.name)
    .sort();
}

/** An instrument's manifest, with the absolute paths its scripts need. */
export function loadInstrument(name) {
  const known = instrumentNames();
  if (!known.includes(name)) {
    throw new Error(`instruments: ${name === undefined ? 'no instrument given' : `no instrument ${name}`} (the instruments are: ${known.join(', ')})`);
  }
  const dir = resolve(workspace, name);
  const manifest = JSON.parse(readFileSync(resolve(dir, 'instrument.json'), 'utf8'));
  const assets = resolve(repository, manifest.assets);
  return {
    ...manifest,
    dir,
    web: resolve(dir, 'web'),
    crates: resolve(workspace, 'crates'),
    assets,
    wasm: resolve(assets, `${manifest.outName}_bg.wasm`),
  };
}

/** The npm scripts of an instrument's web/ that a task runs, in order. */
const tasks = { test: ['test:glue', 'test:wasm:node'] };

function run(command, args) {
  const result = spawnSync(command, args, { stdio: 'inherit', cwd: workspace });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

if (import.meta.main) {
  const [command, ...rest] = process.argv.slice(2);
  const selected = (name) => (name === undefined ? instrumentNames() : [loadInstrument(name).name]);
  if (command === 'list') {
    const names = instrumentNames();
    console.log(rest.includes('--json') ? JSON.stringify(names) : names.join('\n'));
  } else if (command === 'run' && rest[0] !== undefined) {
    for (const name of selected(rest[1])) {
      for (const script of Object.hasOwn(tasks, rest[0]) ? tasks[rest[0]] : [rest[0]]) run('npm', ['run', script, '--workspace', `${name}/web`]);
    }
  } else if (command === 'size') {
    for (const name of selected(rest[0])) {
      const { wasm, wasmBudget, wasmWarn } = loadInstrument(name);
      run('sh', [resolve(import.meta.dirname, 'wasm-size.sh'), wasm, String(wasmBudget), String(wasmWarn)]);
    }
  } else {
    console.error('usage: node scripts/instruments.mjs list [--json] | run <task> [name] | size [name]');
    process.exit(2);
  }
}
