// The cache box: the cache table, one row for each set and one cell for each way, and under it the
// address strip and the wires that tie the two together. A way's cell holds its valid bit, tag,
// words, rank and dirty bit, side by side as the design draws them; a cache with more than four
// ways, or whose ways do not fit side by side, wraps the ways of a set onto rows, and puts each
// way's comparator in the way itself.

import type {
  CacheView,
  Geometry,
  RankRole,
  SetView,
  WayCompare,
  WayView,
} from '../../pkg/cache_explorer';
import { copy } from '../copy';
import { cls, h, type Child } from '../dom';
import { addressStrip } from './address-strip';
import type { Snapshot } from './index';
import { cellKey, type Marks } from './marks';
import type { AddressSource } from './source';
import { createWires, setWires, type WireModel, type Wires } from './wires';

/**
 * The narrowest a wrapped way may be drawn: what its valid bit, tag, rank and dirty bit take,
 * and what each word of a line takes.
 */
const WAY_MIN_BASE_PX = 156;
const WAY_MIN_WORD_PX = 34;

/**
 * What the ways of a set need to sit side by side, as the design draws them: the set's own column
 * and a gap for each way, the parts of a way that are not its words, and about as much for a
 * word as the widest value takes. The box's padding, at both sides, is 36px.
 */
const SET_COLUMN_PX = 64;
const WAY_GAP_PX = 9;
const ROW_WORD_PX = 28;
const BOX_PADDING_PX = 36;

/**
 * How much more room than it needs a box that has wrapped must have before it goes back to one
 * row: without it a width at the edge would flip the table on the scrollbar the flip adds or takes.
 */
const HYSTERESIS_PX = 24;

/** Whether `ways` ways of `wordsPerLine` words fit in a row in a box `width` pixels wide. */
export function fitsInRow(ways: number, wordsPerLine: number, width: number): boolean {
  const way = WAY_MIN_BASE_PX + ROW_WORD_PX * wordsPerLine;
  return SET_COLUMN_PX + WAY_GAP_PX * ways + way * ways + BOX_PADDING_PX <= width;
}

/** The parts of the cache box a root keeps between renders. */
export interface CacheBox {
  element: HTMLElement;
  wires: Wires;
  /** Whether the table was last drawn with its ways wrapped. */
  wrapped: boolean;
}

/**
 * The cache box. `resized` is called when it changes size, so that the table can be drawn again
 * if the ways no longer fit as they were drawn.
 */
export function createCache(resized: () => void): CacheBox {
  const element = h('div', { class: 'cache-explorer__box cache-explorer__box--cache' });
  return { element, wires: createWires(element, resized), wrapped: false };
}

/**
 * Whether the table wraps its ways: the core says so above four ways, and a set whose ways cannot
 * be given room side by side wraps too, which a 4-way cache always is at the widest the tool is
 * drawn, and a 2-way one is on a narrow tablet. A box not yet on the page has no width to go by,
 * and is taken to have room.
 */
function wrapsWays(box: CacheBox, snap: Snapshot): boolean {
  const { cache, geometry } = snap.state;
  const width = box.element.clientWidth;
  if (cache.wrap) return true;
  if (width === 0) return false;
  return !fitsInRow(
    geometry.ways,
    geometry.wordsPerLine,
    box.wrapped ? width - HYSTERESIS_PX : width,
  );
}

/** Whether the box has changed size enough for its table to want drawing the other way. */
export function refits(box: CacheBox, snap: Snapshot): boolean {
  return wrapsWays(box, snap) !== box.wrapped;
}

/** What a way draws, apart from the way itself. */
interface Drawing {
  cache: CacheView;
  marks: Marks;
  /** The comparator of each way of the selected set, drawn in the way when the ways wrap. */
  badges: readonly WayCompare[] | null;
  selected: number | null;
}

function rankCell(way: WayView, drawing: Drawing, set: number, index: number): Child {
  if (!drawing.cache.showRank || !way.rank) return null;
  const role: RankRole | undefined = way.rank.role;
  return h(
    'span',
    { class: cls('cache-explorer__rank', ...drawing.marks.classes(cellKey.rank(set, index))) },
    String(way.rank.value),
    role && h('span', { class: 'cache-explorer__role' }, copy.cache.roles[role]),
  );
}

