// Fixed chrome copy: text that never changes with the machine's state. Anything built from state
// or configuration comes from the core.

export const copy = {
  /** The plane's head, and the section's accessible name. */
  title: 'Interactive Cache Explorer',
  /** The status at the right of the head while the wasm loads. */
  loading: 'Loading',
  /** The status at the right of the head when the page could not start. */
  couldNotStart: 'Could not start',
  couldNotStartMessage:
    "The page's files are out of step, usually after an update. Reload to fix it.",
  reload: 'Reload',
  notices: {
    noWebAssembly: {
      lead: 'The Interactive Cache Explorer needs WebAssembly, which this browser does not provide.',
      text: 'Any current version of Chrome, Edge, Firefox or Safari will run it.',
    },
    narrow: {
      lead: 'The Interactive Cache Explorer needs a wider screen.',
      text: 'The cache table, memory and log have to sit side by side to be readable. Open this page on a tablet or a computer, at least 700 pixels wide.',
    },
  },
} as const;
