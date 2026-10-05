import { beforeEach, describe, expect, it, vi } from 'vitest';
import { delegate } from '../src/delegate';
import { h } from '../src/dom';

let root: HTMLElement;
let label: HTMLElement;
let step: HTMLElement;
let field: HTMLInputElement;

/** A root with a step button (a label inside it), a text field and a bare paragraph. */
function build(): void {
  label = h('span', null, 'Step');
  step = h('button', { 'data-action': 'step', 'data-set': 1 }, label);
  field = h('input', { 'data-action': 'address' });
  root.replaceChildren(step, field, h('p', null, 'no action here'));
}

beforeEach(() => {
  root = h('div');
  document.body.replaceChildren(root);
  build();
});

describe('delegate', () => {
  it('hears a click on a child of an actionable element, as that element', () => {
    const click = vi.fn();
    delegate(root, { click });
    label.click();
    expect(click).toHaveBeenCalledOnce();
    const [event, action] = click.mock.calls[0] as [MouseEvent, { name: string }];
    expect(event).toBeInstanceOf(MouseEvent);
    expect(action).toMatchObject({ name: 'step', params: { set: '1' }, element: step });
  });

  it('hears a scroll that does not bubble, from any element inside the root', () => {
    const scroll = vi.fn();
    delegate(root, { scroll });
    const scroller = h('div', { 'data-action': 'list' });
    root.append(scroller);
    scroller.dispatchEvent(new Event('scroll'));
    expect(scroll).toHaveBeenCalledOnce();
    expect(scroll.mock.calls[0]?.[1]).toMatchObject({ name: 'list', element: scroller });
    const [event] = scroll.mock.calls[0] as [Event];
    expect(event.target).toBe(scroller);
  });

  it('routes input, change, keydown and focusin, each with its action', () => {
    const handlers = {
      input: vi.fn(),
      change: vi.fn(),
      keydown: vi.fn(),
      focusin: vi.fn(),
    };
    delegate(root, handlers);
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.dispatchEvent(new Event('change', { bubbles: true }));
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    field.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    for (const handler of Object.values(handlers)) {
      expect(handler).toHaveBeenCalledOnce();
      expect(handler.mock.calls[0]?.[1]).toMatchObject({ name: 'address', element: field });
    }
    expect((handlers.keydown.mock.calls[0]?.[0] as KeyboardEvent).key).toBe('Enter');
  });

  it('gives null when the event did not come from an actionable element', () => {
    const click = vi.fn();
    delegate(root, { click });
    root.querySelector('p')?.click();
    expect(click).toHaveBeenCalledWith(expect.any(MouseEvent), null);
  });

  it('keeps hearing after the root has been rebuilt', () => {
    const click = vi.fn();
    delegate(root, { click });
    build();
    label.click();
    expect(click.mock.calls[0]?.[1]).toMatchObject({ name: 'step', element: step });
  });

  it('adds one listener per handler it was given, on the root', () => {
    const add = vi.spyOn(root, 'addEventListener');
    delegate(root, { click: vi.fn(), keydown: vi.fn() });
    expect(add.mock.calls.map(([type]) => type)).toEqual(['click', 'keydown']);
  });

  it('stops listening when told to', () => {
    const click = vi.fn();
    const stop = delegate(root, { click });
    stop();
    label.click();
    expect(click).not.toHaveBeenCalled();
  });
});
