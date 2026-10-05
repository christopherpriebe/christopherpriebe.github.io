import 'vitest';

declare module 'vitest' {
  interface ProvidedContext {
    /** The shipped wasm, read by vitest.config.ts; null when web/pkg has not been built. */
    wasmBytes: Uint8Array | null;
  }
}
