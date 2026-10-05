// Fails if a wasm file contains an absolute path of the machine that built it (a home directory
// or a rustup toolchain), which would make the committed wasm differ between machines. Run with
// no argument on the committed wasm, as `npm run check` in web/ does, or with a path.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const file = process.argv[2] ?? resolve(import.meta.dirname, '..', '..', '..', 'assets', 'cache-explorer', 'cache_explorer_bg.wasm');
const text = readFileSync(file).toString('latin1');
const leaks = [...new Set(text.match(/(?:\/Users|\/home|[A-Za-z]:\\Users)[\x21-\x7e]*|[\x21-\x7e]*\.rustup[\x21-\x7e]*/g) ?? [])];
if (leaks.length > 0) {
  console.error(`check-wasm-paths: ${file} embeds local paths (${leaks.slice(0, 3).join(', ')}${leaks.length > 3 ? ', ...' : ''}); remap them in scripts/build-wasm.mjs`);
  process.exit(1);
}
