// Fixed chrome copy: text that never changes with the machine's state. Anything built from state
// or configuration comes from the core.

export const copy = {
  /** The plane's head, and the section's accessible name. */
  title: 'Interactive Cache Explorer',
  /** The status at the right of the head while the wasm loads. */
  loading: 'Loading',
  /** The status at the right of the head once the wasm is up. */
  ready: 'Ready',
} as const;
