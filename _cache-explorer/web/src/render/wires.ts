// The wires that tie the address strip to the cache table, and the comparators on them: the tag
// bus runs from the tag field to a comparator at each way of the selected set, the stored tag of
// each way comes down to the same comparator, the index wire runs round the left edge to the set's
// row, and the offset wire rises to the addressed word. They are drawn from where the elements
// are, so they wait for layout: a render only records what to draw, and a frame later, and again
// whenever the box changes size, they are measured and drawn over the box.

import type { FieldKind, WayCompare } from '../../pkg/cache_explorer';
import { copy } from '../copy';
import { h, s } from '../dom';

/** What the wires need to know about the access or the typed address they follow. */
export interface WireModel {
  /** The selected set. */
  set: number;
  /**
   * What each way's comparator says, or null when the set's ways wrap and the comparators are the
   * badges in the ways, and no tag bus is drawn.
   */
  compare: readonly WayCompare[] | null;
  /** The word the offset wire rises to, or null when it is not drawn. */
  word: number | null;
  /** The ways whose word the offset wire rises to. */
  offsetWays: readonly number[];
}

export interface Rect {
  l: number;
  t: number;
  r: number;
  b: number;
}

/** Where things are, in pixels from the top left of the cache box. */
export interface Measured {
  table: Rect;
  /** The header cell of the selected set's row. */
  rowHead: Rect;
  fields: Partial<Record<FieldKind, Rect>>;
  /** The cell of each way of the selected set. */
  ways: ReadonlyMap<number, Rect>;
  /** The addressed word in each of the ways that hold one. */
  words: ReadonlyMap<number, Rect>;
}

type Kind = 'tag' | 'index' | 'offset' | 'strong';

interface Comparator {
  x: number;
  y: number;
  result: WayCompare;
}

export interface Drawing {
  paths: { d: string; kind: Kind }[];
  heads: { points: string; kind: Kind }[];
  dots: { x: number; y: number; kind: Kind }[];
  comparators: Comparator[];
}

const f1 = (n: number): string => n.toFixed(1);

type Point = readonly [number, number];

/** The path through `points`, as an SVG path's `d`. */
const pathOf = (...points: Point[]): string =>
  `M${points.map(([x, y]) => `${f1(x)} ${f1(y)}`).join(' L')}`;

const up = (x: number, y: number): string =>
  `${f1(x)},${f1(y)} ${f1(x - 5)},${f1(y + 8)} ${f1(x + 5)},${f1(y + 8)}`;
const down = (x: number, y: number): string =>
  `${f1(x)},${f1(y)} ${f1(x - 5)},${f1(y - 8)} ${f1(x + 5)},${f1(y - 8)}`;
const right = (x: number, y: number): string =>
  `${f1(x)},${f1(y)} ${f1(x - 8)},${f1(y - 5)} ${f1(x - 8)},${f1(y + 5)}`;

/**
 * The routes the wires take, from where the parts are. The comparators sit 18px under the table,
 * and the tag bus, the offset bus and the index bus run under them, 40, 50 and 60px down, so the
 * three never cross each other before they reach their fields.
 */
export function routeWires(model: WireModel, at: Measured): Drawing {
  const out: Drawing = { paths: [], heads: [], dots: [], comparators: [] };
  const { table, rowHead } = at;
  const ry = (rowHead.t + rowHead.b) / 2;
  const yComparator = table.b + 18;
  const yTag = table.b + 40;
  const yOffset = table.b + 50;
  const yIndex = table.b + 60;
  const gutter = 6;
  const centre = (rect: Rect): number => (rect.l + rect.r) / 2;
  const path = (kind: Kind, ...points: Point[]): void => {
    out.paths.push({ d: pathOf(...points), kind });
  };

  const { index, tag, offset } = at.fields;
  if (index) {
    const x = centre(index);
    path('index', [x, index.t], [x, yIndex], [gutter, yIndex], [gutter, ry], [rowHead.l - 7, ry]);
    out.heads.push({ points: right(rowHead.l, ry), kind: 'index' });
  }

  if (tag && model.compare) {
    const x = centre(tag);
    const lanes = [...at.ways]
      .sort(([a], [b]) => a - b)
      .map(([way, rect]) => ({ way, rect, x: rect.l - 4.5 }));
    if (lanes.length > 0) {
      const lo = Math.min(x, ...lanes.map((lane) => lane.x));
      const hi = Math.max(x, ...lanes.map((lane) => lane.x));
      path('tag', [x, tag.t], [x, yTag]);
      path('tag', [lo, yTag], [hi, yTag]);
      out.dots.push({ x, y: yTag, kind: 'tag' });
      for (const lane of lanes) {
        // The tag comes up into the comparator, and the way's own tag comes down into it.
        path('tag', [lane.x, yTag], [lane.x, yComparator + 17]);
        out.heads.push({ points: up(lane.x, yComparator + 10), kind: 'tag' });
        path('strong', [lane.rect.l, ry], [lane.x, ry], [lane.x, yComparator - 17]);
        out.heads.push({ points: down(lane.x, yComparator - 10), kind: 'strong' });
        out.dots.push({ x: lane.rect.l, y: ry, kind: 'strong' });
        out.comparators.push({
          x: lane.x,
          y: yComparator,
          result: model.compare[lane.way] ?? 'pending',
        });
      }
    }
  }

  const targets = [...at.words.values()].map(centre);
  if (offset && model.word !== null && targets.length > 0) {
    const x = centre(offset);
    const lo = Math.min(x, ...targets);
    const hi = Math.max(x, ...targets);
    path('offset', [x, offset.t], [x, yOffset]);
    if (hi - lo > 1) path('offset', [lo, yOffset], [hi, yOffset]);
    for (const target of targets) {
      path('offset', [target, yOffset], [target, table.b + 8]);
      out.heads.push({ points: up(target, table.b), kind: 'offset' });
    }
  }
  return out;
}

