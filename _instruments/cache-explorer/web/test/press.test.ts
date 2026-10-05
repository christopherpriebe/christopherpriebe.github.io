import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import { h } from '../src/dom';
import { PRESS_LIMIT_MS, watchPress, type Press } from '../src/press';

let root: HTMLElement;
let control: HTMLElement;
let plain: HTMLElement;
let outside: HTMLElement;
let released: Mock<() => void>;
let press: Press;

const event = (type: string, init: MouseEventInit = {}): MouseEvent =>
  new MouseEvent(type, { bubbles: true, button: 0, ...init });

beforeEach(() => {
  vi.useFakeTimers();
  control = h('button', { type: 'button', 'data-action': 'go' }, 'Go');
  plain = h('p', null, 'Some words');
  root = h('div', null, control, h('label', null, h('input', { type: 'checkbox' }), 'Box'), plain);
  outside = h('div');
  document.body.replaceChildren(root, outside);
  released = vi.fn<() => void>();
  press = watchPress(root, released);
});

afterEach(() => {
  press.stop();
  vi.useRealTimers();
});

describe('a press on a control', () => {
  it.each([
    ['a button', () => control],
    ['a control inside a label', () => root.querySelector('input') ?? control],
    ['the label’s text', () => root.querySelector('label') ?? control],
  ])('is down from the mouse going down on %s until it comes up', (_, target) => {
    target().dispatchEvent(event('mousedown'));
    expect(press.down).toBe(true);
    target().dispatchEvent(event('mouseup'));
    expect(press.down).toBe(false);
  });

  it('is not one when it lands on text, another button, or outside the tool', () => {
    plain.dispatchEvent(event('mousedown'));
    expect(press.down).toBe(false);
    control.dispatchEvent(event('mousedown', { button: 2 }));
    expect(press.down).toBe(false);
    outside.dispatchEvent(event('mousedown'));
    expect(press.down).toBe(false);
    vi.runAllTimers();
    expect(released).not.toHaveBeenCalled();
  });

  it('calls back after the release but not in it, so that the click which follows is not cut off', () => {
    control.dispatchEvent(event('mousedown'));
    control.dispatchEvent(event('mouseup'));
    expect(released).not.toHaveBeenCalled();
    control.dispatchEvent(event('click'));
    vi.runAllTimers();
    expect(released).toHaveBeenCalledOnce();
  });

  it('ends when the mouse comes up somewhere else', () => {
    control.dispatchEvent(event('mousedown'));
    outside.dispatchEvent(event('mouseup'));
    expect(press.down).toBe(false);
    vi.runAllTimers();
    expect(released).toHaveBeenCalledOnce();
  });

  it.each([
    ['a drag takes it over', () => control.dispatchEvent(event('dragend'))],
    ['a context menu opens', () => control.dispatchEvent(event('contextmenu'))],
    ['the window loses focus', () => window.dispatchEvent(new Event('blur'))],
  ])('ends, for it may never see a release, when %s', (_, end) => {
    control.dispatchEvent(event('mousedown'));
    end();
    expect(press.down).toBe(false);
    vi.runAllTimers();
    expect(released).toHaveBeenCalledOnce();
  });

  it('is not one on a list, whose popup may keep the release from the page', () => {
    const list = h('select', { 'data-action': 'width' }, h('option', null, '12-bit'));
    root.append(list);
    list.dispatchEvent(event('mousedown'));
    expect(press.down).toBe(false);
    list.querySelector('option')?.dispatchEvent(event('mousedown'));
    expect(press.down).toBe(false);
  });

  it('ends of itself when no release comes, so the tool is not left as it was drawn', () => {
    control.dispatchEvent(event('mousedown'));
    vi.advanceTimersByTime(PRESS_LIMIT_MS - 1);
    expect(press.down).toBe(true);
    expect(released).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(press.down).toBe(false);
    vi.runAllTimers();
    expect(released).toHaveBeenCalledOnce();
  });

  it('gives a press the whole limit from the moment it went down', () => {
    control.dispatchEvent(event('mousedown'));
    vi.advanceTimersByTime(PRESS_LIMIT_MS - 10);
    control.dispatchEvent(event('mouseup'));
    control.dispatchEvent(event('mousedown'));
    vi.advanceTimersByTime(PRESS_LIMIT_MS - 10);
    expect(press.down).toBe(true);
    control.dispatchEvent(event('mouseup'));
    expect(vi.getTimerCount()).toBe(1);
  });

  it('is one press when another goes down before the callback for the last has run', () => {
    control.dispatchEvent(event('mousedown'));
    control.dispatchEvent(event('mouseup'));
    control.dispatchEvent(event('mousedown'));
    vi.advanceTimersByTime(0);
    expect(released).not.toHaveBeenCalled();
    control.dispatchEvent(event('mouseup'));
    vi.runAllTimers();
    expect(released).toHaveBeenCalledOnce();
  });

  it('calls back for a release only once, however many events end it', () => {
    control.dispatchEvent(event('mousedown'));
    control.dispatchEvent(event('mouseup'));
    control.dispatchEvent(event('contextmenu'));
    window.dispatchEvent(new Event('blur'));
    vi.runAllTimers();
    expect(released).toHaveBeenCalledOnce();
  });
});

describe('stopping', () => {
  it('stops listening and cancels a callback still waiting', () => {
    control.dispatchEvent(event('mousedown'));
    control.dispatchEvent(event('mouseup'));
    press.stop();
    expect(vi.getTimerCount()).toBe(0);
    control.dispatchEvent(event('mousedown'));
    expect(press.down).toBe(false);
    vi.runAllTimers();
    expect(released).not.toHaveBeenCalled();
  });
});
