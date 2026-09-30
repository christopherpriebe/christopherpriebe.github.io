// The main memory table: a caption and the "Jump to" box over one row per line the run has
// touched or the student asked to see, with a column for each word of a line. The core hands over a
// window of the rows, and the table stands in for the rest with two blank rows, so its scrollbar is
// the length of the whole list and the rows in view are built.

import type { MemoryLineView, MemoryWordView } from '../../pkg/cache_explorer';
import { commit, control } from '../controls';
import { copy } from '../copy';
import { cls, h } from '../dom';
import { MEMORY_ROW_PX } from '../metrics';
import type { UiState } from '../ui-state';
import type { Snapshot } from './index';
import { cellKey, type Marks } from './marks';

/** The parts of the memory box a root keeps between renders. */
export interface MemoryBox {
  element: HTMLElement;
  /** The caption and the jump box: rebuilt, but the jump box in it is kept. */
  header: HTMLElement;
  /** The scroller: kept, so its scroll position is, and its content rebuilt. */
  scroller: HTMLElement;
}

export function createMemory(): MemoryBox {
  const header = h('div', { class: 'cache-explorer__box-head' });
  const scroller = h('div', { class: 'cache-explorer__scroller', 'data-scroll': 'memory' });
  const element = h(
    'div',
    { class: 'cache-explorer__box cache-explorer__box--memory' },
    header,
    scroller,
  );
  return { element, header, scroller };
}

/** The id of the message under the jump box, which the box points to while it is shown. */
const JUMP_NOTE_ID = 'cache-explorer-jump-note';

function jumpBox(root: HTMLElement, ui: UiState): HTMLInputElement {
  const input = control(root, 'jump', () =>
    h('input', {
      type: 'text',
      class: 'cache-explorer__field cache-explorer__field--jump',
      'data-action': 'jump',
      'aria-label': copy.memory.jumpLabel,
      placeholder: copy.memory.jumpPlaceholder,
      autocomplete: 'off',
      spellcheck: 'false',
    }),
  );
  if (ui.jumpError === null) {
    input.removeAttribute('aria-invalid');
    input.removeAttribute('aria-describedby');
  } else {
    input.setAttribute('aria-invalid', 'true');
    input.setAttribute('aria-describedby', JUMP_NOTE_ID);
  }
  return input;
}

function wordCell(word: MemoryWordView, marks: Marks): HTMLElement {
  const { stale } = word;
  return h(
    'td',
    {
      role: 'cell',
      class: cls('cache-explorer__mem-word', ...marks.classes(cellKey.memory(word.addr))),
      'data-action': 'edit-memory',
      'data-addr': word.addr,
      tabindex: 0,
      title: stale ? stale.text : word.description,
      'aria-label': `${stale ? `${word.description} ${stale.text}` : word.description} ${copy.memory.edit}`,
    },
    stale && h('span', { class: 'cache-explorer__stale-bar', 'aria-hidden': 'true' }),
    stale && h('span', { class: 'cache-explorer__badge' }, copy.memory.stale),
    word.text,
  );
}

function lineRow(row: MemoryLineView, index: number, marks: Marks): HTMLElement {
  return h(
    'tr',
    { role: 'row', class: 'cache-explorer__mem-row', 'data-row': index, 'data-line': row.line },
    h(
      'th',
      {
        scope: 'row',
        role: 'rowheader',
        class: cls(
          'cache-explorer__mem-line',
          row.onBus && 'cache-explorer__mem-line--bus',
          row.watched && 'cache-explorer__mem-line--watched',
        ),
        title: row.watched ? copy.memory.watched : null,
      },
      row.lineText,
    ),
    h('td', { role: 'cell', class: 'cache-explorer__mem-quiet' }, row.tagText),
    h('td', { role: 'cell', class: 'cache-explorer__mem-quiet' }, String(row.set)),
    ...row.words.map((word) => wordCell(word, marks)),
  );
}

/** A blank row as tall as the `rows` it stands for; nothing to a screen reader. */
function spacer(rows: number): HTMLElement | null {
  if (rows <= 0) return null;
  return h('tr', {
    role: 'row',
    class: 'cache-explorer__mem-spacer',
    'aria-hidden': 'true',
    style: `height: ${String(rows * MEMORY_ROW_PX)}px`,
  });
}

function headRow(heads: readonly string[]): HTMLElement {
  const { line, tag, set } = copy.memory.heads;
  const head = (text: string, field: string): HTMLElement =>
    h(
      'th',
      {
        scope: 'col',
        role: 'columnheader',
        class: `cache-explorer__mem-head cache-explorer__mem-head--${field}`,
      },
      text,
    );
  return h(
    'tr',
    { role: 'row', class: 'cache-explorer__mem-heads' },
    head(line, 'line'),
    head(tag, 'tag'),
    head(set, 'set'),
    ...heads.map((each) => head(each, 'word')),
  );
}

/** Brings row `index`, which is built, into view if it is not, keeping the head clear of it. */
function reveal(scroller: HTMLElement, index: number): void {
  const row = scroller.querySelector<HTMLElement>(`[data-row='${String(index)}']`);
  const head = scroller.querySelector<HTMLElement>('.cache-explorer__mem-heads');
  if (!row) return;
  const top = row.offsetTop;
  const bottom = top + row.offsetHeight;
  const clear = head?.offsetHeight ?? 0;
  if (top < scroller.scrollTop + clear) scroller.scrollTop = Math.max(0, top - clear);
  else if (bottom > scroller.scrollTop + scroller.clientHeight) {
    scroller.scrollTop = bottom - scroller.clientHeight;
  }
}

/**
 * Draws the memory box. `reveal` is the row to bring into view: the one on the bus when the cursor
 * moved, or the one the student jumped to.
 */
export function renderMemory(
  root: HTMLElement,
  memory: MemoryBox,
  snap: Snapshot,
  ui: UiState,
  marks: Marks,
  revealRow: number | null,
): void {
  const { memory: table } = snap.state;
  commit(memory.header, (put) => [
    h(
      'span',
      { class: 'cache-explorer__label cache-explorer__label--strong' },
      `${copy.memory.caption} · ${table.caption}`,
    ),
    h(
      'label',
      { class: 'cache-explorer__jump' },
      h('span', { class: 'cache-explorer__label' }, copy.memory.jump),
      put(jumpBox(root, ui), ui.jumpText),
    ),
    // The core's reason, in the head, where the box it is about is.
    ...(ui.jumpError === null
      ? []
      : [h('p', { class: 'cache-explorer__note', id: JUMP_NOTE_ID }, ui.jumpError)]),
  ]);

  const { memoryFirst: first } = ui;
  const built = snap.memoryRows.length;
  const rows = h(
    'table',
    {
      role: 'table',
      class: 'cache-explorer__mem',
      style: `--cache-explorer-words: ${String(table.wordHeads.length)}`,
      'aria-label': copy.memory.caption,
    },
    h('thead', { role: 'rowgroup' }, headRow(table.wordHeads)),
    h(
      'tbody',
      { role: 'rowgroup' },
      spacer(first),
      ...snap.memoryRows.map((row, offset) => lineRow(row, first + offset, marks)),
      spacer(table.lineCount - first - built),
    ),
  );
  memory.scroller.replaceChildren(rows);
  if (table.lineCount === 0) {
    memory.scroller.append(h('p', { class: 'cache-explorer__empty' }, copy.memory.empty));
  }
  if (revealRow !== null) reveal(memory.scroller, revealRow);
}
