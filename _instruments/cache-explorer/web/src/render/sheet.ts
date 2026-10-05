// The shortcuts sheet: a small non-modal dialog over the corner of the plane, listing the key map.

import { copy } from '../copy';
import { h } from '../dom';

export function renderSheet(): HTMLElement {
  return h(
    'div',
    { class: 'cache-explorer__sheet', role: 'dialog', 'aria-label': copy.transport.shortcuts },
    h(
      'div',
      { class: 'cache-explorer__sheet-head' },
      h('span', { class: 'cache-explorer__label' }, copy.sheet.title),
      h(
        'button',
        {
          type: 'button',
          class: 'cache-explorer__sheet-close',
          'data-action': 'sheet-close',
          'aria-label': copy.sheet.close,
        },
        '×',
      ),
    ),
    h(
      'dl',
      { class: 'cache-explorer__sheet-keys' },
      ...copy.sheet.rows.flatMap(([key, what]) => [h('dt', null, key), h('dd', null, what)]),
    ),
  );
}
