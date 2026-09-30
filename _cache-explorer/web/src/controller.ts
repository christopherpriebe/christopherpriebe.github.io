// The controller owns the session and the UI state. It calls the facade, then `render`; nothing
// else touches the DOM. It never works out what the machine does: after every change it asks the
// core again and draws the answer.

import type { MemoryLines, State, Timeline } from '../pkg/cache_explorer';
import { configActions } from './config-actions';
import type { ActionHandler, Context } from './context';
import { delegate } from './delegate';
import { editActions } from './edit-actions';
import { logActions, LOG_MARGIN, scrollLog, stopScrub } from './log-actions';
import { captureFocus, restoreFocus } from './focus';
import type { Layout } from './layout';
import { machineActions, MEMORY_MARGIN, scrollMemory } from './machine-actions';
import { onKeydown, onKeyup } from './keyboard';
import { FLASH_MS, RUN_MS } from './motion';
import { watchPress } from './press';
import { detach, render, type LogWindow, type Snapshot } from './render';
import { transportActions, tick } from './transport-actions';
import { initialUiState, type MemoryEditor, type UiState } from './ui-state';
import type { Session } from './wasm';

/** The rows and entries fetched around the first one in view: more than the scrollers show. */
const MEMORY_WINDOW = 24;
const LOG_WINDOW = 40;

/**
 * Everything one render draws, asked of the session as it is now. `now` is the clock the flash's
 * age is read from, in the milliseconds `performance.now` counts.
 */
export function buildSnapshot(
  session: Session,
  ui: UiState,
  now: number = performance.now(),
): { snapshot: Snapshot; log: LogWindow } {
  const state = session.state();
  const timeline = session.timeline();
  keepEditor(ui, state);
  placeMemoryWindow(ui, state);
  placeLogWindow(ui, state, timeline);
  const { draft, program, editor, modules } = ui;
  const memoryRows = session.memoryLines(ui.memoryFirst, MEMORY_WINDOW);
  if (editor?.kind === 'memory') keepValue(editor, memoryRows);
  const snapshot: Snapshot = {
    state,
    timeline,
    ticks: session.ticks(),
    memoryRows,
    config: session.configControls(ui.pending ?? state.config),
    draft: session.draftIssue(draft.kind, draft.addrText, draft.valueText),
    flashAge: ui.flash ? Math.max(0, now - ui.flash.since) : null,
  };
  if (program.text !== '')
    snapshot.programDraft = session.parseProgram(program.text, program.format);
  if (editor?.kind === 'way') {
    snapshot.editor = session.draftEdit(editor.set, editor.way, editor.form);
  } else if (editor?.kind === 'memory') {
    snapshot.editor = session.draftMemoryEdit(editor.addr, editor.typed ? editor.valueText : '');
  }
  if (modules.run) snapshot.module = modules.run.view(session);
  if (modules.index) snapshot.modules = modules.index;
  return { snapshot, log: session.entries(ui.logFirst, LOG_WINDOW) };
}

/**
 * Lets go of an editor whose way the run no longer has, as a new configuration ends its cache, and
 * gives the way editor the inputs of the line the cache now has.
 */
function keepEditor(ui: UiState, state: State): void {
  const { editor } = ui;
  if (editor?.kind !== 'way') return;
  const words = state.geometry.wordsPerLine;
  const view = state.cache.sets[editor.set]?.ways[editor.way];
  if (!view) {
    ui.editor = null;
    return;
  }
  // What the student has not set follows the way at the cursor, so that Apply never puts back
  // contents the run has since moved past.
  const { form, touched } = editor;
  if (!touched.valid) form.valid = view.valid;
  if (!touched.line) form.lineText = view.lineText ?? '';
  if (!touched.dirty) form.dirty = view.valid && view.dirty;
  if (editor.typed.length !== words) {
    editor.form.wordTexts = Array.from({ length: words }, () => '');
    editor.typed = Array.from({ length: words }, () => false);
  }
}

/** Lets the memory editor's box show the word's value at the cursor, while the student has not typed. */
function keepValue(editor: MemoryEditor, rows: MemoryLines): void {
  if (editor.typed) return;
  for (const row of rows) {
    const word = row.words.find((w) => w.addr === editor.addr);
    if (word) {
      editor.shown = word.text;
      return;
    }
  }
}

/**
 * Keeps the window of memory rows on the rows that matter: when the cursor has moved to an event
 * on the bus, the window is moved to the line it moves if that line is not built, and the window
 * never starts past the last row, which an Undo may have moved back.
 */
function placeMemoryWindow(ui: UiState, state: State): void {
  const { busRow, lineCount } = state.memory;
  const moved = state.position.pos !== ui.lastPos;
  if (
    moved &&
    busRow !== undefined &&
    (busRow < ui.memoryFirst || busRow >= ui.memoryFirst + MEMORY_WINDOW)
  ) {
    ui.memoryFirst = Math.max(0, busRow - MEMORY_MARGIN);
  }
  ui.memoryFirst = Math.min(ui.memoryFirst, Math.max(0, lineCount - 1));
}

/**
 * Keeps the window of log entries on the entries that matter: when the cursor has moved to an
 * entry that is not built, the window is moved to it, and it never starts past the last entry,
 * which an Undo or a new run may have moved back.
 */