/** A pending comparator is a small `=?`; the rest are one glyph, larger. */
function comparatorNode(box: Comparator): HTMLElement {
  const { glyph, title } = copy.cache.compare[box.result];
  return h(
    'span',
    {
      class: `cache-explorer__comparator cache-explorer__comparator--${box.result}`,
      style: `left: ${f1(box.x - 10)}px; top: ${f1(box.y - 10)}px`,
      title,
    },
    glyph,
  );
}

/** The parts of the overlay a cache box keeps, and what it draws from. */
export interface Wires {
  box: HTMLElement;
  svg: SVGSVGElement;
  /** The comparators: HTML on top of the SVG, since they hold text. */
  badges: HTMLElement;
  /** What the render says to draw; null for nothing. */
  model: WireModel | null;
  frame: number | null;
  observer: ResizeObserver;
}

/** `resized` is called when the box changes size, before the wires are drawn again. */
export function createWires(box: HTMLElement, resized: () => void): Wires {
  const svg = s('svg', { class: 'cache-explorer__wires', 'aria-hidden': 'true' });
  const badges = h('div', { class: 'cache-explorer__comparators', 'aria-hidden': 'true' });
  const wires: Wires = {
    box,
    svg,
    badges,
    model: null,
    frame: null,
    observer: new ResizeObserver(() => {
      resized();
      schedule(wires);
    }),
  };
  wires.observer.observe(box);
  return wires;
}

function schedule(wires: Wires): void {
  if (wires.model === null || wires.frame !== null) return;
  wires.frame = requestAnimationFrame(() => {
    wires.frame = null;
    drawWires(wires);
  });
}

/** Records what to draw: the wires are drawn a frame later, from where the elements are by then. */
export function setWires(wires: Wires, model: WireModel | null): void {
  wires.model = model;
  if (model === null) {
    if (wires.frame !== null) cancelAnimationFrame(wires.frame);
    wires.frame = null;
    wires.svg.replaceChildren();
    wires.badges.replaceChildren();
    return;
  }
  schedule(wires);
}

/** Stops watching the box and cancels a frame that was asked for. */
export function stopWires(wires: Wires): void {
  wires.observer.disconnect();
  if (wires.frame !== null) cancelAnimationFrame(wires.frame);
  wires.frame = null;
}

/** Where the parts of the box are, or null when the box holds none of what the model needs. */
export function measure(wires: Wires, model: WireModel): Measured | null {
  const { box } = wires;
  const origin = box.getBoundingClientRect();
  const scale = origin.width / (box.offsetWidth || 1) || 1;
  const rel = (element: Element): Rect => {
    const r = element.getBoundingClientRect();
    return {
      l: (r.left - origin.left) / scale,
      t: (r.top - origin.top) / scale,
      r: (r.right - origin.left) / scale,
      b: (r.bottom - origin.top) / scale,
    };
  };
  const table = box.querySelector('[data-cachetable]');
  const rowHead = box.querySelector(`[data-rowh='${String(model.set)}']`);
  if (!table || !rowHead) return null;
  const fields: Partial<Record<FieldKind, Rect>> = {};
  for (const field of ['tag', 'index', 'offset'] as const) {
    const found = box.querySelector(`[data-fld='${field}']`);
    if (found) fields[field] = rel(found);
  }
  const cell = (way: number): Element | null =>
    box.querySelector(
      `[data-action='edit-way'][data-set='${String(model.set)}'][data-way='${String(way)}']`,
    );
  const ways = new Map<number, Rect>();
  const words = new Map<number, Rect>();
  const count = model.compare?.length ?? 0;
  for (let way = 0; way < count; way++) {
    const element = cell(way);
    if (element) ways.set(way, rel(element));
  }
  if (model.word !== null) {
    for (const way of model.offsetWays) {
      const word = cell(way)?.querySelector(`[data-word='${String(model.word)}']`);
      if (word) words.set(way, rel(word));
    }
  }
  return { table: rel(table), rowHead: rel(rowHead), fields, ways, words };
}

/** Draws the wires now, from where the elements are. */
export function drawWires(wires: Wires): void {
  const { model, box } = wires;
  if (model === null) return;
  const at = measure(wires, model);
  if (at === null) {
    wires.svg.replaceChildren();
    wires.badges.replaceChildren();
    return;
  }
  const drawing = routeWires(model, at);
  wires.svg.setAttribute('width', String(box.offsetWidth));
  wires.svg.setAttribute('height', String(box.offsetHeight));
  wires.svg.replaceChildren(
    ...drawing.paths.map((path) =>
      s('path', { class: `cache-explorer__wire cache-explorer__wire--${path.kind}`, d: path.d }),
    ),
    ...drawing.heads.map((head) =>
      s('polygon', {
        class: `cache-explorer__wire-head cache-explorer__wire-head--${head.kind}`,
        points: head.points,
      }),
    ),
    ...drawing.dots.map((dot) =>
      s('circle', {
        class: `cache-explorer__wire-dot cache-explorer__wire-dot--${dot.kind}`,
        cx: f1(dot.x),
        cy: f1(dot.y),
        r: 3,
      }),
    ),
  );
  wires.badges.replaceChildren(...drawing.comparators.map(comparatorNode));
}
