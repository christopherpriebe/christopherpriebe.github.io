import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import type { ChangeRef, State } from '../pkg/cache_explorer';
import { config, loads, SCENARIOS } from '../dev/sessions';
import { buildSnapshot } from '../src/controller';
import { h } from '../src/dom';
import { render } from '../src/render';
import { fitsInRow } from '../src/render/cache';
import { initialUiState, type UiState } from '../src/ui-state';
import { Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

let root: HTMLElement;
let session: Session | null;
let ui: UiState;

function current(): Session {
  if (!session) throw new Error('no session');
  return session;
}

/** Hands the test a session it owns; it is freed after the test. */
function open(next: Session): Session {
  session?.free();
  session = next;
  return next;
}

/** One render of the session as it is now. */
function draw(): void {
  const { snapshot, log } = buildSnapshot(current(), ui);
  render(root, snapshot, log, ui);
}

/** A render as the step to the cursor's position makes it: the cursor has moved since the last. */
function drawStep(previous: number): void {
  ui.lastPos = previous;
  draw();
  ui.lastPos = current().position().pos;
}

const all = (selector: string, within: ParentNode = root): HTMLElement[] => [
  ...within.querySelectorAll<HTMLElement>(selector),
];

const query = (selector: string, within: ParentNode = root): HTMLElement => {
  const found = within.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`nothing matches ${selector}`);
  return found;
};

const texts = (selector: string, within: ParentNode = root): (string | null)[] =>
  all(selector, within).map((element) => element.textContent);

/** The cell of a way, by its set and way. */
const way = (set: number, index: number): HTMLElement =>
  query(`[data-action='edit-way'][data-set='${String(set)}'][data-way='${String(index)}']`);

/** The way's parts, left to right. */
const part = (cell: HTMLElement, name: string): HTMLElement =>
  query(`.cache-explorer__${name}`, cell);

const PULSES = '.cache-explorer__pulse-a, .cache-explorer__pulse-b';

beforeAll(loadWasm);

beforeEach(() => {
  session = Session.firstVisit();
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
  ui = initialUiState('desktop');
});

afterEach(() => {
  const owned = session;
  session = null;
  owned?.free();
});

