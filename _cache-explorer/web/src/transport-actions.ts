// What the transport does: each button and each shortcut asks the session for one move, and the
// core says whether it happened. Nothing here works out what a move does; a refusal, or an idle
// answer, goes to the status line as the core worded it.

import type { ControlAction, IssueDraft, ProgramSource } from '../pkg/cache_explorer';
import type { Action } from './actions';
import type { ActionHandler, Context } from './context';
import { refusal } from './context';
import { copy } from './copy';

/** The drawer's text and format, which a step passes so the core can tell when it has changed. */
function programOf(context: Context): ProgramSource | null {
  const { text, format } = context.ui.program;
  return text === '' ? null : { source: text, format };
}

/** Whether the core has the control live now. */
function live(context: Context, control: ControlAction): boolean {
  return context.session.state().controls.enabled.includes(control);
}

/** A control the core has switched off says why in the status line, and does nothing else. */
function blocked(context: Context, control: ControlAction): boolean {
  if (live(context, control)) return false;
  context.ui.status = context.session.state().controls.guard ?? '';
  context.refresh();
  return true;
}

/** Runs one move the student asked for: it ends a run, and a refusal is shown, not thrown. */
function move(context: Context, control: ControlAction, go: () => string | undefined): void {
  context.setRunning(false);
  if (blocked(context, control)) return;
  try {
    context.ui.status = go() ?? '';
  } catch (error) {
    context.ui.status = refusal(error);
  }
  context.refresh();
}

export function step(context: Context): void {
  move(context, 'step', () => {
    const outcome = context.session.stepEvent(programOf(context));
    return outcome.status === 'idle' ? outcome.message : undefined;
  });
}

export function stepAccess(context: Context): void {
  move(context, 'stepAccess', () => {
    const outcome = context.session.stepAccess(programOf(context));
    return outcome.status === 'idle' ? outcome.message : undefined;
  });
}

export function undo(context: Context): void {
  move(context, 'undo', () => {
    context.session.undo();
    return undefined;
  });
}

export function redo(context: Context): void {
  move(context, 'redo', () => {
    context.session.redo();
    return undefined;
  });
}

/** A new, empty run with the configuration in force; the core keeps the old run under Undo. */
export function reset(context: Context): void {
  move(context, 'reset', () => {
    context.session.applyOp({ op: 'config', config: context.session.state().config });
    return copy.transport.resetDone;
  });
}

/** Why the typed access cannot be issued, in the core's words where it has them. */
function whyNot(draft: IssueDraft): string {
  const { address, value } = draft;
  if (address.status === 'invalid' || address.status === 'misaligned') return address.message;
  if (address.status === 'empty') return copy.transport.needAddress;
  if (value?.status === 'invalid') return value.message;
  return copy.transport.needValue;
}

/** Issues the access in the boxes: the core reads the text, and says which op it comes to. */
export function issue(context: Context): void {
  const { session, ui } = context;
  move(context, 'issue', () => {
    const { kind, addrText, valueText } = ui.draft;
    const draft = session.draftIssue(kind, addrText, valueText);
    if (!draft.op) return whyNot(draft);
    session.applyOp(draft.op);
    return undefined;
  });
}

/**
 * Starts the run, or stops it. It steps once every `RUN_MS`, through `tick`; a run that has
 * nowhere to go, or that the core no longer allows, stops on its own.
 */
export function toggleRun(context: Context): void {
  if (context.ui.running) {
    context.setRunning(false);
  } else {
    if (blocked(context, 'run')) return;
    context.ui.status = '';
    context.setRunning(true);
  }
  context.refresh();
}

/** One beat of the run; false when the run is over. The caller draws. */
export function tick(context: Context): boolean {
  const { session, ui } = context;
  if (!live(context, 'run')) {
    ui.status = session.state().controls.guard ?? '';
    return false;
  }
  try {
    const outcome = session.stepEvent(programOf(context));
    if (outcome.status === 'idle') {
      ui.status = outcome.message;
      return false;
    }
  } catch (error) {
    ui.status = refusal(error);
    return false;
  }
  return true;
}

/** Opens the shortcuts sheet, or closes it and gives focus back to the button that opened it. */
export function toggleSheet(context: Context, open = !context.ui.sheetOpen): void {
  context.ui.sheetOpen = open;
  context.refresh();
  const target = context.root.querySelector<HTMLElement>(
    open ? '[data-action="sheet-close"]' : '[data-action="sheet"]',
  );
  target?.focus({ preventScroll: true });
}

const typeInto =
  (field: 'addrText' | 'valueText'): ActionHandler =>
  (context, action: Action) => {
    if (!(action.element instanceof HTMLInputElement)) return;
    const { ui } = context;
    ui.draft[field] = action.element.value;
    // The strip, the wires and the lanes follow what is typed until the cursor next moves.
    ui.stripSource = 'draft';
    context.refresh();
  };

function chooseKind(context: Context, action: Action): void {
  const kind = action.params.option;
  if (kind !== 'read' && kind !== 'write') return;
  context.ui.draft.kind = kind;
  context.ui.stripSource = 'draft';
  context.refresh();
}

/** The transport's handlers, by the `data-action` and the event they answer. */
export const transportActions = {
  clicks: new Map<string, ActionHandler>([
    ['step', step],
    ['step-access', stepAccess],
    ['run', toggleRun],
    ['undo', undo],
    ['redo', redo],
    ['reset', reset],
    ['issue', issue],
    ['kind', chooseKind],
    [
      'sheet',
      (context) => {
        toggleSheet(context);
      },
    ],
    [
      'sheet-close',
      (context) => {
        toggleSheet(context, false);
      },
    ],
  ]),
  inputs: new Map<string, ActionHandler>([
    ['addr', typeInto('addrText')],
    ['value', typeInto('valueText')],
  ]),
};
