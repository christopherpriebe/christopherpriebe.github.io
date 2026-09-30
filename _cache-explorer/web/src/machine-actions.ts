// What the machine's controls do: the "Jump to" box asks the core to show a line of memory, and the
// memory table's scrolling moves the window of rows the table builds.

import type { LineAddr } from '../pkg/cache_explorer';
import type { Action } from './actions';
import type { ActionHandler, Context } from './context';
import { refusal } from './context';
import { MEMORY_ROW_PX } from './metrics';
import type { Session } from './wasm';

/** The rows built above the first one in view, so a short scroll finds rows already there. */
export const MEMORY_MARGIN = 4;

/**
 * The index of the row that shows `line`, among the `count` rows the core lists in ascending
 * order, or null when there is none. The rows are asked for one at a time: a run that touched
 * thousands of lines is searched in a dozen calls, and no row is built that is not looked at.
 */
export function rowOf(session: Session, line: LineAddr, count: number): number | null {
  let low = 0;
  let high = count - 1;
  while (low <= high) {
    const middle = Math.floor((low + high) / 2);
    const found = session.memoryLines(middle, 1)[0];
    if (!found) return null;
    if (found.line === line) return middle;
    if (found.line < line) low = middle + 1;
    else high = middle - 1;
  }
  return null;
}

/** The memory table was scrolled: build the rows around the ones now in view. */
export function scrollMemory(context: Context, scroller: HTMLElement): void {
  const first = Math.max(0, Math.floor(scroller.scrollTop / MEMORY_ROW_PX) - MEMORY_MARGIN);
  if (first === context.ui.memoryFirst) return;
  context.ui.memoryFirst = first;
  context.refresh();
}

/** The jump box keeps what is typed in it; it is read when the student leaves it. */
function typeJump(context: Context, action: Action): void {
  if (action.element instanceof HTMLInputElement) context.ui.jumpText = action.element.value;
}

/**
 * The box holds an address, which the core reads. A line it names is watched, so it has a row
 * whether or not the run has touched it, and the table scrolls to it; an empty box stops watching;
 * text the core cannot read as an address leaves the watch as it was and says why on the box.
 */
function sendJump(context: Context, action: Action): void {
  const { ui, session } = context;
  if (!(action.element instanceof HTMLInputElement)) return;
  ui.jumpText = action.element.value;
  try {
    const read = session.readAddress(ui.jumpText);
    if (read.status === 'invalid') {
      ui.jumpError = read.message;
    } else if (read.status === 'empty') {
      ui.jumpError = null;
      session.setMemoryWatch([]);
    } else {
      ui.jumpError = null;
      session.setMemoryWatch([read.decoded.line]);
      const row = rowOf(session, read.decoded.line, session.state().memory.lineCount);
      if (row !== null) {
        ui.memoryFirst = Math.max(0, row - MEMORY_MARGIN);
        ui.memoryReveal = row;
      }
    }
  } catch (error) {
    ui.status = refusal(error);
  }
  context.refresh();
}

/** The machine's handlers, by the `data-action` and the event they answer. */
export const machineActions = {
  inputs: new Map<string, ActionHandler>([['jump', typeJump]]),
  changes: new Map<string, ActionHandler>([['jump', sendJump]]),
};
