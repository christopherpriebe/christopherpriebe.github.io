// The event log: the header with the cursor's step, the timeline's ticks and its scrubber, and the
// entries. The entries are windowed: the core hands over the ones around the first in view, and
// blank rows stand in for the rest, so a long run costs no more to draw than a short one. Only the
// current entry shows its events.

import type { Cause, EntryView, EventRow, Outcome } from '../../pkg/cache_explorer';
import { commit, control } from '../controls';
import { copy } from '../copy';
import { cls, h, s } from '../dom';
import { LOG_ROW_PX, TICK_GAP_PX, TICK_PX, TICK_SHORT_PX, TICKS_HEIGHT_PX } from '../metrics';
import { beatOf } from '../motion';
import type { UiState } from '../ui-state';
import type { LogWindow, Snapshot } from './index';

/** The parts of the log a root keeps between renders. */
export interface LogBox {
  element: HTMLElement;
  /** The header, the ticks and the scrubber: rebuilt, but the scrubber in it is kept. */
  timeline: HTMLElement;
  /** The scroller: kept, so its scroll position is, and its content rebuilt. */
  scroller: HTMLElement;
  /** Whether the log has been drawn once: the first drawing brings the current entry into view too. */
  drawn: boolean;
  /** How many steps a tick was last drawn for: 1 for a tick of its own, more for a bucket. */
  bucket: number;
  /** Tells the box when the width of the ticks' strip changes. */
  observer: ResizeObserver;
}

/**
 * The log. `resized` is called when the strip of ticks changes width, so that the ticks can be
 * drawn again if they now fit differently from how they were drawn.
 */
export function createLog(resized: () => void): LogBox {
  const timeline = h('div', { class: 'cache-explorer__timeline' });
  const scroller = h('div', { class: 'cache-explorer__log-scroller', 'data-scroll': 'log' });
  const element = h('div', { class: 'cache-explorer__log' }, timeline, scroller);
  const observer = new ResizeObserver(resized);
  observer.observe(timeline);
  return { element, timeline, scroller, drawn: false, bucket: 1, observer };
}

/** Stops watching the strip. */
export function stopLog(box: LogBox): void {
  box.observer.disconnect();
}

/**
 * How a tick or a bucket is drawn: `on` when the cursor has reached it, and for the first step of
 * a miss `open` instead, as the entry's square is; `ahead` for an entry's first step the cursor
 * has not reached, `off` for the rest.
 */
type TickLook = 'on' | 'open' | 'ahead' | 'off';

const FIRST_STEP = 1;
const MISS_STEP = 2;

function look(bits: number, reached: boolean): TickLook {
  const first = (bits & FIRST_STEP) !== 0;
  const miss = (bits & MISS_STEP) !== 0;
  if (reached) return first && miss ? 'open' : 'on';
  return first && !miss ? 'ahead' : 'off';
}

function tickClass(kind: TickLook, tall: boolean): string {
  return cls(
    'cache-explorer__tick',
    `cache-explorer__tick--${kind}`,
    tall && 'cache-explorer__tick--tall',
  );
}

/** One element for each step, as wide as the strip allows. */
function stepTicks(snap: Snapshot): HTMLElement {
  const { ticks, timeline } = snap;
  return h(
    'div',
    { class: 'cache-explorer__ticks', 'aria-hidden': 'true' },
    ...Array.from(ticks, (bits, index) =>
      h('span', {
        class: tickClass(look(bits, index < timeline.step), (bits & FIRST_STEP) !== 0),
        'data-action': 'seek-tick',
        'data-step': index + 1,
      }),
    ),
  );
}

/**
 * One SVG for a run with more steps than the strip has room for: a bucket of `size` steps is drawn
 * on when any of its steps is at or before the cursor, and tall when one of them starts an entry.
 * A bucket seeks to its first step.
 */
function bucketTicks(snap: Snapshot, size: number): HTMLElement {
  const { ticks, timeline } = snap;
  const count = Math.ceil(ticks.length / size);
  const pitch = TICK_PX + TICK_GAP_PX;
  const marks: SVGElement[] = [];
  for (let bucket = 0; bucket < count; bucket++) {
    const from = bucket * size;
    let tall = false;
    for (const bits of ticks.subarray(from, from + size)) tall ||= (bits & FIRST_STEP) !== 0;
    const height = tall ? TICKS_HEIGHT_PX : TICK_SHORT_PX;
    marks.push(
      s('rect', {
        class: tickClass(from < timeline.step ? 'on' : 'off', tall),
        x: bucket * pitch,
        y: TICKS_HEIGHT_PX - height,
        width: TICK_PX,
        height,
      }),
      // The bucket's whole pitch takes the click, not the 2px that are drawn.
      s('rect', {
        class: 'cache-explorer__tick-target',
        x: bucket * pitch,
        y: 0,
        width: pitch,
        height: TICKS_HEIGHT_PX,
        'data-action': 'seek-tick',
        'data-step': from + 1,
      }),
    );
  }
  return h(
    'div',
    { class: 'cache-explorer__ticks cache-explorer__ticks--buckets', 'aria-hidden': 'true' },
    s(
      'svg',
      {
        width: '100%',
        height: TICKS_HEIGHT_PX,
        viewBox: `0 0 ${String(count * pitch - TICK_GAP_PX)} ${String(TICKS_HEIGHT_PX)}`,
        preserveAspectRatio: 'none',
      },
      ...marks,
    ),
  );
}

