// What the cursor's step changed, as the marks on the cells the core names. `highlight` is the net
// change of the entry so far and gets the changed ground, which stays; `changed` is what the event
// at the cursor alone changed and gets the ring, which plays once and only when the cursor moved.

import type { ChangeRef } from '../../pkg/cache_explorer';
import { beatOf, type Beat } from '../motion';
import type { UiState } from '../ui-state';
import type { Snapshot } from './index';

/** The name of a cell, the same one whichever way a `ChangeRef` or a region asks for it. */
export const cellKey = {
  valid: (set: number, way: number): string => `valid.${String(set)}.${String(way)}`,
  tag: (set: number, way: number): string => `tag.${String(set)}.${String(way)}`,
  dirty: (set: number, way: number): string => `dirty.${String(set)}.${String(way)}`,
  rank: (set: number, way: number): string => `rank.${String(set)}.${String(way)}`,
  word: (set: number, way: number, word: number): string =>
    `word.${String(set)}.${String(way)}.${String(word)}`,
  memory: (addr: number): string => `memory.${String(addr)}`,
  register: (register: string): string => `register.${register}`,
};

/** The cell a change names, or null for a counter or the pc, which other regions draw. */
export function keyOf(ref: ChangeRef): string | null {
  switch (ref.cell) {
    case 'valid':
      return cellKey.valid(ref.set, ref.way);
    case 'tag':
      return cellKey.tag(ref.set, ref.way);
    case 'dirty':
      return cellKey.dirty(ref.set, ref.way);
    case 'rank':
      return cellKey.rank(ref.set, ref.way);
    case 'word':
      return cellKey.word(ref.set, ref.way, ref.word);
    case 'memory':
      return cellKey.memory(ref.addr);
    case 'register':
      return cellKey.register(ref.register);
    case 'pc':
    case 'counter':
      return null;
  }
}

function keys(refs: readonly ChangeRef[]): Set<string> {
  const found = new Set<string>();
  for (const ref of refs) {
    const key = keyOf(ref);
    if (key !== null) found.add(key);
  }
  return found;
}

/** The marks of one render. */
export interface Marks {
  /** The classes a cell gets: the ground when it is in the highlight, the ring when it just changed. */
  classes(key: string): string[];
}

/**
 * The marks the cursor's step puts on cells. The ring is added only when the cursor moved since
 * the previous render, and alternates its keyframes by the position's parity.
 */
export function marksOf(snap: Snapshot, ui: UiState): Marks {
  const { state } = snap;
  const ground = keys(state.highlight);
  const beat: Beat | null = beatOf(state.position.pos, ui.lastPos);
  const ring = beat === null ? new Set<string>() : keys(state.event?.changed ?? []);
  return {
    classes(key) {
      const found: string[] = [];
      if (ground.has(key)) found.push('cache-explorer__changed');
      if (beat !== null && ring.has(key)) found.push(`cache-explorer__pulse-${beat}`);
      return found;
    },
  };
}
