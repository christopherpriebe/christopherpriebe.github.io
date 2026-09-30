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
  memory: {
    caption: 'Main memory',
    jump: 'Jump to',
    jumpLabel: 'Jump to address',
    jumpPlaceholder: '0x…',
    /** A line the student asked to see: the title of its address, and its name for a screen reader. */
    watched: 'Watched: you asked to see this line.',
    empty: 'Nothing touched yet. A line appears here the first time the cache reads or writes it.',
    stale: 'stale',
    heads: { line: 'Line', tag: 'tag', set: 'set' },
    /** The word's own description says what it is; this is the way to change it. */
    edit: 'Press Enter to edit.',
  },
  cache: {
    editHint: 'Click a way to edit it',
    setHead: ['Set', 'index'],
    way: 'Way',
    heads: { valid: 'V', tag: 'Tag', data: 'Data', dirty: 'D' },
    rank: { lru: 'LRU', age: 'Age' },
    /** The word beside a rank at either end of the order. */
    roles: { mru: 'MRU', lru: 'LRU', new: 'new', old: 'old' },
    victim: 'victim',
    /** What a screen reader is told after a way's own description. */
    edit: 'Press Enter to edit.',
    /** The badge in a wrapped way, by what the comparator says. */
    compare: {
      pending: { glyph: '=?', title: 'Tag not compared yet' },
      match: { glyph: '=', title: 'Tag matches' },
      mismatch: { glyph: '≠', title: 'Tag differs' },
      invalid: { glyph: '≠', title: 'Empty way: no tag to match' },
    },
  },
  strip: {
    address: 'Address',
    /** The name of each field, before its value. */
    fields: { tag: 'Tag', index: 'Index', offset: 'Offset' },
    /** The right-hand text of the strip's footer, and the request lane's sub-label, for a typed address. */
    typed: 'typed, not issued yet',
    none: 'type an address below',
    /** The address text while nothing decodes. */
    noAddress: '—',
  },
  cpu: {
    title: 'CPU',
    registers: 'Registers',
    /** What the square before the status says, for a screen reader and the pointer. */
    marker: { hit: 'Hit', miss: 'Miss' },
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
