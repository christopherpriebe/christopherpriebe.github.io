import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { SCENARIOS } from '../dev/sessions';
import { start, type Controller } from '../src/controller';
import { h } from '../src/dom';
import { Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

let root: HTMLElement;
let controller: Controller | null;
/** The session the controller owns; a test reads it and moves its run, and never frees it. */
let lent: Session;

beforeAll(loadWasm);

beforeEach(() => {
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
  controller = null;
});

afterEach(() => {
  controller?.dispose();
  controller = null;
});

/** Starts the tool on one of the development scenarios. */
function begin(name: string): void {
  const build = SCENARIOS[name];
  if (!build) throw new Error(`no scenario ${name}`);
  lent = build();
  controller = start(root, lent, 'desktop');
}

const query = (selector: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`nothing matches ${selector}`);
  return found;
};

const input = (selector: string): HTMLInputElement => {
  const found = query(selector);
  if (!(found instanceof HTMLInputElement)) throw new Error(`${selector} is not an input`);
  return found;
};

const wayCell = (set: number, way: number): HTMLElement =>
  query(`[data-action="edit-way"][data-set="${String(set)}"][data-way="${String(way)}"]`);
const memoryWord = (addr: number): HTMLElement =>
  query(`[data-action="edit-memory"][data-addr="${String(addr)}"]`);
const dialog = (): HTMLElement => query('[role="dialog"]');
const primary = (): HTMLButtonElement => {
  const found = query('.cache-explorer__editor-actions .cache-explorer__button--primary');
  if (!(found instanceof HTMLButtonElement)) throw new Error('the primary is not a button');
  return found;
};
const title = (): string => query('.cache-explorer__editor-title').textContent;
const lines = (): string[] =>
  [...root.querySelectorAll('.cache-explorer__editor-lines li')].map((li) => li.textContent);

/** Types `text` into a field and leaves it, as a student does. */
function type(field: HTMLInputElement, text: string): void {
  field.value = text;
  field.dispatchEvent(new Event('input', { bubbles: true }));
  field.dispatchEvent(new Event('change', { bubbles: true }));
}

const option = (action: string, value: number): HTMLElement =>
  query(`button[data-action="${action}"][data-option="${String(value)}"]`);

