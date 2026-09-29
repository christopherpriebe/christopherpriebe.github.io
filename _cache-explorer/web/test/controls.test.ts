import { beforeEach, describe, expect, it } from 'vitest';
import { control, syncValue } from '../src/controls';
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
