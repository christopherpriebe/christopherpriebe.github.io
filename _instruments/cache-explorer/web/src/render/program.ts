// The program drawer: a line that opens and closes it, and under it the listing or the text box on
// the left, and on the right the input type, a reminder of the syntax, and the button that swaps
// the listing for the box. The listing is the loaded program with its current line marked, or,
// while the text is edited or no program is loaded, the text as written, with the lines the core
// found wrong marked. The core reads the text on every change; nothing here parses it.

import type { ParseError, ParseResult, ProgramFormat } from '../../pkg/cache_explorer';
import { commit, control, type Put } from '../controls';
import { copy } from '../copy';
import { cls, h } from '../dom';
import type { UiState } from '../ui-state';
import type { Snapshot } from './index';

/** The parts of the drawer a root keeps between renders. */
export interface ProgramDrawer {
  element: HTMLElement;
  /** The line that opens the drawer. */
  head: HTMLElement;
  /** The drawer's content: rebuilt on every render, but for the listing and the text box. */
  body: HTMLElement;
  /** The listing: kept, so that it keeps its scroll while the cursor and the marks move. */
  listing: HTMLElement;
}

export function createProgramDrawer(): ProgramDrawer {
  const head = h('div', { class: 'cache-explorer__program-head' });
  const body = h('div', { class: 'cache-explorer__program-body' });
  const listing = h('div', {
    class: 'cache-explorer__listing',
    role: 'list',
    'data-action': 'program-edit',
  });
  return {
    element: h('div', { class: 'cache-explorer__program' }, head, body),
    head,
    body,
    listing,
  };
}

/** What the drawer shows: the run's program, or the text the student has written. */
interface Shown {
  /** The loaded program with its current line, or the written text. */
  kind: 'loaded' | 'written' | 'empty';
  lines: string[];
  currentLine: number | null;
  /** The drawer's line, next to its name. */
  summary: string;
}

/** The run's program whenever it holds one and the text is not being edited; else the text. */
function shown(snap: Snapshot, ui: UiState): Shown {
  const { text, editing } = ui.program;
  const loaded = snap.state.program;
  if (loaded && !editing) {
    return {
      kind: 'loaded',
      lines: loaded.lines.map((line) => line.text),
      currentLine: loaded.currentLine ?? null,
      summary: loaded.summary,
    };
  }
  const draft: ParseResult | undefined = snap.programDraft;
  if (text === '' || !draft) {
    return { kind: 'empty', lines: [], currentLine: null, summary: copy.program.none };
  }
  return {
    kind: 'written',
    lines: text.split('\n'),
    currentLine: null,
    summary: draft.summary,
  };
}

/** The listing's rows: the line number, the mark, and the text with its spaces kept. */
function rows(
  lines: readonly string[],
  current: number | null,
  wrong: ReadonlySet<number>,
): Node[] {
  return lines.map((text, index) => {
    const number = index + 1;
    const here = number === current;
    const bad = wrong.has(number);
    return h(
      'div',
      {
        role: 'listitem',
        class: cls(
          'cache-explorer__listing-row',
          here && 'cache-explorer__listing-row--current',
          bad && 'cache-explorer__listing-row--wrong',
        ),
        'data-line': number,
      },
      h('span', { class: 'cache-explorer__listing-number' }, number),
      h(
        'span',
        { class: 'cache-explorer__listing-mark', 'aria-hidden': 'true' },
        here ? '▸' : bad ? '!' : '',
      ),
      h('span', { class: 'cache-explorer__listing-text' }, text === '' ? ' ' : text),
      here && h('span', { class: 'cache-explorer__visually-hidden' }, ' next'),
      bad && h('span', { class: 'cache-explorer__visually-hidden' }, ' has an error'),
    );
  });
}

/** Brings the current row, when there is one, into view of the listing's scroller. */
function reveal(listing: HTMLElement): void {
  const row = listing.querySelector<HTMLElement>('.cache-explorer__listing-row--current');
  if (!row) return;
  const top = row.offsetTop;
  const bottom = top + row.offsetHeight;
  if (top < listing.scrollTop) listing.scrollTop = top;
  else if (bottom > listing.scrollTop + listing.clientHeight) {
    listing.scrollTop = bottom - listing.clientHeight;
  }
}