/** Presses a key on `target`, and returns the event so a test can see whether it was prevented. */
function press(key: string, target: HTMLElement, init: KeyboardEventInit = {}): Event {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

describe('the way editor', () => {
  it('opens under the cell, with the way as it is, and focus in its first field', () => {
    begin('editors-end-of-access');
    wayCell(2, 0).click();
    expect(dialog().getAttribute('aria-label')).toBe('Edit set 2 · way 0');
    expect(wayCell(2, 0).contains(dialog())).toBe(true);
    expect(wayCell(2, 0).classList.contains('cache-explorer__way--editing')).toBe(true);
    expect(input('input[data-action="edit-line"]').value).toBe('0x0A0');
    expect(document.activeElement).toBe(input('input[data-action="edit-line"]'));
    expect(option('edit-valid', 1).getAttribute('aria-pressed')).toBe('true');
    expect(option('edit-dirty', 0).getAttribute('aria-pressed')).toBe('true');
    expect(option('edit-rank', 0).getAttribute('aria-pressed')).toBe('true');
    const words = [...root.querySelectorAll<HTMLInputElement>('input[data-action="edit-word"]')];
    expect(words.map((word) => word.value)).toEqual(['40', '53', '66', '79']);
    // Nothing is edited yet, and the core says so.
    expect(title()).toBe('No changes');
    expect(primary().disabled).toBe(true);
  });

  it('confirms an edit with the re-rank previewed, and applies it as one step', () => {
    begin('editors-end-of-access');
    wayCell(2, 0).click();
    type(input('input[data-action="edit-word"][data-pos="1"]'), '9');
    option('edit-dirty', 1).click();
    option('edit-rank', 1).click();
    expect(title()).toBe('Ready to apply');
    expect(lines()).toEqual([
      'word 1: 53 → 9',
      'dirty 0 → 1',
      'rank way 0: 0 → 1',
      'rank way 1: 1 → 0',
    ]);
    expect(input('input[data-action="edit-word"][data-pos="1"]').className).toContain(
      'cache-explorer__field--changed',
    );
    expect(input('input[data-action="edit-word"][data-pos="1"]').className).toContain(
      'cache-explorer__field--differs',
    );
    const ops = lent.opCount();
    expect(primary().textContent).toBe('Apply');
    primary().click();
    expect(lent.opCount()).toBe(ops + 1);
    const way = lent.state().cache.sets[2]?.ways[0];
    expect(way?.words[1]).toBe('9');
    expect(way?.dirty).toBe(true);
    expect(root.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(wayCell(2, 0));
  });

  it('sends a data input the student has not typed in as empty, and shows what the core reads', () => {
    begin('editors-end-of-access');
    wayCell(3, 0).click();
    type(input('input[data-action="edit-line"]'), '0x0C0');
    // The way would take another line, so untouched inputs show memory's copy of that line.
    const shown = [
      ...root.querySelectorAll<HTMLInputElement>('input[data-action="edit-word"]'),
    ].map((word) => word.value);
    expect(shown).toEqual(
      lent.draftEdit(3, 0, {
        valid: true,
        lineText: '0x0C0',
        wordTexts: ['', '', '', ''],
        dirty: true,
      }).wordTexts,
    );
    expect(shown).not.toEqual(['5', '8', '21', '34']);
  });

  it('shows a word left empty as the word the core reads for it again', () => {
    begin('editors-end-of-access');
    wayCell(2, 0).click();
    const word = input('input[data-action="edit-word"][data-pos="0"]');
    type(word, '7');
    expect(title()).toBe('Data differs from memory');
    type(word, '');
    expect(word.value).toBe('40');
    expect(title()).toBe('No changes');
  });

  it('offers to set the dirty bit when a clean line is given data that memory lacks (4b)', () => {
    begin('editors-end-of-access');
    wayCell(3, 0).click();
    option('edit-dirty', 0).click();
    type(input('input[data-action="edit-word"][data-pos="2"]'), '40');
    expect(title()).toBe('Data differs from memory');
    expect(query('.cache-explorer__editor-status').className).toContain('--caution');
    expect(primary().textContent).toBe('Set dirty and apply');
    expect(primary().dataset.action).toBe('edit-suggest');
    expect(lines()).toEqual(['word 2: 21 → 40']);
    primary().click();
    const way = lent.state().cache.sets[3]?.ways[0];
    expect(way?.words[2]).toBe('40');
    expect(way?.dirty).toBe(true);
  });

  it('offers to move a line to the set its index bits name (4c)', () => {
    begin('editors-end-of-access');
    wayCell(3, 0).click();
    type(input('input[data-action="edit-line"]'), '0x0C0');
    expect(title()).toBe('0x0C0 belongs in set 0');
    expect(query('.cache-explorer__editor-status').className).toContain('--move');
    expect(primary().textContent).toBe('Move to set 0, way 0');
    primary().click();
    const { sets } = lent.state().cache;
    expect(sets[0]?.ways[0]?.valid).toBe(true);
    expect(sets[0]?.ways[0]?.lineText).toBe('0x0C0');
    expect(sets[3]?.ways[0]?.lineText).toBe('0x0B0');
  });

  it('refuses a line whose set is full, and applies nothing (4d)', () => {
    begin('editors-end-of-access');
    wayCell(3, 0).click();
    type(input('input[data-action="edit-line"]'), '0x120');
    expect(title()).toBe('Refused');
    expect(query('.cache-explorer__editor-status').className).toContain('--refused');
    expect(primary().disabled).toBe(true);
    expect(primary().textContent).toBe('Cannot apply');
    const ops = lent.opCount();
    primary().click();
    press('Enter', input('input[data-action="edit-line"]'));
    expect(lent.opCount()).toBe(ops);
    expect(root.querySelector('[role="dialog"]')).not.toBeNull();
  });

  it('marks a line that does not read, and says why', () => {
    begin('editors-end-of-access');
    wayCell(3, 0).click();
    type(input('input[data-action="edit-line"]'), 'banana');
    expect(input('input[data-action="edit-line"]').getAttribute('aria-invalid')).toBe('true');
    expect(title()).toBe('Not an address');
    expect(primary().disabled).toBe(true);
  });

  it('opens an invalid way with valid 0 and no line, so that a bare Apply changes nothing', () => {
    begin('editors-end-of-access');
    wayCell(0, 0).click();
    expect(option('edit-valid', 0).getAttribute('aria-pressed')).toBe('true');
    expect(input('input[data-action="edit-line"]').value).toBe('');
    expect(primary().disabled).toBe(true);
    const ops = lent.opCount();
    press('Enter', input('input[data-action="edit-line"]'));
    expect(lent.opCount()).toBe(ops);
  });

  it('applies on Enter in a field when the primary action is enabled', () => {
    begin('editors-end-of-access');
    wayCell(2, 0).click();
    const word = input('input[data-action="edit-word"][data-pos="3"]');
    type(word, '80');
    const ops = lent.opCount();
    expect(press('Enter', word).defaultPrevented).toBe(true);
    expect(lent.opCount()).toBe(ops + 1);
    expect(lent.state().cache.sets[2]?.ways[0]?.words[3]).toBe('80');
  });

  it('closes on Escape from a field, or its buttons, and gives focus back to the cell', () => {
    begin('editors-end-of-access');
    wayCell(2, 1).click();
    expect(press('Escape', input('input[data-action="edit-line"]')).defaultPrevented).toBe(true);
    expect(root.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(wayCell(2, 1));
    wayCell(2, 1).click();
    query('[data-action="edit-close"]').click();
    expect(root.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(wayCell(2, 1));
    wayCell(2, 1).click();
    query('[data-action="edit-cancel"]').click();
    expect(root.querySelector('[role="dialog"]')).toBeNull();
  });

  it('opens on Enter on a focused cell, and Esc returns focus to that way’s new cell', () => {
    begin('editors-end-of-access');
    const cell = wayCell(2, 0);
    cell.focus();
    expect(press('Enter', cell).defaultPrevented).toBe(true);
    expect(dialog().getAttribute('aria-label')).toBe('Edit set 2 · way 0');
    press('Escape', input('input[data-action="edit-line"]'));
    expect(wayCell(2, 0)).not.toBe(cell);
    expect(document.activeElement).toBe(wayCell(2, 0));
  });

  it('leaves the keys of the editor’s own buttons to the browser', () => {
    begin('editors-end-of-access');
    wayCell(2, 0).click();
    const before = lent.position().pos;
    const seg = option('edit-dirty', 1);
    seg.focus();
    expect(press(' ', seg).defaultPrevented).toBe(false);
    expect(press('z', seg).defaultPrevented).toBe(false);
    expect(lent.position().pos).toBe(before);
  });

  it('keeps the way editor open across a click on the popover, and switches to another cell', () => {
    begin('editors-end-of-access');
    wayCell(2, 0).click();
    type(input('input[data-action="edit-word"][data-pos="0"]'), '7');
    query('.cache-explorer__editor-grid').click();
    expect(input('input[data-action="edit-word"][data-pos="0"]').value).toBe('7');
    wayCell(2, 1).click();
    expect(dialog().getAttribute('aria-label')).toBe('Edit set 2 · way 1');
    expect(input('input[data-action="edit-word"][data-pos="0"]').value).toBe('96');
  });

  it('keeps what is typed when the cursor moves, and lets go of a way the run no longer has', () => {
    begin('editors-end-of-access');
    wayCell(2, 0).click();
    type(input('input[data-action="edit-word"][data-pos="1"]'), '9');
    lent.undo();
    controller?.refresh();
    expect(input('input[data-action="edit-word"][data-pos="1"]').value).toBe('9');
    const { config } = lent.state();
    lent.applyOp({
      op: 'config',
      config: { ...config, cache: { ...config.cache, sizeBytes: 16, associativity: 1 } },
    });
    controller?.refresh();
    expect(root.querySelector('[role="dialog"]')).toBeNull();
  });

  it('refuses in the middle of an access, and Go to the end finishes it (E0)', () => {
    begin('editors-mid-access');
    wayCell(2, 0).click();
    expect(title()).toBe('Finish this access first');
    expect(primary().disabled).toBe(true);
    const before = lent.position();
    query('[data-action="edit-end"]').click();
    expect(lent.position().pos).toBeGreaterThan(before.pos);
    expect(['opEnd', 'logEnd']).toContain(lent.position().boundary);
    // The editor stays open, and now says the form is unchanged.
    expect(dialog().getAttribute('aria-label')).toBe('Edit set 2 · way 0');
    expect(title()).toBe('No changes');
    expect(root.querySelector('[data-action="edit-end"]')).toBeNull();
  });

  it('shows a gated refusal as the core words it, with nothing to apply', () => {
    begin('editors-end-of-access');
    const ask = lent.draftEdit.bind(lent);
    lent.draftEdit = (set, way, form) => ({
      ...ask(set, way, form),
      outcome: {
        outcome: 'refusal',
        reason: 'gated',
        tone: 'refused',
        title: 'Refused',
        text: 'Answer the prediction in the module panel first.',
        wordMarks: [],
      },
    });
    wayCell(2, 0).click();
    expect(title()).toBe('Refused');
    expect(query('.cache-explorer__editor-text').textContent).toBe(
      'Answer the prediction in the module panel first.',
    );
    expect(primary().disabled).toBe(true);
    expect(root.querySelector('[data-action="edit-end"]')).toBeNull();
  });

  it('lays a rank out as a list when the set has more ways than buttons fit', () => {
    begin('wrapped-four-sets');
    lent.seek(lent.timeline().end);
    controller?.refresh();
    wayCell(2, 7).click();
    expect(dialog().getAttribute('data-side')).toBe('right');
    const list = query('select[data-action="edit-rank"]');
    if (!(list instanceof HTMLSelectElement)) throw new Error('the rank is not a list');
    expect(list.options).toHaveLength(8);
    list.value = '0';
    list.dispatchEvent(new Event('change', { bubbles: true }));
    expect(title()).toBe('Ready to apply');
    expect(lines().some((line) => line.startsWith('rank way 7'))).toBe(true);
  });
});

describe('the memory editor', () => {
  it('opens under the memory table with the value the word has, and warns of a newer cached value (4e)', () => {
    begin('editors-end-of-access');
    memoryWord(0xb0).click();
    expect(dialog().getAttribute('aria-label')).toBe('Edit memory 0x0B0');
    expect(query('.cache-explorer__box--memory').contains(dialog())).toBe(true);
    expect(input('input[data-action="edit-value"]').value).toBe('92');
    expect(document.activeElement).toBe(input('input[data-action="edit-value"]'));
    expect(title()).toBe('Set 3, way 0 holds a newer value');
    expect(query('.cache-explorer__editor-status').className).toContain('--caution');
    // The value is unchanged, so there is nothing to apply yet.
    expect(primary().disabled).toBe(true);
  });

  it('applies a new value to memory on Enter, and gives focus back to the word', () => {
    begin('editors-end-of-access');
    memoryWord(0xb0).click();
    const value = input('input[data-action="edit-value"]');
    type(value, '93');
    expect(primary().textContent).toBe('Apply to memory');
    expect(lines()).toEqual(['memory 0x0B0: 92 → 93']);
    const ops = lent.opCount();
    press('Enter', value);
    expect(lent.opCount()).toBe(ops + 1);
    expect(root.querySelector('[role="dialog"]')).toBeNull();
    expect(memoryWord(0xb0).dataset.value).toBe('93');
    expect(document.activeElement).toBe(memoryWord(0xb0));
  });

  it('marks a value that does not read, and closes on Escape', () => {
    begin('editors-end-of-access');
    memoryWord(0xb0).click();
    const value = input('input[data-action="edit-value"]');
    type(value, '99999999999');
    expect(value.getAttribute('aria-invalid')).toBe('true');
    expect(title()).toBe('Check the value');
    expect(press('Escape', value).defaultPrevented).toBe(true);
    expect(root.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(memoryWord(0xb0));
  });

  it('opens on Enter on a focused word', () => {
    begin('editors-end-of-access');
    const word = memoryWord(0xa4);
    word.focus();
    press('Enter', word);
    expect(dialog().getAttribute('aria-label')).toBe('Edit memory 0x0A4');
  });

  it('updates the cached copy too when a clean line holds the word (M4)', () => {
    begin('editors-end-of-access');
    // 0x0A4 sits in the clean way 0 of set 2.
    memoryWord(0xa4).click();
    type(input('input[data-action="edit-value"]'), '12');
    expect(primary().textContent).toBe('Apply to both');
    primary().click();
    expect(lent.state().cache.sets[2]?.ways[0]?.words[1]).toBe('12');
  });
});