describe('the cache table', () => {
  it('has a row for each set and a cell for each way, each with a word for every column', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    draw();
    const state = current().state();
    const rows = all('.cache-explorer__cache tbody tr');
    expect(rows).toHaveLength(state.geometry.sets);
    for (const row of rows) expect(all('[data-action="edit-way"]', row)).toHaveLength(2);
    for (const cell of all('[data-action="edit-way"]')) {
      expect(all('.cache-explorer__word', cell)).toHaveLength(state.geometry.wordsPerLine);
    }
  });

  it('is a table whose sets are row headers and whose ways have columns named for them', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    draw();
    const table = query('table[data-cachetable]');
    expect(table.getAttribute('aria-label')).toBe(current().state().cache.header);
    expect(texts('table[data-cachetable] tbody th[scope=row]')).toEqual(['0', '1', '2', '3']);
    expect(
      all('table[data-cachetable] thead th[scope=col]').map((head) => head.textContent),
    ).toEqual(['Setindex', expect.stringContaining('Way 0'), expect.stringContaining('Way 1')]);
  });

  it('keeps its table roles, since the stylesheet lays its rows out as a grid, which some browsers read as no table', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    draw();
    for (const selector of ['table[data-cachetable]', '.cache-explorer__mem']) {
      const table = query(selector);
      expect(table.getAttribute('role')).toBe('table');
      expect(
        all('thead, tbody', table).every((group) => group.getAttribute('role') === 'rowgroup'),
      ).toBe(true);
      expect(all('tr[role]', table).length).toBe(all('tr', table).length);
      expect(
        all('th', table).every((cell) =>
          /^(row|column)header$/.test(cell.getAttribute('role') ?? ''),
        ),
      ).toBe(true);
    }
    expect(
      all('table[data-cachetable] td').every((cell) => cell.getAttribute('role') === 'cell'),
    ).toBe(true);
  });

  it('heads each way with the core’s word heads, and its rank and dirty columns, in the design’s order', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    draw();
    const heads = all('.cache-explorer__way-heads')[0];
    expect(heads?.children).toHaveLength(5);
    expect(texts('.cache-explorer__sub-head', heads)).toEqual([
      'V',
      'Tag',
      `Data ${current().state().cache.wordHeads.join(' ')}`,
      'LRU',
      'D',
    ]);
  });

  it('names the rank column Age under FIFO', () => {
    const fifo = config(128, 16, 2);
    open(Session.withConfig({ ...fifo, cache: { ...fifo.cache, replacement: 'fifo' } }));
    draw();
    expect(texts('.cache-explorer__sub-head--rank')).toEqual(['Age', 'Age']);
    expect(texts('.cache-explorer__role').slice(0, 2)).toEqual(['new', 'old']);
  });

  it('draws the D column in Basics as in Advanced, because Basics is write-back', () => {
    draw();
    expect(current().state().advanced).toBe(false);
    expect(current().state().cache.showDirty).toBe(true);
    expect(all('.cache-explorer__dirty')).toHaveLength(current().state().geometry.sets);
    expect(texts('.cache-explorer__sub-head--dirty')).toEqual(['D']);
  });

  it('leaves the rank column out where the core hides it: one way, or Random', () => {
    draw();
    expect(current().state().cache.showRank).toBe(false);
    expect(all('.cache-explorer__rank')).toHaveLength(0);
    expect(all('.cache-explorer__sub-head--rank')).toHaveLength(0);

    const random = config(128, 16, 2);
    open(Session.withConfig({ ...random, cache: { ...random.cache, replacement: 'random' } }));
    draw();
    expect(all('.cache-explorer__rank')).toHaveLength(0);
    expect(all('.cache-explorer__way--rank')).toHaveLength(0);
  });

  it('draws an invalid way empty, with a dash for its tag and a dot for each word', () => {
    draw();
    const cell = way(0, 0);
    expect(cell.classList.contains('cache-explorer__way--empty')).toBe(true);
    expect(part(cell, 'valid').textContent).toBe('0');
    expect(part(cell, 'tag').textContent).toBe('—');
    expect(texts('.cache-explorer__word', cell)).toEqual(['·', '·', '·', '·']);
    expect(part(cell, 'dirty').textContent).toBe('·');
  });

  it('draws a valid way with its tag, its words, its rank with the role at either end, and D', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    draw();
    const view = current().state().cache.sets[2]?.ways[1];
    const cell = way(2, 1);
    expect(cell.classList.contains('cache-explorer__way--empty')).toBe(false);
    expect(part(cell, 'valid').textContent).toBe('1');
    expect(part(cell, 'tag').textContent).toBe(view?.tagText);
    expect(texts('.cache-explorer__word', cell)).toEqual(view?.words);
    expect(part(cell, 'rank').textContent).toBe('1LRU');
    expect(part(cell, 'dirty').textContent).toBe('D');
    expect(query('.cache-explorer__dirty-bar', cell)).toBeTruthy();
    expect(part(way(2, 0), 'rank').textContent).toBe('0MRU');
    expect(way(2, 0).querySelector('.cache-explorer__dirty-bar')).toBeNull();
  });

  it('outlines the victim and gives it a tab that says so', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    draw();
    expect(current().state().cache.sets[2]?.ways[1]?.victim).toBe(true);
    const cell = way(2, 1);
    expect(cell.classList.contains('cache-explorer__way--victim')).toBe(true);
    expect(part(cell, 'victim').textContent).toBe('victim');
    expect(all('.cache-explorer__way--victim')).toHaveLength(1);
    expect(all('.cache-explorer__victim')).toHaveLength(1);
  });

  it('underlines the word the access is at, in the way that holds its line', () => {
    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    draw();
    const addressed = all('.cache-explorer__word--addressed');
    expect(addressed).toHaveLength(1);
    expect(addressed[0]?.dataset.word).toBe('1');
    expect(addressed[0]?.closest('[data-action="edit-way"]')).toBe(way(2, 0));
  });

  it('marks the selected set’s row header, and only that one', () => {
    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    draw();
    const selected = all('.cache-explorer__set--selected');
    expect(selected.map((head) => head.textContent)).toEqual(['2']);
  });

  it('marks the typed address’s set before anything is issued', () => {
    draw();
    expect(texts('.cache-explorer__set--selected')).toEqual(['2']);
  });

  it('makes each way a stop for the keyboard that says what it is and how to change it', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    draw();
    const view = current().state().cache.sets[3]?.ways[0];
    const cell = way(3, 0);
    expect(cell.getAttribute('tabindex')).toBe('0');
    expect(cell.getAttribute('aria-label')).toBe(`${view?.description ?? ''} Press Enter to edit.`);
  });
});

