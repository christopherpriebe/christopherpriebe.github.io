// What the program drawer does. The drawer's text is typed input, not an op: nothing here loads it.
// It reaches the run with the next Step, Step access or Run, which pass it to the core along with
// the move (transport-actions.ts); the core adds the load first when the text has changed.

import type { ParseError } from '../pkg/cache_explorer';
import type { Action } from './actions';
import type { ActionHandler, Context } from './context';

const TEXT_BOX = 'textarea[data-action="program-text"]';

function textBox(context: Context): HTMLTextAreaElement | null {
  const found = context.root.querySelector(TEXT_BOX);
  return found instanceof HTMLTextAreaElement ? found : null;
}

function toggle(context: Context): void {
  context.ui.program.open = !context.ui.program.open;
  context.refresh();
}

/** Swaps the listing for the text box and back, and puts the caret in the box when it opens. */
function edit(context: Context): void {
  const { program } = context.ui;
  program.editing = !program.editing;
  context.refresh();
  if (program.editing) textBox(context)?.focus({ preventScroll: true });
}

function chooseFormat(context: Context, action: Action): void {
  const format = action.params.option;
  if (format !== 'asm' && format !== 'trace') return;
  const { program } = context.ui;
  if (program.format === format) return;
  // Only the text's format changes; a program the run holds is not touched.
  program.format = format;
  context.refresh();
}

function type(context: Context, action: Action): void {
  if (!(action.element instanceof HTMLTextAreaElement)) return;
  context.ui.program.text = action.element.value;
  context.refresh();
}

/**
 * Where an error is in the text, as the offsets of the box: the core counts columns in Unicode
 * scalar values, the box in UTF-16 units. A line past the text's last is read as the last.
 */
export function errorRange(
  text: string,
  error: Pick<ParseError, 'line' | 'column' | 'length'>,
): { start: number; end: number } {
  const lines = text.split('\n');
  const index = Math.min(error.line, lines.length) - 1;
  let start = 0;
  for (const before of lines.slice(0, index)) start += before.length + 1;
  const scalars = Array.from(lines[index] ?? '');
  const from = Math.min(Math.max(error.column - 1, 0), scalars.length);
  const to = Math.min(from + error.length, scalars.length);
  return {
    start: start + scalars.slice(0, from).join('').length,
    end: start + scalars.slice(0, to).join('').length,
  };
}

/** Opens the text box on the error's place and selects the part the core says to underline. */
function showError(context: Context, action: Action): void {
  const { program } = context.ui;
  const line = Number(action.params.line);
  const column = Number(action.params.column);
  const length = Number(action.params.length);
  if (!Number.isInteger(line) || !Number.isInteger(column) || !Number.isInteger(length)) return;
  program.editing = true;
  context.refresh();
  const box = textBox(context);
  if (!box) return;
  const { start, end } = errorRange(program.text, { line, column, length });
  box.focus({ preventScroll: true });
  box.setSelectionRange(start, end);
}

/** The drawer's handlers, by the `data-action` and the event they answer. */
export const programActions = {
  clicks: new Map<string, ActionHandler>([
    ['program-toggle', toggle],
    ['program-edit', edit],
    ['program-format', chooseFormat],
    ['program-error', showError],
  ]),
  inputs: new Map<string, ActionHandler>([['program-text', type]]),
};
