import { afterEach, beforeAll, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import type { Cause } from '../pkg/cache_explorer';
import type { Action } from '../src/actions';
import { configActions } from '../src/config-actions';
import type { Context } from '../src/context';
import { start, type Controller } from '../src/controller';
import { copy } from '../src/copy';
import { h } from '../src/dom';
import { initialUiState, SAMPLE_PROGRAM, type UiState } from '../src/ui-state';
import { Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

let root: HTMLElement;
/** The session a controller owns: a test may still move the run through it, but never frees it. */
let lent: Session;
let controller: Controller;

const program = { source: SAMPLE_PROGRAM, format: 'asm' } as const;

const query = (selector: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`nothing matches ${selector}`);
  return found;
};

/** The element that matches, which must be a `kind`. */
function typed<E extends HTMLElement>(kind: new () => E, selector: string): E {
  const found = query(selector);
  if (!(found instanceof kind)) throw new Error(`${selector} is not a ${kind.name}`);
  return found;
}

const inputAt = (selector: string): HTMLInputElement => typed(HTMLInputElement, selector);
const buttonAt = (selector: string): HTMLButtonElement => typed(HTMLButtonElement, selector);
const widthList = (): HTMLSelectElement => typed(HTMLSelectElement, 'select');

const option = (group: string, index: number): HTMLButtonElement =>
  buttonAt(`[data-action='config-option'][data-group='${group}'][data-option='${String(index)}']`);

const detail = (which: 'basics' | 'advanced'): HTMLButtonElement =>
  buttonAt(`[data-action='detail'][data-option='${which}']`);

const prompt = (): HTMLElement | null => root.querySelector('[role="alert"]');

/** Types into one of the timing fields, as the student does. */
function type(action: string, text: string): HTMLInputElement {
  const input = inputAt(`[data-action='${action}']`);
  input.focus();
  input.value = text;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  return input;
}

const leave = (input: HTMLElement): void => {
  input.dispatchEvent(new Event('change', { bubbles: true }));
};

/** Starts the tool on a first visit, at a desktop width, so the bar is open. */
function open(layout: 'desktop' | 'tablet' = 'desktop'): void {
  lent = Session.firstVisit();
  controller = start(root, lent, layout);
}

/** Space a few times: the run has entries, so a change to the configuration has to ask. */
function step(times = 2): void {
  for (let i = 0; i < times; i += 1) lent.stepEvent(program);
  controller.refresh();
}

beforeAll(loadWasm);

beforeEach(() => {
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
});

afterEach(() => {
  vi.useRealTimers();
  controller.dispose();
});

describe('a change on a run with no entries', () => {
  it('applies at once, as a configuration of its own, and draws it', () => {
    open();
    option('associativity', 1).click();
    expect(lent.state().config.cache.associativity).toBe(2);
    expect(prompt()).toBeNull();
    expect(query('.cache-explorer__config-toggle').textContent).toContain('2-way · LRU');
    expect(option('associativity', 1).getAttribute('aria-pressed')).toBe('true');
    expect(option('associativity', 0).getAttribute('aria-pressed')).toBe('false');
  });

  it('is one undo stop for each change', () => {
    open();
    option('replacement', 1).click();
    option('cacheSize', 2).click();
    expect(lent.state().config.cache.sizeBytes).toBe(256);
    expect(lent.opCount()).toBe(2);
    expect(lent.undo()).toBe(true);
    expect(lent.state().config.cache.sizeBytes).toBe(128);
    expect(lent.state().config.cache.replacement).toBe('fifo');
  });

  it('does nothing for an option the core disables', () => {
    open();
    const disabled = option('associativity', 3);
    expect(disabled.disabled).toBe(true);
    disabled.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(lent.opCount()).toBe(0);
    expect(prompt()).toBeNull();
  });

  it('keeps focus on the same option after the bar is rebuilt', () => {
    open();
    option('lineSize', 2).focus();
    option('lineSize', 2).click();
    expect(document.activeElement).toBe(option('lineSize', 2));
    expect(document.activeElement?.getAttribute('aria-pressed')).toBe('true');
  });
});

describe('a change on a run with entries', () => {
  it('asks "Reset to apply?" with the core’s question, and changes nothing yet', () => {
    open();
    step(3);
    const { stepCount } = lent.timeline();
    const ops = lent.opCount();
    option('associativity', 1).click();
    expect(prompt()?.querySelector('.cache-explorer__pending-text')?.textContent).toBe(
      `Configuration is fixed for a run. Applying starts a new, empty run; this one has ${String(stepCount)} steps and stays reachable with Undo.`,
    );
    expect(lent.state().config.cache.associativity).toBe(1);
    expect(lent.opCount()).toBe(ops);
  });

  it('shows the pending configuration in the bar while the run stays as it was', () => {
    open();
    step();
    option('associativity', 1).click();
    expect(query('.cache-explorer__config-toggle').textContent).toContain('2-way · LRU');
    expect(option('associativity', 1).getAttribute('aria-pressed')).toBe('true');
    expect(root.textContent).toContain(lent.state().labels.statusText);
    expect(lent.state().labels.configSummary).toBe('128 B · 16 B lines · direct-mapped');
  });

  it('builds the next change on the pending one', () => {
    open();
    step();
    option('associativity', 1).click();
    option('replacement', 1).click();
    expect(query('.cache-explorer__config-toggle').textContent).toContain('2-way · FIFO');
    expect(prompt()).not.toBeNull();
  });

  it('starts a new run with "Reset and apply", which Undo can leave again', () => {
    open();
    step(3);
    const { stepCount } = lent.timeline();
    option('associativity', 1).click();
    query("[data-action='apply-pending']").click();
    expect(lent.state().config.cache.associativity).toBe(2);
    expect(prompt()).toBeNull();
    expect(root.textContent).toContain('0 steps in this run');
    expect(lent.undo()).toBe(true);
    expect(lent.state().config.cache.associativity).toBe(1);
    controller.refresh();
    expect(root.textContent).toContain(`${String(stepCount)} steps in this run`);
  });

  it('drops the change with "Keep this run"', () => {
    open();
    step();
    option('cacheSize', 3).click();
    query("[data-action='keep-run']").click();
    expect(prompt()).toBeNull();
    expect(query('.cache-explorer__config-toggle').textContent).toContain('128 B');
    expect(option('cacheSize', 1).getAttribute('aria-pressed')).toBe('true');
    expect(lent.state().config.cache.sizeBytes).toBe(128);
  });

  it('takes the question back when the student picks the run’s own choice again', () => {
    open();
    step();
    const ops = lent.opCount();
    option('associativity', 1).click();
    expect(prompt()).not.toBeNull();
    option('associativity', 0).click();
    expect(prompt()).toBeNull();
    expect(lent.opCount()).toBe(ops);
  });

  it('puts a refusal aside without a question, and lets a trap through', () => {
    open();
    step();
    vi.spyOn(lent, 'proposeConfig').mockImplementationOnce(() => {
      throw new Error('A link can hold at most 10,000 actions.');
    });
    option('associativity', 1).click();
    expect(prompt()).toBeNull();
    expect(lent.state().config.cache.associativity).toBe(1);

    const errors: unknown[] = [];
    const record = (event: ErrorEvent): void => {
      event.preventDefault();
      errors.push(event.error);
    };
    window.addEventListener('error', record);
    vi.spyOn(lent, 'proposeConfig').mockImplementationOnce(() => {
      throw new WebAssembly.RuntimeError('unreachable');
    });
    option('associativity', 1).click();
    window.removeEventListener('error', record);
    expect(errors).toHaveLength(1);
    expect(errors[0]).toBeInstanceOf(WebAssembly.RuntimeError);
  });
});

describe('the detail switch', () => {
  it('goes to Advanced without asking, and the bar gains its groups', () => {
    open();
    step();
    const ops = lent.opCount();
    detail('advanced').click();
    expect(lent.state().advanced).toBe(true);
    expect(prompt()).toBeNull();
    expect(root.textContent).toContain(lent.state().labels.statusText);
    expect(lent.state().labels.statusText).toMatch(/^Advanced · \d+ steps in this run$/);
    expect(root.querySelector('.cache-explorer__group[data-group="timing"]')).not.toBeNull();
    expect(detail('advanced').getAttribute('aria-pressed')).toBe('true');
    expect(lent.opCount()).toBe(ops);
  });

  it('goes back to Basics without asking when the run fits it', () => {
    open();
    detail('advanced').click();
    detail('basics').click();
    expect(lent.state().advanced).toBe(false);
    expect(root.querySelector('.cache-explorer__group[data-group="timing"]')).toBeNull();
  });

  it('asks before leaving a run Basics cannot show, and leaves Advanced once it has been applied', () => {
    open();
    detail('advanced').click();
    option('writePolicy', 1).click();
    expect(lent.state().config.cache.writePolicy).toBe('writeThrough');
    step();

    detail('basics').click();
    expect(lent.state().advanced).toBe(true);
    expect(prompt()).not.toBeNull();
    expect(query('.cache-explorer__config-toggle').textContent).toContain('write-back');

    query("[data-action='apply-pending']").click();
    expect(lent.state().config.cache.writePolicy).toBe('writeBack');
    expect(lent.state().advanced).toBe(false);
    expect(prompt()).toBeNull();
    expect(detail('basics').getAttribute('aria-pressed')).toBe('true');
  });

  it('stays in Advanced when the switch to Basics is kept out of', () => {
    open();
    detail('advanced').click();
    option('writePolicy', 1).click();
    step();
    detail('basics').click();
    query("[data-action='keep-run']").click();
    expect(lent.state().advanced).toBe(true);
    expect(lent.state().config.cache.writePolicy).toBe('writeThrough');
  });

  it('withdraws the question about Basics when the student chooses Advanced again', () => {
    open();
    detail('advanced').click();
    option('writePolicy', 1).click();
    step();
    detail('basics').click();
    expect(prompt()).not.toBeNull();
    detail('advanced').click();
    expect(prompt()).toBeNull();
    expect(lent.state().advanced).toBe(true);
  });

  it('does not take a change made in Advanced for a switch to Basics when it is applied', () => {
    open();
    detail('advanced').click();
    step();
    option('cacheSize', 3).click();
    query("[data-action='apply-pending']").click();
    expect(lent.state().config.cache.sizeBytes).toBe(512);
    expect(lent.state().advanced).toBe(true);
  });
});

describe('the bar itself', () => {
  it('opens and closes on its summary line', () => {
    open();
    query("[data-action='config-toggle']").click();
    expect(root.querySelector('.cache-explorer__config-groups')).toBeNull();
    expect(query('.cache-explorer__config-toggle').getAttribute('aria-expanded')).toBe('false');
    query("[data-action='config-toggle']").click();
    expect(query('.cache-explorer__config-groups')).not.toBeNull();
  });

  it('starts collapsed at a tablet width, with the detail switch still in reach', () => {
    open('tablet');
    expect(root.querySelector('.cache-explorer__config-groups')).toBeNull();
    detail('advanced').click();
    expect(lent.state().advanced).toBe(true);
  });

  it('keeps the question when it is closed', () => {
    open();
    step();
    option('cacheSize', 0).click();
    query("[data-action='config-toggle']").click();
    expect(prompt()).not.toBeNull();
  });
});

describe('address and word', () => {
  it('opens onto its two controls and closes again', () => {
    open();
    query("[data-action='address-toggle']").click();
    expect(root.querySelector('select')).not.toBeNull();
    query("[data-action='address-toggle']").click();
    expect(root.querySelector('select')).toBeNull();
  });

  it('changes the address width from the list, by the option it is on', () => {
    open();
    query("[data-action='address-toggle']").click();
    const select = widthList();
    select.selectedIndex = 8;
    leave(select);
    expect(lent.state().config.addressBits).toBe(16);
    expect(widthList().options[8]?.selected).toBe(true);
  });

  it('asks first on a run with entries, and shows the width it would apply', () => {
    open();
    step();
    query("[data-action='address-toggle']").click();
    const select = widthList();
    select.selectedIndex = 12;
    leave(select);
    expect(lent.state().config.addressBits).toBe(12);
    expect(prompt()).not.toBeNull();
    expect(select.options[select.selectedIndex]?.textContent).toBe('20-bit');
    query("[data-action='keep-run']").click();
    expect(widthList().options[4]?.selected).toBe(true);
  });

  it('returns the list to the width in force when the change is refused', () => {
    open();
    query("[data-action='address-toggle']").click();
    vi.spyOn(lent, 'proposeConfig').mockImplementationOnce(() => {
      throw new Error('No room for another action.');
    });
    const select = widthList();
    select.selectedIndex = 10;
    leave(select);
    expect(select.options[select.selectedIndex]?.textContent).toBe('12-bit');
  });

  it('changes the word size from its buttons', () => {
    open();
    query("[data-action='address-toggle']").click();
    option('wordSize', 3).click();
    expect(lent.state().config.wordBytes).toBe(8);
    expect(option('wordSize', 3).getAttribute('aria-pressed')).toBe('true');
    expect(query('.cache-explorer__address')).not.toBeNull();
  });
});

describe('timing', () => {
  it('turns timed mode on and off with its checkbox', () => {
    open();
    detail('advanced').click();
    const box = inputAt("[data-action='timed']");
    box.click();
    expect(lent.state().config.timing).toEqual({ hitCycles: 1, missCycles: 20 });
    expect(inputAt("[data-action='timed']").checked).toBe(true);
    inputAt("[data-action='timed']").click();
    expect(lent.state().config.timing).toBeUndefined();
  });

  it('sends a typed number when the student leaves the field, and turns timing on with it', () => {
    open();
    detail('advanced').click();
    const miss = type('timing-miss', '35');
    expect(lent.state().config.timing).toBeUndefined();
    leave(miss);
    expect(lent.state().config.timing).toEqual({ hitCycles: 1, missCycles: 35 });
    expect(inputAt("[data-action='timed']").checked).toBe(true);
    expect(root.querySelector('[aria-invalid]')).toBeNull();
  });

  it('marks text that is not a number as the student types, and sends nothing', () => {
    open();
    detail('advanced').click();
    const hit = type('timing-hit', '1.5');
    expect(hit.getAttribute('aria-invalid')).toBe('true');
    expect(query('.cache-explorer__note').textContent).toBe(
      'Type a whole number of cycles, 1 to 1,000.',
    );
    leave(hit);
    expect(hit.value).toBe('1.5');
    expect(lent.opCount()).toBe(0);
  });

  it('gives the core’s range message for a number out of range, and clears it once it reads', () => {
    open();
    detail('advanced').click();
    const miss = type('timing-miss', '1001');
    expect(query('.cache-explorer__note').textContent).toContain('Miss penalty must be between');
    type('timing-miss', '1000');
    expect(root.querySelector('.cache-explorer__note')).toBeNull();
    expect(miss.hasAttribute('aria-invalid')).toBe(false);
  });

  it('puts the value in force back in a field that does not read once another change is made', () => {
    open();
    detail('advanced').click();
    const hit = type('timing-hit', 'abc');
    hit.blur();
    option('cacheSize', 2).click();
    expect(root.querySelector('.cache-explorer__note')).toBeNull();
    expect(hit.value).toBe('1');
  });

  it('asks first on a run with entries, and shows the typed cost as pending', () => {
    open();
    detail('advanced').click();
    step();
    leave(type('timing-hit', '4'));
    expect(prompt()).not.toBeNull();
    expect(lent.state().config.timing).toBeUndefined();
    expect(inputAt("[data-action='timed']").checked).toBe(true);
    expect(inputAt("[data-action='timing-hit']").value).toBe('4');
  });
});

describe('a log line’s "why:" link', () => {
  const link = (cause: string): void => {
    const element = h('button', { 'data-action': 'why', 'data-group': cause });
    root.append(element);
    element.click();
    element.remove();
  };

  it('opens a closed bar and flashes the group for 1.6 seconds', () => {
    vi.useFakeTimers();
    open();
    query("[data-action='config-toggle']").click();
    link('associativity');
    expect(query('.cache-explorer__config-groups')).not.toBeNull();
    const flashing = (): string[] =>
      [...root.querySelectorAll('.cache-explorer__group--why')].map(
        (group) => group.getAttribute('data-group') ?? '',
      );
    expect(flashing()).toEqual(['associativity']);
    vi.advanceTimersByTime(1599);
    expect(flashing()).toEqual(['associativity']);
    vi.advanceTimersByTime(1);
    expect(flashing()).toEqual([]);
  });

  it('follows the newest link, and the first one’s end does not cut the second short', () => {
    vi.useFakeTimers();
    open();
    link('lineSize');
    vi.advanceTimersByTime(1000);
    link('replacement');
    vi.advanceTimersByTime(1000);
    expect(root.querySelector('.cache-explorer__group--why')?.getAttribute('data-group')).toBe(
      'replacement',
    );
    vi.advanceTimersByTime(600);
    expect(root.querySelector('.cache-explorer__group--why')).toBeNull();
  });

  it('starts again from the beginning when the same link points at the group again', () => {
    vi.useFakeTimers();
    open();
    const age = (): string | null => query("[data-group='lineSize']").getAttribute('style');
    link('lineSize');
    vi.advanceTimersByTime(1000);
    controller.refresh();
    expect(age()).toBe('animation-delay: -1000ms');
    link('lineSize');
    expect(age()).toBeNull();
    vi.advanceTimersByTime(700);
    controller.refresh();
    // 1,700 ms after the first link, 700 after the second: the flash has not ended, and the
    // element built now carries on from 700 ms, not from the first link's 1,700.
    expect(query("[data-group='lineSize']").classList).toContain('cache-explorer__group--why');
    expect(age()).toBe('animation-delay: -700ms');
    vi.advanceTimersByTime(899);
    expect(root.querySelector('.cache-explorer__group--why')).not.toBeNull();
    vi.advanceTimersByTime(1);
    expect(root.querySelector('.cache-explorer__group--why')).toBeNull();
  });

  it('draws a flash begun at another group from its own beginning', () => {
    vi.useFakeTimers();
    open();
    link('lineSize');
    vi.advanceTimersByTime(900);
    link('replacement');
    expect(query("[data-group='replacement']").hasAttribute('style')).toBe(false);
    expect(query("[data-group='lineSize']").classList).not.toContain('cache-explorer__group--why');
  });

  it('shows a group Basics hides, read-only, and takes it away when the flash is over', () => {
    vi.useFakeTimers();
    open();
    link('allocatePolicy');
    expect(query("[data-group='allocatePolicy'] .cache-explorer__note").textContent).toBe(
      'Advanced changes this.',
    );
    vi.advanceTimersByTime(1600);
    expect(root.querySelector("[data-group='allocatePolicy']")).toBeNull();
  });

  it('ignores a name that is not a group', () => {
    open();
    link('cacheSize');
    link('__proto__');
    expect(root.querySelector('.cache-explorer__group--why')).toBeNull();
  });

  it('is not left running when the tool is disposed', () => {
    vi.useFakeTimers();
    open();
    link('timing');
    controller.dispose();
    expect(() => vi.advanceTimersByTime(2000)).not.toThrow();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe('a press on a control while a timing field is being left', () => {
  const down = (target: Element): void => {
    target.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
  };
  const up = (target: Element): void => {
    target.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, button: 0 }));
  };

  it('holds the redraw back, so the click reaches the button the press began on', () => {
    open();
    detail('advanced').click();
    const miss = type('timing-miss', '35');
    const pressed = option('associativity', 1);
    down(pressed);
    // What the browser does next: the field loses focus, and its change is sent.
    leave(miss);
    expect(lent.state().config.timing).toEqual({ hitCycles: 1, missCycles: 35 });
    expect(pressed.isConnected).toBe(true);
    up(pressed);
    expect(pressed.isConnected).toBe(true);
    pressed.click();
    expect(lent.state().config.cache.associativity).toBe(2);
    expect(lent.state().config.timing).toEqual({ hitCycles: 1, missCycles: 35 });
    expect(lent.opCount()).toBe(2);
    expect(option('associativity', 1).getAttribute('aria-pressed')).toBe('true');
    expect(inputAt("[data-action='timed']").checked).toBe(true);
  });

  it('does the same for the Basics and Advanced switch', () => {
    open();
    detail('advanced').click();
    const miss = type('timing-miss', '35');
    const basics = detail('basics');
    down(basics);
    leave(miss);
    up(basics);
    basics.click();
    expect(lent.state().config.timing).toBeUndefined();
    expect(lent.state().advanced).toBe(false);
    expect(lent.opCount()).toBe(2);
  });

  it('asks for what the Timed mode box says when the cost typed has already turned it on', () => {
    open();
    detail('advanced').click();
    const miss = type('timing-miss', '35');
    const box = inputAt("[data-action='timed']");
    expect(box.checked).toBe(false);
    down(box);
    leave(miss);
    up(box);
    box.click();
    expect(lent.state().config.timing).toEqual({ hitCycles: 1, missCycles: 35 });
    expect(inputAt("[data-action='timed']").checked).toBe(true);
    expect(lent.opCount()).toBe(1);
  });

  it('still turns timed mode off when the box says so', () => {
    open();
    detail('advanced').click();
    inputAt("[data-action='timed']").click();
    expect(lent.state().config.timing).toBeDefined();
    inputAt("[data-action='timed']").click();
    expect(lent.state().config.timing).toBeUndefined();
  });

  it('makes the redraw once the press is over when no click follows it', () => {
    vi.useFakeTimers();
    open();
    detail('advanced').click();
    const miss = type('timing-miss', '35');
    const pressed = option('associativity', 1);
    down(pressed);
    leave(miss);
    up(pressed);
    expect(inputAt("[data-action='timed']").checked).toBe(false);
    vi.runAllTimers();
    expect(inputAt("[data-action='timed']").checked).toBe(true);
    expect(pressed.isConnected).toBe(false);
  });

  it('redraws at once for a press on something that is not a control', () => {
    open();
    detail('advanced').click();
    const miss = type('timing-miss', '35');
    down(query('.cache-explorer__head'));
    leave(miss);
    expect(inputAt("[data-action='timed']").checked).toBe(true);
  });

  it('redraws at once when no press is down, as a change made from the keyboard does', () => {
    open();
    detail('advanced').click();
    const miss = type('timing-miss', '35');
    leave(miss);
    expect(inputAt("[data-action='timed']").checked).toBe(true);
    expect(document.activeElement).toBe(miss);
  });

  it('draws the state of every change that came while it was held', () => {
    vi.useFakeTimers();
    open();
    detail('advanced').click();
    const hit = type('timing-hit', '3');
    const pressed = option('cacheSize', 3);
    down(pressed);
    leave(hit);
    leave(type('timing-miss', '9'));
    up(pressed);
    vi.runAllTimers();
    expect(inputAt("[data-action='timing-hit']").value).toBe('3');
    expect(inputAt("[data-action='timing-miss']").value).toBe('9');
    expect(lent.state().config.timing).toEqual({ hitCycles: 3, missCycles: 9 });
  });

  it('lets go of the press when the tool is disposed', () => {
    vi.useFakeTimers();
    open();
    const pressed = option('cacheSize', 3);
    down(pressed);
    up(pressed);
    controller.dispose();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe('a change made while the switch to Basics is being asked about', () => {
  /** A write-through run with entries, and the question about leaving Advanced for it. */
  function askAboutBasics(): void {
    open();
    detail('advanced').click();
    option('writePolicy', 1).click();
    step();
    detail('basics').click();
    expect(prompt()).not.toBeNull();
  }

  it('is another change: applying it starts the run the student last chose, and stays in Advanced', () => {
    askAboutBasics();
    const leaving = vi.spyOn(lent, 'setAdvanced');
    const ops = lent.opCount();
    // The bar shows the Basics form on offer, which allocates on a store miss.
    option('allocatePolicy', 1).click();
    expect(prompt()).not.toBeNull();
    query("[data-action='apply-pending']").click();
    expect(lent.opCount()).toBe(ops + 1);
    expect(lent.state().config.cache.allocatePolicy).toBe('writeNoAllocate');
    expect(lent.state().advanced).toBe(true);
    expect(leaving).not.toHaveBeenCalled();
    expect(prompt()).toBeNull();
    expect(detail('advanced').getAttribute('aria-pressed')).toBe('true');
  });

  it('is another change too when it is a typed cost', () => {
    askAboutBasics();
    const leaving = vi.spyOn(lent, 'setAdvanced');
    leave(type('timing-miss', '35'));
    expect(prompt()).not.toBeNull();
    query("[data-action='apply-pending']").click();
    expect(lent.state().config.timing).toEqual({ hitCycles: 1, missCycles: 35 });
    expect(lent.state().advanced).toBe(true);
    expect(leaving).not.toHaveBeenCalled();
  });

  it('is dropped, with the question, when the student picks the run’s own choice again', () => {
    askAboutBasics();
    option('writePolicy', 1).click();
    expect(prompt()).toBeNull();
    expect(lent.state().advanced).toBe(true);
    expect(lent.state().config.cache.writePolicy).toBe('writeThrough');
  });

  it('leaves the page in Advanced even when the change would fit Basics', () => {
    askAboutBasics();
    option('cacheSize', 3).click();
    expect(query('.cache-explorer__config-toggle').textContent).toContain('512 B');
    query("[data-action='apply-pending']").click();
    expect(lent.state().config.cache.sizeBytes).toBe(512);
    expect(lent.state().config.cache.writePolicy).toBe('writeBack');
    expect(lent.state().advanced).toBe(true);
    expect(prompt()).toBeNull();
  });

  it('is still asked about as before when nothing else is chosen', () => {
    askAboutBasics();
    query("[data-action='apply-pending']").click();
    expect(lent.state().config.cache.writePolicy).toBe('writeBack');
    expect(lent.state().advanced).toBe(false);
  });

  it('is not lost when the other change is refused', () => {
    askAboutBasics();
    vi.spyOn(lent, 'proposeConfig').mockImplementationOnce(() => {
      throw new Error('A link can hold at most 10,000 actions.');
    });
    option('cacheSize', 3).click();
    query("[data-action='apply-pending']").click();
    expect(lent.state().config.cache.writePolicy).toBe('writeBack');
    expect(lent.state().advanced).toBe(false);
  });

  it('starts the Basics run at once, with no question, when the run has no entries', () => {
    open();
    detail('advanced').click();
    option('writePolicy', 1).click();
    detail('basics').click();
    expect(prompt()).toBeNull();
    expect(lent.state().advanced).toBe(false);
    expect(lent.state().config.cache.writePolicy).toBe('writeBack');
    expect(detail('basics').getAttribute('aria-pressed')).toBe('true');
    expect(lent.undo()).toBe(true);
    expect(lent.state().config.cache.writePolicy).toBe('writeThrough');
  });
});

describe('a group a log line has pointed at while Basics hides it', () => {
  it('ignores a click on its buttons, and changes nothing', () => {
    vi.useFakeTimers();
    open();
    const element = h('button', { 'data-action': 'why', 'data-group': 'writePolicy' });
    root.append(element);
    element.click();
    element.remove();
    option('writePolicy', 1).click();
    expect(lent.opCount()).toBe(0);
    expect(lent.state().config.cache.writePolicy).toBe('writeBack');
    expect(prompt()).toBeNull();
    expect(option('writePolicy', 0).getAttribute('aria-pressed')).toBe('true');
  });
});

describe('what the handlers say to the status line', () => {
  let session: Session;
  let ui: UiState;
  let refresh: Mock<() => void>;
  let context: Context;

  const act = (
    kind: 'clicks' | 'changes',
    name: string,
    params: Action['params'] = {},
    element: HTMLElement = h('button'),
  ): void => {
    const handler = configActions[kind].get(name);
    if (!handler) throw new Error(`no handler for ${name}`);
    handler(context, { name, params, element });
  };

  /** A click on an option of the bar. */
  const choose = (group: string, index: number): void => {
    act('clicks', 'config-option', { group, option: String(index) });
  };

  beforeEach(() => {
    session = Session.firstVisit();
    ui = initialUiState('desktop');
    refresh = vi.fn<() => void>();
    context = {
      root: document.body,
      session,
      ui,
      modules: null,
      disposed: false,
      refresh,
      flash: vi.fn<(cause: Cause) => void>(),
      setRunning: vi.fn<(on: boolean) => void>(),
    };
    for (let i = 0; i < 2; i += 1) session.stepEvent(program);
  });

  afterEach(() => {
    session.free();
  });

  it('says why the core refused a change, and asks nothing', () => {
    vi.spyOn(session, 'proposeConfig').mockImplementationOnce(() => {
      throw new Error('A link can hold at most 10,000 actions.');
    });
    choose('associativity', 1);
    expect(ui.status).toBe('A link can hold at most 10,000 actions.');
    expect(ui.pending).toBeNull();
    expect(ui.pendingMessage).toBeNull();
    expect(refresh).toHaveBeenCalledOnce();
  });

  it('says nothing for a change the core asks about', () => {
    choose('associativity', 1);
    expect(ui.status).toBe('');
    expect(ui.pending?.cache.associativity).toBe(2);
    expect(ui.pendingMessage).toMatch(/^Configuration is fixed for a run\./);
  });

  it('says the new configuration is applied, and that the run starts empty', () => {
    choose('associativity', 1);
    act('clicks', 'apply-pending');
    expect(ui.status).toBe('New configuration applied. The run starts empty.');
    expect(ui.status).toBe(copy.config.applied);
    expect(ui.pending).toBeNull();
    expect(session.state().config.cache.associativity).toBe(2);
  });

  it('keeps the question, and says why, when "Reset and apply" is refused', () => {
    choose('associativity', 1);
    const { pending, pendingMessage } = ui;
    const ops = session.opCount();
    vi.spyOn(session, 'applyOp').mockImplementationOnce(() => {
      throw new Error('A link can hold at most 10,000 actions, and this would pass that.');
    });
    act('clicks', 'apply-pending');
    expect(ui.status).toBe('A link can hold at most 10,000 actions, and this would pass that.');
    expect(ui.pending).toBe(pending);
    expect(ui.pendingMessage).toBe(pendingMessage);
    expect(session.opCount()).toBe(ops);
    expect(session.state().config.cache.associativity).toBe(1);
    // The student can still agree once the core has room.
    act('clicks', 'apply-pending');
    expect(ui.status).toBe(copy.config.applied);
    expect(session.state().config.cache.associativity).toBe(2);
  });

  it('keeps the switch to Basics too when "Reset and apply" is refused', () => {
    session.setAdvanced(true);
    choose('writePolicy', 1);
    act('clicks', 'apply-pending');
    session.stepEvent(program);
    act('clicks', 'detail', { option: 'basics' });
    expect(ui.pendingBasics).toBe(true);
    vi.spyOn(session, 'applyOp').mockImplementationOnce(() => {
      throw new Error('Answer the prediction in the module panel first.');
    });
    act('clicks', 'apply-pending');
    expect(ui.status).toBe('Answer the prediction in the module panel first.');
    expect(ui.pendingBasics).toBe(true);
    expect(ui.pending).not.toBeNull();
    expect(session.state().advanced).toBe(true);
  });

  it('clears the question about Basics when another change is proposed', () => {
    session.setAdvanced(true);
    choose('writePolicy', 1);
    act('clicks', 'apply-pending');
    session.stepEvent(program);
    act('clicks', 'detail', { option: 'basics' });
    expect(ui.pendingBasics).toBe(true);
    choose('cacheSize', 3);
    expect(ui.pendingBasics).toBe(false);
    expect(ui.pending).not.toBeNull();
  });

  it('lets a refusal that is not one through', () => {
    vi.spyOn(session, 'applyOp').mockImplementationOnce(() => {
      throw new WebAssembly.RuntimeError('unreachable');
    });
    choose('associativity', 1);
    expect(() => {
      act('clicks', 'apply-pending');
    }).toThrow(WebAssembly.RuntimeError);
  });
});
