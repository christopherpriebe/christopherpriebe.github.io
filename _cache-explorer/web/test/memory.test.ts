import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { SCENARIOS } from '../dev/sessions';
import { buildSnapshot } from '../src/controller';
import { h } from '../src/dom';
import { MEMORY_ROW_PX } from '../src/metrics';
import { render } from '../src/render';
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

function open(next: Session): Session {
  session?.free();
  session = next;
  return next;
}

function draw(): void {
  const { snapshot, log } = buildSnapshot(current(), ui);
  render(root, snapshot, log, ui);
}

const all = (selector: string): HTMLElement[] => [...root.querySelectorAll<HTMLElement>(selector)];

const query = (selector: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`nothing matches ${selector}`);
  return found;
};

const scenario = (name: string): Session => {
  const build = SCENARIOS[name];
  if (!build) throw new Error(`no scenario ${name}`);
  return open(build());
};

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

describe('the memory table', () => {
  it('says when nothing has been touched, under its heads', () => {
    draw();
    expect(query('.cache-explorer__empty').textContent).toBe(
      'Nothing touched yet. A line appears here the first time the cache reads or writes it.',
    );
    expect(all('.cache-explorer__mem-row')).toHaveLength(0);
    expect(all('.cache-explorer__mem-head').map((head) => head.textContent)).toEqual([
      'Line',
      'tag',
      'set',
      '+0',
      '+4',
      '+8',
      '+C',
    ]);
  });

  it('captions itself with the core’s count, and names its jump box', () => {
    scenario('frame-3a');
    draw();
    const caption = current().state().memory.caption;
    expect(query('.cache-explorer__box--memory .cache-explorer__box-head span').textContent).toBe(
      `Main memory · ${caption}`,
    );
    const jump = query('input[data-action="jump"]');
    expect(jump.getAttribute('aria-label')).toBe('Jump to address');
    expect(jump.getAttribute('placeholder')).toBe('0x…');
  });

  it('has a row for each line, with the line, tag and set the core gives and a cell for each word', () => {
    scenario('frame-3a');
    draw();
    const rows = current().memoryLines(0, 10);
    const drawn = all('.cache-explorer__mem-row');
    expect(drawn).toHaveLength(rows.length);
    rows.forEach((row, i) => {
      const cells = [...(drawn[i]?.children ?? [])].map((cell) => cell.textContent);
      expect(cells).toEqual([
        row.lineText,
        row.tagText,
        String(row.set),
        ...row.words.map((w) => w.text),
      ]);
      expect(drawn[i]?.querySelector('th')?.getAttribute('scope')).toBe('row');
    });
  });

  it('marks the stale word with a bar and a badge, and says why in its title', () => {
    scenario('frame-3b');
    draw();
    const stale = current()
      .memoryLines(0, 10)
      .flatMap((row) => row.words)
      .filter((word) => word.stale);
    expect(stale).toHaveLength(1);
    const cells = all('.cache-explorer__mem-word').filter((cell) =>
      cell.querySelector('.cache-explorer__stale-bar'),
    );
    expect(cells).toHaveLength(1);
    expect(cells[0]?.querySelector('.cache-explorer__badge')?.textContent).toBe('stale');
    expect(cells[0]?.getAttribute('title')).toBe(stale[0]?.stale?.text);
    expect(cells[0]?.textContent).toBe(`stale${stale[0]?.text ?? ''}`);
  });

  it('describes a word that is not stale as the core does, and can be reached by the keyboard', () => {
    scenario('frame-3a');
    draw();
    const word = current().memoryLines(0, 1)[0]?.words[1];
    const cell = query(`[data-action="edit-memory"][data-addr="${String(word?.addr)}"]`);
    expect(cell.getAttribute('title')).toBe(word?.description);
    expect(cell.getAttribute('tabindex')).toBe('0');
    expect(cell.getAttribute('aria-label')).toBe(`${word?.description ?? ''} Press Enter to edit.`);
    expect(cell.querySelector('.cache-explorer__stale-bar')).toBeNull();
  });

  it('puts the row of the line on the bus on the empty ground', () => {
    scenario('frame-3b');
    draw();
    const onBus = current()
      .memoryLines(0, 10)
      .filter((row) => row.onBus)
      .map((row) => row.lineText);
    expect(onBus).toEqual(['0x1A0']);
    expect(all('.cache-explorer__mem-line--bus').map((cell) => cell.textContent)).toEqual(onBus);
  });

  it('puts the ground on a word the entry changed', () => {
    scenario('frame-3b');
    draw();
    const changed = all('.cache-explorer__mem-word.cache-explorer__changed');
    expect(changed.map((cell) => cell.textContent)).toEqual(['7']);
  });

  it('marks a watched line, which the run has not touched, with a title that says why', () => {
    scenario('frame-3a');
    current().setMemoryWatch([0x0c0]);
    draw();
    const watched = all('.cache-explorer__mem-line--watched');
    expect(watched.map((cell) => cell.textContent)).toEqual(['0x0C0']);
    expect(watched[0]?.getAttribute('title')).toBe('Watched: you asked to see this line.');
    expect(all('.cache-explorer__mem-row')).toHaveLength(3);
    expect(
      query('.cache-explorer__box--memory .cache-explorer__box-head span').textContent,
    ).toContain('2 lines touched');
  });
});