function textBox(root: HTMLElement, format: ProgramFormat, invalid: boolean): HTMLTextAreaElement {
  const box = control(root, 'program-text', () =>
    h('textarea', {
      class: 'cache-explorer__program-text',
      'data-action': 'program-text',
      spellcheck: 'false',
      autocomplete: 'off',
      autocapitalize: 'off',
      rows: 7,
    }),
  );
  box.setAttribute('aria-label', copy.program.text[format]);
  if (invalid) box.setAttribute('aria-invalid', 'true');
  else box.removeAttribute('aria-invalid');
  return box;
}

/** What the core found wrong, in source order: each row selects its place in the text box. */
function errorList(errors: readonly ParseError[], format: ProgramFormat): HTMLElement {
  return h(
    'ul',
    { class: 'cache-explorer__program-errors', 'aria-label': copy.program.errors[format] },
    ...errors.map((error) =>
      h(
        'li',
        null,
        h(
          'button',
          {
            type: 'button',
            class: 'cache-explorer__program-error',
            'data-action': 'program-error',
            'data-line': error.line,
            'data-column': error.column,
            'data-length': error.length,
          },
          error.text,
        ),
      ),
    ),
  );
}

function side(ui: UiState, editing: boolean): HTMLElement {
  const { format } = ui.program;
  const option = (each: ProgramFormat): HTMLElement =>
    h(
      'button',
      {
        type: 'button',
        class: 'cache-explorer__seg-option',
        'data-action': 'program-format',
        'data-option': each,
        'aria-pressed': String(format === each),
      },
      copy.program.formats[each],
    );
  return h(
    'div',
    { class: 'cache-explorer__program-side' },
    h(
      'div',
      {
        class: 'cache-explorer__seg cache-explorer__seg--inline',
        role: 'group',
        'aria-label': copy.program.format,
      },
      option('asm'),
      option('trace'),
    ),
    h(
      'p',
      { class: 'cache-explorer__program-help' },
      ...copy.program.help[format].flatMap((line, index) =>
        index === 0 ? [line] : [h('br'), line],
      ),
    ),
    h(
      'button',
      {
        type: 'button',
        class: 'cache-explorer__link cache-explorer__program-edit',
        'data-action': 'program-edit',
      },
      editing ? copy.program.done : copy.program.edit,
    ),
  );
}

export function renderProgram(
  root: HTMLElement,
  drawer: ProgramDrawer,
  snap: Snapshot,
  ui: UiState,
): void {
  const { open, editing, format, text } = ui.program;
  const view = shown(snap, ui);
  const draft = snap.programDraft;
  const errors = draft?.status === 'invalid' ? draft.errors : [];
  drawer.head.replaceChildren(
    h(
      'button',
      {
        type: 'button',
        class: 'cache-explorer__program-toggle',
        'data-action': 'program-toggle',
        'aria-expanded': String(open),
      },
      h(
        'span',
        { class: 'cache-explorer__program-glyph', 'aria-hidden': 'true' },
        open ? '−' : '+',
      ),
      h('span', { class: 'cache-explorer__program-title' }, copy.program.title[format]),
      h('span', { class: 'cache-explorer__program-summary' }, view.summary),
    ),
  );
  if (!open) {
    drawer.body.replaceChildren();
    return;
  }
  const moved = snap.state.position.pos !== ui.lastPos;
  const wrong = new Set(view.kind === 'written' ? errors.map((error) => error.line) : []);
  if (!editing) {
    drawer.listing.setAttribute('aria-label', copy.program.listing[format]);
    drawer.listing.replaceChildren(
      ...(view.kind === 'empty'
        ? [h('p', { class: 'cache-explorer__listing-empty' }, copy.program.empty[format])]
        : rows(view.lines, view.currentLine, wrong)),
    );
  }
  commit(drawer.body, (put: Put) => [
    h(
      'div',
      { class: 'cache-explorer__program-main' },
      editing ? put(textBox(root, format, errors.length > 0), text) : put(drawer.listing),
      errors.length > 0 && errorList(errors, format),
    ),
    side(ui, editing),
  ]);
  if (!editing && moved) reveal(drawer.listing);
}
