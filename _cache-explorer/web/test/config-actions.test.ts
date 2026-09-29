import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { start, type Controller } from '../src/controller';
import { h } from '../src/dom';
import { SAMPLE_PROGRAM } from '../src/ui-state';
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
