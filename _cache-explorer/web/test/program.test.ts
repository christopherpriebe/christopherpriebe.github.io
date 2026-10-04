import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { SCENARIOS } from '../dev/sessions';
import { start, type Controller } from '../src/controller';
import { copy } from '../src/copy';
import { h } from '../src/dom';
import { RUN_MS } from '../src/motion';
import { errorRange } from '../src/program-actions';
import { SAMPLE_PROGRAM } from '../src/ui-state';
import { Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

let root: HTMLElement;
let controller: Controller | null;
/** The session the controller owns: a test may read it, and never frees it. */
let lent: Session;

beforeAll(loadWasm);

beforeEach(() => {
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
  controller = null;
});

afterEach(() => {
  vi.useRealTimers();
  controller?.dispose();
  controller = null;
});

function open(scenario = 'first-visit'): void {
  const build = SCENARIOS[scenario];
  if (!build) throw new Error(`no scenario ${scenario}`);
  lent = build();
  controller = start(root, lent, 'desktop');
}

const query = (selector: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`nothing matches ${selector}`);
  return found;
};

const all = (selector: string): HTMLElement[] => [...root.querySelectorAll<HTMLElement>(selector)];

const click = (selector: string): void => {
  query(selector).click();
};

const box = (): HTMLTextAreaElement => {
  const found = query('textarea[data-action="program-text"]');
  if (!(found instanceof HTMLTextAreaElement)) throw new Error('the program box is not a textarea');
  return found;
};

/** Types `text` into the program box the way a keystroke does: the value changes, then `input`. */
function write(text: string): void {
  const field = box();
  field.value = text;
  field.dispatchEvent(new Event('input', { bubbles: true }));
}

