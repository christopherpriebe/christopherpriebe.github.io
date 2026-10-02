// A run of text the core split into styles: plain, `code` in the mono face, and emphasis. The core
// has already checked that it holds nothing else, so there is no markup to read here.

import type { TextSegment } from '../../pkg/cache_explorer';
import { h } from '../dom';

/** The segments as nodes, ready to go inside a paragraph. */
export function segments(list: readonly TextSegment[]): Node[] {
  return list.map((segment) => {
    switch (segment.style) {
      case 'code':
        return h('code', { class: 'cache-explorer__code' }, segment.text);
      case 'emphasis':
        return h('em', null, segment.text);
      case 'plain':
        return document.createTextNode(segment.text);
    }
  });
}
