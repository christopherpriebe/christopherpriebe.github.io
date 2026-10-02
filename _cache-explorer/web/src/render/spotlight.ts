// The outline a module step puts round the part of the workbench it is about: a region, or one set
// or way of the cache. The regions are elements the root keeps, so the outline is taken off the
// last one and put on the new one in every render.

import type { Callout, CalloutArea } from '../../pkg/cache_explorer';

const SPOT = 'cache-explorer__spot';
const SPOT_CELL = 'cache-explorer__spot--cell';

/** The element of each region, but for the address strip, which is found in the cache box. */
export type Areas = Record<Exclude<CalloutArea, 'addressStrip'>, HTMLElement>;

/** Takes the outline off whatever has it, then puts it on the callout's element, if there is one. */
export function spotlight(plane: HTMLElement, areas: Areas, callout: Callout | undefined): void {
  for (const element of plane.querySelectorAll(`.${SPOT}`))
    element.classList.remove(SPOT, SPOT_CELL);
  if (callout === undefined) return;
  if (typeof callout === 'object') {
    const { set, way } = callout;
    const selector =
      way === undefined
        ? `[data-action="edit-way"][data-set="${String(set)}"]`
        : `[data-action="edit-way"][data-set="${String(set)}"][data-way="${String(way)}"]`;
    const cell = plane.querySelector(selector);
    // A set is outlined as its row, in a table of tiles as well.
    const target = way === undefined ? cell?.closest('tr') : cell;
    target?.classList.add(SPOT, SPOT_CELL);
    return;
  }
  const target =
    callout === 'addressStrip' ? plane.querySelector('.cache-explorer__strip') : areas[callout];
  target?.classList.add(SPOT);
}
