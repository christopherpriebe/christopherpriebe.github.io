// The controller owns the session and the UI state. It calls the facade, then `render`; nothing
// else touches the DOM. It never works out what the machine does: after every change it asks the
// core again and draws the answer.

import type { Action } from './actions';
import type { Session } from './wasm';
import { captureFocus, restoreFocus } from './focus';
import { delegate } from './delegate';
import type { Layout } from './layout';
import { render, type LogWindow, type Snapshot } from './render';
import { initialUiState, type UiState } from './ui-state';

/** The rows and entries fetched around the first one in view: more than the scrollers show. */
const MEMORY_WINDOW = 24;
const LOG_WINDOW = 40;

/** Everything one render draws, asked of the session as it is now. */
export function buildSnapshot(
  session: Session,
  ui: UiState,
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

/**
 * What a click on an element with a `data-action` does; one function per action, added with its
 * region. A map, so that a name an object inherits (`constructor`, `__proto__`) finds nothing.
 */
type ActionHandler = (action: Action, event: MouseEvent) => void;
const ACTIONS: ReadonlyMap<string, ActionHandler> = new Map();

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

  function refresh(): void {
    const focus = captureFocus(root);
    const { snapshot, log } = buildSnapshot(current(), ui);
    render(root, snapshot, log, ui);
    restoreFocus(root, focus);
    ui.lastPos = snapshot.state.position.pos;
  }

  const stopListening = delegate(root, {
    click(event, action) {
      if (action) ACTIONS.get(action.name)?.(action, event);
    },
  });

  function dispose(): void {
    stopListening();
    const session = owned;
    owned = null;
    session?.free();
  }

  try {
    refresh();
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
