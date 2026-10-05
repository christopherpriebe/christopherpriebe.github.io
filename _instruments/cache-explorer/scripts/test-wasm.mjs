// Runs the facade's wasm-bindgen-test suite in headless Chrome, and fails unless every suite ran tests.
// Run through `npm run test:wasm` in web/, which puts _instruments/node_modules/.bin, where wasm-pack is,
// first on PATH.
import { spawnSync } from 'node:child_process';

// Browser mode without run_in_browser, so `test:wasm:node` keeps working. Without the variable,
// wasm-pack's browser run finds a suite configured for Node, prints "so skipping" and exits 0.
const env = { ...process.env, WASM_BINDGEN_USE_BROWSER: '1' };
const r = spawnSync('wasm-pack', ['test', '--headless', '--chrome', '../../crates/cache-explorer-wasm'],
                    { env, encoding: 'utf8', stdio: ['inherit', 'pipe', 'pipe'] });
process.stdout.write(r.stdout ?? '');
process.stderr.write(r.stderr ?? '');
const out = `${r.stdout ?? ''}${r.stderr ?? ''}`;
const results = [...out.matchAll(/test result: (\w+)\. (\d+) passed/g)];
const skipped = /so skipping/.test(out);   // the runner's message when a suite is not configured for this mode
if (r.status !== 0 || skipped || results.length === 0 || results.some(([, ok, n]) => ok !== 'ok' || n === '0')) {
  console.error('test:wasm: the facade suite failed, was skipped, or ran no tests');
  process.exit(1);
}
