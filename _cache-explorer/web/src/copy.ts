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
  editor: {
    /** The way popover's title and name. */
    wayTitle: (set: number, way: number): string => `Edit set ${String(set)} · way ${String(way)}`,
    /** The memory editor's title and name. */
    memoryTitle: (addrText: string): string => `Edit memory ${addrText}`,
    close: 'Close',
    cancel: 'Cancel',
    /** The primary button while the core offers nothing to apply. */
    cannot: 'Cannot apply',
    /** The primary button while there is nothing to change. */
    apply: 'Apply',
    goToEnd: 'Go to the end',
    valid: 'Valid',
    lineAddress: 'Line address',
    data: 'Data',
    dirty: 'Dirty',
    rank: 'rank',
    value: 'Value',
    tag: 'tag',
    index: 'index',
    word: 'Word',
    /** What a swatch or a field shows while there is nothing to read. */
    none: '—',
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
  log: {
    title: 'Event log',
    /** The scrubber's name for a screen reader. */
    timeline: 'Timeline',
    /** Before the run's first entry; the lead is the larger line. */
    empty: {
      lead: 'Type an address or press Space.',
      text: 'Every access you issue is written here step by step, in plain English, with what changed.',
    },
    changed: 'changed',
    why: 'why:',
  },
  stats: {
    title: 'Stats',
    labels: {
      accesses: 'Accesses',
      hits: 'Hits',
      misses: 'Misses',
      hitRate: 'Hit rate',
      evictions: 'Evictions',
      memoryWrites: 'Writes to memory',
      writebacks: 'Write-backs',
      writethroughs: 'Write-throughs',
      writearounds: 'Write-arounds',
      cycles: 'Cycles',
      compulsory: 'Compulsory misses',
      capacity: 'Capacity misses',
      conflict: 'Conflict misses',
    },
  },
  key: {
    title: 'Key',
    tag: 'Tag bits',
    hit: 'Hit, filled',
    index: 'Index bits',
    miss: 'Miss, open',
    offset: 'Offset bits',
    changed: 'Changed',
    invalid: 'Invalid way',
    victim: 'Victim',
    dirty: 'Dirty · D',
    stale: 'Stale in memory',
    fill: 'Fill',
    writeThrough: 'Write-through',
    writeBack: 'Write-back',
  },
  transport: {
    operation: 'Operation',
    load: 'Load',
    store: 'Store',
    address: 'Address',
    addressLabel: 'Address (hex)',
    value: 'Value',
    valueLabel: 'Value to store',
    issue: 'Issue',
    group: 'Transport',
    step: 'Step',
    stepAccess: 'Step access',
    run: 'Run',
    pause: 'Pause',
    reset: 'Reset',
    undo: 'Undo',
    redo: 'Redo',
    resetDone: 'Reset. The configuration is unchanged.',
    shortcuts: 'Keyboard shortcuts',
    /** What Issue says when the address box holds nothing, or a store's value box does. */
    needAddress: 'Type an address to issue.',
    needValue: 'Type a value to store.',
  },
  program: {
    /** The drawer's name, by what it holds. */
    title: { asm: 'Program', trace: 'Trace' },
    /** The listing's and the text box's names for a screen reader, by what the drawer holds. */
    listing: { asm: 'Program listing', trace: 'Trace listing' },
    text: { asm: 'Program', trace: 'Trace' },
    errors: { asm: 'Program errors', trace: 'Trace errors' },
    format: 'Input type',
    formats: { asm: 'Program', trace: 'Trace' },
    edit: 'Edit program',
    done: 'Done editing',
    /** Where the listing would be, when there is nothing written. */
    empty: {
      asm: 'Nothing written yet. Edit the program to write one.',
      trace: 'Nothing written yet. Edit the trace to write one.',
    },
    /** In the drawer's line, when there is nothing to parse. */
    none: 'nothing written yet',
    /** Under the listing's side notes, when the text is not the program the run holds. */
    pending: 'Not loaded yet. The next Step, Step access or Run loads it and logs the load.',
    /** The one-line syntax reminder, as the design words it. */
    help: {
      asm: [
        'ld r1, [0x40]',
        'st [0x40], #7   or  st [0x40], r2',
        'add r1, r1, #4',
        'bnez r1, loop',
        'loop:   labels end in a colon',
      ],
      trace: ['One access per line:', 'R 0x0A4   load', 'W 0x0A4 7   store 7'],
    },
  },
  sheet: {
    title: 'Keyboard',
    close: 'Close',
    rows: [
      ['Space', 'Step one micro-event'],
      ['Shift+Space', 'Step one whole access'],
      ['R', 'Run or pause'],
      ['Z / Y', 'Undo / redo'],
      ['Enter', 'Issue, or edit the focused way'],
      ['Esc', 'Close an editor or this sheet'],
      ['?', 'This sheet'],
    ],
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
