import { describe, expect, it } from 'vitest';
import { actionAt, readAction } from '../src/actions';
import { h } from '../src/dom';

describe('readAction', () => {
  it('reads the action and only the parameters the element carries', () => {
    const cell = h('td', { 'data-action': 'edit-way', 'data-set': 2, 'data-way': 0 });
    expect(readAction(cell)).toEqual({
      name: 'edit-way',
      params: { set: '2', way: '0' },
      element: cell,
    });
  });

  it('reads all five parameters', () => {
    const el = h('button', {
      'data-action': 'go',
      'data-set': 1,
      'data-way': 2,
      'data-pos': 3,
      'data-group': 'lineSize',
      'data-addr': 164,
    });
    expect(readAction(el)?.params).toEqual({
      set: '1',
      way: '2',
      pos: '3',
      group: 'lineSize',
      addr: '164',
    });
  });

  it('is null for an element without an action, and for a parameter alone', () => {
    expect(readAction(h('div'))).toBeNull();
    expect(readAction(h('div', { 'data-set': 1 }))).toBeNull();
  });
});

describe('actionAt', () => {
  it('finds the nearest actionable element above the target', () => {
    const inner = h('span', null, 'Step');
    const button = h('button', { 'data-action': 'step' }, inner);
    const outer = h('div', { 'data-action': 'outer' }, button);
    const root = h('div', null, outer);
    expect(actionAt(inner, root)?.element).toBe(button);
  });

  it('does not look above the root', () => {
    const inner = h('span');
    const root = h('div', null, inner);
    const page = h('div', { 'data-action': 'page' }, root);
    document.body.replaceChildren(page);
    expect(actionAt(inner, root)).toBeNull();
  });

  it('is null for a target that is not an element', () => {
    expect(actionAt(null, h('div'))).toBeNull();
    expect(actionAt(window, h('div'))).toBeNull();
  });
});
