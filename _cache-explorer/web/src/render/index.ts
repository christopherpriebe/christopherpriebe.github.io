// The render contract: `render` draws the whole tool from what the core last said and what the
// student has typed, and nothing else. It diffs nothing: it rebuilds each region's content on every
// call, and keeps only the elements that have to outlive a rebuild, which are the plane, the live
// region, the form controls (see controls.ts), and the machine's boxes, which hold a scroll
// position and a size watcher. Focus is put back by the caller (focus.ts).

import type {
  ConfigControls,
  EditDraft,
  EntryView,
  IssueDraft,
  MemoryEditDraft,
  MemoryLineView,
  ModuleIndexView,
  ModuleView,
  ParseResult,
  State,
  Timeline,
} from '../../pkg/cache_explorer';
import { copy } from '../copy';
import { h } from '../dom';
import { PLANE_CLASS } from '../focus';
import { narrowState } from '../page-states';
import type { UiState } from '../ui-state';
import { refits } from './cache';
import { createConfigBar, renderConfigBar, type ConfigBar } from './config-bar';
import { renderKey } from './key';
import { createLog, refitsTicks, renderLog, stopLog, type LogBox } from './log';
import { createMachine, renderMachine, stopMachine, type Machine } from './machine';
import { createProgramDrawer, renderProgram, type ProgramDrawer } from './program';
import { renderStats } from './stats';
import { createTransport, renderTransport, type Transport } from './transport';

/** Everything one render draws from the session, asked of the facade again after each change. */
export interface Snapshot {
  state: State;
  timeline: Timeline;
  /**
   * One byte per step of the run: bit 0 marks an entry's first step, bit 1 a step of a miss. Asked
   * of the session on every render, not kept while the step count holds: an undo followed by an
   * access of the same length leaves the count as it was and the ticks different.
   */
  ticks: Uint8Array;
  /** The memory rows in view. */
  memoryRows: MemoryLineView[];
  /** The configuration bar's controls for the pending configuration, or the run's. */
  config: ConfigControls;
  /** The typed access, read. */
  draft: IssueDraft;
  /** How long ago the flash a log line started began, in milliseconds, or null when none is showing. */
  flashAge: number | null;
  /** The drawer's text, parsed, while it holds text. */
  programDraft?: ParseResult;
  /** The open editor's form, checked. */
  editor?: EditDraft | MemoryEditDraft;
  module?: ModuleView;
  modules?: ModuleIndexView;
}

/** The log entries in view. */
export type LogWindow = EntryView[];

/** The elements a root keeps between renders. */
interface Shell {
  plane: HTMLElement;
  head: HTMLElement;
  config: ConfigBar;
  machine: Machine;
  log: LogBox;
  stats: HTMLElement;
  key: HTMLElement;
  transport: Transport;
  program: ProgramDrawer;
  live: HTMLElement;
  /**
   * What the last render of the workbench drew from and who to ask for another when the cache
   * box's size asks, or null while the notice is drawn in its place.
   */
  last: { snap: Snapshot; redraw: (() => void) | undefined } | null;
  /** The frame a request for a change of size waits for, or null. */
  refit: number | null;
}

const shells = new WeakMap<HTMLElement, Shell>();

function shellOf(root: HTMLElement): Shell {
  let shell = shells.get(root);
  if (!shell) {
    const head = h('div', { class: 'cache-explorer__head' });
    // A region the render never replaces: a screen reader announces a change of text in a live
    // region that was already there, not one that arrives with the region.
    const live = h('div', {
      class: 'cache-explorer__live',
      'aria-live': 'polite',
      'aria-atomic': 'true',
    });
    const config = createConfigBar();
    const machine = createMachine(() => {
      redrawIfRefit(root);
    });
    const log = createLog(() => {
      redrawIfRefit(root);
    });
    const stats = h('div', { class: 'cache-explorer__stats' });
    const key = h('div', { class: 'cache-explorer__key-box' });
    const transport = createTransport();
    const program = createProgramDrawer();
    const plane = h(
      'section',
      {
        class: `${PLANE_CLASS} cache-explorer__plane--workbench`,
        'aria-label': copy.title,
        tabindex: -1,
      },
      head,
      config.element,
      machine.element,
      log.element,
      h('div', { class: 'cache-explorer__lower' }, stats, key),
      transport.element,
      program.element,
      live,
    );
    shell = {
      plane,
      head,
      config,
      machine,
      log,
      stats,
      key,
      transport,
      program,
      live,
      last: null,
      refit: null,
    };
    shells.set(root, shell);
  }
  return shell;
}

/**
 * The cache box or the strip of ticks changed size. If the ways or the ticks now fit differently
 * from how they were drawn, the caller is asked, a frame later, to draw again: it asks the core
 * again and draws from what the core says and what the student has typed by then, which a resize
 * changes none of. The frame keeps the request out of the observer's own notification, which the
 * new drawing would resize a box in.
 */
function redrawIfRefit(root: HTMLElement): void {
  const shell = shells.get(root);
  // A request already waiting, or a root the tool has let go of, asks for nothing.
  if (shell?.refit !== null) return;
  shell.refit = requestAnimationFrame(() => {
    shell.refit = null;
    const { last } = shell;
    if (
      last?.redraw &&
      (refits(shell.machine.cache, last.snap) || refitsTicks(shell.log, last.snap))
    ) {
      last.redraw();
    }
  });
}

/** Puts `plane` into the root as its only child, leaving it in place when it already is. */
function mount(root: HTMLElement, plane: HTMLElement): void {
  if (root.firstElementChild !== plane || root.childElementCount !== 1) root.replaceChildren(plane);
}

/**
 * Draws the tool into `root`; the workbench, or a notice when the root is too narrow for it.
 * `redraw`, when there is one, is called when the cache box has changed size so that the table
 * wants drawing the other way; it draws the tool again.
 */
export function render(
  root: HTMLElement,
  snap: Snapshot,
  log: LogWindow,
  ui: UiState,
  redraw?: () => void,
): void {
  if (ui.layout === 'narrow') {
    const kept = shells.get(root);
    if (kept) kept.last = null;
    root.replaceChildren(narrowState());
    return;
  }
  const shell = shellOf(root);
  const { plane, head, config, machine, log: logBox, stats, key, transport, program, live } = shell;
  // The plane is on the page before the machine is drawn: the cache table is drawn to the width
  // of its box, which a box that is not on the page does not have.
  mount(root, plane);
  shell.last = { snap, redraw };
  head.replaceChildren(
    h('span', { class: 'cache-explorer__label' }, copy.title),
    h('span', { class: 'cache-explorer__label' }, snap.state.labels.statusText),
  );
  renderConfigBar(root, config, snap, ui);
  renderMachine(root, machine, snap, ui);
  renderLog(root, logBox, snap, log, ui);
  renderStats(stats, snap);
  renderKey(key, snap);
  renderTransport(root, transport, snap, ui);
  renderProgram(root, program, snap, ui);
  if (live.textContent !== snap.timeline.liveText) live.textContent = snap.timeline.liveText;
}

/** Stops what the root's tool watches, when the tool is done with the root. */
export function detach(root: HTMLElement): void {
  const shell = shells.get(root);
  if (!shell) return;
  if (shell.refit !== null) cancelAnimationFrame(shell.refit);
  stopMachine(shell.machine);
  stopLog(shell.log);
  shells.delete(root);
}
