import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { SCENARIOS } from '../dev/sessions';
import { start, type Controller } from '../src/controller';
import { copy } from '../src/copy';
import { h } from '../src/dom';
import { LOG_MARGIN } from '../src/log-actions';
import { LOG_ROW_PX } from '../src/metrics';
import { Session } from '../src/wasm';
import { stubLayout } from './support/layout';
import { loadWasm } from './support/wasm';

let root: HTMLElement;
/** The session the controller owns: a test may still move the run through it, but never frees it. */
let lent: Session;
let controller: Controller;

const all = (selector: string): HTMLElement[] => [...root.querySelectorAll<HTMLElement>(selector)];

const query = (selector: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`nothing matches ${selector}`);
  return found;
};

const scrubber = (): HTMLInputElement => {
  const found = query('input[type="range"]');
  if (!(found instanceof HTMLInputElement)) throw new Error('the scrubber is not an input');
  return found;
};

function open(scenario = 'frame-3a'): void {
  const build = SCENARIOS[scenario];
  if (!build) throw new Error(`no scenario ${scenario}`);
  lent = build();
  controller = start(root, lent, 'desktop');
}

/** The animation frames a test has been asked for, run when the test says. */
let frames: FrameRequestCallback[];
/** The id the latest request for a frame was given. */
let lastFrame: number;

function runFrames(): void {
  const waiting = frames;
  frames = [];
  for (const frame of waiting) frame(0);
}

beforeAll(loadWasm);

beforeEach(() => {
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
  frames = [];
  lastFrame = 0;
  vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => {
    frames.push(frame);
    lastFrame += 1;
    return lastFrame;
  });
  vi.stubGlobal('cancelAnimationFrame', () => undefined);
});

afterEach(() => {
  controller.dispose();
});

describe('the entries', () => {
  it('draws an entry for each access, with its number, its instruction and what it did', () => {
    open();
    const heads = all('.cache-explorer__entry-head');
    expect(heads.map((head) => head.textContent)).toEqual([
      '#1  ld r1, [0x0A4]miss',
      '#2  ld r2, [0x0A8]hit',
      '#3  ld r3, [0x1A4]miss · evict',
    ]);
  });

  it('gives each square its shape: filled for a hit, open for a miss', () => {
    open();
    const shapes = all('.cache-explorer__entry-head .cache-explorer__marker').map((marker) =>
      marker.classList.contains('cache-explorer__marker--hit') ? 'hit' : 'miss',
    );
    expect(shapes).toEqual(['miss', 'hit', 'miss']);
  });

  it('writes what each entry changed on the changed ground, as the core summarises it', () => {
    open();
    const lines = all('.cache-explorer__changed-line').map((line) => line.textContent);
    expect(lines).toContain('changedset 2 way 0 valid, tag, data · r1 · misses');
    expect(lines).toContain('changedr2 · hits');
    expect(query('.cache-explorer__changed-chip').textContent).toBe(copy.log.changed);
  });

  it('shows the events of the current entry only, the one at the cursor marked', () => {
    open();
    const events = all('.cache-explorer__event');
    expect(
      events.map((row) => row.querySelector('.cache-explorer__event-number')?.textContent),
    ).toEqual(['3.1', '3.2', '3.3', '3.4', '3.5', '3.6']);
    expect(all('.cache-explorer__event--current')).toHaveLength(1);
    expect(query('.cache-explorer__event--current').dataset.pos).toBe(String(lent.position().pos));
  });

  it('fades the entries the cursor has not reached, and gives them no changed line', () => {
    open();
    lent.seek(Number(all('.cache-explorer__entry-head')[0]?.dataset.pos));
    controller.refresh();
    expect(all('.cache-explorer__entry').map((entry) => entry.dataset.tense)).toEqual([
      'past',
      'current',
      'future',
      'future',
    ]);
    expect(
      all('.cache-explorer__entry--future').map((entry) =>
        entry.querySelector('.cache-explorer__changed-line'),
      ),
    ).toEqual([null, null]);
    expect(all('.cache-explorer__changed-line')).toHaveLength(1);
  });

  it('points each line at the group that made it happen, in that group’s own name', () => {
    open();
    const links = all('.cache-explorer__why');
    expect(links.length).toBeGreaterThan(0);
    for (const link of links) {
      const cause = link.dataset.group;
      expect(cause).toBeDefined();
      expect(link.textContent).toBe(
        `${copy.log.why} ${copy.config.groups[cause as keyof typeof copy.config.groups]}`,
      );
    }
    expect(links.map((link) => link.textContent)).toContain('why: Associativity');
  });

  it('sets the group flashing when a why link is pressed', () => {
    open();
    const link = all('.cache-explorer__why').find((each) => each.dataset.group === 'associativity');
    link?.click();
    expect(root.querySelector('.cache-explorer__group--why')).not.toBeNull();
  });

  it('draws a program load as a divider carrying the core’s words and no number', () => {
    open();
    const divider = query('.cache-explorer__entry--divider');
    expect(divider.textContent).toBe('Loaded a program of 5 instructions');
    expect(divider.querySelector('.cache-explorer__marker')).toBeNull();
    expect(divider.querySelector('.cache-explorer__entry-outcome')).toBeNull();
  });

  it('draws an error entry’s square as its own shape, with the word for it beside', () => {
    open('first-visit');
    lent.applyOp({ op: 'program', source: 'add r2, r0, #4096\nld r1, [r2]', format: 'asm' });
    lent.applyOp({ op: 'run', n: 2 });
    controller.refresh();
    const head = all('.cache-explorer__entry-head').at(-1);
    expect(head?.querySelector('.cache-explorer__marker--error')).not.toBeNull();
    expect(head?.textContent).toContain('error');
  });

  it('gives an empty log the design’s words, with the first line reworded', () => {
    open('first-visit');
    expect(query('.cache-explorer__log-lead').textContent).toBe('Type an address or press Space.');
    expect(query('.cache-explorer__log-note').textContent).toBe(copy.log.empty.text);
    expect(all('.cache-explorer__entry')).toHaveLength(0);
    expect(query('.cache-explorer__log-head').textContent).toContain('no steps yet');
  });

  it('moves the cursor to the entry’s last event when its header is pressed', () => {
    open();
    const first = all('.cache-explorer__entry-head')[0];
    first?.click();
    expect(lent.position().pos).toBe(Number(first?.dataset.pos));
    expect(query('.cache-explorer__entry--current').textContent).toContain('#1');
  });

  it('moves the cursor to an event row’s own position when the row is pressed', () => {
    open();
    all('.cache-explorer__event')[1]?.click();
    expect(
      query('.cache-explorer__event--current').querySelector('.cache-explorer__event-number')
        ?.textContent,
    ).toBe('3.2');
  });
});