/** One way: `td` in a set's row, or a `div` once the ways of a set wrap. */
function wayCell(
  tag: 'td' | 'div',
  set: number,
  index: number,
  way: WayView,
  drawing: Drawing,
): HTMLElement {
  const { cache, marks } = drawing;
  const words = way.valid ? way.words : cache.wordHeads.map(() => '·');
  return h(
    tag,
    {
      class: cls(
        'cache-explorer__way',
        !way.valid && 'cache-explorer__way--empty',
        way.victim && 'cache-explorer__way--victim',
        cache.showRank && 'cache-explorer__way--rank',
        cache.showDirty && 'cache-explorer__way--dirty',
      ),
      role: tag === 'td' ? 'cell' : 'group',
      tabindex: 0,
      'data-action': 'edit-way',
      'data-set': set,
      'data-way': index,
      'aria-label': `${way.description} ${copy.cache.edit}`,
    },
    way.valid &&
      way.dirty &&
      h('span', { class: 'cache-explorer__dirty-bar', 'aria-hidden': 'true' }),
    h(
      'span',
      { class: cls('cache-explorer__valid', ...marks.classes(cellKey.valid(set, index))) },
      way.valid ? '1' : '0',
    ),
    h(
      'span',
      { class: cls('cache-explorer__tag', ...marks.classes(cellKey.tag(set, index))) },
      way.tagText ?? '—',
    ),
    h(
      'span',
      { class: 'cache-explorer__words' },
      ...words.map((text, word) =>
        h(
          'span',
          {
            class: cls(
              'cache-explorer__word',
              way.addressedWord === word && 'cache-explorer__word--addressed',
              ...marks.classes(cellKey.word(set, index, word)),
            ),
            'data-word': word,
          },
          text,
        ),
      ),
    ),
    rankCell(way, drawing, set, index),
    cache.showDirty &&
      h(
        'span',
        { class: cls('cache-explorer__dirty', ...marks.classes(cellKey.dirty(set, index))) },
        way.valid && way.dirty ? copy.cache.heads.dirty : '·',
      ),
    way.victim && h('span', { class: 'cache-explorer__victim' }, copy.cache.victim),
  );
}

/** The comparator of a wrapped way, in its name line; only the ways of the selected set have one. */
function compareBadge(drawing: Drawing, set: number, index: number): Child {
  const result = drawing.selected === set ? drawing.badges?.[index] : undefined;
  if (result === undefined) return null;
  const { glyph, title } = copy.cache.compare[result];
  return h(
    'span',
    { class: `cache-explorer__compare cache-explorer__compare--${result}`, title },
    glyph,
  );
}

/** The column heads over one way: V, Tag, Data and the words' offsets, rank and D, ruled in their colours. */
function subHeads(cache: CacheView): HTMLElement {
  const { heads, rank } = copy.cache;
  const head = (field: string, ...content: Child[]): HTMLElement =>
    h('span', { class: `cache-explorer__sub-head cache-explorer__sub-head--${field}` }, ...content);
  return h(
    'span',
    {
      class: cls(
        'cache-explorer__way-heads',
        cache.showRank && 'cache-explorer__way--rank',
        cache.showDirty && 'cache-explorer__way--dirty',
      ),
    },
    head('valid', heads.valid),
    head('tag', heads.tag),
    head('data', `${heads.data} ${cache.wordHeads.join(' ')}`),
    cache.showRank && cache.rankHeader && head('rank', rank[cache.rankHeader]),
    cache.showDirty && head('dirty', heads.dirty),
  );
}

function setHead(): HTMLElement {
  const [first, second] = copy.cache.setHead;
  return h(
    'th',
    { scope: 'col', role: 'columnheader', class: 'cache-explorer__set-head' },
    first,
    h('br'),
    second,
  );
}

function rowHead(set: number, selected: boolean): HTMLElement {
  return h(
    'th',
    {
      scope: 'row',
      role: 'rowheader',
      class: cls('cache-explorer__set', selected && 'cache-explorer__set--selected'),
      'data-rowh': set,
    },
    String(set),
  );
}

/** The table as the design draws it: a column for each way, with its name and heads over it. */
function plainTable(cache: CacheView, drawing: Drawing, ways: number): HTMLElement {
  const columns = `--cache-explorer-ways: ${String(ways)}`;
  return h(
    'table',
    {
      role: 'table',
      class: 'cache-explorer__cache',
      style: columns,
      'aria-label': cache.header,
      'data-cachetable': '',
    },
    h(
      'thead',
      { role: 'rowgroup' },
      h(
        'tr',
        { role: 'row', class: 'cache-explorer__cache-row cache-explorer__cache-row--head' },
        setHead(),
        ...Array.from({ length: ways }, (_, way) =>
          h(
            'th',
            { scope: 'col', role: 'columnheader', class: 'cache-explorer__way-head' },
            h('span', { class: 'cache-explorer__way-name' }, `${copy.cache.way} ${String(way)}`),
            subHeads(cache),
          ),
        ),
      ),
    ),
    h(
      'tbody',
      { role: 'rowgroup' },
      ...cache.sets.map((row) =>
        h(
          'tr',
          { role: 'row', class: 'cache-explorer__cache-row' },
          rowHead(row.set, drawing.selected === row.set),
          ...row.ways.map((way, index) => wayCell('td', row.set, index, way, drawing)),
        ),
      ),
    ),
  );
}

