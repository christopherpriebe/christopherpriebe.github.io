// The CPU box: the instruction it is executing, what it is doing about it, and its eight
// registers, with the one an event just wrote on the changed ground.

import { copy } from '../copy';
import { cls, h } from '../dom';
import type { Snapshot } from './index';
import { cellKey, type Marks } from './marks';

/** Draws the CPU box into `box`. */
export function renderCpu(box: HTMLElement, snap: Snapshot, marks: Marks): void {
  const { cpu } = snap.state;
  box.replaceChildren(
    h(
      'div',
      { class: 'cache-explorer__cpu-main' },
      h('span', { class: 'cache-explorer__label' }, copy.cpu.title),
      h('div', { class: 'cache-explorer__instruction' }, cpu.instruction),
      h(
        'div',
        { class: 'cache-explorer__cpu-status' },
        // A hit is a filled square and a miss an open one, as in the log; with neither, no mark.
        h('span', {
          class: `cache-explorer__marker cache-explorer__marker--${cpu.hitMarker}`,
          'aria-hidden': 'true',
        }),
        h('span', { class: 'cache-explorer__label cache-explorer__label--strong' }, cpu.statusText),
      ),
    ),
    h(
      'div',
      { class: 'cache-explorer__registers', role: 'group', 'aria-label': copy.cpu.registers },
      ...cpu.registers.map(({ register, text }) =>
        h(
          'div',
          {
            class: cls('cache-explorer__register', ...marks.classes(cellKey.register(register))),
          },
          h('div', { class: 'cache-explorer__register-name' }, register),
          h('div', { class: 'cache-explorer__register-value' }, text),
        ),
      ),
    ),
  );
}