describe('a cache with more ways than fit in a row', () => {
  const eight = (): Session => open(SCENARIOS['wrapped-four-sets']?.() ?? Session.firstVisit());

  it('wraps the ways of a set into tiles, each with its name over its cell', () => {
    eight();
    draw();
    const state = current().state();
    expect(state.cache.wrap).toBe(true);
    expect(
      query('table[data-cachetable]').classList.contains('cache-explorer__cache--wrapped'),
    ).toBe(true);
    expect(all('table[data-cachetable] tbody tr')).toHaveLength(state.geometry.sets);
    const first = all('table[data-cachetable] tbody tr')[0];
    expect(first ? all('.cache-explorer__tile', first) : []).toHaveLength(8);
    expect(first ? texts('.cache-explorer__way-name', first)[0] : '').toContain('Way 0');
    expect(all('[data-action="edit-way"]')).toHaveLength(state.geometry.lines);
  });

  it('sets the least width of a tile from the words in a line, and leaves the columns to the grid', () => {
    eight();
    draw();
    const { wordsPerLine } = current().state().geometry;
    expect(query('table[data-cachetable]').getAttribute('style')).toBe(
      `--cache-explorer-way-min: ${String(156 + 34 * wordsPerLine)}px`,
    );
  });

  it('keeps the data column, and heads it once, over the first row of tiles', () => {
    eight();
    draw();
    expect(all('.cache-explorer__word', way(0, 0))).toHaveLength(
      current().state().geometry.wordsPerLine,
    );
    const heads = all('.cache-explorer__wrap-head');
    expect(heads).toHaveLength(8);
    expect(heads.map((head) => head.getAttribute('aria-hidden'))).toEqual([
      null,
      ...Array<string>(7).fill('true'),
    ]);
  });

  it('puts each way’s comparator in its tile, in the selected set only, and draws no bus', () => {
    eight();
    draw();
    const state = current().state();
    const badges = all('.cache-explorer__compare');
    expect(badges).toHaveLength(state.geometry.ways);
    const selected = state.cache.selectedSet ?? -1;
    expect(
      badges.every(
        (badge) => badge.closest('tr')?.querySelector('th')?.textContent === String(selected),
      ),
    ).toBe(true);
    const glyph = { pending: '=?', match: '=', mismatch: '≠', invalid: '≠' } as const;
    expect(badges.map((badge) => badge.textContent)).toEqual(
      (state.cache.compare ?? []).map((result) => glyph[result]),
    );
    expect(all('.cache-explorer__comparator')).toHaveLength(0);
  });

  it('gives a badge text a screen reader can use', () => {
    eight();
    draw();
    for (const badge of all('.cache-explorer__compare')) {
      expect(badge.getAttribute('title')).toMatch(/^(Tag|Empty way)/);
    }
  });

  it('opens paused on the write-back of a dirty victim, with the victim’s tile marked', () => {
    eight();
    draw();
    expect(current().state().event?.kind).toBe('writeBack');
    const victims = all('.cache-explorer__way--victim');
    expect(victims).toHaveLength(1);
    const [victim] = victims;
    expect(victim?.closest('.cache-explorer__tile')).not.toBeNull();
    expect(victim ? all('.cache-explorer__dirty-bar', victim) : []).toHaveLength(1);
    expect(victim ? texts('.cache-explorer__victim', victim) : []).toEqual(['victim']);
  });
});

