// The two editors: the way popover, which opens under a way's cell, and the memory editor, which
// opens under the memory table. Both draw what the core says about the form as it stands: the
// fields as it read them, a bar that says whether the edit would be applied and why, the lines
// it would change, and the one button the core offers. Nothing here works out whether an edit is
// allowed; a refusal, and the edit offered instead, are the core's.

import type {
  EditDraft,
  EditOutcome,
  FieldInput,
  MemoryEditDraft,
  WordMark,
} from '../../pkg/cache_explorer';
import { control, type Put } from '../controls';
import { copy } from '../copy';
import { cls, h, type Child } from '../dom';
import type { MemoryEditor, UiState, WayEditor } from '../ui-state';
import type { Snapshot } from './index';

/** The id of the status bar, which the primary button points to. */
const STATUS_ID = 'cache-explorer-editor-status';

/** The way editor and the draft the core made of its form, when a way editor is open. */
export function wayEditorOf(
  snap: Snapshot,
  ui: UiState,
): { editor: WayEditor; draft: EditDraft } | null {
  const { editor } = ui;
  const draft = snap.editor;
  if (editor?.kind !== 'way' || !draft || !('line' in draft)) return null;
  return { editor, draft };
}

/** The memory editor and its draft, when a memory editor is open. */
export function memoryEditorOf(
  snap: Snapshot,
  ui: UiState,
): { editor: MemoryEditor; draft: MemoryEditDraft } | null {
  const { editor } = ui;
  const draft = snap.editor;
  if (editor?.kind !== 'memory' || !draft || !('value' in draft)) return null;
  return { editor, draft };
}

/** What the editor's button does and says: the plan's, the offer's, or nothing to apply. */
export interface Primary {
  action: 'edit-apply' | 'edit-suggest';
  label: string;
  enabled: boolean;
  /** The lines the edit would change, as the core words them. */
  preview: readonly string[];
}

/** The button the core's answer comes to: a plan applies, an offer applies the other edit. */
export function primaryOf(outcome: EditOutcome): Primary {
  if (outcome.outcome === 'plan') {
    return {
      action: 'edit-apply',
      label: outcome.action,
      enabled: true,
      preview: outcome.preview,
    };
  }
  const { suggestion } = outcome;
  if (suggestion) {
    return {
      action: 'edit-suggest',
      label: suggestion.action,
      enabled: true,
      preview: suggestion.preview,
    };
  }
  return {
    action: 'edit-apply',
    label: outcome.reason === 'noChange' ? copy.editor.apply : copy.editor.cannot,
    enabled: false,
    preview: [],
  };
}

function button(
  label: string,
  action: string,
  extra: string | false,
  attrs: Record<string, string | number | boolean | null> = {},
): HTMLButtonElement {
  return h(
    'button',
    {
      type: 'button',
      class: cls('cache-explorer__button', extra),
      'data-action': action,
      ...attrs,
    },
    label,
  );
}

/** Two or more options in a row: the pressed one is navy, as in the configuration bar. */
function segmented(
  label: string,
  action: string,
  options: readonly number[],
  pressed: number | undefined,
): HTMLElement {
  return h(
    'div',
    {
      class: 'cache-explorer__seg cache-explorer__seg--inline',
      role: 'group',
      'aria-label': label,
    },
    ...options.map((value) =>
      h(
        'button',
        {
          type: 'button',
          class: 'cache-explorer__seg-option',
          'data-action': action,
          'data-option': value,
          'aria-pressed': String(value === pressed),
        },
        String(value),
      ),
    ),
  );
}

/** The most ranks the editor lays out as buttons: more than fit a popover's width go in a list. */
const RANK_BUTTONS = 6;

/** The rank: a button for each when there are few, and otherwise a list as long as the set. */
function rankControl(ways: number, pressed: number): HTMLElement {
  const ranks = Array.from({ length: ways }, (_, way) => way);
  if (ways <= RANK_BUTTONS) return segmented(copy.editor.rank, 'edit-rank', ranks, pressed);
  return h(
    'select',
    {
      class: 'cache-explorer__select cache-explorer__select--edit',
      'data-action': 'edit-rank',
      'aria-label': copy.editor.rank,
    },
    ...ranks.map((rank) => h('option', { value: rank, selected: rank === pressed }, String(rank))),
  );
}

