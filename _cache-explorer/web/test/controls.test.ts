import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { commit, control, syncValue } from '../src/controls';
import { h } from '../src/dom';

let root: HTMLElement;

beforeEach(() => {
  root = h('div');
  document.body.replaceChildren(root);
});

describe('control', () => {
  it('makes a control once for a key and hands back the same one', () => {
    let made = 0;
    const make = (): HTMLInputElement => {
      made += 1;
      return h('input');
    };
    const first = control(root, 'address', make);
    expect(control(root, 'address', make)).toBe(first);
    expect(made).toBe(1);
  });

  it('keeps one control per key and per root', () => {
    const other = h('div');
    const a = control(root, 'address', () => h('input'));
    expect(control(root, 'value', () => h('input'))).not.toBe(a);
    expect(control(other, 'address', () => h('input'))).not.toBe(a);
  });

  it('keeps what the student typed when it is moved into a rebuilt region', () => {
    const input = control(root, 'address', () => h('input'));
    root.append(h('div', null, input));
    input.value = '0x1A';
    input.setSelectionRange(1, 3);

    root.replaceChildren(
      h(
        'section',
        null,
        h(
          'div',
          null,
          control(root, 'address', () => h('input')),
        ),
      ),
    );

    expect(input.isConnected).toBe(true);
    expect(input.value).toBe('0x1A');
    expect([input.selectionStart, input.selectionEnd]).toEqual([1, 3]);
  });
});

describe('syncValue', () => {
  it('writes a value that differs', () => {
    const input = h('input');
    syncValue(input, '0x0A4');
    expect(input.value).toBe('0x0A4');
  });

  it('leaves alone a control that already shows the value', () => {
    const input = h('input');
    root.append(input);
    input.value = 'abc';
    input.setSelectionRange(1, 1);
    syncValue(input, 'abc');
    expect(input.selectionStart).toBe(1);
  });

  it('does not overwrite what the student is typing', () => {
    const input = h('input');
    root.append(input);
    input.focus();
    input.value = '0x0';
    syncValue(input, '0x0A4');
    expect(input.value).toBe('0x0');
  });

  it('writes again once the student has left the control', () => {
    const input = h('input');
    root.append(input);
    input.focus();
    input.value = '0x0';
    input.blur();
    syncValue(input, '0x0A4');
    expect(input.value).toBe('0x0A4');
  });

  it('takes a textarea and a select as well', () => {
    const area = h('textarea');
    const select = h(
      'select',
      null,
      h('option', { value: '12' }, '12'),
      h('option', { value: '16' }, '16'),
    );
    syncValue(area, 'ld r1, [0x0A4]');
    syncValue(select, '16');
    expect(area.value).toBe('ld r1, [0x0A4]');
    expect(select.value).toBe('16');
  });
});

type MoveBefore = (this: Element, node: Node, child: Node | null) => void;

/** Gives every element a `moveBefore`, as a browser that has one does; jsdom has none. */
function withMoveBefore(move: MoveBefore): void {
  Object.defineProperty(Element.prototype, 'moveBefore', { configurable: true, value: move });
}

afterEach(() => {
  Reflect.deleteProperty(Element.prototype, 'moveBefore');
});

/** The text control a region shows, made once for `root`. */
const address = (): HTMLInputElement => control(root, 'address', () => h('input'));

describe('commit', () => {
  it('replaces what the region held with what was built, the control where its stand-in was', () => {
    root.append(h('p', null, 'old'), h('p', null, 'older'));
    const input = address();

    commit(root, (put) => [h('label', null, 'Address ', put(input)), h('p', null, 'new')]);

    expect(root.children).toHaveLength(2);
    expect(root.firstElementChild?.tagName).toBe('LABEL');
    expect(root.firstElementChild?.lastChild).toBe(input);
    expect(root.textContent).toBe('Address new');
    expect(root.querySelector('p')?.textContent).toBe('new');
  });

  it('leaves no stand-in behind, and takes the control out of the content it came from', () => {
    const input = address();
    commit(root, (put) => [h('section', null, put(input))]);
    const before = root.firstElementChild;

    commit(root, (put) => [h('section', null, h('div', null, put(input)))]);

    expect(before?.isConnected).toBe(false);
    expect(before?.contains(input)).toBe(false);
    expect(input.parentElement?.parentElement?.parentElement).toBe(root);
    expect(root.querySelectorAll('*')).toHaveLength(3);
    expect(root.innerHTML).not.toContain('<!--');
  });

  it('keeps a node that is in the new content and was in the region', () => {
    const kept = h('p', null, 'kept');
    root.append(kept, h('p', null, 'gone'));
    commit(root, () => [h('p', null, 'first'), kept]);
    expect(root.textContent).toBe('firstkept');
    expect(kept.isConnected).toBe(true);
  });

  it('keeps what the student typed, and their caret', () => {
    const input = address();
    commit(root, (put) => [h('div', null, put(input))]);
    input.value = '0x1A';
    input.setSelectionRange(1, 3);

    commit(root, (put) => [h('div', null, put(input))]);

    expect(input.value).toBe('0x1A');
    expect([input.selectionStart, input.selectionEnd]).toEqual([1, 3]);
  });

  it('writes the value it is given, before the control moves', () => {
    const input = address();
    commit(root, (put) => [h('div', null, put(input, '0x0A4'))]);
    expect(input.value).toBe('0x0A4');
  });

  it('does not write over what is being typed, even though the move takes focus away', () => {
    const input = address();
    commit(root, (put) => [h('div', null, put(input, '0x0A4'))]);
    input.focus();
    input.value = '0x0';

    commit(root, (put) => [h('div', null, put(input, '0x0A4'))]);

    expect(input.value).toBe('0x0');
  });

  it('moves the control with moveBefore where the browser has it, so focus is not lost', () => {
    const moves: { into: Element; node: Node; before: Node | null }[] = [];
    withMoveBefore(function (node, child) {
      moves.push({ into: this, node, before: child });
      this.insertBefore(node, child);
    });
    const input = address();
    commit(root, (put) => [h('div', null, put(input))]);
    // A control with no place yet has nothing to move from.
    expect(moves).toEqual([]);

    commit(root, (put) => [h('div', null, 'Address ', put(input))]);

    expect(moves).toHaveLength(1);
    const [move] = moves;
    expect(move?.node).toBe(input);
    expect(move?.into).toBe(root.firstElementChild);
    expect(move?.before?.nodeType).toBe(Node.COMMENT_NODE);
    expect(root.firstElementChild?.lastChild).toBe(input);
  });

  it('inserts the control when it is not in the same tree as its new place', () => {
    const moveBefore = vi.fn<MoveBefore>();
    withMoveBefore(moveBefore);
    const input = address();
    // The control sits in a tree that is not the document, and the region is in the document.
    h('div', null, input);

    commit(root, (put) => [h('div', null, put(input))]);

    expect(moveBefore).not.toHaveBeenCalled();
    expect(root.contains(input)).toBe(true);
  });

  it('says so when a control was asked for and not put in the content', () => {
    const input = address();
    expect(() => {
      commit(root, (put) => {
        put(input);
        return [h('p', null, 'no stand-in in here')];
      });
    }).toThrow('put in the content');
  });
});
