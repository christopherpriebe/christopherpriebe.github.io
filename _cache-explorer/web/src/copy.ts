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
  config: {
    title: 'Configuration',
    detailLevel: 'Detail level',
    basics: 'Basics',
    advanced: 'Advanced',
    /** The name over each group of options; the core names the options themselves. */
    groups: {
      cacheSize: 'Cache size',
      lineSize: 'Line size',
      associativity: 'Associativity',
      replacement: 'Replacement',
      writePolicy: 'Write policy',
      allocatePolicy: 'On a store miss',
      address: 'Address and word',
      timing: 'Timing',
    },
    /** Under a group that Basics hides, shown because a log line points at it. */
    readOnlyNote: 'Advanced changes this.',
    address: {
      change: 'change…',
      done: 'done',
      addressCaption: 'Address',
      wordCaption: 'Word',
      addressLabel: 'Address width',
      wordLabel: 'Word size',
    },
    timing: {
      hit: 'Hit',
      miss: 'Miss',
      cycles: 'cycles',
      hitLabel: 'Hit time in cycles',
      missLabel: 'Miss penalty in cycles',
      timed: 'Timed mode',
    },
    pending: {
      title: 'Reset to apply?',
      apply: 'Reset and apply',
      keep: 'Keep this run',
    },
    applied: 'New configuration applied. The run starts empty.',
  },
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
