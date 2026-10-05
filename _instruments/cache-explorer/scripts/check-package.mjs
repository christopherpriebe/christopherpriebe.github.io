// Fails if a package.json of the workspace declares runtime dependencies: the page ships one
// script, one stylesheet and the wasm, and imports nothing else at run time. Everything the build
// uses is a devDependency, declared once in _instruments/package.json. Run through `npm run check`
// in web/.
import { readFileSync } from 'node:fs';
import { relative, resolve } from 'node:path';

const workspace = resolve(import.meta.dirname, '..', '..');
let failed = false;
for (const manifest of [resolve(workspace, 'package.json'), resolve(import.meta.dirname, '..', 'web', 'package.json')]) {
  const names = Object.keys(JSON.parse(readFileSync(manifest, 'utf8')).dependencies ?? {});
  if (names.length > 0) {
    console.error(`check-package: ${relative(workspace, manifest)} has runtime dependencies (${names.join(', ')}); move them to devDependencies or drop them`);
    failed = true;
  }
}
if (failed) process.exit(1);
