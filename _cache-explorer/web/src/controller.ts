// The controller owns the session and the UI state. It calls the facade, then `render`; nothing
// else touches the DOM. It never works out what the machine does: after every change it asks the
// core again and draws the answer.

import { configActions } from './config-actions';
import type { ActionHandler, Context } from './context';
import { delegate } from './delegate';
import { captureFocus, restoreFocus } from './focus';
import type { Layout } from './layout';
import { FLASH_MS } from './motion';
import { watchPress } from './press';
import { render, type LogWindow, type Snapshot } from './render';
import { initialUiState, type UiState } from './ui-state';
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
  const { draft, program, editor, modules } = ui;
  const snapshot: Snapshot = {
    state,
    timeline: session.timeline(),
    ticks: session.ticks(),
    memoryRows: session.memoryLines(ui.memoryFirst, MEMORY_WINDOW),
    config: session.configControls(ui.pending ?? state.config),
    draft: session.draftIssue(draft.kind, draft.addrText, draft.valueText),
    flashAge: ui.flash ? Math.max(0, now - ui.flash.since) : null,
  };
  if (program.text !== '')
    snapshot.programDraft = session.parseProgram(program.text, program.format);
  if (editor?.kind === 'way') {
    snapshot.editor = session.draftEdit(editor.set, editor.way, editor.form);
  } else if (editor?.kind === 'memory') {
    snapshot.editor = session.draftMemoryEdit(editor.addr, editor.valueText);
  }
  if (modules.run) snapshot.module = modules.run.view(session);
  if (modules.index) snapshot.modules = modules.index;
  return { snapshot, log: session.entries(ui.logFirst, LOG_WINDOW) };
}

/** What an event on an element with a `data-action` does; each region contributes its own. */
const CLICKS: ReadonlyMap<string, ActionHandler> = new Map(configActions.clicks);
const INPUTS: ReadonlyMap<string, ActionHandler> = new Map(configActions.inputs);
const CHANGES: ReadonlyMap<string, ActionHandler> = new Map(configActions.changes);

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

  function current(): Session {
    if (!owned) throw new Error('the controller has been disposed');
    return owned;
  }

  function draw(): void {
    const focus = captureFocus(root);
    const { snapshot, log } = buildSnapshot(current(), ui);
    render(root, snapshot, log, ui);
    restoreFocus(root, focus);
    ui.lastPos = snapshot.state.position.pos;
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

  const context: Context = {
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
  });

  function dispose(): void {
    clearTimeout(flashTimer);
    press.stop();
    stopListening();
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