describe('the marks a step puts on cells', () => {
  /** The element a change names, or null for a counter or the pc, which other regions draw. */
  function cellOf(ref: ChangeRef): HTMLElement | null {
    switch (ref.cell) {
      case 'valid':
        return part(way(ref.set, ref.way), 'valid');
      case 'tag':
        return part(way(ref.set, ref.way), 'tag');
      case 'dirty':
        return part(way(ref.set, ref.way), 'dirty');
      case 'rank':
        return part(way(ref.set, ref.way), 'rank');
      case 'word':
        return query(`[data-word='${String(ref.word)}']`, way(ref.set, ref.way));
      case 'memory':
        return query(`[data-action='edit-memory'][data-addr='${String(ref.addr)}']`);
      case 'register':
        return all('.cache-explorer__register')[Number(ref.register.slice(1))] ?? null;
      case 'pc':
      case 'counter':
        return null;
    }
  }

  /** The cells a list of changes names, each once. */
  function cellsOf(refs: readonly ChangeRef[]): Set<HTMLElement> {
    const found = new Set<HTMLElement>();
    for (const ref of refs) {
      const cell = cellOf(ref);
      if (cell) found.add(cell);
    }
    return found;
  }

  /** Steps through the whole run one event at a time, checking the marks after each. */
  function walk(events: number): { kinds: Set<string> } {
    const kinds = new Set<string>();
    for (let step = 0; step < events; step++) {
      const before = current().position().pos;
      current().seek(before + 1);
      const state: State = current().state();
      drawStep(before);
      for (const ref of state.event?.changed ?? []) kinds.add(ref.cell);
      for (const ref of state.highlight) kinds.add(ref.cell);

      const pulsed = cellsOf(state.event?.changed ?? []);
      expect(new Set(all(PULSES))).toEqual(pulsed);
      const ground = cellsOf(state.highlight);
      expect(new Set(all('.cache-explorer__changed'))).toEqual(ground);
    }
    return { kinds };
  }

  it('rings the cells the event changed and grounds the cells the entry has changed, event by event', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    current().seek(0);
    ui.lastPos = 0;
    const total = current().timeline().stepCount;
    const { kinds } = walk(total);
    // A miss with a dirty victim uses every kind of cell there is: the cache's five, memory, a register.
    expect([...kinds].sort()).toEqual(
      expect.arrayContaining(['valid', 'tag', 'dirty', 'rank', 'word', 'memory', 'register']),
    );
  });

  it('does the same in a cache of eight ways, where every way’s rank can move', () => {
    const session8 = open(Session.withConfig(config(128, 16, 8)));
    loads(session8, [0x0a4, 0x1a4, 0x2a4, 0x0a8]);
    session8.seek(0);
    ui.lastPos = 0;
    const { kinds } = walk(session8.timeline().stepCount);
    expect([...kinds]).toEqual(
      expect.arrayContaining(['valid', 'tag', 'rank', 'word', 'register']),
    );
  });

  it('puts the ground on a cell whatever the event, and the ring on none when nothing moved', () => {
    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    ui.lastPos = current().position().pos;
    draw();
    expect(all('.cache-explorer__changed').length).toBeGreaterThan(0);
    expect(all(PULSES)).toHaveLength(0);
  });

  it('alternates the ring’s keyframes by the position, so a step straight after another restarts it', () => {
    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    const seen = new Map<number, string>();
    current().seek(0);
    for (let pos = 1; pos <= current().timeline().stepCount; pos++) {
      current().seek(pos);
      drawStep(pos - 1);
      const [ring] = all(PULSES);
      if (ring) seen.set(pos, ring.classList.contains('cache-explorer__pulse-a') ? 'a' : 'b');
    }
    expect(new Set(seen.values())).toEqual(new Set(['a', 'b']));
    for (const [pos, name] of seen) expect(name).toBe(pos % 2 === 1 ? 'a' : 'b');
  });

  it('plays nothing on a render the cursor did not cause', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    ui.lastPos = current().position().pos;
    draw();
    draw();
    expect(all(PULSES)).toHaveLength(0);
    expect(
      all('[class*="cache-explorer__packet--up-"], [class*="cache-explorer__packet--down-"]'),
    ).toHaveLength(0);
  });
});