describe('the memory window', () => {
  const longRun = (): Session => open(SCENARIOS['long-memory']?.() ?? Session.firstVisit());

  it('builds only the rows the core hands over, and stands in for the rest with blank rows', () => {
    longRun();
    draw();
    const total = current().state().memory.lineCount;
    const built = all('.cache-explorer__mem-row');
    expect(total).toBe(40);
    expect(built.length).toBeLessThan(total);
    const [spacer] = all('.cache-explorer__mem-spacer');
    expect(spacer?.getAttribute('style')).toBe(
      `height: ${String((total - built.length) * MEMORY_ROW_PX)}px`,
    );
    expect(all('.cache-explorer__mem-spacer')).toHaveLength(1);
    expect(spacer?.getAttribute('aria-hidden')).toBe('true');
  });

  it('puts a blank row above too when the window starts down the list, and numbers the rows from there', () => {
    longRun();
    ui.memoryFirst = 10;
    draw();
    const total = current().state().memory.lineCount;
    const built = all('.cache-explorer__mem-row');
    const spacers = all('.cache-explorer__mem-spacer');
    expect(spacers).toHaveLength(2);
    expect(spacers[0]?.getAttribute('style')).toBe(`height: ${String(10 * MEMORY_ROW_PX)}px`);
    expect(spacers[1]?.getAttribute('style')).toBe(
      `height: ${String((total - 10 - built.length) * MEMORY_ROW_PX)}px`,
    );
    expect(built[0]?.dataset.row).toBe('10');
    expect(built[0]?.dataset.line).toBe(String(10 * 16));
  });

  it('draws no blank row when every row is built', () => {
    scenario('frame-3a');
    draw();
    expect(all('.cache-explorer__mem-spacer')).toHaveLength(0);
  });

  it('keeps the scroller, and what it holds is rebuilt', () => {
    longRun();
    draw();
    const scroller = query('.cache-explorer__scroller');
    const row = query('.cache-explorer__mem-row');
    draw();
    expect(query('.cache-explorer__scroller')).toBe(scroller);
    expect(query('.cache-explorer__mem-row')).not.toBe(row);
  });

  it('moves the window to the row a step puts on the bus, and scrolls it into view', () => {
    longRun();
    const total = current().state().memory.lineCount;
    let pos = 0;
    while (current().state().memory.busRow !== 30) {
      current().seek(++pos);
      if (pos > current().timeline().stepCount) throw new Error('the bus never reaches row 30');
    }
    // jsdom does no layout: a row has the place a 29px row under a 23px head would have.
    vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get').mockImplementation(function (
      this: HTMLElement,
    ) {
      return this.dataset.row === undefined ? 0 : 23 + Number(this.dataset.row) * MEMORY_ROW_PX;
    });
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (
      this: HTMLElement,
    ) {
      return this.classList.contains('cache-explorer__mem-heads') ? 23 : 28;
    });
    ui.lastPos = current().position().pos;
    draw();
    const scroller = query('.cache-explorer__scroller');
    Object.defineProperty(scroller, 'clientHeight', { value: 200, configurable: true });
    scroller.scrollTop = 0;

    ui.lastPos = pos - 1;
    draw();
    expect(all('.cache-explorer__mem-row').some((row) => row.dataset.row === '30')).toBe(true);
    expect(ui.memoryFirst).toBeGreaterThan(0);
    expect(ui.memoryFirst + 24).toBeLessThanOrEqual(total + 24);
    // The row's bottom edge rests on the bottom of the view.
    expect(scroller.scrollTop).toBe(23 + 30 * MEMORY_ROW_PX + 28 - 200);

    // A render the cursor did not cause leaves the scroll where the student put it.
    scroller.scrollTop = 5;
    ui.lastPos = pos;
    draw();
    expect(scroller.scrollTop).toBe(5);
  });

  it('brings a row into view when asked once, and not after', () => {
    longRun();
    vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get').mockImplementation(function (
      this: HTMLElement,
    ) {
      return this.dataset.row === undefined ? 0 : 23 + Number(this.dataset.row) * MEMORY_ROW_PX;
    });
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(28);
    ui.lastPos = current().position().pos;
    draw();
    const scroller = query('.cache-explorer__scroller');
    Object.defineProperty(scroller, 'clientHeight', { value: 200, configurable: true });
    scroller.scrollTop = 0;
    ui.memoryFirst = 16;
    ui.memoryReveal = 20;
    draw();
    expect(scroller.scrollTop).toBe(23 + 20 * MEMORY_ROW_PX + 28 - 200);
    scroller.scrollTop = 7;
    ui.memoryReveal = null;
    draw();
    expect(scroller.scrollTop).toBe(7);
  });
});
