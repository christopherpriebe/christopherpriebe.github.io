import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { SCENARIOS } from '../dev/sessions';
import { buildSnapshot, start, type Controller } from '../src/controller';
import { h } from '../src/dom';
import { MEMORY_ROW_PX } from '../src/metrics';
import { initialUiState } from '../src/ui-state';
import { Session } from '../src/wasm';
import { stubLayout, type Widths } from './support/layout';
import { loadWasm } from './support/wasm';

let root: HTMLElement;
/** The session the test owns, until it hands it to a controller. */
let session: Session | null;
let controller: Controller | null;
/** The session a controller owns, which a test may still use to move the run, but never free. */
let lent: Session;
let widths: Widths;

function open(next: Session): Session {
  session?.free();
  session = next;
  return next;
}

/** Starts a controller on the session the test holds; the controller owns it from here. */
function begin(): Controller {
  if (!session) throw new Error('no session');
  lent = session;
  session = null;
  controller = start(root, lent, 'desktop');
  return controller;
}

const scenario = (name: string): Session => {
  const build = SCENARIOS[name];
  if (!build) throw new Error(`no scenario ${name}`);
  return open(build());
};

const all = (selector: string): HTMLElement[] => [...root.querySelectorAll<HTMLElement>(selector)];

const query = (selector: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`nothing matches ${selector}`);
  return found;
};

const jumpBox = (): HTMLInputElement => {
  const found = query('input[data-action="jump"]');
  if (!(found instanceof HTMLInputElement)) throw new Error('the jump box is not an input');
  return found;
};

/** Types `text` into the jump box and leaves it, as a student does. */
function jump(text: string): void {
  const box = jumpBox();
  box.value = text;
  box.dispatchEvent(new Event('input', { bubbles: true }));
  box.dispatchEvent(new Event('change', { bubbles: true }));
}

/** Scrolls the memory table to `top`, which jsdom will not do itself, and tells the page. */
function scrollTo(top: number): void {
  const scroller = query('.cache-explorer__scroller');
  Object.defineProperty(scroller, 'scrollTop', { value: top, configurable: true });
  scroller.dispatchEvent(new Event('scroll'));
}

const firstBuilt = (): string | undefined => query('.cache-explorer__mem-row').dataset.row;

beforeAll(loadWasm);

beforeEach(() => {
  widths = stubLayout();
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
  session = Session.firstVisit();
  controller = null;
});

afterEach(() => {
  controller?.dispose();
  const owned = session;
  session = null;
  owned?.free();
});

describe('the jump box', () => {
  it('watches a line the run has not touched, gives it a row, and takes the window to it', () => {
    scenario('long-memory');
    begin();
    jump('0x300');
    expect(lent.state().memory.lineCount).toBe(41);
    expect(all('.cache-explorer__mem-line--watched').map((cell) => cell.textContent)).toEqual([
      '0x300',
    ]);
    expect(firstBuilt()).toBe('36');
    expect(
      root.querySelector('.cache-explorer__box--memory .cache-explorer__box-head span')
        ?.textContent,
    ).toContain('40 lines touched');
  });

  it('takes the row to show from the core, and looks for it in none of the rows', () => {
    scenario('long-memory');
    begin();
    const asked: number[] = [];
    const ask = lent.memoryLines.bind(lent);
    lent.memoryLines = (first, count) => {
      asked.push(count);
      return ask(first, count);
    };
    jump('0x1A0');
    expect(lent.state().memory.watchRow).toBe(26);
    // Only the window the redraw builds is asked for; no row is fetched to find another.
    expect(asked.length).toBeGreaterThan(0);
    expect(asked.every((count) => count > 1)).toBe(true);
    // A line the run has not touched has its row too, among those the core lists.
    jump('0x300');
    expect(lent.state().memory.watchRow).toBe(40);
    jump('');
    expect(lent.state().memory.watchRow).toBeUndefined();
  });

  it('marks a line the run has touched, and takes the window to it', () => {
    scenario('long-memory');
    begin();
    jump('0x1A0');
    const watched = all('.cache-explorer__mem-line--watched');
    expect(watched.map((cell) => cell.textContent)).toEqual(['0x1A0']);
    expect(watched[0]?.closest('tr')?.dataset.row).toBe('26');
    expect(firstBuilt()).toBe('22');
  });

  it('reads any address in the line as the line', () => {
    scenario('long-memory');
    begin();
    jump('0x1A7');
    expect(all('.cache-explorer__mem-line--watched').map((cell) => cell.textContent)).toEqual([
      '0x1A0',
    ]);
  });

  it('keeps one line at a time: the next jump replaces the last', () => {
    scenario('long-memory');
    begin();
    jump('0x1A0');
    jump('0x300');
    expect(all('.cache-explorer__mem-line--watched').map((cell) => cell.textContent)).toEqual([
      '0x300',
    ]);
  });

  it('stops watching when it is emptied', () => {
    scenario('long-memory');
    begin();
    jump('0x300');
    jump('');
    expect(all('.cache-explorer__mem-line--watched')).toHaveLength(0);
    expect(lent.state().memory.lineCount).toBe(40);
  });

  it('says why the text is not an address, beside the box, and leaves the watch as it was', () => {
    scenario('long-memory');
    begin();
    jump('0x300');
    expect(root.querySelector('#cache-explorer-jump-note')).toBeNull();
    jump('banana');
    const reason = lent.readAddress('banana');
    expect(reason.status).toBe('invalid');
    const message = reason.status === 'invalid' ? reason.message : '';
    expect(message).not.toBe('');
    expect(jumpBox().getAttribute('aria-invalid')).toBe('true');
    // The message is text in the memory box's head, and the box points to it.
    const note = query('#cache-explorer-jump-note');
    expect(note.textContent).toBe(message);
    expect(note.closest('.cache-explorer__box--memory')).not.toBeNull();
    expect(jumpBox().getAttribute('aria-describedby')).toBe(note.id);
    expect(all('.cache-explorer__mem-line--watched').map((cell) => cell.textContent)).toEqual([
      '0x300',
    ]);
    jump('0x310');
    expect(jumpBox().hasAttribute('aria-invalid')).toBe(false);
    expect(jumpBox().hasAttribute('aria-describedby')).toBe(false);
    expect(root.querySelector('#cache-explorer-jump-note')).toBeNull();
  });

  it('is the same box every render, and keeps what is typed in it until it is left', () => {
    begin();
    const box = jumpBox();
    box.value = '0x1';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    controller?.refresh();
    expect(jumpBox()).toBe(box);
    expect(box.value).toBe('0x1');
  });

  it('is not a redraw while it is typed in', () => {
    begin();
    const watch = new MutationObserver(() => undefined);
    watch.observe(root, { subtree: true, childList: true, characterData: true });
    const box = jumpBox();
    box.value = '0x1';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    expect(watch.takeRecords()).toEqual([]);
    watch.disconnect();
  });
});