function placeLogWindow(ui: UiState, state: State, timeline: Timeline): void {
  const entry = timeline.current?.index;
  const moved = state.position.pos !== ui.lastPos;
  if (moved && entry !== undefined && (entry < ui.logFirst || entry >= ui.logFirst + LOG_WINDOW)) {
    ui.logFirst = Math.max(0, entry - LOG_MARGIN);
  }
  ui.logFirst = Math.min(ui.logFirst, Math.max(0, timeline.entryCount - 1));
}

/** What an event on an element with a `data-action` does; each region contributes its own. */
const CLICKS: ReadonlyMap<string, ActionHandler> = new Map([
  ...configActions.clicks,
  ...editActions.clicks,
  ...logActions.clicks,
  ...transportActions.clicks,
]);
const INPUTS: ReadonlyMap<string, ActionHandler> = new Map([
  ...configActions.inputs,
  ...editActions.inputs,
  ...logActions.inputs,
  ...machineActions.inputs,
  ...transportActions.inputs,
]);
const CHANGES: ReadonlyMap<string, ActionHandler> = new Map([
  ...configActions.changes,
  ...editActions.changes,
  ...machineActions.changes,
]);

export interface Controller {
  /** Asks the session again and draws what it says. */
  refresh(): void;
  /** The root crossed a width threshold: draws the workbench or the notice. */
  setLayout(layout: Layout): void;
  /** Stops listening and frees what the controller owns. The controller is not used after it. */
  dispose(): void;
}

/**
 * Takes over `session` and draws it into `root`. The controller is its one owner: it lets go of
 * it in `dispose`, or here when the first draw fails.
 */
export function start(root: HTMLElement, session: Session, layout: Layout): Controller {
  let owned: Session | null = session;
  const ui = initialUiState(layout);
  // A session that opens part way through a run, as a link or a file does, is not a step: its
  // first draw plays nothing.
  ui.lastPos = session.position().pos;
  // Nor does it start the log at the top: the entry the cursor is in is the one to build around.
  const entry = session.timeline().current?.index;
  if (entry !== undefined) ui.logFirst = Math.max(0, entry - LOG_MARGIN);

  function current(): Session {
    if (!owned) throw new Error('the controller has been disposed');
    return owned;
  }

  function draw(): void {
    const focus = captureFocus(root);
    // The strip follows what is typed only until the cursor next moves.
    if (current().position().pos !== ui.lastPos) ui.stripSource = 'cursor';
    const { snapshot, log } = buildSnapshot(current(), ui);
    render(root, snapshot, log, ui, refresh);
    restoreFocus(root, focus);
    ui.lastPos = snapshot.state.position.pos;
    ui.memoryReveal = null;
  }

  /** A redraw asked for while a press was down, which waits until the press and its click are over. */
  let held = false;

  function refresh(): void {
    if (press.down) {
      held = true;
      return;
    }
    held = false;
    draw();
  }

  /** Makes the redraw that was held back, if there is one and the press is over. */
  function release(): void {
    if (held && owned) refresh();
  }

  const press = watchPress(root, release);

  let flashTimer: ReturnType<typeof setTimeout> | undefined;
  let runTimer: ReturnType<typeof setInterval> | undefined;

  const context: Context = {
    root,
    get session() {
      return current();
    },
    ui,
    refresh,
    flash(cause) {
      // Pointing at a group again, or at another, starts a flash from the beginning, and its time
      // from the same moment.
      clearTimeout(flashTimer);
      ui.configOpen = true;
      ui.flash = { cause, since: performance.now() };
      flashTimer = setTimeout(() => {
        flashTimer = undefined;
        ui.flash = null;
        if (owned) refresh();
      }, FLASH_MS);
      refresh();
    },
    setRunning(on) {
      clearInterval(runTimer);
      runTimer = undefined;
      ui.running = on;
      if (!on || !owned) return;
      runTimer = setInterval(() => {
        if (!owned) return;
        if (!tick(context)) context.setRunning(false);
        refresh();
      }, RUN_MS);
    },
  };

  const stopListening = delegate(root, {
    click(_, action) {
      if (action) CLICKS.get(action.name)?.(context, action);
    },
    input(_, action) {
      if (action) INPUTS.get(action.name)?.(context, action);
    },
    change(_, action) {
      if (action) CHANGES.get(action.name)?.(context, action);
    },
    keydown(event, action) {
      onKeydown(context, event, action);
    },
    keyup(event) {
      onKeyup(context, event);
    },
    scroll(event) {
      const { target } = event;
      if (!(target instanceof HTMLElement)) return;
      if (target.dataset.scroll === 'memory') scrollMemory(context, target);
      else if (target.dataset.scroll === 'log') scrollLog(context, target);
    },
  });

  function dispose(): void {
    clearTimeout(flashTimer);
    clearInterval(runTimer);
    stopScrub(ui);
    press.stop();
    stopListening();
    detach(root);
    const session = owned;
    owned = null;
    session?.free();
  }

  try {
    draw();
  } catch (error) {
    dispose();
    throw error;
  }

  return {
    refresh,
    setLayout(next) {
      if (next === ui.layout) return;
      ui.layout = next;
      refresh();
    },
    dispose,
  };
}
