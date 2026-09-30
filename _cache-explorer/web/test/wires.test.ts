import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { SCENARIOS } from '../dev/sessions';
import { buildSnapshot } from '../src/controller';
import { h } from '../src/dom';
import { detach, render } from '../src/render';
import { routeWires, type Measured, type WireModel } from '../src/render/wires';
import { initialUiState, type UiState } from '../src/ui-state';
import { Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

/** A rectangle from its corners. */
const rect = (l: number, t: number, r: number, b: number): Measured['table'] => ({ l, t, r, b });

/** Two ways in set 2 of a table 18 to 882 wide, ending at 300, over a strip that starts at 372. */
const AT: Measured = {
  table: rect(18, 60, 882, 300),
  rowHead: rect(18, 200, 82, 230),
  fields: {
    tag: rect(18, 372, 400, 412),
    index: rect(410, 372, 500, 412),
    offset: rect(510, 372, 882, 412),
  },
  ways: new Map([
    [0, rect(100, 200, 480, 230)],
    [1, rect(500, 200, 880, 230)],
  ]),
  words: new Map([
    [0, rect(330, 200, 360, 230)],
    [1, rect(730, 200, 760, 230)],
  ]),
};

const MODEL: WireModel = {
  set: 2,
  compare: ['mismatch', 'pending'],
  word: 1,
  offsetWays: [0, 1],
};

describe('routeWires', () => {
  it('runs the index wire from its field round the left edge into the set’s row header', () => {
    const { paths, heads } = routeWires(MODEL, AT);
    // The index bus is 60px under the table, the gutter is 6px from its left, the row's middle is 215.
    expect(paths[0]).toEqual({
      d: 'M455.0 372.0 L455.0 360.0 L6.0 360.0 L6.0 215.0 L11.0 215.0',
      kind: 'index',
    });
    expect(heads[0]).toEqual({ points: '18.0,215.0 10.0,210.0 10.0,220.0', kind: 'index' });
  });

  it('runs the tag bus from the tag field to every way of the set, and back down to a comparator', () => {
    const { paths, dots } = routeWires(MODEL, AT);
    const tag = paths.filter((path) => path.kind === 'tag').map((path) => path.d);
    // The bus is 40px under the table; each lane is 4.5px left of its way, in the gap between ways.
    expect(tag).toContain('M209.0 372.0 L209.0 340.0');
    expect(tag).toContain('M95.5 340.0 L495.5 340.0');
    expect(tag).toContain('M95.5 340.0 L95.5 335.0');
    expect(tag).toContain('M495.5 340.0 L495.5 335.0');
    expect(dots).toContainEqual({ x: 209, y: 340, kind: 'tag' });
  });

  it('brings each way’s own tag from the left edge of its cell down to the same comparator', () => {
    const { paths, dots, heads } = routeWires(MODEL, AT);
    const strong = paths.filter((path) => path.kind === 'strong').map((path) => path.d);
    expect(strong).toEqual([
      'M100.0 215.0 L95.5 215.0 L95.5 301.0',
      'M500.0 215.0 L495.5 215.0 L495.5 301.0',
    ]);
    expect(dots.filter((dot) => dot.kind === 'strong')).toEqual([
      { x: 100, y: 215, kind: 'strong' },
      { x: 500, y: 215, kind: 'strong' },
    ]);
    expect(heads.filter((head) => head.kind === 'strong')).toHaveLength(2);
  });

  it('puts a comparator 18px under the table at each lane, with what the core says of its way', () => {
    const { comparators } = routeWires(MODEL, AT);
    expect(comparators).toEqual([
      { x: 95.5, y: 318, result: 'mismatch' },
      { x: 495.5, y: 318, result: 'pending' },
    ]);
  });

  it('raises the offset wire from its field to the word of each way it points at', () => {
    const { paths, heads } = routeWires(MODEL, AT);
    const offset = paths.filter((path) => path.kind === 'offset').map((path) => path.d);
    // The offset bus is 50px under the table, and each branch stops 8px under it, at the arrowhead.
    expect(offset).toEqual([
      'M696.0 372.0 L696.0 350.0',
      'M345.0 350.0 L745.0 350.0',
      'M345.0 350.0 L345.0 308.0',
      'M745.0 350.0 L745.0 308.0',
    ]);
    expect(heads.filter((head) => head.kind === 'offset').map((head) => head.points)).toEqual([
      '345.0,300.0 340.0,308.0 350.0,308.0',
      '745.0,300.0 740.0,308.0 750.0,308.0',
    ]);
  });

  it('draws one branch, and no bus, when the offset wire points at a single word straight above its field', () => {
    const one: Measured = { ...AT, words: new Map([[0, rect(690, 200, 702, 230)]]) };
    const { paths } = routeWires({ ...MODEL, offsetWays: [0] }, one);
    expect(paths.filter((path) => path.kind === 'offset').map((path) => path.d)).toEqual([
      'M696.0 372.0 L696.0 350.0',
      'M696.0 350.0 L696.0 308.0',
    ]);
  });

  it('draws no tag bus or comparator, and no offset wire, for ways that wrap: the badges are in the ways', () => {
    const { paths, comparators, heads } = routeWires({ ...MODEL, compare: null, word: null }, AT);
    expect(paths.map((path) => path.kind)).toEqual(['index']);
    expect(comparators).toEqual([]);
    expect(heads.map((head) => head.kind)).toEqual(['index']);
  });

  it('leaves out a wire whose field is not there: a fully associative cache has no index', () => {
    const fields = { tag: rect(18, 372, 400, 412), offset: rect(510, 372, 882, 412) };
    const { paths } = routeWires(MODEL, { ...AT, fields });
    expect(paths.some((path) => path.kind === 'index')).toBe(false);
    expect(paths.some((path) => path.kind === 'tag')).toBe(true);
  });

  it('shows a comparator for the way it could not measure as still waiting', () => {
    const { comparators } = routeWires({ ...MODEL, compare: [] }, AT);
    expect(comparators.map((box) => box.result)).toEqual(['pending', 'pending']);
  });
});

describe('the wires over the cache box', () => {
  let root: HTMLElement;
  let session: Session | null;
  let ui: UiState;
  /** The frames the page has been asked for and has not yet given. */
  let frames: (() => void)[];
  /** What is watching a size, and whether it has been told to stop. */
  let watchers: { callback: () => void; targets: Element[]; stopped: boolean }[];

  function current(): Session {
    if (!session) throw new Error('no session');
    return session;
  }

  function draw(): void {
    const { snapshot, log } = buildSnapshot(current(), ui);
    render(root, snapshot, log, ui);
  }

  /** Gives the page the frames it asked for. */
  function flush(): void {
    const due = frames.splice(0);
    for (const frame of due) frame();
  }

  const query = (selector: string): HTMLElement => {
    const found = root.querySelector<HTMLElement>(selector);
    if (!found) throw new Error(`nothing matches ${selector}`);
    return found;
  };

  /** Where the parts of the box are, in the design's own numbers, from the attributes they carry. */
  function place(this: Element): DOMRect {
    const set = (l: number, t: number, r: number, b: number): DOMRect =>
      new DOMRect(l, t, r - l, b - t);
    const cell = this.closest('[data-action="edit-way"]');
    if (this.classList.contains('cache-explorer__box--cache')) return set(0, 0, 900, 600);
    if (this.hasAttribute('data-cachetable')) return set(18, 60, 882, 300);
    if (this.hasAttribute('data-rowh')) return set(18, 200, 82, 230);
    const field = this.getAttribute('data-fld');
    if (field === 'tag') return set(18, 372, 400, 412);
    if (field === 'index') return set(410, 372, 500, 412);
    if (field === 'offset') return set(510, 372, 882, 412);
    if (this.hasAttribute('data-word') && cell) {
      const left = 100 + Number(cell.getAttribute('data-way')) * 400 + 230;
      return set(left, 200, left + 30, 230);
    }
    if (this.getAttribute('data-action') === 'edit-way') {
      const left = 100 + Number(this.getAttribute('data-way')) * 400;
      return set(left, 200, left + 380, 230);
    }
    return set(0, 0, 0, 0);
  }

  beforeAll(loadWasm);

  beforeEach(() => {
    frames = [];
    watchers = [];
    vi.stubGlobal('requestAnimationFrame', (callback: () => void) => frames.push(callback));
    vi.stubGlobal('cancelAnimationFrame', (id: number) => {
      frames[id - 1] = () => undefined;
    });
    vi.stubGlobal(
      'ResizeObserver',
      class implements ResizeObserver {
        readonly entry: (typeof watchers)[number];
        constructor(callback: ResizeObserverCallback) {
          this.entry = {
            callback: () => {
              callback([], this);
            },
            targets: [],
            stopped: false,
          };
          watchers.push(this.entry);
        }
        observe(target: Element): void {
          this.entry.targets.push(target);
        }
        unobserve(): void {
          // Not used.
        }
        disconnect(): void {
          this.entry.stopped = true;
        }
      },
    );
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(place);
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(900);
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(600);
    session = Session.firstVisit();
    root = h('div', { class: 'cache-explorer' });
    document.body.replaceChildren(root);
    ui = initialUiState('desktop');
  });

  afterEach(() => {
    detach(root);
    const owned = session;
    session = null;
    owned?.free();
  });

  it('are drawn a frame after the render, from where the parts are then', () => {
    session?.free();
    session = SCENARIOS['frame-3b']?.() ?? Session.firstVisit();
    draw();
    expect(root.querySelector('.cache-explorer__wires')?.childElementCount).toBe(0);
    expect(frames).toHaveLength(1);
    flush();
    const svg = query('.cache-explorer__wires');
    expect(svg.getAttribute('width')).toBe('900');
    expect(svg.getAttribute('height')).toBe('600');
    expect(svg.querySelectorAll('path').length).toBeGreaterThan(0);
    expect(svg.querySelectorAll('polygon').length).toBeGreaterThan(0);
    expect(svg.querySelectorAll('circle').length).toBeGreaterThan(0);
  });

  it('put a comparator over each way of the set, where the way’s lane is', () => {
    session?.free();
    session = SCENARIOS['frame-3b']?.() ?? Session.firstVisit();
    draw();
    flush();
    const comparators = [...root.querySelectorAll<HTMLElement>('.cache-explorer__comparator')];
    expect(comparators.map((box) => box.getAttribute('style'))).toEqual([
      'left: 85.5px; top: 308.0px',
      'left: 485.5px; top: 308.0px',
    ]);
    const results = current().state().cache.compare ?? [];
    const glyph = { pending: '=?', match: '=', mismatch: '≠', invalid: '≠' } as const;
    expect(comparators.map((box) => box.textContent)).toEqual(results.map((r) => glyph[r]));
    expect(comparators[0]?.getAttribute('class')).toContain(`--${results[0] ?? ''}`);
  });

  it('draw only the index wire when the ways wrap, since their comparators are in the ways', () => {
    session?.free();
    session = SCENARIOS['wrapped-four-sets']?.() ?? Session.firstVisit();
    draw();
    flush();
    const svg = query('.cache-explorer__wires');
    expect(svg.querySelectorAll('.cache-explorer__wire--index')).toHaveLength(1);
    expect(svg.querySelectorAll('.cache-explorer__wire--tag')).toHaveLength(0);
    expect(svg.querySelectorAll('.cache-explorer__wire--strong')).toHaveLength(0);
    expect(svg.querySelectorAll('.cache-explorer__wire--offset')).toHaveLength(0);
    expect(root.querySelectorAll('.cache-explorer__comparator')).toHaveLength(0);
  });

  it('raise the offset wire to the word of the way that holds the line, or to every way for a typed address', () => {
    session?.free();
    session = SCENARIOS['frame-3b']?.() ?? Session.firstVisit();
    // The end of access #5, once the fill has given a way the line.
    const holders = (): number =>
      (current().state().cache.sets[2]?.ways ?? []).filter((way) => way.addressedWord !== undefined)
        .length;
    for (let pos = 0; holders() !== 1; pos++) {
      current().seek(pos);
      if (pos > current().timeline().stepCount) throw new Error('no way ever holds the line');
    }
    draw();
    flush();
    const arrows = (): number =>
      query('.cache-explorer__wires').querySelectorAll('.cache-explorer__wire-head--offset').length;
    expect(arrows()).toBe(1);

    ui.stripSource = 'draft';
    ui.draft.addrText = '0x2A4';
    draw();
    flush();
    expect(arrows()).toBe(2);
  });

  it('draw =? on the way of the typed address before anything is issued', () => {
    draw();
    flush();
    const [only] = root.querySelectorAll('.cache-explorer__comparator');
    expect(only?.textContent).toBe('=?');
    expect(only?.getAttribute('title')).toBe('Tag not compared yet');
  });

  it('are asked for once however many renders come before the frame, and drawn from the last', () => {
    draw();
    draw();
    draw();
    expect(frames).toHaveLength(1);
  });

  it('stay where they are while the next render waits for its frame, and change when it comes', () => {
    session?.free();
    session = SCENARIOS['frame-3b']?.() ?? Session.firstVisit();
    draw();
    flush();
    const before = query('.cache-explorer__wires').innerHTML;
    expect(before).not.toBe('');
    current().seek(0);
    draw();
    expect(query('.cache-explorer__wires').innerHTML).toBe(before);
    flush();
    expect(query('.cache-explorer__wires').innerHTML).not.toBe('');
  });

  it('are taken away, and their frame cancelled, when nothing decodes', () => {
    draw();
    ui.draft.addrText = 'nope';
    draw();
    flush();
    expect(query('.cache-explorer__wires').childElementCount).toBe(0);
    expect(root.querySelectorAll('.cache-explorer__comparator')).toHaveLength(0);
  });

  it('are drawn again when the box changes size', () => {
    draw();
    flush();
    expect(frames).toHaveLength(0);
    const svg = query('.cache-explorer__wires');
    svg.replaceChildren();
    const watching = watchers.filter((watcher) =>
      watcher.targets.includes(query('.cache-explorer__box--cache')),
    );
    expect(watching).toHaveLength(1);
    watching[0]?.callback();
    flush();
    expect(svg.childElementCount).toBeGreaterThan(0);
  });

  it('are not drawn for a model the box cannot measure, and leave nothing behind', () => {
    draw();
    query("[data-rowh='2']").removeAttribute('data-rowh');
    flush();
    expect(query('.cache-explorer__wires').childElementCount).toBe(0);
  });

  it('stop watching the box, and ask for no more frames, once the tool lets go of the root', () => {
    draw();
    detach(root);
    expect(watchers.every((watcher) => watcher.stopped)).toBe(true);
    flush();
    expect(frames).toHaveLength(0);
  });

  describe('and the table’s ways', () => {
    /** What the cache box measures now; the test moves it. */
    let width = 0;

    beforeEach(() => {
      width = 0;
      vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(function (
        this: HTMLElement,
      ) {
        return this.classList.contains('cache-explorer__box--cache') ? width : 0;
      });
    });

    /** Opens a scenario. */
    function scenario(name: string): void {
      session?.free();
      session = SCENARIOS[name]?.() ?? Session.firstVisit();
    }

    /** The box changes size to `next`, and the page gives the frames that asks for. */
    function resize(next: number): void {
      width = next;
      for (const watcher of watchers) watcher.callback();
      flush();
    }

    const wrapped = (): boolean =>
      query('table[data-cachetable]').classList.contains('cache-explorer__cache--wrapped');

    it('are side by side when they fit, and wrapped, with their comparators in them, when they do not', () => {
      scenario('frame-3b');
      width = 913;
      draw();
      expect(wrapped()).toBe(false);
      expect(root.querySelectorAll('.cache-explorer__compare')).toHaveLength(0);
      width = 500;
      draw();
      expect(wrapped()).toBe(true);
      expect(root.querySelectorAll('.cache-explorer__compare')).toHaveLength(2);
      flush();
      expect(root.querySelectorAll('.cache-explorer__comparator')).toHaveLength(0);
    });

    it('wrap in a 4-way cache at the widest the tool is drawn, though the core does not say so', () => {
      scenario('four-way');
      width = 913;
      draw();
      expect(current().state().cache.wrap).toBe(false);
      expect(wrapped()).toBe(true);
    });

    it('are taken to fit in a box that has no width yet', () => {
      scenario('four-way');
      width = 0;
      draw();
      expect(wrapped()).toBe(false);
    });

    it('go back to one row only when there is room to spare', () => {
      scenario('frame-3b');
      // Two ways of four words need 654px.
      width = 660;
      draw();
      expect(wrapped()).toBe(false);
      width = 650;
      draw();
      expect(wrapped()).toBe(true);
      width = 660;
      draw();
      expect(wrapped()).toBe(true);
      width = 678;
      draw();
      expect(wrapped()).toBe(false);
    });

    it('are drawn the other way when the box is resized across the line, without playing the step again', () => {
      scenario('frame-3b');
      width = 913;
      ui.lastPos = current().position().pos - 1;
      draw();
      ui.lastPos = current().position().pos;
      expect(wrapped()).toBe(false);
      const plain = query('table[data-cachetable]');
      resize(500);
      expect(wrapped()).toBe(true);
      expect(plain.isConnected).toBe(false);
      expect(root.querySelectorAll('[class*="cache-explorer__pulse-"]')).toHaveLength(0);
      resize(913);
      expect(wrapped()).toBe(false);
    });

    it('keep the focus on the way that had it, when the table is drawn the other way', () => {
      scenario('frame-3b');
      width = 913;
      draw();
      const cell = query("[data-action='edit-way'][data-set='3'][data-way='1']");
      cell.focus();
      expect(document.activeElement).toBe(cell);
      resize(500);
      expect(wrapped()).toBe(true);
      const after = query("[data-action='edit-way'][data-set='3'][data-way='1']");
      expect(after).not.toBe(cell);
      expect(document.activeElement).toBe(after);
    });

    it('are left alone when a resize does not change how they fit', () => {
      scenario('frame-3b');
      width = 913;
      draw();
      flush();
      const table = query('table[data-cachetable]');
      resize(900);
      expect(query('table[data-cachetable]')).toBe(table);
    });

    it('are not drawn again for a resize once the tool has let go of the root', () => {
      scenario('frame-3b');
      width = 913;
      draw();
      flush();
      const table = query('table[data-cachetable]');
      width = 500;
      for (const watcher of watchers) watcher.callback();
      detach(root);
      flush();
      expect(query('table[data-cachetable]')).toBe(table);
    });
  });
});