function press(
  key: string,
  init: KeyboardEventInit = {},
  target: HTMLElement = query('.cache-explorer__plane'),
): Event {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

const summary = (): string => query('.cache-explorer__program-summary').textContent;
const dividers = (): string[] =>
  all('.cache-explorer__entry--divider').map((row) => row.textContent);
const errorLines = (): string[] =>
  all('.cache-explorer__program-error').map((row) => row.dataset.line ?? '');
const rowsOf = (): string[] => all('.cache-explorer__listing-text').map((row) => row.textContent);

describe('the program drawer', () => {
  it('holds the sample program, not loaded, with the core’s summary of it', () => {
    open();
    expect(query('.cache-explorer__program-title').textContent).toBe('Program');
    expect(summary()).toBe('5 instructions · next: ld r1, [0x0A4]');
    expect(rowsOf()).toEqual(SAMPLE_PROGRAM.split('\n'));
    expect(query('.cache-explorer__listing').getAttribute('role')).toBe('list');
    expect(all('.cache-explorer__listing-row--current')).toHaveLength(0);
    expect(lent.opCount()).toBe(0);
  });

  it('opens and closes on its line, with aria-expanded telling which', () => {
    open();
    const toggle = query('[data-action="program-toggle"]');
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    toggle.click();
    expect(query('[data-action="program-toggle"]').getAttribute('aria-expanded')).toBe('false');
    expect(root.querySelector('.cache-explorer__listing')).toBeNull();
    expect(summary()).toBe('5 instructions · next: ld r1, [0x0A4]');
    click('[data-action="program-toggle"]');
    expect(root.querySelector('.cache-explorer__listing')).not.toBeNull();
  });

  it('marks the current line of a loaded program, and moves the mark with the run', () => {
    open();
    press(' ');
    const current = (): string[] =>
      all('.cache-explorer__listing-row--current').map((row) => row.dataset.line ?? '');
    expect(current()).toEqual(['2']);
    expect(query('.cache-explorer__listing-row--current').textContent).toContain('▸');
    const before = current();
    // An instruction's line stays current until its last event retires it.
    for (let presses = 0; presses < 12 && current().join() === before.join(); presses += 1) {
      press(' ');
    }
    const line = lent.state().program?.currentLine;
    expect(line).toBeDefined();
    expect(current()).toEqual([String(line)]);
    expect(current()).not.toEqual(before);
  });

  it('gives the loaded program’s own summary once it is loaded', () => {
    open();
    lent.stepAccess({ source: SAMPLE_PROGRAM, format: 'asm' });
    controller?.refresh();
    expect(summary()).toBe(lent.state().program?.summary);
    expect(summary()).toContain('next: ld r2, [0x0A8]');
  });

  it('opens on the program a run already holds, so the next step does not replace it', () => {
    open('frame-3a');
    expect(rowsOf()).toEqual(SAMPLE_PROGRAM.split('\n'));
    expect(all('.cache-explorer__listing-row--current')).toHaveLength(1);
    const ops = lent.opCount();
    press(' ');
    expect(lent.opCount()).toBe(ops + 1);
    expect(dividers()).toHaveLength(1);
  });
});

describe('editing the text', () => {
  it('swaps the listing for a box holding the text, and back', () => {
    open();
    click('[data-action="program-edit"]');
    expect(box().value).toBe(SAMPLE_PROGRAM);
    expect(box().getAttribute('aria-label')).toBe('Program');
    expect(root.querySelector('.cache-explorer__listing')).toBeNull();
    const done = query('button[data-action="program-edit"]');
    expect(done.textContent).toBe(copy.program.done);
    done.click();
    expect(root.querySelector('textarea')).toBeNull();
    expect(query('button[data-action="program-edit"]').textContent).toBe(copy.program.edit);
  });

  it('takes focus into the box on opening it, and keeps it while typing', () => {
    open();
    click('[data-action="program-edit"]');
    expect(document.activeElement).toBe(box());
    write('ld r1, [0x0A4]');
    expect(document.activeElement).toBe(box());
    expect(box().value).toBe('ld r1, [0x0A4]');
  });

  it('parses on each change and shows the core’s summary, appending nothing', () => {
    open();
    click('[data-action="program-edit"]');
    write('ld r1, [0x0A4]\nld r2, [0x0A8]');
    expect(summary()).toBe('2 instructions · next: ld r1, [0x0A4]');
    expect(root.querySelector('.cache-explorer__program-errors')).toBeNull();
    expect(lent.opCount()).toBe(0);
  });

  it('lists each error with its line and column, and says how many in its line', () => {
    open();
    click('[data-action="program-edit"]');
    write('ld r1, [0x0A4]\nlw r2, [0x1]\nadd r3');
    const errors = all('.cache-explorer__program-error').map((row) => row.textContent);
    expect(errors).toHaveLength(2);
    expect(errors[0]).toMatch(/^Line 2, column 1: unknown instruction/);
    expect(errors[1]).toMatch(/^Line 3, column 1: /);
    expect(summary()).toBe('2 errors · first on line 2');
    expect(box().getAttribute('aria-invalid')).toBe('true');
    write('lw r2, [0x1]');
    expect(summary()).toBe('1 error on line 1');
    write('ld r1, [0x0A4]');
    expect(box().hasAttribute('aria-invalid')).toBe(false);
  });

  it('marks the lines with errors in the listing, with text and not colour alone', () => {
    open();
    click('[data-action="program-edit"]');
    write('ld r1, [0x0A4]\nlw r2, [0x1]');
    click('button[data-action="program-edit"]');
    const wrong = all('.cache-explorer__listing-row--wrong');
    expect(wrong.map((row) => row.dataset.line)).toEqual(['2']);
    expect(wrong[0]?.textContent).toContain('!');
    expect(wrong[0]?.textContent).toContain('has an error');
    expect(all('.cache-explorer__program-error')).toHaveLength(1);
  });

  it('selects the error’s place in the box when its row is pressed', () => {
    open();
    click('[data-action="program-edit"]');
    write('ld r1, [0x0A4]\n  lw r2, [0x1]');
    click('[data-action="program-edit"]');
    click('.cache-explorer__program-error');
    const field = box();
    expect(document.activeElement).toBe(field);
    expect(field.value.slice(field.selectionStart, field.selectionEnd)).toBe('lw');
  });

  it('keeps Space, Enter and the letters for the box, and steps nothing', () => {
    open();
    click('[data-action="program-edit"]');
    const event = press(' ', {}, box());
    expect(event.defaultPrevented).toBe(false);
    expect(press('Enter', {}, box()).defaultPrevented).toBe(false);
    expect(press('z', {}, box()).defaultPrevented).toBe(false);
    expect(lent.opCount()).toBe(0);
  });

  it('turns the listing into the box for a click on the listing itself', () => {
    open();
    click('.cache-explorer__listing');
    expect(root.querySelector('textarea')).not.toBeNull();
  });
});

describe('the program reaching the run', () => {
  it('loads on the first Space and logs the load as a divider, undone with the step', () => {
    open();
    expect(dividers()).toEqual([]);
    press(' ');
    expect(dividers()).toEqual(['Loaded an assembly program of 5 instructions']);
    expect(lent.opCount()).toBe(2);
    press('z');
    expect(lent.state().position.pos).toBe(0);
    expect(lent.state().program).toBeUndefined();
    expect(all('.cache-explorer__entry--divider').map((row) => row.dataset.tense)).toEqual([
      'future',
    ]);
    expect(summary()).toBe('5 instructions · next: ld r1, [0x0A4]');
  });

  it('loads a changed text with the next Shift+Space, and only then', () => {
    open();
    press(' ', { shiftKey: true });
    click('[data-action="program-edit"]');
    write('add r1, r0, #1\nadd r1, r1, #1');
    expect(dividers()).toHaveLength(1);
    click('button[data-action="program-edit"]');
    press(' ', { shiftKey: true });
    expect(dividers()).toEqual([
      'Loaded an assembly program of 5 instructions',
      'Loaded an assembly program of 2 instructions',
    ]);
    expect(lent.state().program?.source).toBe('add r1, r0, #1\nadd r1, r1, #1');
  });

  it('loads on a Run tick as it does on Space', () => {
    open();
    lent.stepAccess({ source: SAMPLE_PROGRAM, format: 'asm' });
    controller?.refresh();
    click('[data-action="program-edit"]');
    write('add r1, r0, #1');
    click('button[data-action="program-edit"]');
    vi.useFakeTimers();
    press('r');
    vi.advanceTimersByTime(RUN_MS);
    press('r');
    expect(dividers()).toHaveLength(2);
  });

  it('loads nothing while the text has errors, and says why in the status line', () => {
    open();
    click('[data-action="program-edit"]');
    write('lw r1, [0x1]');
    click('button[data-action="program-edit"]');
    const ops = lent.opCount();
    press(' ');
    expect(lent.opCount()).toBe(ops);
    expect(query('.cache-explorer__status').textContent).toContain('line 1');
    expect(dividers()).toEqual([]);
  });

  it('keeps the plain name of its button, and does not call it a toggle', () => {
    open();
    const edit = query('button[data-action="program-edit"]');
    expect(edit.textContent).toBe(copy.program.edit);
    expect(edit.hasAttribute('aria-pressed')).toBe(false);
    edit.click();
    expect(query('button[data-action="program-edit"]').textContent).toBe(copy.program.done);
    expect(query('button[data-action="program-edit"]').hasAttribute('aria-pressed')).toBe(false);
  });

  it('shows the program the run holds when the cursor is inside the log, whatever the text', () => {
    open('frame-3a');
    click('[data-action="program-edit"]');
    write('add r1, r0, #1');
    click('button[data-action="program-edit"]');
    press('z');
    // The edit loads nothing at the cursor: the listing, the current line and the summary stay
    // the run's own, and Space only moves on.
    expect(lent.state().program?.currentLine).toBeDefined();
    expect(rowsOf()).toEqual(SAMPLE_PROGRAM.split('\n'));
    expect(all('.cache-explorer__listing-row--current').map((row) => row.dataset.line)).toEqual([
      String(lent.state().program?.currentLine),
    ]);
    expect(summary()).toBe(lent.state().program?.summary);
    const ops = lent.opCount();
    const before = dividers();
    press(' ');
    expect(lent.opCount()).toBe(ops);
    expect(dividers()).toEqual(before);
  });

  it('reports the core’s no-program message when the drawer is empty', () => {
    open();
    click('[data-action="program-edit"]');
    write('');
    click('button[data-action="program-edit"]');
    const twin = SCENARIOS['first-visit']?.();
    if (!twin) throw new Error('no first-visit scenario');
    const outcome = twin.stepEvent(null);
    twin.free();
    if (outcome.status !== 'idle') throw new Error('an empty run moved');
    const none = outcome.message;
    expect(query('.cache-explorer__listing-empty').textContent).toBe(copy.program.empty.asm);
    expect(summary()).toBe(copy.program.none);
    press(' ');
    expect(query('.cache-explorer__status').textContent).toBe(none);
    expect(lent.opCount()).toBe(0);
  });
});

describe('the input type', () => {
  it('changes the text’s format and reads it again, leaving a loaded program alone', () => {
    open();
    press(' ');
    click('[data-action="program-format"][data-option="trace"]');
    expect(query('.cache-explorer__program-title').textContent).toBe('Trace');
    expect(
      query('[data-action="program-format"][data-option="trace"]').getAttribute('aria-pressed'),
    ).toBe('true');
    expect(lent.state().program?.format).toBe('asm');
    // The run's program stays on show, whatever the format of the text.
    expect(all('.cache-explorer__listing-row--wrong')).toHaveLength(0);
    click('[data-action="program-edit"]');
    expect(errorLines()).not.toHaveLength(0);
    click('button[data-action="program-edit"]');
    expect(query('.cache-explorer__program-help').textContent).toContain('One access per line');
    click('[data-action="program-format"][data-option="asm"]');
    expect(all('.cache-explorer__listing-row--wrong')).toHaveLength(0);
    expect(all('.cache-explorer__listing-row--current')).toHaveLength(1);
  });

  it('loads a trace when the text is one', () => {
    open();
    click('[data-action="program-format"][data-option="trace"]');
    click('[data-action="program-edit"]');
    write('R 0x0A4\nW 0x0A4 7');
    expect(summary()).toContain('2 ');
    click('button[data-action="program-edit"]');
    press(' ');
    expect(lent.state().program?.format).toBe('trace');
    expect(dividers()).toHaveLength(1);
  });
});

describe('errorRange', () => {
  it('finds a place by line and column', () => {
    expect(errorRange('ab\ncd ef', { line: 2, column: 4, length: 2 })).toEqual({
      start: 6,
      end: 8,
    });
  });

  it('counts columns in characters, not in UTF-16 units', () => {
    expect(errorRange('\u{1F600}x y', { line: 1, column: 3, length: 1 })).toEqual({
      start: 3,
      end: 4,
    });
  });

  it('puts an empty error, or one past the line’s end, at the caret, and reads a line past the text as its last', () => {
    expect(errorRange('ab', { line: 1, column: 3, length: 0 })).toEqual({ start: 2, end: 2 });
    expect(errorRange('ab', { line: 1, column: 1, length: 9 })).toEqual({ start: 0, end: 2 });
    expect(errorRange('ab\ncd', { line: 9, column: 1, length: 1 })).toEqual({ start: 3, end: 4 });
  });
});