describe('the ticks and the scrubber', () => {
  it('draws one tick for each step, tall on an entry’s first step', () => {
    open();
    const ticks = all('.cache-explorer__ticks > .cache-explorer__tick');
    expect(ticks).toHaveLength(lent.timeline().stepCount);
    const tall = ticks.filter((tick) => tick.classList.contains('cache-explorer__tick--tall'));
    expect(tall).toHaveLength(lent.timeline().entryCount - 1);
  });

  it('draws a tick for a step the cursor has reached filled, and an entry that missed open', () => {
    open();
    const ticks = all('.cache-explorer__ticks > .cache-explorer__tick');
    const looks = ticks.map((tick) =>
      [...tick.classList].find((name) => /--(on|open|ahead|off)$/.test(name)),
    );
    expect(looks[0]).toBe('cache-explorer__tick--open');
    expect(looks[1]).toBe('cache-explorer__tick--on');
    lent.seekStep(1);
    controller.refresh();
    const after = all('.cache-explorer__ticks > .cache-explorer__tick').map((tick) =>
      [...tick.classList].find((name) => /--(on|open|ahead|off)$/.test(name)),
    );
    expect(after[0]).toBe('cache-explorer__tick--open');
    expect(after.slice(1).every((look) => /--(ahead|off)$/.test(look ?? ''))).toBe(true);
  });

  it('moves the cursor to a step when its tick is pressed', () => {
    open();
    const ticks = all('.cache-explorer__ticks > .cache-explorer__tick');
    ticks[3]?.click();
    expect(lent.timeline().step).toBe(4);
  });

  it('is a range over the steps, named Timeline, that reads out the header', () => {
    open();
    const range = scrubber();
    expect(range.min).toBe('0');
    expect(range.max).toBe(String(lent.timeline().stepCount));
    expect(range.value).toBe(String(lent.timeline().step));
    expect(range.getAttribute('aria-label')).toBe('Timeline');
    expect(range.getAttribute('aria-valuetext')).toBe(lent.timeline().headerText);
    expect(query('.cache-explorer__log-head').textContent).toContain('step 14 of 14');
  });

  it('seeks once a frame, to the latest step asked for', () => {
    open();
    const seek = vi.spyOn(lent, 'seekStep');
    const range = scrubber();
    for (const value of ['3', '5', '7']) {
      range.value = value;
      range.dispatchEvent(new Event('input', { bubbles: true }));
    }
    expect(seek).not.toHaveBeenCalled();
    runFrames();
    expect(seek).toHaveBeenCalledTimes(1);
    expect(seek).toHaveBeenCalledWith(7);
    expect(lent.timeline().step).toBe(7);
    expect(scrubber().value).toBe('7');
    expect(scrubber()).toBe(range);
  });

  it('redraws while the scrubber is held down, not once it is let go', () => {
    open();
    const range = scrubber();
    range.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
    range.value = '5';
    range.dispatchEvent(new Event('input', { bubbles: true }));
    runFrames();
    expect(query('.cache-explorer__log-head').textContent).toContain('step 5 of 14');
    expect(query('.cache-explorer__entry--current').dataset.entry).toBe(
      String(lent.timeline().current?.index),
    );
    expect(scrubber()).toBe(range);
  });

  it('puts the thumb where the core put the cursor, even with the scrubber focused', () => {
    open();
    const range = scrubber();
    range.focus();
    const seek = lent.seekStep.bind(lent);
    vi.spyOn(lent, 'seekStep').mockImplementation(() => {
      seek(4);
    });
    range.value = '9';
    range.dispatchEvent(new Event('input', { bubbles: true }));
    runFrames();
    expect(document.activeElement).toBe(range);
    expect(scrubber().value).toBe('4');
  });

  it('gives the ticks’ box a class of its own and sets it 9px under the header', () => {
    open();
    const box = query('.cache-explorer__ticks').parentElement;
    expect(box?.className).toBe('cache-explorer__scrub');
    expect(box?.previousElementSibling?.className).toBe('cache-explorer__log-head');
    expect(query('.cache-explorer__log').querySelector('.cache-explorer__strip')).toBeNull();
  });

  it('stops the waiting seek when the controller is disposed', () => {
    open();
    const range = scrubber();
    range.value = '2';
    range.dispatchEvent(new Event('input', { bubbles: true }));
    const cancel = vi.fn();
    vi.stubGlobal('cancelAnimationFrame', cancel);
    controller.dispose();
    expect(cancel).toHaveBeenCalledWith(lastFrame);
    open();
  });

  it('counts a later run’s steps from its own origin, past a program load that is no step', () => {
    open();
    lent.applyOp({
      op: 'config',
      config: { ...lent.state().config, cache: { ...lent.state().config.cache, associativity: 2 } },
    });
    lent.stepAccess({ source: 'ld r1, [0x0A4]', format: 'asm' });
    controller.refresh();
    expect(lent.timeline().origin).toBeGreaterThan(0);
    const range = scrubber();
    expect(range.max).toBe(String(lent.timeline().stepCount));
    range.value = '1';
    range.dispatchEvent(new Event('input', { bubbles: true }));
    runFrames();
    expect(lent.timeline().run).toBe(2);
    expect(lent.timeline().step).toBe(1);
    range.value = '0';
    range.dispatchEvent(new Event('input', { bubbles: true }));
    runFrames();
    expect(lent.timeline().cursor).toBe(lent.timeline().origin);
  });

  describe('in a run with more steps than fit', () => {
    let widths: ReturnType<typeof stubLayout>;

    beforeEach(() => {
      widths = stubLayout(1000);
    });

    it('draws buckets of steps in one SVG, and one tick a step while they fit', () => {
      open('long-log');
      const { stepCount } = lent.timeline();
      const timeline = query('.cache-explorer__timeline');
      widths.set(timeline, 1000);
      controller.refresh();
      const room = Math.floor(1002 / 4);
      expect(stepCount).toBeGreaterThan(room);
      const size = Math.ceil(stepCount / room);
      const marks = all('.cache-explorer__ticks--buckets rect.cache-explorer__tick');
      expect(all('.cache-explorer__ticks > .cache-explorer__tick')).toHaveLength(0);
      expect(marks).toHaveLength(Math.ceil(stepCount / size));

      widths.set(timeline, 4 * stepCount);
      controller.refresh();
      expect(all('.cache-explorer__ticks--buckets')).toHaveLength(0);
      expect(all('.cache-explorer__ticks > .cache-explorer__tick')).toHaveLength(stepCount);
    });

    it('fills a bucket when any of its steps is at or before the cursor, and takes it tall when one starts an entry', () => {
      open('long-log');
      const { stepCount } = lent.timeline();
      const room = Math.floor(1002 / 4);
      const size = Math.ceil(stepCount / room);
      lent.seekStep(3 * size + 1);
      controller.refresh();
      const marks = all('.cache-explorer__ticks--buckets rect.cache-explorer__tick');
      expect(
        marks.slice(0, 4).every((mark) => mark.classList.contains('cache-explorer__tick--on')),
      ).toBe(true);
      expect(marks[4]?.classList.contains('cache-explorer__tick--off')).toBe(true);
      const bits = lent.ticks();
      const tall = marks.map((mark) => mark.classList.contains('cache-explorer__tick--tall'));
      const expected = marks.map((_, bucket) =>
        bits.subarray(bucket * size, (bucket + 1) * size).some((byte) => (byte & 1) !== 0),
      );
      expect(tall).toEqual(expected);
    });

    it('moves the cursor to a bucket’s first step when it is pressed', () => {
      open('long-log');
      const { stepCount } = lent.timeline();
      const size = Math.ceil(stepCount / Math.floor(1002 / 4));
      all('.cache-explorer__tick-target')[5]?.dispatchEvent(
        new MouseEvent('click', { bubbles: true }),
      );
      expect(lent.timeline().step).toBe(5 * size + 1);
    });
  });
});

