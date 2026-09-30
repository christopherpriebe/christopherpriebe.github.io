// What the editors do. Opening one puts a form in the UI state; typing changes the form's texts;
// the core reads them again on every render. Applying sends the patch the core made of the form,
// or the edit it offered instead, as an op, and the core has the last word on both.

import type { EditOutcome, EditPatch, EditTarget } from '../pkg/cache_explorer';
import type { Action } from './actions';
import type { ActionHandler, Context } from './context';
import { refusal } from './context';
import { primaryOf } from './render/editor';
import { stepAccess } from './transport-actions';
import type { MemoryEditor, WayEditor } from './ui-state';

/** The selector of the element that opened an editor, which gets focus back when it closes. */
function opener(editor: WayEditor | MemoryEditor): string {
  return editor.kind === 'way'
    ? `[data-action="edit-way"][data-set="${String(editor.set)}"][data-way="${String(editor.way)}"]`
    : `[data-action="edit-memory"][data-addr="${String(editor.addr)}"]`;
}

/** Puts focus in the editor's first field, once it has been drawn. */
function focusFirstField(context: Context): void {
  const field = context.root.querySelector<HTMLElement>(
    '.cache-explorer__editor input[data-action]',
  );
  field?.focus();
}

/** Opens the way editor on the cell, with the fields as the way has them. */
function openWay(context: Context, action: Action): void {
  const { ui, session } = context;
  const set = Number(action.params.set);
  const way = Number(action.params.way);
  // A press on the popover itself lands on the cell it is drawn in: the editor is already open.
  if (ui.editor?.kind === 'way' && ui.editor.set === set && ui.editor.way === way) return;
  const { cache, geometry } = session.state();
  const view = cache.sets[set]?.ways[way];
  if (!view) return;
  ui.editor = {
    kind: 'way',
    set,
    way,
    // An invalid way opens with valid 0 and an empty line, so a bare Apply changes nothing.
    form: {
      valid: view.valid,
      lineText: view.lineText ?? '',
      wordTexts: Array.from({ length: geometry.wordsPerLine }, () => ''),
      dirty: view.valid && view.dirty,
    },
    touched: { valid: false, line: false, dirty: false },
    typed: Array.from({ length: geometry.wordsPerLine }, () => false),
  };
  context.refresh();
  focusFirstField(context);
}

/** Opens the memory editor on the word, showing the value it has. */
function openMemory(context: Context, action: Action): void {
  const { ui, session } = context;
  const addr = Number(action.params.addr);
  if (ui.editor?.kind === 'memory' && ui.editor.addr === addr) return;
  ui.editor = {
    kind: 'memory',
    addr,
    addrText: session.decode(addr).addrText,
    valueText: '',
    typed: false,
    shown: action.element.dataset.value ?? '',
  };
  context.refresh();
  focusFirstField(context);
}

/** Closes the open editor and gives focus back to the cell or word that opened it. */
export function closeEditor(context: Context): void {
  const { ui, root } = context;
  const { editor } = ui;
  if (!editor) return;
  ui.editor = null;
  context.refresh();
  root.querySelector<HTMLElement>(opener(editor))?.focus({ preventScroll: true });
}

/** The core's answer for the open editor's form as it is now. */
function outcomeOf(context: Context): {
  outcome: EditOutcome;
  target: EditTarget;
  patch: EditPatch | undefined;
} | null {
  const { ui, session } = context;
  const { editor } = ui;
  if (editor?.kind === 'way') {
    const draft = session.draftEdit(editor.set, editor.way, editor.form);
    return {
      outcome: draft.outcome,
      target: { kind: 'way', set: editor.set, way: editor.way },
      patch: draft.patch,
    };
  }
  if (editor?.kind === 'memory') {
    const draft = session.draftMemoryEdit(editor.addr, editor.typed ? editor.valueText : '');
    return {
      outcome: draft.outcome,
      target: { kind: 'memory', addr: editor.addr },
      patch: draft.patch,
    };
  }
  return null;
}

/** Sends one edit as an op, and closes the editor when the core takes it. */
function send(context: Context, target: EditTarget, patch: EditPatch): void {
  context.setRunning(false);
  try {
    context.session.applyOp({ op: 'edit', target, patch });
    context.ui.status = '';
  } catch (error) {
    context.ui.status = refusal(error);
    context.refresh();
    return;
  }
  closeEditor(context);
}

