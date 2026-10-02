// The guided modules drawer, under the program drawer and built like it: a line that opens it, with
// the core's summary beside the name, and under it the modules as ruled rows. A row is the module's
// number, its title with a text badge when it needs Advanced, what it teaches, and the button that
// starts it. While a module runs the summary is the core's line about the run, and its row says so.

import type { ModuleCard, ModuleView } from '../../pkg/cache_explorer';
import { copy } from '../copy';
import { cls, h } from '../dom';
import type { UiState } from '../ui-state';
import type { Snapshot } from './index';
import { segments } from './segments';

/** The parts of the drawer a root keeps between renders. */
export interface ModulePicker {
  element: HTMLElement;
  head: HTMLElement;
  body: HTMLElement;
}

export function createModulePicker(): ModulePicker {
  const head = h('div', { class: 'cache-explorer__modules-head' });
  const body = h('div', { class: 'cache-explorer__modules-body' });
  return { element: h('div', { class: 'cache-explorer__modules' }, head, body), head, body };
}

/** The line next to the drawer's name: the run's, the list's, or that the list could not be had. */
function summaryOf(snap: Snapshot): string {
  if (snap.module) return snap.module.drawerSummary;
  if (snap.modules) return snap.modules.summary;
  return copy.modules.failed;
}

function row(card: ModuleCard, running: ModuleView | undefined, ui: UiState): HTMLElement {
  const here = running?.id === card.id;
  const starting = ui.modules.starting === card.id;
  return h(
    'li',
    { class: cls('cache-explorer__module-row', here && 'cache-explorer__module-row--running') },
    h('span', { class: 'cache-explorer__module-number' }, card.number),
    h(
      'div',
      { class: 'cache-explorer__module-card' },
      h(
        'div',
        { class: 'cache-explorer__module-title' },
        h('span', { class: 'cache-explorer__module-name' }, card.title),
        card.advanced &&
          h('span', { class: 'cache-explorer__module-badge' }, copy.modules.advanced),
        here && h('span', { class: 'cache-explorer__module-badge' }, copy.modules.running),
      ),
      h('p', { class: 'cache-explorer__module-about' }, ...segments(card.description)),
    ),
    h(
      'button',
      {
        type: 'button',
        class: 'cache-explorer__button cache-explorer__module-start',
        'data-action': 'module-start',
        'data-module': card.id,
        disabled: ui.modules.starting !== null,
      },
      starting ? copy.modules.starting : here ? copy.modules.startAgain : copy.modules.start,
      h('span', { class: 'cache-explorer__visually-hidden' }, ` ${card.title}`),
    ),
  );
}

/** Draws the drawer; a page with no modules to offer draws nothing at all. */
export function renderModulePicker(picker: ModulePicker, snap: Snapshot, ui: UiState): void {
  const { open, load, index, error } = ui.modules;
  // Nothing is drawn until the list has come or has failed, so a page that has it at once never shows the wait.
  if (load === 'none' || load === 'loading') {
    picker.head.replaceChildren();
    picker.body.replaceChildren();
    return;
  }
  picker.head.replaceChildren(
    h(
      'button',
      {
        type: 'button',
        class: 'cache-explorer__program-toggle',
        'data-action': 'modules-toggle',
        'aria-expanded': String(open),
      },
      h(
        'span',
        { class: 'cache-explorer__program-glyph', 'aria-hidden': 'true' },
        open ? '−' : '+',
      ),
      h('span', { class: 'cache-explorer__program-title' }, copy.modules.title),
      h('span', { class: 'cache-explorer__program-summary' }, summaryOf(snap)),
    ),
  );
  if (!open) {
    picker.body.replaceChildren();
    return;
  }
  picker.body.replaceChildren(
    ...(error === null
      ? []
      : [
          h(
            'div',
            {
              class: 'cache-explorer__module-note cache-explorer__module-note--refused',
              role: 'alert',
            },
            error,
          ),
        ]),
    load === 'failed'
      ? h(
          'div',
          { class: 'cache-explorer__modules-failed' },
          h('span', null, copy.modules.failedText),
          ' ',
          h(
            'button',
            {
              type: 'button',
              class: 'cache-explorer__link',
              'data-action': 'modules-retry',
            },
            copy.modules.retry,
          ),
        )
      : h(
          'ol',
          { class: 'cache-explorer__module-list', 'aria-label': copy.modules.list },
          ...(index?.modules ?? []).map((card) => row(card, snap.module, ui)),
        ),
  );
}
