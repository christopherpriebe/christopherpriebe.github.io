// Motion that has to outlast a render. Render rebuilds the elements it draws, and a rebuilt element
// starts its CSS animation over; what is remembered here is how far along an animation had got, so
// the rebuilt element can carry on from there.

import type { Cause } from '../pkg/cache_explorer';

interface Flash {
  readonly cause: Cause;
  /** When the render that first drew this flash ran, in milliseconds. */
  readonly since: number;
}

/** How long a configuration group's flash lasts: 400 ms in, a hold, and 400 ms out. */
export const FLASH_MS = 1600;

const flashes = new WeakMap<HTMLElement, Flash>();

/**
 * How long the flash of `cause` has been showing in `root`, in milliseconds: 0 for the render that
 * first draws it, more for the renders after it, which start their animation that far in. Null
 * when no flash is showing. A different cause is a new flash, and so is the same one once a
 * flash's time has run out.
 */
export function flashAge(
  root: HTMLElement,
  cause: Cause | null,
  now = performance.now(),
): number | null {
  if (cause === null) {
    flashes.delete(root);
    return null;
  }
  const flash = flashes.get(root);
  if (flash?.cause === cause && now - flash.since < FLASH_MS) return Math.max(0, now - flash.since);
  flashes.set(root, { cause, since: now });
  return 0;
}
