// The stats region: the counters of the run so far, with the ones the cursor's entry moved on the
// changed ground; in timed mode the average memory access time and what the run spent per access;
// and a row of squares for the latest accesses, a hit filled and a miss open.

import { copy } from '../copy';
import { h } from '../dom';
import type { Snapshot } from './index';

/** Draws the stats into `box`. */
export function renderStats(box: HTMLElement, snap: Snapshot): void {
  const { stats } = snap.state;
  box.replaceChildren(
    h(
      'div',
      { class: 'cache-explorer__stats-body' },
      h('span', { class: 'cache-explorer__label cache-explorer__label--strong' }, copy.stats.title),
      h(
        'div',
        { class: 'cache-explorer__stat-grid' },
        ...stats.rows.map((row) =>
          h(
            'div',
            { class: 'cache-explorer__stat', 'data-stat': row.key },
            h('span', { class: 'cache-explorer__stat-label' }, copy.stats.labels[row.key]),
            h(
              'output',
              {
                class: row.changed
                  ? 'cache-explorer__stat-value cache-explorer__stat-value--changed'
                  : 'cache-explorer__stat-value',
              },
              row.text,
            ),
          ),
        ),
      ),
      stats.amat && h('p', { class: 'cache-explorer__amat' }, stats.amat.text),
      stats.measured &&
        h('p', { class: 'cache-explorer__amat cache-explorer__measured' }, stats.measured.text),
      stats.recent.length > 0 &&
        h(
          'div',
          {
            class: 'cache-explorer__spark',
            role: 'img',
            'aria-label': stats.recentText,
          },
          ...stats.recent.map((outcome) =>
            h('span', {
              class: `cache-explorer__marker cache-explorer__marker--${outcome} cache-explorer__marker--small`,
              'aria-hidden': 'true',
            }),
          ),
          h('span', { class: 'cache-explorer__spark-text' }, stats.recentText),
        ),
    ),
  );
}
