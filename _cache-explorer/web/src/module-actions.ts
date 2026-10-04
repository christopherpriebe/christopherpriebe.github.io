// What the guided modules do: the drawer lists the index, a click starts a module, and the panel's
// buttons hand each move to the run. Progress is the core's: the run decides when a step is done,
// what a wrong answer shows and what the gate locks; the UI only sends the click and draws what
// the run says next. A run is owned by the UI state, which lets go of its reference before it
// frees the run, once.

import type { Action } from './actions';
import type { ActionHandler, Context } from './context';
import { refusal } from './context';
import { copy } from './copy';
import type { UiState } from './ui-state';
import { ModuleRun, parseModuleIndex } from './wasm';

/** Frees the run the state holds, if any, after the state has let go of it. */
export function releaseModule(ui: UiState): void {
  const run = ui.modules.run;
  ui.modules.run = null;
  run?.free();
}

/**
 * Ends the module because the run it was on is gone: the new session has no gate for it to
 * lift, so the run is freed as it is, and the status line says why.
 */
export function endModule(context: Context): void {
  if (!context.ui.modules.run) return;
  releaseModule(context.ui);
  context.ui.status = copy.modules.ended;
}

/** Reads the index into the list the drawer draws; a page with no modules draws no drawer. */
export async function loadModules(context: Context): Promise<void> {
  const { modules, ui } = context;
  if (!modules || ui.modules.load === 'loading') return;
  ui.modules.load = 'loading';
  try {
    const text = await modules.index();
    if (context.disposed) return;
    ui.modules.index = parseModuleIndex(text);
    ui.modules.load = 'ready';
  } catch (error) {
    if (context.disposed) return;
    if (error instanceof WebAssembly.RuntimeError) throw error;
    console.error('The guided modules could not be listed.', error);
    ui.modules.load = 'failed';
  }
  context.refresh();
}

function toggle(context: Context): void {
  context.ui.modules.open = !context.ui.modules.open;
  context.refresh();
}

function retry(context: Context): void {
  void loadModules(context);
}

/**
 * Puts `next` in place of the run in progress. It starts on the session first, which may refuse,
 * and then the run it replaces lets go of its gate; the workbench takes up the module's program and
 * drops what belonged to the run before.
 */
function adopt(context: Context, next: ModuleRun): void {
  const { ui, session } = context;
  next.start(session);
  const old = ui.modules.run;
  ui.modules.run = next;
  if (old) {
    try {
      old.exit(session);
    } finally {
      old.free();
    }
  }
  context.setRunning(false);
  ui.pending = null;
  ui.pendingMessage = null;
  ui.pendingBasics = false;
  ui.timingError = null;
  ui.editor = null;
  ui.status = '';
  const loaded = session.state().program;
  if (loaded) {
    ui.program = { ...ui.program, format: loaded.format, text: loaded.source, editing: false };
  }
}

/** Fetches a module, reads it, and starts it; what goes wrong is said in the drawer. */
export async function startModule(context: Context, id: string): Promise<void> {
  const { modules, ui } = context;
  if (!modules || ui.modules.starting !== null) return;
  ui.modules.starting = id;
  ui.modules.error = null;
  context.refresh();
  let error: string | null = null;
  let started = false;
  try {
    let text: string | null = null;
    try {
      text = await modules.module(id);
    } catch (failure) {
      if (context.disposed) return;
      console.error('A guided module could not be fetched.', failure);
      error = copy.modules.startFailed;
    }
    if (text !== null) {
      if (context.disposed) return;
      const next = ModuleRun.fromJson(text);
      try {
        adopt(context, next);
        started = true;
      } catch (failure) {
        // The run was not adopted, so nothing else owns it.
        if (ui.modules.run !== next) next.free();
        throw failure;
      }
    }
  } catch (failure) {
    error = refusal(failure);
  }
  ui.modules.starting = null;
  ui.modules.error = error;
  if (started) announceStart(context);
  context.refresh();
  if (started) showPanel(context);
}

/** Says, on the status line, which module began and where it is: the drawer is far from the panel. */
function announceStart(context: Context): void {
  const view = context.ui.modules.run?.view(context.session);
  if (view) context.ui.status = copy.modules.started(view.title, view.progressText);
}

/** Brings the panel into view, without the glide for a student who asked for less motion. */
function showPanel(context: Context): void {
  const panel = context.root.querySelector('.cache-explorer__module-panel');
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  panel?.scrollIntoView({ behavior: reduced ? 'auto' : 'smooth', block: 'start' });
}

function start(context: Context, action: Action): void {
  const id = action.params.module;
  if (id !== undefined) void startModule(context, id);
}

/** Sends one move to the run in progress, then draws what the run says. */
function move(context: Context, go: (run: ModuleRun) => void): void {
  const run = context.ui.modules.run;
  if (!run) return;
  go(run);
  context.refresh();
}

function select(context: Context, action: Action): void {
  const option = Number(action.params.option);
  if (!Number.isInteger(option)) return;
  move(context, (run) => {
    run.select(option);
  });
}

function check(context: Context): void {
  move(context, (run) => {
    run.check(context.session);
  });
}

function next(context: Context): void {
  move(context, (run) => {
    run.next(context.session);
  });
}

function showMe(context: Context): void {
  context.setRunning(false);
  move(context, (run) => {
    run.showMe(context.session);
  });
}

function exit(context: Context): void {
  const run = context.ui.modules.run;
  if (!run) return;
  context.ui.modules.run = null;
  try {
    run.exit(context.session);
  } finally {
    run.free();
  }
  context.refresh();
}

/** The handlers of the drawer and the panel, by `data-action`. */
export const moduleActions = {
  clicks: new Map<string, ActionHandler>([
    ['modules-toggle', toggle],
    ['modules-retry', retry],
    ['module-start', start],
    ['module-select', select],
    ['module-check', check],
    ['module-next', next],
    ['module-restart', next],
    ['module-show-me', showMe],
    ['module-exit', exit],
  ]),
};
