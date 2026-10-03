import { readFileSync } from 'node:fs';
import { defineConfig } from 'vitest/config';

/** The shipped wasm, or null when web/pkg has not been built (test/support/wasm.ts says so). */
function wasmBytes(): Uint8Array | null {
  try {
    return readFileSync(new URL('pkg/cache_explorer_bg.wasm', import.meta.url));
  } catch {
    return null;
  }
}

export default defineConfig({
  define: { __BUILD_HASH__: JSON.stringify('test') },
  // The module tests read the core's fixtures, which live outside this folder.
  server: { fs: { allow: ['..'] } },
  test: {
    environment: 'jsdom',
    include: ['test/**/*.test.ts'],
    setupFiles: ['test/support/dom.ts'],
    // The web-target package runs the shipped artifact through initSync (test/support/wasm.ts).
    provide: { wasmBytes: wasmBytes() },
    restoreMocks: true,
    unstubEnvs: true,
    unstubGlobals: true,
  },
});
