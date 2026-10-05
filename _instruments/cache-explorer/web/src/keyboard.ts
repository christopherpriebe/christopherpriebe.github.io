// The key map. One listener on the tool's root hears every key pressed while focus is inside it,
// so a page that has focus elsewhere keeps its own keys, Space scrolling it included.

import type { Action } from './actions';
import type { Context } from './context';
import { applyEditor, closeEditor, openFocused } from './edit-actions';
import { issue, redo, stepAccess, step, toggleRun, toggleSheet, undo } from './transport-actions';

/** A control the student types in: the shortcuts leave its keys alone. */
function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      target instanceof HTMLSelectElement ||
      target.isContentEditable ||
      target.closest('[contenteditable]:not([contenteditable="false"])') !== null)
  );
}

/** What a key does, or null when it is not a shortcut. Undo and redo answer either case of the letter. */
function shortcut(event: KeyboardEvent): ((context: Context) => void) | null {
  switch (event.key) {
    case ' ':
      return event.shiftKey ? stepAccess : step;
    case 'r':
    case 'R':
      return toggleRun;
    case 'z':
    case 'Z':
      return undo;
    case 'y':
    case 'Y':
      return redo;
    case '?':
      return (context) => {
        toggleSheet(context);
      };
    default:
      return null;
  }
}

/** Closes what Escape closes: the sheet, or the editor; false when nothing was open. */
function escape(context: Context): boolean {
  const { ui } = context;
  if (ui.sheetOpen) {
    toggleSheet(context, false);
    return true;
  }
  if (ui.editor) {
    closeEditor(context);
    return true;
  }
  return false;
}

/**
 * A key pressed inside the tool. It is ignored with Ctrl, Meta or Alt held, and in a control the
 * student types in, except for Enter in the address and value boxes, which issues, Enter in an
 * editor's field, which applies, and Escape, which closes an open sheet or editor unless the
 * student is typing outside it. Enter on a focused way or memory word opens its editor. Inside an
 * open editor the other keys are the browser's. `preventDefault` is called only when a shortcut
 * fires.
 */
export function onKeydown(context: Context, event: KeyboardEvent, action: Action | null): void {
  if (event.ctrlKey || event.metaKey || event.altKey || event.isComposing) return;
  const typing = isTyping(event.target);
  // A control of an open editor keeps its own keys: Space presses its button, and Enter applies.
  const inEditor =
    event.target instanceof Element && event.target.closest('.cache-explorer__editor') !== null;
  if (event.key === 'Escape') {
    // A box the student is typing in keeps its focus, unless it is inside the sheet or an editor.
    const inside =
      event.target instanceof Element &&
      event.target.closest('.cache-explorer__sheet, [role="dialog"]') !== null;
    if ((!typing || inside) && escape(context)) event.preventDefault();
    return;
  }
  if (typing) {
    const box = action?.name === 'addr' || action?.name === 'value';
    if (event.key === 'Enter' && box) {
      event.preventDefault();
      issue(context);
    } else if (event.key === 'Enter' && inEditor) {
      event.preventDefault();
      applyEditor(context);
    }
    return;
  }
  if (inEditor) return;
  // Enter on a focused way or memory word opens its editor.
  if (event.key === 'Enter' && event.target === action?.element) {
    if (openFocused(context, action)) event.preventDefault();
    return;
  }
  const run = shortcut(event);
  if (!run) return;
  event.preventDefault();
  // A button that has focus would click on the key's release as well; the release is left unhandled.
  if (event.key === ' ' && event.target instanceof HTMLButtonElement) context.ui.spaceHeld = true;
  run(context);
}

/** The release of Space that a shortcut has handled must not click the button that has focus. */
export function onKeyup(context: Context, event: KeyboardEvent): void {
  if (event.key !== ' ' || !context.ui.spaceHeld) return;
  context.ui.spaceHeld = false;
  event.preventDefault();
}