/**
 * The editor's primary button: a plan applies the form's patch; a refusal that offers another
 * edit applies that one; one that offers none does nothing.
 */
export function applyEditor(context: Context): void {
  const answer = outcomeOf(context);
  if (!answer || !primaryOf(answer.outcome).enabled) return;
  const { outcome, target, patch } = answer;
  if (outcome.outcome === 'plan') {
    if (patch) send(context, target, patch);
  } else if (outcome.suggestion) {
    send(context, outcome.suggestion.target, outcome.suggestion.patch);
  }
}

/** The way editor's form, or null when another editor, or none, is open. */
function wayForm(context: Context): WayEditor | null {
  const { editor } = context.ui;
  return editor?.kind === 'way' ? editor : null;
}

function chooseValid(context: Context, action: Action): void {
  const editor = wayForm(context);
  if (!editor) return;
  editor.form.valid = action.params.option === '1';
  editor.touched.valid = true;
  context.refresh();
}

function chooseDirty(context: Context, action: Action): void {
  const editor = wayForm(context);
  if (!editor) return;
  editor.form.dirty = action.params.option === '1';
  editor.touched.dirty = true;
  context.refresh();
}

/** A rank chosen on a button, or, in a set with many ways, in the list the buttons give way to. */
function chooseRank(context: Context, action: Action): void {
  const editor = wayForm(context);
  if (!editor) return;
  const { element } = action;
  if (element instanceof HTMLSelectElement) editor.form.rank = Number(element.value);
  else editor.form.rank = Number(action.params.option);
  context.refresh();
}

function typeLine(context: Context, action: Action): void {
  const editor = wayForm(context);
  if (!editor || !(action.element instanceof HTMLInputElement)) return;
  editor.form.lineText = action.element.value;
  editor.touched.line = true;
  context.refresh();
}

function typeWord(context: Context, action: Action): void {
  const editor = wayForm(context);
  if (!editor || !(action.element instanceof HTMLInputElement)) return;
  const index = Number(action.params.pos);
  editor.form.wordTexts[index] = action.element.value;
  editor.typed[index] = true;
  context.refresh();
}

/** A word input the student left empty goes back to showing what the core reads for it. */
function leaveWord(context: Context, action: Action): void {
  const editor = wayForm(context);
  if (!editor || !(action.element instanceof HTMLInputElement)) return;
  const index = Number(action.params.pos);
  if (action.element.value.trim() !== '') return;
  editor.form.wordTexts[index] = '';
  editor.typed[index] = false;
  action.element.value = '';
  context.refresh();
}

function typeValue(context: Context, action: Action): void {
  const { editor } = context.ui;
  if (editor?.kind !== 'memory' || !(action.element instanceof HTMLInputElement)) return;
  editor.valueText = action.element.value;
  editor.typed = true;
  context.refresh();
}

/** The editors' handlers, by the `data-action` and the event they answer. */
export const editActions = {
  clicks: new Map<string, ActionHandler>([
    ['edit-way', openWay],
    ['edit-memory', openMemory],
    ['edit-valid', chooseValid],
    ['edit-dirty', chooseDirty],
    [
      'edit-rank',
      (context, action) => {
        // Opening the list is a click too; the choice arrives as a change.
        if (!(action.element instanceof HTMLSelectElement)) chooseRank(context, action);
      },
    ],
    ['edit-apply', applyEditor],
    ['edit-suggest', applyEditor],
    ['edit-end', stepAccess],
    ['edit-cancel', closeEditor],
    ['edit-close', closeEditor],
  ]),
  inputs: new Map<string, ActionHandler>([
    ['edit-line', typeLine],
    ['edit-word', typeWord],
    ['edit-value', typeValue],
  ]),
  changes: new Map<string, ActionHandler>([
    ['edit-word', leaveWord],
    ['edit-rank', chooseRank],
  ]),
};

/** Opens the editor of the focused cell or word, as Enter does. */
export function openFocused(context: Context, action: Action): boolean {
  const open =
    action.name === 'edit-way' ? openWay : action.name === 'edit-memory' ? openMemory : null;
  if (!open) return false;
  open(context, action);
  return true;
}
