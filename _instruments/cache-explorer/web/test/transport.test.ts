import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { start, type Controller } from '../src/controller';
import { h } from '../src/dom';
import { RUN_MS } from '../src/motion';
import { SAMPLE_PROGRAM } from '../src/ui-state';
import { Session } from '../src/wasm';
import { loadWasm } from './support/wasm';

let root: HTMLElement;
let controller: Controller | null;
/** The session the controller owns; a test spies on it and reads it, and never frees it. */
let lent: Session;

beforeAll(loadWasm);

beforeEach(() => {
  root = h('div', { class: 'cache-explorer' });
  document.body.replaceChildren(root);
  lent = Session.firstVisit();
  controller = start(root, lent, 'desktop');
});

afterEach(() => {
  vi.useRealTimers();
  controller?.dispose();
  controller = null;
});

const query = (selector: string): HTMLElement => {
  const found = root.querySelector<HTMLElement>(selector);
  if (!found) throw new Error(`nothing matches ${selector}`);
  return found;
};

const input = (action: string): HTMLInputElement => {
  const found = query(`input[data-action="${action}"]`);
  if (!(found instanceof HTMLInputElement)) throw new Error(`${action} is not an input`);
  return found;
};

const button = (action: string): HTMLButtonElement => {
  const found = query(`button[data-action="${action}"]`);
  if (!(found instanceof HTMLButtonElement)) throw new Error(`${action} is not a button`);
  return found;
};