describe('the transfer lane', () => {
  it('rests as a dotted line and says the bus is idle', () => {
    draw();
    const lane = query('.cache-explorer__lane--transfer');
    expect(lane.textContent).toBe('Bus idle');
    expect(
      query('.cache-explorer__lane-line', lane).classList.contains(
        'cache-explorer__lane-line--idle',
      ),
    ).toBe(true);
    expect(lane.querySelector('polygon')).toBeNull();
    expect(lane.querySelector('.cache-explorer__packet')).toBeNull();
    expect(lane.querySelector('.cache-explorer__lane-label--idle')).not.toBeNull();
  });

  it('draws a write-back as the core says it, with its cost, and the packet arriving where it goes', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    const { bus } = current().state();
    expect(bus.transfer).toBe('writeBack');
    draw();
    const lane = query('.cache-explorer__lane--transfer');
    expect(query('.cache-explorer__lane-label', lane).textContent).toBe(bus.text);
    expect(query('.cache-explorer__chip', lane).textContent).toBe(bus.cyclesText);
    expect(query('.cache-explorer__lane-line', lane).getAttribute('class')).toContain(
      '--write-back',
    );
    expect(query('polygon', lane).getAttribute('points')).toBe('32,2 25,14 39,14');
    expect(query('.cache-explorer__packet', lane).getAttribute('y')).toBe('8');
  });

  it('plays the packet up for a write-back and down for a fill, only when the cursor moved', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    const at = (kind: string): number => {
      for (let pos = 0; pos <= current().timeline().stepCount; pos++) {
        current().seek(pos);
        if (current().state().bus.transfer === kind) return pos;
      }
      throw new Error(`no ${kind}`);
    };
    const fill = at('fill');
    drawStep(fill - 1);
    let packet = query('.cache-explorer__lane--transfer .cache-explorer__packet');
    expect(packet.getAttribute('class')).toMatch(/cache-explorer__packet--down-[ab]/);
    expect(packet.getAttribute('y')).toBe('66');
    expect(query('.cache-explorer__lane--transfer polygon').getAttribute('points')).toBe(
      '32,79 25,67 39,67',
    );

    const writeBack = at('writeBack');
    drawStep(writeBack - 1);
    packet = query('.cache-explorer__lane--transfer .cache-explorer__packet');
    expect(packet.getAttribute('class')).toMatch(/cache-explorer__packet--up-[ab]/);

    ui.lastPos = current().position().pos;
    draw();
    packet = query('.cache-explorer__lane--transfer .cache-explorer__packet');
    expect(packet.getAttribute('class')).not.toMatch(/--(up|down)-/);
  });
});