/** Whether a field's text did not read, and the flag a screen reader is given for it. */
function unread(input: FieldInput | undefined): boolean {
  return input?.status === 'invalid';
}

const MARK_CLASS: Record<WordMark, string | false> = {
  same: false,
  changed: 'cache-explorer__field--changed',
  differsFromMemory: 'cache-explorer__field--differs',
  changedAndDiffers: 'cache-explorer__field--changed cache-explorer__field--differs',
};

/** The status bar: what the core says of the edit, its lines, and the way to make it possible. */
function statusBar(outcome: EditOutcome, preview: readonly string[], extra: Child): HTMLElement {
  return h(
    'div',
    {
      class: `cache-explorer__editor-status cache-explorer__editor-status--${outcome.tone}`,
      id: STATUS_ID,
    },
    h('div', { class: 'cache-explorer__editor-title' }, outcome.title),
    h('div', { class: 'cache-explorer__editor-text' }, outcome.text),
    extra,
    preview.length > 0 &&
      h(
        'ul',
        { class: 'cache-explorer__editor-lines' },
        ...preview.map((line) => h('li', null, line)),
      ),
  );
}

function head(title: string): HTMLElement {
  return h(
    'div',
    { class: 'cache-explorer__editor-head' },
    h('span', { class: 'cache-explorer__label cache-explorer__label--strong' }, title),
    button('×', 'edit-close', 'cache-explorer__editor-close', {
      'aria-label': copy.editor.close,
    }),
  );
}

function actions(primary: Primary): HTMLElement {
  return h(
    'div',
    { class: 'cache-explorer__editor-actions' },
    button(primary.label, primary.action, 'cache-explorer__button--primary', {
      disabled: !primary.enabled,
      'aria-describedby': STATUS_ID,
    }),
    button(copy.editor.cancel, 'edit-cancel', false),
  );
}

/** A row of the popover's grid: the field's name, then what sets it. */
function row(label: string, ...content: Child[]): Child[] {
  return [h('span', { class: 'cache-explorer__editor-label' }, label), ...content];
}

function wordInputs(
  put: Put,
  root: HTMLElement,
  editor: WayEditor,
  draft: EditDraft,
  marks: readonly WordMark[],
  fields: number,
): HTMLElement {
  return h(
    'div',
    {
      class: 'cache-explorer__editor-words',
      style: `--cache-explorer-words: ${String(Math.min(fields, 4))}`,
    },
    ...Array.from({ length: fields }, (_, index) => {
      const input = control(root, `edit-word-${String(index)}`, () =>
        h('input', {
          type: 'text',
          class: 'cache-explorer__field cache-explorer__field--edit',
          'data-action': 'edit-word',
          'data-pos': index,
          'aria-label': `${copy.editor.word} ${String(index)}`,
          autocomplete: 'off',
          spellcheck: 'false',
        }),
      );
      input.className = cls(
        'cache-explorer__field',
        'cache-explorer__field--edit',
        MARK_CLASS[marks[index] ?? 'same'],
      );
      if (unread(draft.words[index])) input.setAttribute('aria-invalid', 'true');
      else input.removeAttribute('aria-invalid');
      const shown = editor.typed[index]
        ? (editor.form.wordTexts[index] ?? '')
        : (draft.wordTexts[index] ?? '');
      return put(input, shown);
    }),
  );
}

/**
 * The way popover, drawn under the cell of the way it edits. `put` places the form's inputs,
 * which live as long as the root does. It sits at the cell's left, or at its right when the way
 * is in the right half of its set, so that it opens toward the table as the design's does.
 */