/**
 * How many steps a tick stands for in a strip `width` pixels wide: one each while they fit at the
 * narrowest a tick is drawn, else buckets. A box not on the page has no width and is taken to
 * have room.
 */
function bucketSize(steps: number, width: number): number {
  const room = Math.floor((width + TICK_GAP_PX) / (TICK_PX + TICK_GAP_PX));
  if (width <= 0 || steps <= room) return 1;
  return Math.ceil(steps / room);
}

/** Whether the strip has changed width enough for its ticks to want drawing the other way. */
export function refitsTicks(box: LogBox, snap: Snapshot): boolean {
  return bucketSize(snap.ticks.length, box.timeline.getBoundingClientRect().width) !== box.bucket;
}

function scrubber(root: HTMLElement, snap: Snapshot): HTMLInputElement {
  const { timeline } = snap;
  const input = control(root, 'scrubber', () =>
    h('input', {
      type: 'range',
      min: 0,
      class: 'cache-explorer__scrubber',
      'data-action': 'scrub',
      'aria-label': copy.log.timeline,
    }),
  );
  input.max = String(timeline.stepCount);
  // A range has no caret to keep, and the core may have clamped the step the student asked for, so
  // the thumb goes where the core put the cursor even while it has focus.
  input.value = String(timeline.step);
  input.setAttribute('aria-valuetext', timeline.headerText);
  return input;
}

/** A square as the design draws the log's: filled for a hit, open for a miss. */
function marker(outcome: EntryView['outcome']): HTMLElement {
  const kind: Record<EntryView['outcome'], Outcome | 'error'> = {
    hit: 'hit',
    miss: 'miss',
    edit: 'hit',
    cpuOnly: 'hit',
    program: 'hit',
    error: 'error',
  };
  return h('span', {
    class: `cache-explorer__marker cache-explorer__marker--${kind[outcome]}`,
    'aria-hidden': 'true',
  });
}

function whyLink(cause: Cause, pos: number): HTMLElement {
  return h(
    'button',
    {
      type: 'button',
      class: 'cache-explorer__why',
      'data-action': 'why',
      'data-group': cause,
      'data-pos': pos,
    },
    `${copy.log.why} ${copy.config.groups[cause]}`,
  );
}

function eventRow(row: EventRow): HTMLElement {
  return h(
    'div',
    {
      class: cls('cache-explorer__event', `cache-explorer__event--${row.tense}`),
      'data-action': 'seek-event',
      'data-pos': row.pos,
      'data-tense': row.tense,
    },
    h('span', { class: 'cache-explorer__event-number' }, row.label),
    h(
      'span',
      { class: 'cache-explorer__event-text' },
      row.text,
      row.cause && ' ',
      row.cause && whyLink(row.cause, row.pos),
    ),
    row.cycle !== undefined &&
      h('span', { class: 'cache-explorer__event-cycle' }, copy.log.cycle(row.cycle)),
  );
}

/** What a past or current entry changed, on the changed ground the cells use. */
function changedLine(summary: string): HTMLElement {
  return h(
    'div',
    { class: 'cache-explorer__changed-line' },
    h('span', { class: 'cache-explorer__changed-chip' }, copy.log.changed),
    h('span', { class: 'cache-explorer__changed-text' }, summary),
  );
}

/** A program load is a divider between the entries before it and the ones after. */
function divider(entry: EntryView): HTMLElement {
  return h(
    'div',
    {
      class: cls(
        'cache-explorer__entry',
        'cache-explorer__entry--divider',
        `cache-explorer__entry--${entry.tense}`,
      ),
      'data-entry': entry.index,
      'data-tense': entry.tense,
    },
    h(
      'button',
      {
        type: 'button',
        class: 'cache-explorer__divider',
        'data-action': 'seek-entry',
        'data-pos': entry.last,
      },
      h('span', { class: 'cache-explorer__divider-text' }, entry.label),
    ),
    entry.events.length > 0 &&
      h('div', { class: 'cache-explorer__events' }, ...entry.events.map(eventRow)),
  );
}