describe('the request lane', () => {
  it('is a dashed arrow for the typed address, with the fixed line under its label', () => {
    draw();
    const lane = query('.cache-explorer__lane--request');
    expect(query('.cache-explorer__lane-label', lane).textContent).toBe('Load 0x0A4');
    expect(query('.cache-explorer__lane-sub', lane).textContent).toBe('typed, not issued yet');
    expect(query('.cache-explorer__lane-line', lane).getAttribute('class')).toContain('--typed');
  });

  it('is solid for the access at the cursor, with the instruction under its label', () => {
    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    draw();
    const { request } = current().state();
    const lane = query('.cache-explorer__lane--request');
    expect(query('.cache-explorer__lane-label', lane).textContent).toBe(request.text);
    expect(query('.cache-explorer__lane-sub', lane).textContent).toBe(request.source);
    expect(query('.cache-explorer__lane-line', lane).getAttribute('class')).toContain('--issued');
  });

  it('says there is no request when the address box holds no address', () => {
    ui.draft.addrText = 'xyz';
    draw();
    const lane = query('.cache-explorer__lane--request');
    expect(query('.cache-explorer__lane-label', lane).textContent).toBe('No request');
    expect(lane.querySelector('.cache-explorer__lane-sub')).toBeNull();
    expect(query('.cache-explorer__lane-line', lane).getAttribute('class')).toContain('--typed');
  });

  it('follows the typed address, not the access, from a keystroke until the cursor moves', () => {
    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    ui.draft.addrText = '0x0A8';
    ui.stripSource = 'draft';
    draw();
    expect(query('.cache-explorer__lane--request .cache-explorer__lane-label').textContent).toBe(
      'Load 0x0A8',
    );
    expect(texts('.cache-explorer__strip-foot span')).toEqual([
      'Address 0x0A8',
      'typed, not issued yet',
    ]);
  });
});

