// The key: what each swatch, square and line in the tool means, in the design's order. Basics
// leaves out the write-through line, which cannot occur there; every item has its words beside it.

import { copy } from '../copy';
import { h, s, type Child } from '../dom';
import type { Snapshot } from './index';

/** A small square of colour, or a shape, before its words. */
function item(swatch: Child, text: string): HTMLElement {
  return h('span', { class: 'cache-explorer__key-item' }, swatch, text);
}

const swatch = (kind: string): HTMLElement =>
  h('span', {
    class: `cache-explorer__swatch cache-explorer__swatch--${kind}`,
    'aria-hidden': 'true',
  });

/** A short length of the line a transfer is drawn with in the lanes. */
function line(kind: 'fill' | 'write-through' | 'write-back'): SVGElement {
  return s(
    'svg',
    { class: 'cache-explorer__key-line', width: 18, height: 11, 'aria-hidden': 'true' },
    s('line', {
      class: `cache-explorer__lane-line cache-explorer__lane-line--${kind}`,
      x1: 0,
      y1: 5.5,
      x2: 18,
      y2: 5.5,
    }),
  );
}

/** Draws the key into `box`. */
export function renderKey(box: HTMLElement, snap: Snapshot): void {
  const { key } = copy;
  const { advanced } = snap.state;
  box.replaceChildren(
    h('span', { class: 'cache-explorer__label cache-explorer__label--strong' }, key.title),
    h(
      'div',
      { class: 'cache-explorer__key' },
      item(swatch('tag'), key.tag),
      item(swatch('hit'), key.hit),
      item(swatch('index'), key.index),
      item(swatch('miss'), key.miss),
      item(swatch('offset'), key.offset),
      item(swatch('changed'), key.changed),
      item(swatch('invalid'), key.invalid),
      item(swatch('victim'), key.victim),
      item(swatch('dirty'), key.dirty),
      item(swatch('dirty'), key.stale),
      item(line('fill'), key.fill),
      advanced && item(line('write-through'), key.writeThrough),
      item(line('write-back'), key.writeBack),
    ),
  );
}
