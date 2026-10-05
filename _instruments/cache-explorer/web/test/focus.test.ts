import { beforeEach, describe, expect, it, vi } from 'vitest';
import { h } from '../src/dom';
import { captureFocus, findByKey, focusKey, PLANE_CLASS, restoreFocus } from '../src/focus';

let root: HTMLElement;
let plane: HTMLElement;

/** The workbench as a render leaves it: the plane, and two ways of the cache inside it. */
function rebuild(): { way00: HTMLElement; way01: HTMLElement; step: HTMLElement } {
  const way00 = h('td', { 'data-action': 'edit-way', 'data-set': 0, 'data-way': 0, tabindex: 0 });
  const way01 = h('td', { 'data-action': 'edit-way', 'data-set': 0, 'data-way': 1, tabindex: 0 });
  const step = h('button', { 'data-action': 'step' }, 'Step');
  plane.replaceChildren(way00, way01, step);
  return { way00, way01, step };
}

beforeEach(() => {
  plane = h('section', { class: PLANE_CLASS, tabindex: -1 });
  root = h('div', null, plane);
  document.body.replaceChildren(h('button', { id: 'outside' }), root);
});

describe('focusKey', () => {
  it('is the action with its parameters, and null without an action', () => {
    const { way01 } = rebuild();
    expect(focusKey(way01)).toEqual({ name: 'edit-way', params: { set: '0', way: '1' } });
    expect(focusKey(plane)).toBeNull();
  });
});

describe('findByKey', () => {
  it('matches the action and every parameter exactly', () => {
    const { way01, step } = rebuild();
    expect(findByKey(root, { name: 'edit-way', params: { set: '0', way: '1' } })).toBe(way01);
    expect(findByKey(root, { name: 'step', params: {} })).toBe(step);
    expect(findByKey(root, { name: 'edit-way', params: { set: '0' } })).toBeNull();
    expect(findByKey(root, { name: 'edit-way', params: { set: '1', way: '1' } })).toBeNull();
  });
});

describe('focus across a rebuild', () => {
  it('goes to the new element with the same key', () => {
    rebuild().way01.focus();
    const memory = captureFocus(root);

    const after = rebuild();
    expect(document.activeElement).not.toBe(after.way01);
    restoreFocus(root, memory);

    expect(document.activeElement).toBe(after.way01);
  });

  it('goes to the plane when the element has no successor', () => {
    rebuild().step.focus();
    const memory = captureFocus(root);

    plane.replaceChildren(h('p', null, 'the step button is gone'));
    restoreFocus(root, memory);

    expect(document.activeElement).toBe(plane);
  });

  it('goes to the plane from an element that carried no action', () => {
    const link = h('a', { href: '#', tabindex: 0 }, 'a link');
    plane.replaceChildren(link);
    link.focus();
    const memory = captureFocus(root);

    plane.replaceChildren(h('a', { href: '#' }, 'a link'));
    restoreFocus(root, memory);

    expect(document.activeElement).toBe(plane);
  });

  it('keeps the plane when the plane had it', () => {
    rebuild();
    plane.focus();
    const memory = captureFocus(root);
    rebuild();
    restoreFocus(root, memory);
    expect(document.activeElement).toBe(plane);
  });

  it('puts the caret back in a text control', () => {
    const address = h('input', { 'data-action': 'address' });
    plane.replaceChildren(address);
    address.value = '0x0A4';
    address.focus();
    address.setSelectionRange(2, 4);
    const memory = captureFocus(root);

    const next = h('input', { 'data-action': 'address' });
    next.value = '0x0A4';
    plane.replaceChildren(next);
    restoreFocus(root, memory);

    expect(document.activeElement).toBe(next);
    expect([next.selectionStart, next.selectionEnd]).toEqual([2, 4]);
  });

  it('leaves a control alone that survived the render and kept focus', () => {
    // A persistent control with no action, such as the jump box: nothing names it but itself.
    const jump = h('input');
    plane.replaceChildren(jump);
    jump.value = '12';
    jump.focus();
    jump.setSelectionRange(1, 2);
    const memory = captureFocus(root);

    const focus = vi.spyOn(jump, 'focus');
    const select = vi.spyOn(jump, 'setSelectionRange');
    restoreFocus(root, memory);

    expect(document.activeElement).toBe(jump);
    expect(focus).not.toHaveBeenCalled();
    expect(select).not.toHaveBeenCalled();
  });

  it('takes focus back for a control that survived the render but lost it in a move', () => {
    const jump = h('input');
    plane.replaceChildren(jump);
    jump.value = '12';
    jump.focus();
    jump.setSelectionRange(1, 2);
    const memory = captureFocus(root);

    plane.replaceChildren(h('div', null, jump));
    expect(document.activeElement).not.toBe(jump);
    restoreFocus(root, memory);

    expect(document.activeElement).toBe(jump);
    expect([jump.selectionStart, jump.selectionEnd]).toEqual([1, 2]);
  });

  it('prefers the element that survived to a new one with the same key', () => {
    const old = h('button', { 'data-action': 'step' }, 'Step');
    plane.replaceChildren(old);
    old.focus();
    const memory = captureFocus(root);

    plane.replaceChildren(h('button', { 'data-action': 'step' }, 'Step'), old);
    restoreFocus(root, memory);

    expect(document.activeElement).toBe(old);
  });

  it('does not take focus from outside the tool', () => {
    rebuild();
    const outside = document.getElementById('outside');
    outside?.focus();
    const memory = captureFocus(root);
    expect(memory).toBeNull();

    rebuild();
    restoreFocus(root, memory);
    expect(document.activeElement).toBe(outside);
  });

  it('does not scroll the page to the element it focuses', () => {
    const { way00 } = rebuild();
    way00.focus();
    const memory = captureFocus(root);
    const after = rebuild();
    const focus = vi.spyOn(after.way00, 'focus');
    restoreFocus(root, memory);
    expect(focus).toHaveBeenCalledWith({ preventScroll: true });
  });
});
