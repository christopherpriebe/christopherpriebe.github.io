import { inject } from 'vitest';
import { initSync } from '../../src/wasm';

/** Starts the shipped wasm in this test file's own realm; calling it again does nothing. */
export function loadWasm(): void {
  const bytes = inject('wasmBytes');
  if (bytes === null) throw new Error('web/pkg is not built: run `npm run build:wasm` first');
  initSync({ module: bytes });
}