describe('scrolling the memory table', () => {
  it('builds the rows around the ones in view, a few above them', () => {
    scenario('long-memory');
    begin();
    expect(firstBuilt()).toBe('0');
    scrollTo(20 * MEMORY_ROW_PX);
    expect(firstBuilt()).toBe('16');
    scrollTo(0);
    expect(firstBuilt()).toBe('0');
  });

  it('does not draw again while the same rows are built', () => {
    scenario('long-memory');
    begin();
    scrollTo(20 * MEMORY_ROW_PX);
    const watch = new MutationObserver(() => undefined);
    watch.observe(root, { subtree: true, childList: true });
    scrollTo(20 * MEMORY_ROW_PX + 10);
    expect(watch.takeRecords()).toEqual([]);
    watch.disconnect();
  });

  it('ignores the scrolling of anything else in the tool', () => {
    scenario('long-memory');
    begin();
    const before = query('.cache-explorer__scroller');
    const other = h('div', { 'data-scroll': 'log' });
    root.append(other);
    Object.defineProperty(other, 'scrollTop', { value: 500, configurable: true });
    other.dispatchEvent(new Event('scroll'));
    expect(query('.cache-explorer__scroller')).toBe(before);
    expect(firstBuilt()).toBe('0');
  });
});

describe('the window of rows', () => {
  it('moves to the line a step puts on the bus, when that row is not built', () => {
    const run = scenario('long-memory');
    const ui = initialUiState('desktop');
    let pos = 0;
    while (run.state().memory.busRow !== 30) run.seek(++pos);
    ui.lastPos = pos - 1;
    const { snapshot } = buildSnapshot(run, ui);
    expect(ui.memoryFirst).toBe(26);
    expect(snapshot.memoryRows.some((row) => row.onBus)).toBe(true);
  });

  it('stays where the student put it when the cursor has not moved', () => {
    const run = scenario('long-memory');
    const ui = initialUiState('desktop');
    ui.memoryFirst = 3;
    let pos = 0;
    while (run.state().memory.busRow !== 30) run.seek(++pos);
    ui.lastPos = pos;
    buildSnapshot(run, ui);
    expect(ui.memoryFirst).toBe(3);
  });

  it('does not start past the last row, which an Undo may have moved back', () => {
    const run = scenario('long-memory');
    const ui = initialUiState('desktop');
    ui.memoryFirst = 500;
    ui.lastPos = run.position().pos;
    buildSnapshot(run, ui);
    expect(ui.memoryFirst).toBe(39);
    const empty = Session.firstVisit();
    ui.memoryFirst = 500;
    buildSnapshot(empty, ui);
    expect(ui.memoryFirst).toBe(0);
    empty.free();
  });
});

describe('the controller and the machine', () => {
  it('opens a session part way through its run without playing the last step', () => {
    scenario('frame-3b');
    begin();
    expect(root.querySelectorAll('[class*="cache-explorer__pulse-"]')).toHaveLength(0);
    expect(root.querySelector('.cache-explorer__changed')).not.toBeNull();
  });

  it('plays the step that follows', () => {
    scenario('frame-3b');
    const started = begin();
    lent.stepEvent({ source: '', format: 'asm' });
    started.refresh();
    expect(lent.state().event?.changed.length).toBeGreaterThan(0);
    expect(root.querySelectorAll('[class*="cache-explorer__pulse-"]').length).toBeGreaterThan(0);
    // A redraw the step did not cause plays nothing.
    started.refresh();
    expect(root.querySelectorAll('[class*="cache-explorer__pulse-"]')).toHaveLength(0);
  });

  it('stops watching the cache box when it is disposed', () => {
    const started = begin();
    const box = query('.cache-explorer__box--cache');
    expect(widths.observers(box)).toBe(1);
    started.dispose();
    expect(widths.observers(box)).toBe(0);
  });
});
