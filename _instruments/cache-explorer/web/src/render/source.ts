// What the address strip, the set bar, the wires, the comparators and the request lane follow: one
// source, so they never disagree. While the cursor rests on an access they follow it, from its
// decode to its last event; otherwise, and from a keystroke in the address box until the cursor
// next moves, they follow the typed address.

import type { Decoded, WayCompare } from '../../pkg/cache_explorer';
import { copy } from '../copy';
import type { UiState } from '../ui-state';
import type { Snapshot } from './index';

export interface AddressSource {
  /**
   * `access`: the access at the cursor. `typed`: the address the student has typed and not issued,
   * which decodes and has `=?` on the ways of its set. `none`: the address box is empty or holds
   * something that is not an address, and there is nothing to follow.
   */
  kind: 'access' | 'typed' | 'none';
  decoded: Decoded | null;
  /** The set the address maps to, when there is one. */
  set: number | null;
  /** What the comparator of each way of that set says. */
  compare: readonly WayCompare[];
  /** The right-hand text of the strip's footer. */
  footer: string;
  /** The request lane's label, and the line under it. */
  request: { text: string; sub: string };
}

export function addressSource(snap: Snapshot, ui: UiState): AddressSource {
  const { request } = snap.state;
  if (ui.stripSource === 'cursor' && request.status === 'issued' && request.decoded) {
    return {
      kind: 'access',
      decoded: request.decoded,
      set: snap.state.cache.selectedSet ?? null,
      compare: snap.state.cache.compare ?? [],
      footer: request.sourceText,
      request: { text: request.text, sub: request.source ?? '' },
    };
  }
  const { address } = snap.draft;
  if (address.status === 'valid' || address.status === 'misaligned') {
    return {
      kind: 'typed',
      decoded: address.decoded,
      set: snap.draft.selectedSet ?? null,
      compare: snap.draft.compare ?? [],
      footer: copy.strip.typed,
      request: { text: snap.draft.requestText, sub: copy.strip.typed },
    };
  }
  return {
    kind: 'none',
    decoded: null,
    set: null,
    compare: [],
    footer: copy.strip.none,
    request: { text: snap.draft.requestText, sub: '' },
  };
}