export function wayEditor(
  put: Put,
  root: HTMLElement,
  snap: Snapshot,
  editor: WayEditor,
  draft: EditDraft,
): HTMLElement {
  const { cache, geometry } = snap.state;
  const { outcome } = draft;
  const primary = primaryOf(outcome);
  const marks = outcome.wordMarks;
  const line = control(root, 'edit-line', () =>
    h('input', {
      type: 'text',
      class: 'cache-explorer__field cache-explorer__field--edit-line',
      'data-action': 'edit-line',
      'aria-label': copy.editor.lineAddress,
      autocomplete: 'off',
      spellcheck: 'false',
    }),
  );
  if (unread(draft.line)) line.setAttribute('aria-invalid', 'true');
  else line.removeAttribute('aria-invalid');
  const right = geometry.ways > 1 && editor.way >= geometry.ways / 2;
  const rank = draft.result.rank;
  const title = `${copy.editor.edit} set ${String(editor.set)} · way ${String(editor.way)}`;
  return h(
    'div',
    {
      class: cls('cache-explorer__editor', 'cache-explorer__editor--way'),
      role: 'dialog',
      'aria-label': title,
      'data-side': right ? 'right' : 'left',
    },
    head(title),
    h(
      'div',
      { class: 'cache-explorer__editor-grid' },
      ...row(
        copy.editor.valid,
        segmented(copy.editor.valid, 'edit-valid', [0, 1], editor.form.valid ? 1 : 0),
      ),
      ...row(
        copy.editor.lineAddress,
        h(
          'div',
          { class: 'cache-explorer__editor-line' },
          put(line, editor.form.lineText),
          h(
            'span',
            { class: 'cache-explorer__editor-swatches' },
            h('span', { class: 'cache-explorer__swatch cache-explorer__swatch--tag' }),
            `${copy.editor.tag} ${draft.tagText ?? copy.editor.none}`,
            geometry.indexBits > 0 &&
              h('span', { class: 'cache-explorer__swatch cache-explorer__swatch--index' }),
            geometry.indexBits > 0 && `${copy.editor.index} ${draft.indexText ?? copy.editor.none}`,
          ),
        ),
      ),
      ...row(copy.editor.data, wordInputs(put, root, editor, draft, marks, geometry.wordsPerLine)),
      ...(cache.showDirty
        ? row(
            copy.editor.dirty,
            segmented(copy.editor.dirty, 'edit-dirty', [0, 1], editor.form.dirty ? 1 : 0),
          )
        : []),
      ...(cache.showRank && cache.rankHeader && rank
        ? row(
            `${copy.cache.rank[cache.rankHeader]} ${copy.editor.rank}`,
            rankControl(geometry.ways, rank.value),
          )
        : []),
    ),
    statusBar(outcome, primary.preview, goToEnd(outcome)),
    actions(primary),
  );
}

/** The way out of a refusal that only asks for the access to be finished. */
function goToEnd(outcome: EditOutcome): Child {
  if (outcome.outcome !== 'refusal' || outcome.reason !== 'midAccess') return null;
  return h(
    'div',
    { class: 'cache-explorer__editor-end' },
    button(copy.editor.goToEnd, 'edit-end', false),
  );
}

/** The memory editor, drawn under the memory table. */
export function memoryEditor(
  put: Put,
  root: HTMLElement,
  editor: MemoryEditor,
  draft: MemoryEditDraft,
): HTMLElement {
  const { outcome } = draft;
  const primary = primaryOf(outcome);
  const value = control(root, 'edit-value', () =>
    h('input', {
      type: 'text',
      class: 'cache-explorer__field cache-explorer__field--edit-value',
      'data-action': 'edit-value',
      'aria-label': copy.editor.value,
      autocomplete: 'off',
      spellcheck: 'false',
    }),
  );
  if (unread(draft.value)) value.setAttribute('aria-invalid', 'true');
  else value.removeAttribute('aria-invalid');
  const title = `${copy.editor.edit} memory ${editor.addrText}`;
  return h(
    'div',
    {
      class: 'cache-explorer__editor cache-explorer__editor--memory',
      role: 'dialog',
      'aria-label': title,
    },
    head(title),
    h(
      'label',
      { class: 'cache-explorer__editor-value' },
      h('span', { class: 'cache-explorer__editor-label' }, copy.editor.value),
      put(value, editor.valueText),
    ),
    statusBar(outcome, primary.preview, goToEnd(outcome)),
    actions(primary),
  );
}
