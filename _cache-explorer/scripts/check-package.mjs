// Fails if web/package.json declares runtime dependencies: the page ships one script, one
// stylesheet and the wasm, and imports nothing else at run time. Everything the build uses is a
// devDependency. Run through `npm run check` in web/.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const manifest = resolve(import.meta.dirname, '..', 'web', 'package.json');
const dependencies = JSON.parse(readFileSync(manifest, 'utf8')).dependencies ?? {};
const names = Object.keys(dependencies);
if (names.length > 0) {
  console.error(`check-package: web/package.json has runtime dependencies (${names.join(', ')}); move them to devDependencies or drop them`);
  process.exit(1);
}