/**
 * The table once the ways of a set wrap onto rows, which the design does not draw. Each way is a
 * tile: its name over its cell, the design's "Way n" head moved down to it. The column
 * heads are drawn once, over the first row of tiles, by the same grid the tiles are in, so each
 * sits over its own column; the copies that would fall on later rows are given no height.
 */
function wrappedTable(cache: CacheView, drawing: Drawing, geometry: Geometry): HTMLElement {
  const min = WAY_MIN_BASE_PX + WAY_MIN_WORD_PX * geometry.wordsPerLine;
  const ways = geometry.ways;
  return h(
    'table',
    {
      role: 'table',
      class: 'cache-explorer__cache cache-explorer__cache--wrapped',
      style: `--cache-explorer-way-min: ${String(min)}px`,
      'aria-label': cache.header,
      'data-cachetable': '',
    },
    h(
      'thead',
      { role: 'rowgroup' },
      h(
        'tr',
        { role: 'row', class: 'cache-explorer__cache-row cache-explorer__cache-row--head' },
        setHead(),
        h(
          'th',
          { scope: 'col', role: 'columnheader', class: 'cache-explorer__wrap-heads' },
          ...Array.from({ length: ways }, (_, way) =>
            h(
              'span',
              { class: 'cache-explorer__wrap-head', 'aria-hidden': way === 0 ? null : 'true' },
              subHeads(cache),
            ),
          ),
        ),
      ),
    ),
    h(
      'tbody',
      { role: 'rowgroup' },
      ...cache.sets.map((row: SetView) =>
        h(
          'tr',
          { role: 'row', class: 'cache-explorer__cache-row' },
          rowHead(row.set, drawing.selected === row.set),
          h(
            'td',
            { role: 'cell', class: 'cache-explorer__tiles' },
            ...row.ways.map((way, index) =>
              h(
                'div',
                { class: 'cache-explorer__tile' },
                h(
                  'span',
                  { class: 'cache-explorer__way-name' },
                  `${copy.cache.way} ${String(index)}`,
                  compareBadge(drawing, row.set, index),
                ),
                wayCell('div', row.set, index, way, drawing),
              ),
            ),
          ),
        ),
      ),
    ),
  );
}

/** What the wires need of the source: null while there is nothing they could tie together. */
function wireModel(cache: CacheView, source: AddressSource, wrapped: boolean): WireModel | null {
  const { decoded, set } = source;
  if (decoded === null || set === null) return null;
  const ways = cache.sets[set]?.ways ?? [];
  // While an access is at the cursor, the offset wire rises to the way that holds its line; before
  // that, or for an address that is only typed, to the word in every way.
  const holding = ways.flatMap((way, index) => (way.addressedWord === undefined ? [] : [index]));
  const every = ways.map((_, index) => index);
  return {
    set,
    compare: wrapped ? null : source.compare,
    word: wrapped ? null : decoded.word,
    offsetWays: source.kind === 'access' && holding.length > 0 ? holding : every,
  };
}

/** Draws the cache box: heading, table, strip, and the wires over both. */
export function renderCache(
  box: CacheBox,
  snap: Snapshot,
  source: AddressSource,
  marks: Marks,
): void {
  const { cache, geometry } = snap.state;
  const wrapped = wrapsWays(box, snap);
  box.wrapped = wrapped;
  const drawing: Drawing = {
    cache,
    marks,
    badges: wrapped ? source.compare : null,
    selected: source.set,
  };
  box.element.replaceChildren(
    h(
      'div',
      { class: 'cache-explorer__box-head' },
      h(
        'span',
        {
          class:
            'cache-explorer__label cache-explorer__label--strong cache-explorer__label--nowrap',
        },
        cache.header,
      ),
      h('span', { class: 'cache-explorer__label' }, copy.cache.editHint),
    ),
    wrapped ? wrappedTable(cache, drawing, geometry) : plainTable(cache, drawing, geometry.ways),
    addressStrip(source, geometry),
    box.wires.svg,
    box.wires.badges,
  );
  setWires(box.wires, wireModel(cache, source, wrapped));
}