describe('the address strip', () => {
  it('shows the access at the cursor: its bits, each field’s value and note, and where it came from', () => {
    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    draw();
    const decoded = current().state().request.decoded;
    const fields = all('.cache-explorer__field-col');
    expect(fields).toHaveLength(decoded?.fields.length ?? -1);
    expect(fields.map((field) => texts('.cache-explorer__bit-digit', field).join(''))).toEqual(
      decoded?.fields.map((field) => field.bits),
    );
    expect(texts('.cache-explorer__field-value')).toEqual(['Tag 0x03', 'Index 2', 'Offset 4']);
    expect(texts('.cache-explorer__field-note')).toEqual(
      decoded?.fields.map((field) => field.note),
    );
    expect(texts('.cache-explorer__strip-foot span')).toEqual(['Address 0x1A4', 'from access #3']);
  });

  it('keeps a field’s name, value and note in one text beside its swatch', () => {
    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    draw();
    const labels = all('.cache-explorer__field-label');
    expect(labels).toHaveLength(3);
    for (const label of labels) {
      expect([...label.children].map((child) => child.className)).toEqual([
        expect.stringContaining('cache-explorer__swatch'),
        'cache-explorer__field-text',
      ]);
      const text = query('.cache-explorer__field-text', label);
      expect(all('.cache-explorer__field-value, .cache-explorer__field-note', text)).toHaveLength(
        2,
      );
    }
    // The note runs on after the value at a space, so a narrow field wraps them as text.
    expect(texts('.cache-explorer__field-text')[0]).toBe('Tag 0x03 5 bits');
  });

  it('gives each field a share of the width that is its share of the bits, and a block for each bit', () => {
    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    draw();
    expect(query('.cache-explorer__fields').getAttribute('style')).toBe(
      'grid-template-columns: minmax(0, 5fr) minmax(0, 3fr) minmax(0, 4fr)',
    );
    expect(all('.cache-explorer__bit')).toHaveLength(12);
    expect(all('.cache-explorer__bit-seam')).toHaveLength(2 + 4 + 3);
    expect(all('svg[data-fld]').map((svg) => svg.getAttribute('data-fld'))).toEqual([
      'tag',
      'index',
      'offset',
    ]);
  });

  it('colours the bits’ underline by field through classes, never through the text', () => {
    draw();
    expect(all('.cache-explorer__bit-rule').map((rule) => rule.getAttribute('class'))).toEqual([
      'cache-explorer__bit-rule cache-explorer__bit-rule--tag',
      'cache-explorer__bit-rule cache-explorer__bit-rule--index',
      'cache-explorer__bit-rule cache-explorer__bit-rule--offset',
    ]);
    for (const digit of all('.cache-explorer__bit-digit')) {
      expect(digit.getAttribute('fill')).toBeNull();
    }
  });

  it('previews the typed address before anything is issued', () => {
    draw();
    expect(texts('.cache-explorer__strip-foot span')).toEqual([
      'Address 0x0A4',
      'typed, not issued yet',
    ]);
    expect(texts('.cache-explorer__field-value')).toEqual(['Tag 0x01', 'Index 2', 'Offset 4']);
  });

  it('leaves the fields blank, with their widths, when the address box holds no address', () => {
    ui.draft.addrText = '';
    draw();
    const geometry = current().state().geometry;
    expect(all('.cache-explorer__bit-digit')).toHaveLength(0);
    expect(all('.cache-explorer__field-col')).toHaveLength(3);
    expect(texts('.cache-explorer__field-value')).toEqual(['Tag', 'Index', 'Offset']);
    expect(query('.cache-explorer__fields').getAttribute('style')).toBe(
      `grid-template-columns: minmax(0, ${String(geometry.tagBits)}fr) minmax(0, ${String(geometry.indexBits)}fr) minmax(0, ${String(geometry.offsetBits)}fr)`,
    );
    expect(texts('.cache-explorer__strip-foot span')).toEqual([
      'Address —',
      'type an address below',
    ]);
    expect(query('.cache-explorer__strip').classList.contains('cache-explorer__strip--blank')).toBe(
      true,
    );
  });

  it('drops a field with no bits: a fully associative cache has no index', () => {
    open(SCENARIOS['wrapped-fully-associative']?.() ?? Session.firstVisit());
    draw();
    expect(all('svg[data-fld]').map((svg) => svg.getAttribute('data-fld'))).toEqual([
      'tag',
      'offset',
    ]);
  });

  it('follows a misaligned address while it is typed, as the core decodes it', () => {
    ui.draft.addrText = '0x0A5';
    draw();
    expect(texts('.cache-explorer__strip-foot span')[0]).toBe('Address 0x0A5');
  });
});

describe('the CPU box', () => {
  it('shows the instruction, what the CPU is doing, and the registers', () => {
    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    draw();
    const { cpu } = current().state();
    expect(query('.cache-explorer__instruction').textContent).toBe(cpu.instruction);
    expect(query('.cache-explorer__cpu-status .cache-explorer__label').textContent).toBe(
      cpu.statusText,
    );
    expect(texts('.cache-explorer__register-name')).toEqual(cpu.registers.map((r) => r.register));
    expect(texts('.cache-explorer__register-value')).toEqual(cpu.registers.map((r) => r.text));
    expect(query('.cache-explorer__registers').getAttribute('aria-label')).toBe('Registers');
  });

  it('marks a miss with an open square, a hit with a filled one, and neither with none', () => {
    const marker = (): string =>
      query('.cache-explorer__marker')
        .getAttribute('class')
        ?.replace('cache-explorer__marker ', '') ?? '';
    draw();
    expect(current().state().cpu.hitMarker).toBe('none');
    expect(marker()).toBe('cache-explorer__marker--none');

    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    draw();
    expect(marker()).toBe('cache-explorer__marker--miss');

    current().seek(4);
    const hit = current().state().cpu.hitMarker;
    draw();
    expect(marker()).toBe(`cache-explorer__marker--${hit}`);
  });

  it('says in words what a hit or a miss square means, and hides the square that means nothing', () => {
    const square = (): HTMLElement => query('.cache-explorer__marker');
    draw();
    expect(current().state().cpu.hitMarker).toBe('none');
    expect(square().getAttribute('aria-hidden')).toBe('true');
    expect(square().hasAttribute('aria-label')).toBe(false);
    expect(square().hasAttribute('title')).toBe(false);

    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    draw();
    expect(current().state().cpu.hitMarker).toBe('miss');
    expect(square().getAttribute('role')).toBe('img');
    expect(square().getAttribute('aria-label')).toBe('Miss');
    expect(square().getAttribute('title')).toBe('Miss');
    expect(square().hasAttribute('aria-hidden')).toBe(false);

    for (let pos = 0; current().state().cpu.hitMarker !== 'hit'; pos++) {
      current().seek(pos);
      if (pos > current().timeline().stepCount) throw new Error('no step ever hits');
    }
    draw();
    expect(square().getAttribute('aria-label')).toBe('Hit');
  });

  it('puts the register an event wrote on the changed ground', () => {
    open(SCENARIOS['frame-3a']?.() ?? Session.firstVisit());
    draw();
    const grounded = all('.cache-explorer__register').filter((register) =>
      register.classList.contains('cache-explorer__changed'),
    );
    expect(grounded.map((register) => register.firstElementChild?.textContent)).toEqual(['r3']);
  });
});