function entryRow(entry: EntryView): HTMLElement {
  if (entry.kind === 'program') return divider(entry);
  return h(
    'div',
    {
      class: cls('cache-explorer__entry', `cache-explorer__entry--${entry.tense}`),
      'data-entry': entry.index,
      'data-tense': entry.tense,
    },
    h(
      'button',
      {
        type: 'button',
        class: 'cache-explorer__entry-head',
        'data-action': 'seek-entry',
        'data-pos': entry.last,
      },
      marker(entry.outcome),
      h(
        'span',
        { class: 'cache-explorer__entry-label' },
        entry.number === undefined ? entry.label : `#${String(entry.number)}  ${entry.label}`,
      ),
      h('span', { class: 'cache-explorer__entry-outcome' }, entry.outcomeText),
    ),
    entry.events.length > 0 &&
      h('div', { class: 'cache-explorer__events' }, ...entry.events.map(eventRow)),
    entry.changedSummary !== undefined && changedLine(entry.changedSummary),
  );
}

/** A blank row as tall as the `entries` it stands for; nothing to a screen reader. */
function spacer(entries: number): HTMLElement | null {
  if (entries <= 0) return null;
  return h('div', {
    class: 'cache-explorer__log-spacer',
    'aria-hidden': 'true',
    style: `height: ${String(entries * LOG_ROW_PX)}px`,
  });
}

/**
 * The first built entry that is in view, or null when none is: nothing is built, or the scroller
 * is in one of the blank rows, where the entry it shows is a guess.
 */
export function firstInView(scroller: HTMLElement): HTMLElement | null {
  const rows = scroller.querySelectorAll<HTMLElement>('[data-entry]');
  const { scrollTop } = scroller;
  const [top] = rows;
  if (!top || scrollTop < top.offsetTop) return null;
  for (const row of rows) {
    if (row.offsetTop + row.offsetHeight > scrollTop) return row;
  }
  return null;
}

/** Where the first entry in view is, so that it can be put back there. */
function anchorOf(scroller: HTMLElement): { entry: string; offset: number } | null {
  const row = firstInView(scroller);
  if (!row?.dataset.entry) return null;
  return { entry: row.dataset.entry, offset: row.offsetTop - scroller.scrollTop };
}

/** Puts the entry `anchor` names back where it was; the blank rows are estimates, and the built ones are not. */
function holdAnchor(scroller: HTMLElement, anchor: { entry: string; offset: number } | null): void {
  if (!anchor) return;
  const row = scroller.querySelector<HTMLElement>(`[data-entry='${anchor.entry}']`);
  if (row) scroller.scrollTop = row.offsetTop - anchor.offset;
}

/** Brings `row`, which is built, into view if it is not; the page does not move. */
function reveal(scroller: HTMLElement, row: HTMLElement): void {
  const top = row.offsetTop;
  const bottom = top + row.offsetHeight;
  if (top < scroller.scrollTop) scroller.scrollTop = top;
  else if (bottom > scroller.scrollTop + scroller.clientHeight) {
    scroller.scrollTop = bottom - scroller.clientHeight;
  }
}

/**
 * Draws the log. When the cursor has moved since the last render, and on the first drawing, the
 * current entry's row comes into view.
 */
export function renderLog(
  root: HTMLElement,
  box: LogBox,
  snap: Snapshot,
  log: LogWindow,
  ui: UiState,
): void {
  const { timeline } = snap;
  const moved = !box.drawn || beatOf(snap.state.position.pos, ui.lastPos) !== null;
  box.drawn = true;
  box.bucket = bucketSize(snap.ticks.length, box.timeline.getBoundingClientRect().width);
  commit(box.timeline, (put) => [
    h(
      'div',
      { class: 'cache-explorer__log-head' },
      h('span', { class: 'cache-explorer__label cache-explorer__label--strong' }, copy.log.title),
      h('span', { class: 'cache-explorer__label' }, timeline.headerText),
    ),
    // The scrubber sits inside a box of its own: `commit` puts the control where its stand-in is,
    // and a control that is a child of the region itself would be swept away with the old content.
    h(
      'div',
      { class: 'cache-explorer__scrub' },
      box.bucket === 1 ? stepTicks(snap) : bucketTicks(snap, box.bucket),
      put(scrubber(root, snap)),
    ),
  ]);

  const anchor = moved ? null : anchorOf(box.scroller);
  const first = ui.logFirst;
  box.scroller.replaceChildren(
    ...(timeline.entryCount === 0
      ? [
          h(
            'div',
            { class: 'cache-explorer__log-empty' },
            h('p', { class: 'cache-explorer__log-lead' }, copy.log.empty.lead),
            h('p', { class: 'cache-explorer__log-note' }, copy.log.empty.text),
          ),
        ]
      : [
          spacer(first),
          ...log.map(entryRow),
          spacer(timeline.entryCount - first - log.length),
        ].filter((node) => node !== null)),
  );
  holdAnchor(box.scroller, anchor);
  if (moved) {
    const target =
      box.scroller.querySelector<HTMLElement>('.cache-explorer__event--current') ??
      box.scroller.querySelector<HTMLElement>('.cache-explorer__entry--current');
    if (target) reveal(box.scroller, target);
  }
}