describe('the window of entries', () => {
  it('builds the entries around the one the cursor is in, and blank rows for the rest', () => {
    open('long-log');
    const { entryCount, current } = lent.timeline();
    const built = all('.cache-explorer__entry').length;
    expect(entryCount).toBe(90);
    expect(built).toBeLessThan(entryCount);
    const index = current?.index ?? -1;
    expect(query('.cache-explorer__entry--current').dataset.entry).toBe(String(index));
    const spacers = all('.cache-explorer__log-spacer');
    const first = Number(all('.cache-explorer__entry')[0]?.dataset.entry);
    expect(first).toBe(index - LOG_MARGIN);
    expect(spacers[0]?.style.height).toBe(`${String(first * LOG_ROW_PX)}px`);
    expect(spacers[0]?.getAttribute('aria-hidden')).toBe('true');
  });

  it('brings the window to an entry the cursor moves to', () => {
    open('long-log');
    lent.seek(3);
    controller.refresh();
    expect(query('.cache-explorer__entry--current').dataset.entry).toBe('0');
    expect(all('.cache-explorer__entry')[0]?.dataset.entry).toBe('0');
    expect(all('.cache-explorer__log-spacer')).toHaveLength(1);
  });

  it('builds the entries around the ones a scroll reaches', () => {
    open('long-log');
    const scroller = query('.cache-explorer__log-scroller');
    scroller.scrollTop = 40 * LOG_ROW_PX;
    scroller.dispatchEvent(new Event('scroll'));
    expect(all('.cache-explorer__entry')[0]?.dataset.entry).toBe(String(40 - LOG_MARGIN));
  });

  it('never starts past the last entry after the run has been shortened', () => {
    open('long-log');
    const scroller = query('.cache-explorer__log-scroller');
    scroller.scrollTop = 80 * LOG_ROW_PX;
    scroller.dispatchEvent(new Event('scroll'));
    expect(all('.cache-explorer__entry')[0]?.dataset.entry).toBe(String(80 - LOG_MARGIN));
    // A new run drops the undone ops after the cursor, and the entries with them.
    lent.seek(0);
    lent.applyOp({
      op: 'config',
      config: { ...lent.state().config, cache: { ...lent.state().config.cache, associativity: 2 } },
    });
    lent.stepAccess({ source: 'ld r1, [0x0A4]', format: 'asm' });
    controller.refresh();
    const { entryCount } = lent.timeline();
    expect(entryCount).toBeLessThan(80 - LOG_MARGIN);
    const built = all('.cache-explorer__entry');
    expect(built.length).toBeGreaterThan(0);
    expect(Number(built[0]?.dataset.entry)).toBeLessThanOrEqual(entryCount - 1);
    expect(query('.cache-explorer__entry--current')).toBeTruthy();
  });

  it('brings the current event into view when the cursor moves, and the page stays', () => {
    open();
    const scroller = query('.cache-explorer__log-scroller');
    Object.defineProperty(scroller, 'clientHeight', { configurable: true, value: 100 });
    const top = vi.spyOn(HTMLElement.prototype, 'offsetTop', 'get').mockImplementation(function (
      this: HTMLElement,
    ) {
      return this.classList.contains('cache-explorer__event--current') ? 500 : 0;
    });
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (
      this: HTMLElement,
    ) {
      return this.classList.contains('cache-explorer__event--current') ? 29 : 0;
    });
    const page = vi.fn();
    vi.stubGlobal('scrollTo', page);
    scroller.scrollTop = 0;
    lent.seek(lent.timeline().cursor - 2);
    controller.refresh();
    expect(top).toHaveBeenCalled();
    expect(scroller.scrollTop).toBe(500 + 29 - 100);
    expect(page).not.toHaveBeenCalled();
    expect(window.scrollY).toBe(0);
    vi.restoreAllMocks();
  });
});
