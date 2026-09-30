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
import { captureFocus, PLANE_CLASS, restoreFocus } from '../focus';
import { narrowState } from '../page-states';
import type { UiState } from '../ui-state';
import { refits } from './cache';
import { createConfigBar, renderConfigBar, type ConfigBar } from './config-bar';
import { createMachine, renderMachine, stopMachine, type Machine } from './machine';

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
  live: HTMLElement;
  /** What the last render drew from, so that it can be drawn again when the cache box's size asks. */
  last: { snap: Snapshot; log: LogWindow; ui: UiState } | null;
  /** The frame a redraw for a change of size waits for, or null. */
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
      live,
    );
    shell = { plane, head, config, machine, live, last: null, refit: null };
    shells.set(root, shell);
  }
  return shell;
}

/**
 * The cache box changed size. If its ways now fit differently from how they were drawn, the
 * workbench is drawn again, a frame later, from what it was drawn from, which is still what the
 * core says: a resize changes nothing else. No step is played again, since the cursor has not
 * moved. The frame keeps the redraw out of the observer's own notification, which the redraw
 * would resize the box in.
 */
function redrawIfRefit(root: HTMLElement): void {
  const shell = shells.get(root);
  if (shell?.refit !== null) return;
  shell.refit = requestAnimationFrame(() => {
    shell.refit = null;
    const { last } = shell;
    if (!last || last.ui.layout === 'narrow') return;
    if (!refits(shell.machine.cache, last.snap)) return;
    // The controller keeps focus across its own draws; this one is not the controller's.
    const focus = captureFocus(root);
    render(root, last.snap, last.log, last.ui);
    restoreFocus(root, focus);
  });
}

/** Puts `plane` into the root as its only child, leaving it in place when it already is. */
function mount(root: HTMLElement, plane: HTMLElement): void {
  if (root.firstElementChild !== plane || root.childElementCount !== 1) root.replaceChildren(plane);
}

/** Draws the tool into `root`; the workbench, or a notice when the root is too narrow for it. */
export function render(root: HTMLElement, snap: Snapshot, log: LogWindow, ui: UiState): void {
  if (ui.layout === 'narrow') {
    root.replaceChildren(narrowState());
    return;
  }
  const shell = shellOf(root);
  const { plane, head, config, machine, live } = shell;
  shell.last = { snap, log, ui };
  head.replaceChildren(
    h('span', { class: 'cache-explorer__label' }, copy.title),
    h('span', { class: 'cache-explorer__label' }, snap.state.labels.statusText),
  );
  renderConfigBar(root, config, snap, ui);
  renderMachine(root, machine, snap, ui);
  if (live.textContent !== snap.timeline.liveText) live.textContent = snap.timeline.liveText;
  mount(root, plane);
}

/** Stops what the root's tool watches, when the tool is done with the root. */
export function detach(root: HTMLElement): void {
  const shell = shells.get(root);
  if (!shell) return;
  if (shell.refit !== null) cancelAnimationFrame(shell.refit);
  stopMachine(shell.machine);
  shells.delete(root);
}