describe('the machine’s parts', () => {
  it('are the same elements every render, so what they hold survives', () => {
    open(SCENARIOS['frame-3b']?.() ?? Session.firstVisit());
    draw();
    const parts = [
      '.cache-explorer__box--memory',
      '.cache-explorer__scroller',
      '.cache-explorer__box--cache',
      '.cache-explorer__cpu',
      '.cache-explorer__lane--transfer',
    ].map((selector) => query(selector));
    draw();
    current().seek(3);
    draw();
    expect(parts.map((element) => element.isConnected)).toEqual(parts.map(() => true));
    expect(
      [
        '.cache-explorer__box--memory',
        '.cache-explorer__scroller',
        '.cache-explorer__box--cache',
        '.cache-explorer__cpu',
        '.cache-explorer__lane--transfer',
      ].map((selector) => query(selector)),
    ).toEqual(parts);
  });

  it('come in the order the design stacks them, inside the plane, then the log, the stats and key, the transport, the program drawer and the live region', () => {
    draw();
    const machine = query('.cache-explorer__machine');
    expect([...machine.children].map((element) => element.className.split(' ')[0])).toEqual([
      'cache-explorer__box',
      'cache-explorer__lane',
      'cache-explorer__box',
      'cache-explorer__lane',
      'cache-explorer__cpu',
    ]);
    const after: string[] = [];
    for (let next = machine.nextElementSibling; next; next = next.nextElementSibling) {
      after.push(next.className.split(' ')[0] ?? '');
    }
    expect(after).toEqual([
      'cache-explorer__log',
      'cache-explorer__lower',
      'cache-explorer__transport',
      'cache-explorer__program',
      'cache-explorer__modules',
      'cache-explorer__live',
    ]);
  });
});

describe('fitsInRow', () => {
  it('says whether the ways of a set, as the design draws them, have the room they need', () => {
    // One way of four words: 64 for the set, 9 of gap, 156 and 4 x 28 for the way, 36 of padding.
    expect(fitsInRow(1, 4, 377)).toBe(true);
    expect(fitsInRow(1, 4, 376)).toBe(false);
    expect(fitsInRow(2, 4, 654)).toBe(true);
    expect(fitsInRow(2, 4, 653)).toBe(false);
  });

  it('needs more room for more ways and for more words, and none of them fit the widest box at four ways', () => {
    expect(fitsInRow(2, 8, 654)).toBe(false);
    expect(fitsInRow(3, 4, 913)).toBe(false);
    expect(fitsInRow(3, 2, 913)).toBe(true);
    expect(fitsInRow(4, 4, 913)).toBe(false);
  });
});
