// Presentation and unsent input: what the student has typed, which panels are open, where the
// lists are scrolled. Nothing here is machine state, and nothing here is derived from it; the
// core answers for that, and each render asks it again.

import type {
  AccessKind,
  Addr,
  Cause,
  Config,
  ConfigNumberField,
  ModuleIndexView,
  LoadFailure,
  ModuleRun,
  ProgramFormat,
  ShareSummary,
  WayForm,
} from '../pkg/cache_explorer';
import type { Layout } from './layout';

/** The address the first visit has typed, decoded and marked in the address strip. */
export const FIRST_VISIT_ADDRESS = '0x0A4';
/** The value a store would write, until the student types another. */
export const FIRST_VISIT_VALUE = '7';
/** The program the first visit's drawer holds, not yet loaded: five instructions on set 2. */
export const SAMPLE_PROGRAM = [
  'start:',
  '  ld  r1, [0x0A4]',
  '  ld  r2, [0x0A8]',
  '  ld  r3, [0x1A4]',
  '  st  [0x0A4], #7',
  '  ld  r4, [0x0A4]',
].join('\n');

export interface WayEditor {
  kind: 'way';
  set: number;
  way: number;
  form: WayForm;
  /** Which of the form's first three fields the student has set; the others follow the way. */
  touched: { valid: boolean; line: boolean; dirty: boolean };
  /** Which data inputs the student has typed in; the others show what the core reads for them. */
  typed: boolean[];
}

export interface MemoryEditor {
  kind: 'memory';
  addr: Addr;
  /** The address as the core writes it, `0x0B0`, for the editor's title. */
  addrText: string;
  /** What the student typed; the core reads it only once `typed`, and reads an empty text as the word's own. */
  valueText: string;
  /** Whether the student has typed in the value box. */
  typed: boolean;
  /** The word's value as the run has it at the cursor, which the box shows until the student types. */
  shown: string;
}

export interface UiState {
  layout: Layout;
  /** The configuration bar: open at desktop, collapsed to its summary line at tablet. */
  configOpen: boolean;
  /** After a change that needs a new run: the configuration "Reset and apply" would append. */
  pending: Config | null;
  /** Why the core asked for a reset, shown by "Reset to apply?". */
  pendingMessage: string | null;
  /** The pending change is the switch to Basics: once it is applied, the page leaves Advanced. */
  pendingBasics: boolean;
  /** The "Address and word" group shows its two controls in place of its summary line. */
  addressOpen: boolean;
  /** The timing field the student typed in that does not read, and why; null when both do. */
  timingError: { field: ConfigNumberField; message: string } | null;
  /**
   * The group a `why:` link is flashing and when the flash began, in the milliseconds
   * `performance.now` counts, until the flash has run its course.
   */
  flash: { cause: Cause; since: number } | null;
  draft: { kind: AccessKind; addrText: string; valueText: string };
  /** What the address strip follows: the access at the cursor, or the typed address until the cursor next moves. */
  stripSource: 'cursor' | 'draft';
  editor: WayEditor | MemoryEditor | null;
  jumpText: string;
  /** Why the core does not read the jump box's text as an address; null when it does. */
  jumpError: string | null;
  program: { open: boolean; editing: boolean; format: ProgramFormat; text: string };
  /**
   * The guided modules: the drawer, the list the core read from the index (null until it has, and
   * `load` says how that went), the run in progress, the module being fetched to start, and why the
   * last start did not happen.
   */
  modules: {
    open: boolean;
    load: 'none' | 'loading' | 'ready' | 'failed';
    index: ModuleIndexView | null;
    run: ModuleRun | null;
    starting: string | null;
    error: string | null;
  };
  /** The Run timer is stepping. */
  running: boolean;
  sheetOpen: boolean;
  /** A shortcut handled the press of Space on a button: its release must not click the button. */
  spaceHeld: boolean;
  /** The transport's status line. */
  status: string;
  /** The link to show in a field when the clipboard is not available. */
  shareFallback: string | null;
  /**
   * A link or a file being replayed, while it is: when it began, in the milliseconds
   * `performance.now` counts, the core's account of the replay so far (null before the first
   * slice), and the call that stops it and lets its objects go. The workbench is not drawn from
   * the session meanwhile, which has been given up (9.4).
   */
  loading: { since: number; summary: ShareSummary | null; cancel: () => void } | null;
  /**
   * What the core said about a link or a file it could not open, with whether a run was kept to go
   * back to and whether the contents are shown; null when nothing failed.
   */
  failure: { failure: LoadFailure; keep: boolean; contents: boolean } | null;
  /** The first log entry in view. */
  logFirst: number;
  /**
   * The scrubber's latest step and the animation frame that will seek to it, or null when none
   * is waiting: a drag moves the cursor at most once a frame.
   */
  scrub: { step: number; frame: number | null };
  /** The first memory row built: the rows around the ones in view. */
  memoryFirst: number;
  /** A memory row to bring into view in the next render, and then not again; null for none. */
  memoryReveal: number | null;
  /** The cursor at the previous render: motion plays only when it has moved since. */
  lastPos: number;
}

/** The state of a first visit at this layout. */
export function initialUiState(layout: Layout): UiState {
  return {
    layout,
    configOpen: layout === 'desktop',
    pending: null,
    pendingMessage: null,
    pendingBasics: false,
    addressOpen: false,
    timingError: null,
    flash: null,
    draft: { kind: 'read', addrText: FIRST_VISIT_ADDRESS, valueText: FIRST_VISIT_VALUE },
    stripSource: 'cursor',
    editor: null,
    jumpText: '',
    jumpError: null,
    program: { open: true, editing: false, format: 'asm', text: SAMPLE_PROGRAM },
    modules: { open: false, load: 'none', index: null, run: null, starting: null, error: null },
    running: false,
    sheetOpen: false,
    spaceHeld: false,
    status: '',
    shareFallback: null,
    loading: null,
    failure: null,
    logFirst: 0,
    scrub: { step: 0, frame: null },
    memoryFirst: 0,
    memoryReveal: null,
    lastPos: 0,
  };
}
