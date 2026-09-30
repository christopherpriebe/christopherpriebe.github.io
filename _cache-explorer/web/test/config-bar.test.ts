import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { buildSnapshot } from '../src/controller';
import { h } from '../src/dom';
import { render } from '../src/render';
import { initialUiState, SAMPLE_PROGRAM, type UiState } from '../src/ui-state';
import { Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

let root: HTMLElement;
let session: Session | null;
let ui: UiState;

function current(): Session {
  if (!session) throw new Error('no session');
  return session;
}

/**
 * One render of the session as it is now, at `now` on the clock a flash's age is read from;
 * `change` may alter what the snapshot says first.
 */
function draw(
  change?: (snapshot: ReturnType<typeof buildSnapshot>['snapshot']) => void,
  now = 0,
): void {
  const { snapshot, log } = buildSnapshot(current(), ui, now);
  change?.(snapshot);
  render(root, snapshot, log, ui);
}

function step(): void {
  current().stepEvent({ source: SAMPLE_PROGRAM, format: 'asm' });
}

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

/** The group boxes, by the name each one is drawn under. */
const groupNames = (): (string | undefined)[] =>
  [...root.querySelectorAll('.cache-explorer__group')].map(
    (group) => group.querySelector('.cache-explorer__group-label')?.textContent,
  );

const group = (key: string): HTMLElement => query(`.cache-explorer__group[data-group='${key}']`);

const labels = (key: string): (string | null)[] =>
  [...group(key).querySelectorAll('button')].map((button) => button.textContent);

const optionOf = (key: string, index: number): HTMLButtonElement =>
  buttonAt(`[data-action='config-option'][data-group='${key}'][data-option='${String(index)}']`);

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

describe('the summary line', () => {
  it('opens onto the configuration, named, with the core summary beside it', () => {
    draw();
    const toggle = query('.cache-explorer__config-toggle');
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(toggle.dataset.action).toBe('config-toggle');
    expect(toggle.textContent).toBe('−Configuration128 B · 16 B lines · direct-mapped');
  });

  it('collapses to itself and the detail switch, with a plus for the glyph', () => {
    ui.configOpen = false;
    draw();
    expect(query('.cache-explorer__config-toggle').getAttribute('aria-expanded')).toBe('false');
    expect(query('.cache-explorer__config-glyph').textContent).toBe('+');
    expect(root.querySelector('.cache-explorer__config-groups')).toBeNull();
    expect(root.querySelector('[data-action="detail"]')).not.toBeNull();
  });

  it('follows the summary the core gives for the configuration on show', () => {
    const config = current().state().config;
    ui.pending = { ...config, cache: { ...config.cache, associativity: 2 } };
    draw();
    expect(query('.cache-explorer__config-toggle').textContent).toContain('2-way · LRU');
  });

  it('keeps the detail switch beside it, pressed for the mode in force', () => {
    draw();
    const switchOf = (): [string | null, string | null][] =>
      [...root.querySelectorAll('[data-action="detail"]')].map((b) => [
        b.textContent,
        b.getAttribute('aria-pressed'),
      ]);
    expect(query('.cache-explorer__seg--inline').getAttribute('aria-label')).toBe('Detail level');
    expect(switchOf()).toEqual([
      ['Basics', 'true'],
      ['Advanced', 'false'],
    ]);
    current().setAdvanced(true);
    draw();
    expect(switchOf()).toEqual([
      ['Basics', 'false'],
      ['Advanced', 'true'],
    ]);
  });
});

describe('the groups', () => {
  it('are the design’s five in Basics, in its order', () => {
    draw();
    expect(groupNames()).toEqual([
      'Cache size',
      'Line size',
      'Associativity',
      'Replacement',
      'Address and word',
    ]);
  });

  it('are listed by the core in the order the bar draws them, the two of the address cell together', () => {
    const kinds = current()
      .configControls(current().state().config)
      .groups.map((group) => group.group);
    expect(kinds).toEqual([
      'cacheSize',
      'lineSize',
      'associativity',
      'replacement',
      'writePolicy',
      'allocatePolicy',
      'addressBits',
      'wordSize',
    ]);
  });

  it('follow the order the core lists them in, the address cell where the first of its two is', () => {
    current().setAdvanced(true);
    draw((snapshot) => {
      snapshot.config.groups.reverse();
    });
    expect(groupNames()).toEqual([
      'Address and word',
      'On a store miss',
      'Write policy',
      'Replacement',
      'Associativity',
      'Line size',
      'Cache size',
      'Timing',
    ]);
  });

  it('gain write policy and store miss before the address, and timing after it, in Advanced', () => {
    current().setAdvanced(true);
    draw();
    expect(groupNames()).toEqual([
      'Cache size',
      'Line size',
      'Associativity',
      'Replacement',
      'Write policy',
      'On a store miss',
      'Address and word',
      'Timing',
    ]);
  });

  it('carry the core’s options, in its order, with the current choice pressed', () => {
    draw();
    expect(labels('cacheSize')).toEqual(['64 B', '128 B', '256 B', '512 B']);
    expect(labels('lineSize')).toEqual(['8 B', '16 B', '32 B']);
    expect(labels('associativity')).toEqual(['Direct', '2-way', '4-way', '8-way', 'Full']);
    expect(labels('replacement')).toEqual(['LRU', 'FIFO', 'Random']);
    const bar = query('.cache-explorer__config');
    const pressed = [...bar.querySelectorAll('[aria-pressed="true"]')].map((b) => b.textContent);
    expect(pressed).toEqual(['Basics', '128 B', '16 B', 'Direct', 'LRU']);
  });

  it('name themselves for assistive technology, and their options by group and index', () => {
    draw();
    const seg = group('cacheSize').querySelector('[role="group"]');
    expect(seg?.getAttribute('aria-label')).toBe('Cache size');
    expect(optionOf('cacheSize', 3).textContent).toBe('512 B');
    expect(optionOf('associativity', 4).textContent).toBe('Full');
  });

  it('switch off an option the core disables, and say why in its title', () => {
    draw();
    const eight = optionOf('associativity', 3);
    expect(eight.disabled).toBe(true);
    expect(eight.title).toBe('Same as Full for an 8-line cache.');
    expect(optionOf('associativity', 2).disabled).toBe(false);
    expect(optionOf('associativity', 2).title).toBe('');
  });

  it('are switched off, with the guard as the title, while the core says the bar is not live', () => {
    current().setAdvanced(true);
    ui.addressOpen = true;
    draw((snapshot) => {
      snapshot.state.controls.enabled = snapshot.state.controls.enabled.filter(
        (action) => action !== 'configure',
      );
      snapshot.state.controls.guard = 'Answer the question first.';
    });
    const size = optionOf('cacheSize', 0);
    expect(size.disabled).toBe(true);
    expect(size.title).toBe('Answer the question first.');
    // The core's own reason for an option stays the reason.
    expect(optionOf('associativity', 3).title).toBe('Same as Full for an 8-line cache.');
    const off = [
      widthList(),
      inputAt('[data-action="timing-hit"]'),
      inputAt('[data-action="timing-miss"]'),
      inputAt('[data-action="timed"]'),
      optionOf('wordSize', 0),
    ];
    expect(off.map((control) => control.disabled)).toEqual([true, true, true, true, true]);
    expect(off.map((control) => control.title)).toEqual(
      Array(5).fill('Answer the question first.'),
    );
  });

  it('show a configuration that does not validate with the core’s reason', () => {
    const config = current().state().config;
    ui.pending = { ...config, cache: { ...config.cache, sizeBytes: 100 } };
    draw();
    expect(query('.cache-explorer__problem').textContent).toContain('power of two');
  });
});

describe('address and word', () => {
  it('rests as a line of the two current choices with a way to change them', () => {
    draw();
    const line = query('.cache-explorer__summary-line');
    expect(line.textContent).toBe('12-bit address · 4-byte words change…');
    const link = query('[data-action="address-toggle"]');
    expect(link.textContent).toBe('change…');
    expect(link.getAttribute('aria-expanded')).toBe('false');
    expect(root.querySelector('select')).toBeNull();
  });

  it('draws the core’s line for the configuration on show, as it gives it', () => {
    const config = current().state().config;
    ui.pending = { ...config, addressBits: 16, wordBytes: 8 };
    draw();
    expect(query('.cache-explorer__summary-line').textContent).toBe(
      '16-bit address · 8-byte words change…',
    );
    draw((snapshot) => {
      snapshot.config.addressSummary = 'as the core says';
    });
    expect(query('.cache-explorer__summary-line').textContent).toBe('as the core says change…');
  });

  it('opens into a list of the address widths and the four word sizes', () => {
    ui.addressOpen = true;
    draw();
    const select = widthList();
    expect(select.getAttribute('aria-label')).toBe('Address width');
    expect([...select.options].map((option) => option.textContent)).toEqual(
      Array.from({ length: 25 }, (_, i) => `${String(8 + i)}-bit`),
    );
    expect(select.options[select.selectedIndex]?.textContent).toBe('12-bit');
    expect(labels('address')).toEqual(['1 B', '2 B', '4 B', '8 B', 'done']);
    expect(optionOf('wordSize', 2).getAttribute('aria-pressed')).toBe('true');
    expect(query('[data-action="address-toggle"]').textContent).toBe('done');
  });

  it('keeps one list for the life of the root, with the student’s place in it', () => {
    ui.addressOpen = true;
    draw();
    const select = widthList();
    const options = [...select.options];
    draw();
    expect(query('select')).toBe(select);
    expect([...select.options]).toEqual(options);
    ui.addressOpen = false;
    draw();
    expect(root.querySelector('select')).toBeNull();
    ui.addressOpen = true;
    draw();
    expect(query('select')).toBe(select);
  });

  it('marks the width the pending configuration has, and disables one the core refuses', () => {
    const config = current().state().config;
    ui.addressOpen = true;
    // 512 B of 16 B lines, direct-mapped: 8-bit addresses would leave no tag.
    ui.pending = { ...config, addressBits: 16, cache: { ...config.cache, sizeBytes: 512 } };
    draw();
    const select = widthList();
    expect(select.options[select.selectedIndex]?.textContent).toBe('16-bit');
    const narrow = select.options[0];
    expect(narrow?.textContent).toBe('8-bit');
    expect(narrow?.disabled).toBe(true);
    expect(narrow?.title).toContain('no tag bits');
    expect(select.options[8]?.disabled).toBe(false);
  });
});

describe('timing', () => {
  it('shows the hit time and the miss penalty with the checkbox off, and on', () => {
    current().setAdvanced(true);
    draw();
    const hit = inputAt('[data-action="timing-hit"]');
    const miss = inputAt('[data-action="timing-miss"]');
    expect([hit.value, miss.value]).toEqual(['1', '20']);
    expect(inputAt('[data-action="timed"]').checked).toBe(false);

    const controls = current().configControls(current().state().config);
    current().proposeConfig(controls.timing.toggle);
    draw();
    expect([hit.value, miss.value]).toEqual(['1', '20']);
    expect(inputAt('[data-action="timed"]').checked).toBe(true);
  });

  it('takes typed text as text, not as numbers, and names the fields', () => {
    current().setAdvanced(true);
    draw();
    const hit = inputAt('[data-action="timing-hit"]');
    expect(hit.type).toBe('text');
    expect(hit.getAttribute('inputmode')).toBe('numeric');
    expect(hit.getAttribute('aria-label')).toBe('Hit time in cycles');
    expect(query('[data-action="timing-miss"]').getAttribute('aria-label')).toBe(
      'Miss penalty in cycles',
    );
  });

  it('keeps the fields for the life of the root, and what was typed in them', () => {
    current().setAdvanced(true);
    draw();
    const hit = inputAt('[data-action="timing-hit"]');
    hit.focus();
    hit.value = '7';
    draw();
    expect(query('[data-action="timing-hit"]')).toBe(hit);
    expect(hit.value).toBe('7');
    hit.blur();
    draw();
    expect(hit.value).toBe('1');
  });

  it('marks a field that does not read, keeps what was typed in it, and says why', () => {
    current().setAdvanced(true);
    ui.timingError = { field: 'hitCycles', message: 'Type a whole number of cycles, 1 to 1,000.' };
    draw();
    const hit = inputAt('[data-action="timing-hit"]');
    hit.value = 'abc';
    draw();
    expect(hit.value).toBe('abc');
    expect(hit.getAttribute('aria-invalid')).toBe('true');
    const note = query('.cache-explorer__note');
    expect(hit.getAttribute('aria-describedby')).toBe(note.id);
    expect(note.textContent).toBe('Type a whole number of cycles, 1 to 1,000.');
    // The note sits straight after the two fields it is about, above the checkbox.
    expect(note.previousElementSibling).toBe(query('.cache-explorer__timing'));
    expect(note.nextElementSibling).toBe(query('.cache-explorer__check'));
    expect(query('[data-action="timing-miss"]').hasAttribute('aria-invalid')).toBe(false);

    ui.timingError = null;
    draw();
    expect(hit.hasAttribute('aria-invalid')).toBe(false);
    expect(hit.hasAttribute('aria-describedby')).toBe(false);
    expect(hit.value).toBe('1');
  });
});

describe('"Reset to apply?"', () => {
  function ask(): void {
    step();
    ui.pending = { ...current().state().config, addressBits: 16 };
    ui.pendingMessage = 'Configuration is fixed for a run. Applying starts a new, empty run.';
  }

  it('is not there when nothing is pending', () => {
    draw();
    expect(root.querySelector('[role="alert"]')).toBeNull();
  });

  it('is an alert with the core’s question and the two ways to answer it', () => {
    ask();
    draw();
    const prompt = query('[role="alert"]');
    expect(prompt.querySelector('.cache-explorer__pending-title')?.textContent).toBe(
      'Reset to apply?',
    );
    expect(prompt.querySelector('.cache-explorer__pending-text')?.textContent).toBe(
      'Configuration is fixed for a run. Applying starts a new, empty run.',
    );
    const [apply, keep] = prompt.querySelectorAll('button');
    expect([apply?.textContent, apply?.dataset.action]).toEqual([
      'Reset and apply',
      'apply-pending',
    ]);
    expect(apply?.classList.contains('cache-explorer__button--primary')).toBe(true);
    expect([keep?.textContent, keep?.dataset.action]).toEqual(['Keep this run', 'keep-run']);
  });

  it('sits under the groups, inside the ruled bar', () => {
    ask();
    draw();
    const bar = query('.cache-explorer__config');
    expect([...bar.children].map((child) => child.className)).toEqual([
      'cache-explorer__config-body',
      'cache-explorer__pending',
    ]);
  });

  it('stays the same element while its question does, so it is not announced again', () => {
    ask();
    draw();
    const prompt = query('[role="alert"]');
    const apply = query('[data-action="apply-pending"]');
    const watch = new MutationObserver(() => undefined);
    watch.observe(prompt, {
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
    });
    draw();
    draw();
    expect(watch.takeRecords()).toEqual([]);
    watch.disconnect();
    expect(query('[role="alert"]')).toBe(prompt);
    expect(query('[data-action="apply-pending"]')).toBe(apply);
  });

  it('is rewritten when the question changes, and goes when it is answered', () => {
    ask();
    draw();
    const prompt = query('[role="alert"]');
    ui.pendingMessage = 'Another question.';
    draw();
    expect(query('[role="alert"]')).toBe(prompt);
    expect(query('.cache-explorer__pending-text').textContent).toBe('Another question.');

    ui.pending = null;
    ui.pendingMessage = null;
    draw();
    expect(root.querySelector('[role="alert"]')).toBeNull();
    expect(prompt.isConnected).toBe(false);
  });
});

describe('the flash a log line starts', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  const flashing = (): (string | null)[] =>
    [...root.querySelectorAll('.cache-explorer__group--why')].map((g) =>
      g.getAttribute('data-group'),
    );

  it('marks the group it names, and only that one', () => {
    ui.flash = { cause: 'lineSize', since: 0 };
    draw();
    expect(flashing()).toEqual(['lineSize']);
  });

  it('starts at the beginning when it has just begun, and carries on from how far it has got', () => {
    ui.flash = { cause: 'replacement', since: 1000 };
    draw(undefined, 1000);
    expect(group('replacement').hasAttribute('style')).toBe(false);
    draw(undefined, 1600);
    expect(group('replacement').getAttribute('style')).toBe('animation-delay: -600ms');
  });

  it('is a function of what it is given, and never reads the clock', () => {
    ui.flash = { cause: 'associativity', since: 1000 };
    const { snapshot, log } = buildSnapshot(current(), ui, 1250);
    const now = vi.spyOn(performance, 'now').mockReturnValue(99_999);
    render(root, snapshot, log, ui);
    render(root, snapshot, log, ui);
    expect(now).not.toHaveBeenCalled();
    expect(group('associativity').getAttribute('style')).toBe('animation-delay: -250ms');
  });

  it('draws a group Basics hides, read-only, with a note, for as long as it flashes', () => {
    draw();
    expect(root.querySelector('.cache-explorer__group[data-group="writePolicy"]')).toBeNull();

    ui.flash = { cause: 'writePolicy', since: 0 };
    draw();
    const policy = group('writePolicy');
    expect(labels('writePolicy')).toEqual(['Write-back', 'Write-through']);
    expect(policy.querySelector('.cache-explorer__note')?.textContent).toBe(
      'Advanced changes this.',
    );
    expect(groupNames()).toEqual([
      'Cache size',
      'Line size',
      'Associativity',
      'Replacement',
      'Write policy',
      'Address and word',
    ]);

    ui.flash = null;
    draw();
    expect(root.querySelector('.cache-explorer__group[data-group="writePolicy"]')).toBeNull();
  });

  it('keeps the current choice of a read-only group whole, and stills the others', () => {
    ui.flash = { cause: 'writePolicy', since: 0 };
    draw();
    expect(
      query('.cache-explorer__group[data-group="writePolicy"] .cache-explorer__seg').className,
    ).toContain('cache-explorer__seg--read-only');
    for (const [index, pressed] of [true, false].entries()) {
      const button = optionOf('writePolicy', index);
      // Not `disabled`, which is how the core and the gate switch a button off.
      expect(button.disabled).toBe(false);
      expect(button.getAttribute('aria-disabled')).toBe('true');
      expect(button.getAttribute('aria-pressed')).toBe(String(pressed));
    }
  });

  it('leaves a button the core switches off disabled, in a read-only group as in any other', () => {
    current().setAdvanced(true);
    draw();
    expect(optionOf('associativity', 3).disabled).toBe(true);
    expect(optionOf('associativity', 3).hasAttribute('aria-disabled')).toBe(false);
    expect(group('associativity').querySelector('.cache-explorer__seg--read-only')).toBeNull();
  });

  it('draws the timing Basics hides, read-only, when a log line points at it', () => {
    ui.flash = { cause: 'timing', since: 0 };
    draw();
    expect(group('timing').querySelector('.cache-explorer__note')?.textContent).toBe(
      'Advanced changes this.',
    );
    expect(inputAt('[data-action="timing-hit"]').disabled).toBe(true);
    expect(inputAt('[data-action="timed"]').disabled).toBe(true);
  });

  it('leaves a group Advanced shows live while it flashes', () => {
    current().setAdvanced(true);
    ui.flash = { cause: 'writePolicy', since: 0 };
    draw();
    expect(optionOf('writePolicy', 1).disabled).toBe(false);
    expect(optionOf('writePolicy', 1).hasAttribute('aria-disabled')).toBe(false);
    expect(group('writePolicy').querySelector('.cache-explorer__note')).toBeNull();
  });
});