/** Presses a key on `target` and returns the event, so a test can see whether it was prevented. */
function press(key: string, init: KeyboardEventInit = {}, target: HTMLElement = plane()): Event {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

function plane(): HTMLElement {
  const element = query('.cache-explorer__plane');
  element.focus();
  return element;
}

const live = (): string => query('.cache-explorer__live').textContent;
const status = (): string => query('.cache-explorer__status').textContent;
const stepCount = (): number => lent.timeline().stepCount;

describe('the transport bar', () => {
  it('draws the operation, boxes, Issue and the buttons with their keys', () => {
    expect(root.querySelector('[role="group"][aria-label="Operation"]')).not.toBeNull();
    expect(input('addr').value).toBe('0x0A4');
    expect(root.querySelector('input[data-action="value"]')).toBeNull();
    expect(button('issue').textContent).toBe('Issue');
    const keys = [...root.querySelectorAll('[role="group"][aria-label="Transport"] button')].map(
      (b) => [b.textContent, b.getAttribute('aria-keyshortcuts'), b.getAttribute('title')],
    );
    expect(keys).toEqual([
      ['Step', 'Space', 'Space'],
      ['Step access', 'Shift+Space', 'Shift+Space'],
      ['Run', 'R', 'R'],
      ['Reset', null, null],
      ['Undo', 'Z', 'Z'],
      ['Redo', 'Y', 'Y'],
    ]);
  });

  it('switches off what the core has switched off', () => {
    const { enabled } = lent.state().controls;
    for (const [action, control] of [
      ['undo', 'undo'],
      ['redo', 'redo'],
      ['reset', 'reset'],
      ['step', 'step'],
    ] as const) {
      expect(button(action).disabled).toBe(!enabled.includes(control));
    }
    expect(button('undo').disabled).toBe(true);
    expect(button('step').disabled).toBe(false);
  });

  it('shows the value box for a store only', () => {
    query('button[data-action="kind"][data-option="write"]').click();
    expect(input('value').value).toBe('7');
    expect(query('button[data-option="write"]').getAttribute('aria-pressed')).toBe('true');
    query('button[data-action="kind"][data-option="read"]').click();
    expect(root.querySelector('input[data-action="value"]')).toBeNull();
  });

  it('reads the typed address on every keystroke and marks one the core cannot read', () => {
    const draft = vi.spyOn(lent, 'draftIssue');
    const box = input('addr');
    box.value = '0x0A';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    box.value = 'zz';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    expect(draft).toHaveBeenLastCalledWith('read', 'zz', '7');
    expect(draft.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(box.getAttribute('aria-invalid')).toBe('true');
    expect(box.value).toBe('zz');
    box.value = '0x0A8';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    expect(box.hasAttribute('aria-invalid')).toBe(false);
  });

  it('issues the typed access with the button and with Enter in the address box', () => {
    const apply = vi.spyOn(lent, 'applyOp');
    button('issue').click();
    expect(apply).toHaveBeenCalledWith({ op: 'issue', kind: 'read', addr: 0x0a4 });
    const before = stepCount();
    expect(before).toBeGreaterThan(0);
    const box = input('addr');
    box.value = '0x0A8';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    const enter = press('Enter', {}, box);
    expect(enter.defaultPrevented).toBe(true);
    expect(apply).toHaveBeenLastCalledWith({ op: 'issue', kind: 'read', addr: 0x0a8 });
    expect(stepCount()).toBeGreaterThan(before);
  });

  it('issues a store with its value', () => {
    query('button[data-option="write"]').click();
    const apply = vi.spyOn(lent, 'applyOp');
    button('issue').click();
    expect(apply).toHaveBeenCalledWith({ op: 'issue', kind: 'write', addr: 0x0a4, value: 7n });
  });

  it('says why a typed access cannot be issued, and appends nothing', () => {
    const apply = vi.spyOn(lent, 'applyOp');
    const box = input('addr');
    box.value = 'zz';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    button('issue').click();
    expect(apply).not.toHaveBeenCalled();
    expect(status()).not.toBe('');
    box.value = '';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    button('issue').click();
    expect(status()).toBe('Type an address to issue.');
  });

  it('resets to a new empty run with the configuration in force, and undoes it', () => {
    button('step').click();
    const apply = vi.spyOn(lent, 'applyOp');
    button('reset').click();
    expect(apply).toHaveBeenCalledWith({ op: 'config', config: lent.state().config });
    expect(stepCount()).toBe(0);
    button('undo').click();
    expect(stepCount()).toBeGreaterThan(0);
  });

  it('says the configuration is unchanged after a Reset', () => {
    button('step').click();
    button('reset').click();
    expect(status()).toBe('Reset. The configuration is unchanged.');
  });

  it('issues a store with Enter in the value box', () => {
    query('button[data-option="write"]').click();
    const apply = vi.spyOn(lent, 'applyOp');
    const enter = press('Enter', {}, input('value'));
    expect(enter.defaultPrevented).toBe(true);
    expect(apply).toHaveBeenCalledWith({ op: 'issue', kind: 'write', addr: 0x0a4, value: 7n });
  });

  it('puts the strip on what is typed until the cursor next moves', () => {
    const typed = 'typed, not issued yet';
    press(' ', { shiftKey: true });
    expect(root.textContent).not.toContain(typed);
    const box = input('addr');
    box.value = '0x0A8';
    box.dispatchEvent(new Event('input', { bubbles: true }));
    expect(root.textContent).toContain(typed);
    press(' ', { shiftKey: true });
    expect(root.textContent).not.toContain(typed);
  });

  it('puts the strip on the typed access when Load or Store is chosen', () => {
    press(' ', { shiftKey: true });
    expect(root.textContent).not.toContain('typed, not issued yet');
    query('button[data-action="kind"][data-option="write"]').click();
    expect(root.textContent).toContain('typed, not issued yet');
  });
});

describe('the keyboard', () => {
  it('steps one micro-event on Space and one access on Shift+Space', () => {
    const event = vi.spyOn(lent, 'stepEvent');
    const access = vi.spyOn(lent, 'stepAccess');
    expect(press(' ').defaultPrevented).toBe(true);
    expect(event).toHaveBeenCalledTimes(1);
    expect(event.mock.calls[0]?.[0]).toMatchObject({ format: 'asm' });
    expect(press(' ', { shiftKey: true }).defaultPrevented).toBe(true);
    expect(access).toHaveBeenCalledTimes(1);
    expect(event).toHaveBeenCalledTimes(1);
  });

  it('undoes on Z and redoes on Y, in either case', () => {
    press(' ');
    const pos = lent.position().pos;
    const undo = vi.spyOn(lent, 'undo');
    const redo = vi.spyOn(lent, 'redo');
    press('z');
    expect(undo).toHaveBeenCalledTimes(1);
    const back = lent.position().pos;
    expect(back).toBeLessThan(pos);
    press('Y');
    expect(redo).toHaveBeenCalledTimes(1);
    expect(lent.position().pos).toBeGreaterThan(back);
  });

  it('steps again on Space after the Step button was clicked', () => {
    button('step').click();
    const first = lent.position().pos;
    query('.cache-explorer__plane');
    const focused = button('step');
    focused.focus();
    press(' ', {}, focused);
    expect(lent.position().pos).toBeGreaterThan(first);
  });

  it('ignores keys in an input, with a modifier held, and any it does not know', () => {
    const event = vi.spyOn(lent, 'stepEvent');
    const undo = vi.spyOn(lent, 'undo');
    const box = input('addr');
    expect(press(' ', {}, box).defaultPrevented).toBe(false);
    expect(press('z', {}, box).defaultPrevented).toBe(false);
    expect(press(' ', { ctrlKey: true }).defaultPrevented).toBe(false);
    expect(press('z', { metaKey: true }).defaultPrevented).toBe(false);
    expect(press('r', { altKey: true }).defaultPrevented).toBe(false);
    expect(press('q').defaultPrevented).toBe(false);
    expect(press('Enter').defaultPrevented).toBe(false);
    expect(event).not.toHaveBeenCalled();
    expect(undo).not.toHaveBeenCalled();
  });

  it('listens on the tool, not the page', () => {
    const event = vi.spyOn(lent, 'stepEvent');
    const outside = h('button', { type: 'button' });
    document.body.append(outside);
    const pressed = new KeyboardEvent('keydown', { key: ' ', bubbles: true, cancelable: true });
    outside.dispatchEvent(pressed);
    expect(pressed.defaultPrevented).toBe(false);
    expect(event).not.toHaveBeenCalled();
  });

  it('does not click a button on the release of a Space that stepped', () => {
    const focused = button('step');
    focused.focus();
    press(' ', {}, focused);
    expect(document.activeElement).toBe(button('step'));
    const release = new KeyboardEvent('keyup', { key: ' ', bubbles: true, cancelable: true });
    // The step drew again, and focus went back to the new button.
    (document.activeElement ?? focused).dispatchEvent(release);
    expect(release.defaultPrevented).toBe(true);
    const other = new KeyboardEvent('keyup', { key: ' ', bubbles: true, cancelable: true });
    query('.cache-explorer__plane').dispatchEvent(other);
    expect(other.defaultPrevented).toBe(false);
  });

  it('opens the sheet on ?, and Esc closes it and gives focus back to the ? button', () => {
    expect(root.querySelector('[role="dialog"]')).toBeNull();
    expect(press('?', { shiftKey: true }).defaultPrevented).toBe(true);
    const sheet = query('[role="dialog"][aria-label="Keyboard shortcuts"]');
    expect(sheet.textContent).toContain('Step one micro-event');
    expect(button('sheet').getAttribute('aria-expanded')).toBe('true');
    expect(document.activeElement).toBe(button('sheet-close'));
    const esc = press('Escape', {}, button('sheet-close'));
    expect(esc.defaultPrevented).toBe(true);
    expect(root.querySelector('[role="dialog"]')).toBeNull();
    expect(document.activeElement).toBe(button('sheet'));
  });

  it('leaves the sheet open and focus in the box on Escape in the address box', () => {
    button('sheet').click();
    const box = input('addr');
    box.focus();
    expect(press('Escape', {}, box).defaultPrevented).toBe(false);
    expect(root.querySelector('[role="dialog"]')).not.toBeNull();
    expect(document.activeElement).toBe(box);
  });

  it('keeps the shortcuts working after Undo or Redo disables itself', () => {
    button('step').click();
    button('step').click();
    const undo = button('undo');
    undo.focus();
    while (!button('undo').disabled) button('undo').click();
    expect(document.activeElement).toBe(query('.cache-explorer__plane'));
    const before = lent.position().pos;
    expect(press('y', {}, query('.cache-explorer__plane')).defaultPrevented).toBe(true);
    expect(lent.position().pos).toBeGreaterThan(before);
    button('redo').focus();
    while (!button('redo').disabled) button('redo').click();
    expect(document.activeElement).toBe(query('.cache-explorer__plane'));
    const after = lent.position().pos;
    press('z', {}, query('.cache-explorer__plane'));
    expect(lent.position().pos).toBeLessThan(after);
  });

  it('opens and closes the sheet with its buttons', () => {
    button('sheet').click();
    expect(root.querySelector('[role="dialog"]')).not.toBeNull();
    button('sheet-close').click();
    expect(root.querySelector('[role="dialog"]')).toBeNull();
    expect(press('Escape').defaultPrevented).toBe(false);
  });
});

describe('Run', () => {
  it('steps one micro-event every 450 ms until it is paused', () => {
    vi.useFakeTimers();
    const event = vi.spyOn(lent, 'stepEvent');
    press('r');
    expect(button('run').textContent).toBe('Pause');
    expect(button('run').getAttribute('aria-pressed')).toBe('true');
    expect(event).not.toHaveBeenCalled();
    vi.advanceTimersByTime(RUN_MS - 1);
    expect(event).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(event).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(RUN_MS * 2);
    expect(event).toHaveBeenCalledTimes(3);
    press('R');
    expect(button('run').textContent).toBe('Run');
    vi.advanceTimersByTime(RUN_MS * 3);
    expect(event).toHaveBeenCalledTimes(3);
  });

  it('stops on its own where the core has nowhere to go, and says why', () => {
    vi.useFakeTimers();
    const program = { source: SAMPLE_PROGRAM, format: 'asm' } as const;
    while (lent.stepAccess(program).status !== 'idle');
    press('r');
    vi.advanceTimersByTime(RUN_MS);
    expect(button('run').textContent).toBe('Run');
    expect(status()).not.toBe('');
  });

  it('ends when the student steps, and when the tool is let go', () => {
    vi.useFakeTimers();
    const event = vi.spyOn(lent, 'stepEvent');
    press('r');
    press(' ');
    expect(button('run').textContent).toBe('Run');
    expect(event).toHaveBeenCalledTimes(1);
    press('r');
    controller?.dispose();
    controller = null;
    vi.advanceTimersByTime(RUN_MS * 3);
    expect(event).toHaveBeenCalledTimes(1);
  });
});

describe('the live region', () => {
  it('holds the words for the event at the cursor, and follows it', () => {
    press(' ');
    const first = lent.timeline().liveText;
    expect(first).not.toBe('');
    expect(live()).toBe(first);
    press(' ');
    expect(live()).toBe(lent.timeline().liveText);
    expect(live()).not.toBe(first);
    press('z');
    expect(live()).toBe(lent.timeline().liveText);
    expect(query('.cache-explorer__live').getAttribute('aria-live')).toBe('polite');
  });
});
