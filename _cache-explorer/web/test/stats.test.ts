import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { SCENARIOS } from '../dev/sessions';
import { start, type Controller } from '../src/controller';
import { copy } from '../src/copy';
import { h } from '../src/dom';
import { Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

let root: HTMLElement;
let lent: Session;
let controller: Controller;

const all = (selector: string): HTMLElement[] => [...root.querySelectorAll<HTMLElement>(selector)];

const query = (selector: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`nothing matches ${selector}`);
  return found;
};

function open(scenario: string): void {
  const build = SCENARIOS[scenario];
  if (!build) throw new Error(`no scenario ${scenario}`);
  lent = build();
  controller = start(root, lent, 'desktop');
}

/** The rows of the stats: the label and the value of each. */
const rows = (): string[][] =>
  all('.cache-explorer__stat').map((stat) => [
    stat.querySelector('.cache-explorer__stat-label')?.textContent ?? '',
    stat.querySelector('output')?.textContent ?? '',
  ]);

beforeAll(loadWasm);

beforeEach(() => {
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
});

afterEach(() => {
  controller.dispose();
});

describe('the stats', () => {
  it('shows the Basics rows with the core’s values, named as the page names them', () => {
    open('frame-3a');
    expect(rows()).toEqual([
      ['Accesses', '3'],
      ['Hits', '1'],
      ['Misses', '2'],
      ['Hit rate', '33%'],
      ['Evictions', '1'],
      ['Writes to memory', '0'],
    ]);
  });

  it('puts the changed ground on the rows the cursor’s entry moved, and only those', () => {
    open('frame-3a');
    const changed = all('.cache-explorer__stat').filter((stat) =>
      stat.querySelector('output')?.classList.contains('cache-explorer__stat-value--changed'),
    );
    expect(changed.map((stat) => stat.dataset.stat)).toEqual(['accesses', 'misses', 'evictions']);
  });

  it('shows a hit rate as an em dash before the first access', () => {
    open('first-visit');
    expect(rows()).toContainEqual(['Hit rate', '—']);
    expect(all('.cache-explorer__spark')).toHaveLength(0);
    expect(all('.cache-explorer__amat')).toHaveLength(0);
  });

  it('shows the Advanced rows, cycles and the miss classes among them', () => {
    open('frame-3b');
    const labels = rows().map(([label]) => label);
    expect(labels).toEqual([
      'Accesses',
      'Hits',
      'Misses',
      'Hit rate',
      'Evictions',
      'Write-backs',
      'Write-throughs',
      'Write-arounds',
      'Cycles',
      'Compulsory misses',
      'Capacity misses',
      'Conflict misses',
    ]);
  });

  it('shows AMAT and what the run measured, in the core’s words, under the counters in timed mode', () => {
    open('frame-3b');
    const { amat, measured } = lent.state().stats;
    const lines = all('.cache-explorer__amat').map((line) => line.textContent);
    expect(lines).toEqual([amat?.text, measured?.text]);
    expect(lines[0]).toMatch(/^AMAT = /);
    expect(lines[1]).toMatch(/^Measured: /);
    expect(query('.cache-explorer__measured')).toBe(all('.cache-explorer__amat')[1]);
  });

  it('keeps both lines at the last completed access while a dirty victim is written back', () => {
    open('frame-3b');
    expect(lent.state().event?.kind).toBe('writeBack');
    const lines = all('.cache-explorer__amat').map((line) => line.textContent);
    expect(lines).toEqual([
      'AMAT = 1 + 0.75 × 20 = 16.0 cycles',
      'Measured: 16.0 cycles per access (AMAT plus 0.0 for 0 write-backs).',
    ]);
    expect(rows()).toContainEqual(['Cycles', '85']);
    expect(rows()).toContainEqual(['Write-backs', '1']);
  });

  it('moves the lines to the new access once its last event is reached', () => {
    open('frame-3b');
    lent.seekStep(lent.timeline().stepCount);
    controller.refresh();
    expect(all('.cache-explorer__amat').map((line) => line.textContent)).toEqual([
      'AMAT = 1 + 0.80 × 20 = 17.0 cycles',
      'Measured: 21.0 cycles per access (AMAT plus 4.0 for 1 write-back).',
    ]);
    expect(rows()).toContainEqual(['Cycles', '105']);
  });

  it('shows neither line untimed', () => {
    open('frame-3a');
    expect(all('.cache-explorer__amat')).toHaveLength(0);
  });

  it('draws a square for each of the latest accesses, a hit filled and a miss open, and says what they are', () => {
    open('frame-3a');
    const squares = all('.cache-explorer__spark .cache-explorer__marker');
    expect(
      squares.map((square) =>
        square.classList.contains('cache-explorer__marker--hit') ? 'hit' : 'miss',
      ),
    ).toEqual(lent.state().stats.recent);
    const label = query('.cache-explorer__spark').getAttribute('aria-label');
    expect(label).toBe(lent.state().stats.recentText);
    expect(query('.cache-explorer__spark-text').textContent).toBe(label);
  });

  it('follows the cursor back through the run', () => {
    open('frame-3a');
    lent.seekStep(0);
    controller.refresh();
    expect(rows()[0]).toEqual(['Accesses', '0']);
  });
});

describe('the key', () => {
  const items = (): string[] => all('.cache-explorer__key-item').map((item) => item.textContent);

  it('lists the design’s items in its order for Basics, without write-through', () => {
    open('frame-3a');
    expect(items()).toEqual([
      copy.key.tag,
      copy.key.hit,
      copy.key.index,
      copy.key.miss,
      copy.key.offset,
      copy.key.changed,
      copy.key.invalid,
      copy.key.victim,
      copy.key.dirty,
      copy.key.stale,
      copy.key.fill,
      copy.key.writeBack,
    ]);
  });

  it('adds write-through between fill and write-back in Advanced', () => {
    open('frame-3b');
    expect(items().slice(-3)).toEqual([copy.key.fill, copy.key.writeThrough, copy.key.writeBack]);
  });

  it('gives every swatch words beside it, and hides the swatch itself from a screen reader', () => {
    open('frame-3b');
    for (const item of all('.cache-explorer__key-item')) {
      expect(item.textContent).not.toBe('');
      expect(item.firstElementChild?.getAttribute('aria-hidden')).toBe('true');
    }
  });
});
