// The two lanes of the machine, each a short arrow between two boxes with its label at the side:
// the transfer lane between memory and the cache, which is lit only on the event that moves a line
// or a word, and the request lane between the CPU and the cache, which follows the address strip's
// source. The arrows are SVG, in the design's styles for each kind of transfer.

import type { BusView, Transfer } from '../../pkg/cache_explorer';
import { cls, h, s } from '../dom';
import { PACKET_TRAVEL_PX, type Beat } from '../motion';
import type { AddressSource } from './source';

/** The name of each kind of transfer in the class of the elements that draw it. */
const KIND: Record<Transfer, string> = {
  idle: 'idle',
  fill: 'fill',
  writeBack: 'write-back',
  writeThrough: 'write-through',
};

interface TransferLook {
  /** The arrowhead's points, and the way the packet crosses: down towards the cache, or up. */
  head: string;
  direction: 'down' | 'up';
}

const LOOKS: Record<Exclude<Transfer, 'idle'>, TransferLook> = {
  fill: { head: '32,79 25,67 39,67', direction: 'down' },
  writeBack: { head: '32,2 25,14 39,14', direction: 'up' },
  writeThrough: { head: '32,2 25,14 39,14', direction: 'up' },
};

/** Draws the transfer lane into `lane`: the bus at rest, or the arrow of the transfer at the cursor. */
export function renderTransferLane(lane: HTMLElement, bus: BusView, beat: Beat | null): void {
  const look = bus.transfer === 'idle' ? null : LOOKS[bus.transfer];
  const arrow = s(
    'svg',
    { width: 64, height: 81, viewBox: '0 0 64 81', 'aria-hidden': 'true' },
    s('line', {
      class: `cache-explorer__lane-line cache-explorer__lane-line--${KIND[bus.transfer]}`,
      x1: 32,
      y1: 6,
      x2: 32,
      y2: 75,
    }),
    look &&
      s('polygon', {
        class: `cache-explorer__lane-head cache-explorer__lane-head--${KIND[bus.transfer]}`,
        points: look.head,
      }),
    look &&
      s('rect', {
        // At rest the packet sits where it arrives; a step plays its crossing to there.
        class: cls(
          'cache-explorer__packet',
          `cache-explorer__packet--${KIND[bus.transfer]}`,
          beat !== null && `cache-explorer__packet--${look.direction}-${beat}`,
        ),
        x: 26,
        y: look.direction === 'down' ? 8 + PACKET_TRAVEL_PX : 8,
        width: 12,
        height: 9,
      }),
  );
  lane.replaceChildren(
    h('span'),
    arrow,
    h(
      'div',
      null,
      h(
        'div',
        {
          class: cls(
            'cache-explorer__lane-label',
            look === null && 'cache-explorer__lane-label--idle',
          ),
        },
        bus.text,
      ),
      bus.cyclesText !== undefined &&
        look !== null &&
        h('div', { class: 'cache-explorer__chip' }, bus.cyclesText),
    ),
  );
}

/**
 * Draws the request lane into `lane`. An access at the cursor is a solid arrow with the
 * instruction under its label; an address that is only typed is dashed, and no address at all is
 * the same dashed arrow with its label saying so.
 */
export function renderRequestLane(lane: HTMLElement, source: AddressSource): void {
  const issued = source.kind === 'access';
  const arrow = s(
    'svg',
    { width: 64, height: 72, viewBox: '0 0 64 72', 'aria-hidden': 'true' },
    s('line', {
      class: `cache-explorer__lane-line cache-explorer__lane-line--${issued ? 'issued' : 'typed'}`,
      x1: 32,
      y1: 8,
      x2: 32,
      y2: 72,
    }),
    s('polygon', {
      class: `cache-explorer__lane-head cache-explorer__lane-head--${issued ? 'issued' : 'typed'}`,
      points: '32,0 26,10 38,10',
    }),
  );
  const { text, sub } = source.request;
  lane.replaceChildren(
    h('span'),
    arrow,
    h(
      'div',
      null,
      h(
        'div',
        { class: cls('cache-explorer__lane-label', !issued && 'cache-explorer__lane-label--idle') },
        text,
      ),
      sub !== '' && h('div', { class: 'cache-explorer__lane-sub' }, sub),
    ),
  );
}
