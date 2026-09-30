// jsdom has no ResizeObserver, and the cache box watches its size. Every test file gets one that
// watches nothing; a test that needs the observer to report a size takes the one in layout.ts.

if (typeof globalThis.ResizeObserver === 'undefined') {
  globalThis.ResizeObserver = class implements ResizeObserver {
    observe(): void {
      // Nothing changes size in jsdom.
    }
    unobserve(): void {
      // Nothing changes size in jsdom.
    }
    disconnect(): void {
      // Nothing changes size in jsdom.
    }
  };
}
